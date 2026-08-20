//! Times the converter against real documents, so a speed change can be
//! measured rather than guessed at.
//!
//! Three separate things are timed, and NEVER summed into one number,
//! because they scale differently and one would mask the others:
//!
//! 1. **`bench inline`** — `convert()` alone, on a fixed in-repo corpus. No
//!    disk, no ZIP, no classifier. This is the number the table-scan and
//!    allocation work in `apply_map`/`rearrange` moves.
//! 2. **`bench classify`** — `convert_pieces` on document text already
//!    extracted into memory. Isolates the double-`convert()` call and the
//!    per-word classifier cost from I/O and XML parsing.
//! 3. **`bench convert`** — `convert_office`/`convert_legacy_office` end to
//!    end, reading from disk. What a user actually experiences.
//!
//! # Why this exists
//!
//! Until this crate existed, the only timing code in the workspace was
//! `crates/mukti-formats/tests/performance.rs`, which asserts a RATIO to
//! catch a return to quadratic behaviour. It would not notice a 30%
//! constant-factor change in either direction, so nothing would have caught
//! the difference between `opt-level = "s"` and `opt-level = 3`, or an
//! allocation-heavy rewrite of `convert()`.
//!
//! # Why not criterion
//!
//! The unit of work here is "convert one real 41 MB workbook, several
//! seconds" — `n = 3` is what an honest measurement can afford, and
//! criterion's warm-up and outlier-rejection machinery is built for
//! microsecond-scale work run thousands of times. It would also add roughly
//! 40 packages to a workspace that has just shed 74 of them.
//!
//! # Why not a `#[test]`
//!
//! The corpus is gigabytes of real documents, deliberately outside this
//! repository and git-ignored (see `.sandbox/corpus-paths.local`). A check
//! that needs material CI cannot have is not a test that runs in CI, so it
//! is not a test.
//!
//! # How it avoids being flaky
//!
//! - Every file is read into memory before any clock starts, in every mode,
//!   so the page cache is never part of what is measured.
//! - Each file is timed `--repeats` times (default 3) and the **median** is
//!   reported, with the min and max alongside it — never the mean, which one
//!   slow outlier (a core waking from idle, an allocator pause) distorts
//!   badly on a sample this small.
//! - `bench noise-floor` runs the same measurement twice against the SAME
//!   binary and reports the spread between the two runs. Anything smaller
//!   than that spread is "no measured effect", not a result.
//! - Every report's header records the machine, the toolchain, the commit
//!   and the resolved profile, because a timing number with none of those
//!   attached cannot be compared to anything later.
//!
//! # Keying
//!
//! Every row is keyed on `sha256` of the INPUT bytes, never the path.
//! `Dev-Memory/LESSONS.md` records 650 phantom failures once produced by
//! keying a comparison on path when a corpus directory was renamed between
//! runs — the corpus behind this project has moved four times.

use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::Instant;

use sha2::{Digest, Sha256};

use gru953_mukti::classify::{convert_pieces, count};
use gru953_mukti::convert;
use mukti_formats::{convert_legacy_office, convert_office, runs, LegacyFormat};

/// The six extensions Mukti converts. Kept in one place so this harness can
/// never silently drift from what the shipped CLI actually accepts.
const SUPPORTED: [&str; 6] = ["doc", "docx", "ppt", "pptx", "xls", "xlsx"];

fn main() -> std::process::ExitCode {
    match run() {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut args = std::env::args().skip(1);
    let mode = args.next().ok_or_else(usage)?;

    match mode.as_str() {
        "inline" => inline_mode(args),
        "classify" => classify_mode(args),
        "convert" => convert_mode(args),
        "noise-floor" => noise_floor_mode(args),
        _ => Err(usage()),
    }
}

fn usage() -> String {
    "usage:\n  \
     bench inline       [--repeats N]\n  \
     bench classify     <dir>... [--repeats N] [--limit N]\n  \
     bench convert      <dir>... [--repeats N] [--limit N] [--tier text|mid|media|all]\n  \
     bench noise-floor  <dir>... [--limit N]\n\n\
     Every mode prints the machine, toolchain, commit and profile it was built with, \
     then a median/min/max per tier. Nothing here is a #[test]: the corpus is real \
     documents kept outside this repository."
        .to_owned()
}

// ---------------------------------------------------------------------
// Shared plumbing
// ---------------------------------------------------------------------

struct Config {
    roots: Vec<PathBuf>,
    repeats: usize,
    limit: Option<usize>,
    tier: Option<Tier>,
}

fn parse_config(mut args: impl Iterator<Item = String>) -> Result<Config, String> {
    let mut cfg = Config {
        roots: Vec::new(),
        repeats: 3,
        limit: None,
        tier: None,
    };
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--repeats" => {
                let n = args.next().ok_or("--repeats needs a number")?;
                cfg.repeats = n.parse().map_err(|_| format!("not a number: {n}"))?;
            }
            "--limit" => {
                let n = args.next().ok_or("--limit needs a number")?;
                cfg.limit = Some(n.parse().map_err(|_| format!("not a number: {n}"))?);
            }
            "--tier" => {
                let t = args.next().ok_or("--tier needs a value")?;
                cfg.tier = match t.as_str() {
                    "text" => Some(Tier::Text),
                    "mid" => Some(Tier::Mid),
                    "media" => Some(Tier::Media),
                    "all" => None,
                    other => return Err(format!("unknown tier {other}")),
                };
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            path => cfg.roots.push(PathBuf::from(path)),
        }
    }
    if cfg.repeats == 0 {
        return Err("--repeats must be at least 1".to_owned());
    }
    Ok(cfg)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Tier {
    /// Under 1 MB — where `convert()` and the classifier dominate.
    Text,
    /// 1 MB to 50 MB — a mix of classifier cost and archive I/O.
    Mid,
    /// Over 50 MB — dominated by ZIP I/O and copying embedded media through
    /// untouched. Five real `.pptx` files in this project's own corpus
    /// exceed 200 MB and are almost entirely media, so a single aggregate
    /// across all tiers would be meaningless: report each tier separately.
    Media,
}

impl Tier {
    fn of(bytes: u64) -> Self {
        if bytes < 1_000_000 {
            Tier::Text
        } else if bytes < 50_000_000 {
            Tier::Mid
        } else {
            Tier::Media
        }
    }
    fn label(self) -> &'static str {
        match self {
            Tier::Text => "text (<1 MB)",
            Tier::Mid => "mid (1-50 MB)",
            Tier::Media => "media (>50 MB)",
        }
    }
}

/// One real file, found once regardless of how many roots or symlinks
/// mention it: deduplicated on a hash of its content, never its path.
struct Found {
    path: PathBuf,
    bytes: Vec<u8>,
    hash: String,
    ext: String,
    tier: Tier,
}

fn collect(roots: &[PathBuf], limit: Option<usize>) -> Result<Vec<Found>, String> {
    if roots.is_empty() {
        return Err("name at least one folder to read documents from".to_owned());
    }
    let mut paths = Vec::new();
    for root in roots {
        walk(root, &mut paths).map_err(|e| format!("walking {}: {e}", root.display()))?;
    }
    paths.sort();

    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for path in paths {
        let Some(ext) = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        if !SUPPORTED.contains(&ext.as_str()) {
            continue;
        }
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skipping {}: could not read it: {e}", path.display());
                continue;
            }
        };
        if bytes.is_empty() {
            continue;
        }
        let hash = sha256_hex(&bytes);
        if !seen.insert(hash.clone()) {
            continue; // the same content, found again under another name or root
        }
        let tier = Tier::of(bytes.len() as u64);
        out.push(Found {
            path,
            bytes,
            hash,
            ext,
            tier,
        });
        if let Some(n) = limit {
            if out.len() >= n {
                break;
            }
        }
    }
    if out.is_empty() {
        return Err("found no files of a supported kind under those folders".to_owned());
    }
    Ok(out)
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk(&path, out)?;
        } else if file_type.is_file() {
            out.push(path);
        }
        // Symlinks are neither: skipped, rather than followed into a loop.
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// Median, min and max of a set of durations, in seconds.
fn median_min_max(mut v: Vec<f64>) -> (f64, f64, f64) {
    v.sort_by(|a, b| a.partial_cmp(b).expect("a duration is never NaN"));
    let min = v[0];
    let max = v[v.len() - 1];
    let median = if v.len() % 2 == 1 {
        v[v.len() / 2]
    } else {
        (v[v.len() / 2 - 1] + v[v.len() / 2]) / 2.0
    };
    (median, min, max)
}

fn print_header(title: &str) {
    println!("=== {title} ===");
    println!("commit:   {}", commit());
    println!("rustc:    {}", rustc_version());
    println!("target:   {}", std::env::consts::ARCH);
    println!(
        "profile:  {}",
        if cfg!(debug_assertions) {
            "debug (unoptimised — build with --release for a real number)"
        } else {
            // Cannot distinguish "release" from "profiling" here: both set
            // debug-assertions = false, and Cargo gives a crate no reliable
            // way to name its own profile at compile time. Say only what is
            // actually known -- optimisations are on -- rather than assert
            // a specific profile name that might be wrong.
            "optimised (release or profiling; check the build command used)"
        }
    );
    println!();
}

fn commit() -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown (not a git checkout?)".to_owned())
}

fn rustc_version() -> String {
    std::process::Command::new("rustc")
        .arg("--version")
        .output()
        .ok()
        .filter(|o| o.status.success())
        .and_then(|o| String::from_utf8(o.stdout).ok())
        .map(|s| s.trim().to_owned())
        .unwrap_or_else(|| "unknown".to_owned())
}

// ---------------------------------------------------------------------
// Mode 1: `convert()` alone, on a fixed in-repo corpus
// ---------------------------------------------------------------------

/// A small, fixed corpus, embedded rather than read from disk. Every Bijoy
/// string here is one already used and verified elsewhere in this
/// workspace — `Awd†mi bvgt Kg©m~wP` is the README's own worked example, and
/// `Kg\u{a9}m~wP`/`bvg`/`cÖwZ\u{2020}e\`b` are the same three words
/// `classify.rs`'s own tests use — rather than a new one invented for this
/// file and never checked against the tables. Alongside them: ordinary
/// English, and text already in Unicode — the three things `convert()` is
/// asked to look at in real documents.
const INLINE_CORPUS: &[&str] = &[
    "Awd†mi bvgt Kg©m~wP ZvwiLt wefvMt",
    "Kg\u{a9}m~wP bvg cÖwZ\u{2020}e`b",
    "Programme review 2026 and budget allocation for the fiscal year",
    "The quick brown fox jumps over the lazy dog, twelve times a day",
    "এই অংশটি ইউনিকোডে আছে এবং ঠিক আছে",
    "সরকার একটি নতুন কর্মসূচি গ্রহণ করেছে জনগণের কল্যাণে",
    "Report: annual summary of activities for the district office",
    "welfare programme for rural households and vulnerable groups 2026",
];

fn inline_mode(args: impl Iterator<Item = String>) -> Result<(), String> {
    let cfg = parse_config(args)?;
    print_header("bench inline — convert() alone, no disk, no classifier");

    let mut total_words = 0usize;
    let mut all_seconds = Vec::new();
    for _ in 0..cfg.repeats {
        let started = Instant::now();
        let mut words = 0usize;
        for text in INLINE_CORPUS {
            for word in text.split_whitespace() {
                let _ = convert(word);
                words += 1;
            }
        }
        all_seconds.push(started.elapsed().as_secs_f64());
        total_words = words;
    }
    let (median, min, max) = median_min_max(all_seconds);
    let words_per_sec = total_words as f64 / median.max(1e-9);
    println!(
        "{total_words} words per repeat, {} repeats: median {median:.6}s, min {min:.6}s, max {max:.6}s",
        cfg.repeats
    );
    println!("-> {words_per_sec:.0} words/second (median)");
    Ok(())
}

// ---------------------------------------------------------------------
// Mode 2: classify_words/convert_pieces on already-extracted document text
// ---------------------------------------------------------------------

fn classify_mode(args: impl Iterator<Item = String>) -> Result<(), String> {
    let cfg = parse_config(args)?;
    let found = collect(&cfg.roots, cfg.limit)?;

    print_header("bench classify — convert_pieces on extracted text, tiered");

    // Only the three modern formats carry runs(); .doc/.xls/.ppt are read
    // through convert_legacy_office directly, which does not expose a
    // separate text-extraction step to time in isolation.
    let modern: Vec<&Found> = found
        .iter()
        .filter(|f| matches!(f.ext.as_str(), "docx" | "xlsx" | "pptx"))
        .collect();
    if modern.is_empty() {
        return Err("no .docx/.xlsx/.pptx files found under those folders".to_owned());
    }

    for tier in [Tier::Text, Tier::Mid, Tier::Media] {
        if let Some(want) = cfg.tier {
            if want != tier {
                continue;
            }
        }
        let in_tier: Vec<&&Found> = modern.iter().filter(|f| f.tier == tier).collect();
        if in_tier.is_empty() {
            continue;
        }

        let mut per_file_medians = Vec::new();
        let mut total_words = 0usize;
        for file in &in_tier {
            // Extract once, outside the clock: this mode measures the
            // classifier, not the ZIP/XML read.
            let joined = match runs(Cursor::new(file.bytes.as_slice())) {
                Ok(runs) => runs.iter().map(|r| r.text.as_str()).collect::<String>(),
                Err(e) => {
                    eprintln!(
                        "skipping {}: could not extract runs: {e}",
                        file.path.display()
                    );
                    continue;
                }
            };

            let mut seconds = Vec::new();
            let mut converted = 0usize;
            let mut untouched = 0usize;
            for _ in 0..cfg.repeats {
                let started = Instant::now();
                let pieces = convert_pieces(&joined);
                seconds.push(started.elapsed().as_secs_f64());
                let (c, u) = count(&pieces);
                converted = c;
                untouched = u;
            }
            let (median, _min, _max) = median_min_max(seconds);
            per_file_medians.push(median);
            total_words += converted + untouched;
        }

        if per_file_medians.is_empty() {
            continue;
        }
        let (median, min, max) = median_min_max(per_file_medians);
        println!(
            "{}: {} files, {total_words} words total",
            tier.label(),
            in_tier.len()
        );
        println!("  per-file median {median:.6}s, min {min:.6}s, max {max:.6}s");
    }
    Ok(())
}

// ---------------------------------------------------------------------
// Mode 3: end-to-end conversion, reading from disk
// ---------------------------------------------------------------------

fn convert_mode(args: impl Iterator<Item = String>) -> Result<(), String> {
    let cfg = parse_config(args)?;
    let found = collect(&cfg.roots, cfg.limit)?;

    print_header("bench convert — end to end, reading from disk");

    // One un-timed pass over everything first, to warm the page cache the
    // same way for every tier before any clock starts.
    for file in &found {
        let _ = file.bytes.len();
    }

    println!("in_hash\tkind\tbytes\tconvert_median_s\tconvert_min_s\tconvert_max_s\twords_converted\twords_untouched");
    for tier in [Tier::Text, Tier::Mid, Tier::Media] {
        if let Some(want) = cfg.tier {
            if want != tier {
                continue;
            }
        }
        let in_tier: Vec<&Found> = found.iter().filter(|f| f.tier == tier).collect();
        if in_tier.is_empty() {
            continue;
        }

        let mut tier_medians = Vec::new();
        for file in &in_tier {
            let mut seconds = Vec::new();
            let mut converted = 0usize;
            let mut untouched = 0usize;
            for _ in 0..cfg.repeats {
                let started = Instant::now();
                let outcome = convert_one(file);
                seconds.push(started.elapsed().as_secs_f64());
                match outcome {
                    Ok((c, u)) => {
                        converted = c;
                        untouched = u;
                    }
                    Err(e) => {
                        eprintln!("skipping {}: {e}", file.path.display());
                        continue;
                    }
                }
            }
            if seconds.is_empty() {
                continue;
            }
            let (median, min, max) = median_min_max(seconds);
            tier_medians.push(median);
            println!(
                "{}\t{}\t{}\t{median:.6}\t{min:.6}\t{max:.6}\t{converted}\t{untouched}",
                file.hash,
                file.ext,
                file.bytes.len()
            );
        }

        if !tier_medians.is_empty() {
            let (median, min, max) = median_min_max(tier_medians);
            eprintln!(
                "\n{}: {} files — per-file median convert time {median:.6}s (min {min:.6}s, max {max:.6}s)",
                tier.label(),
                in_tier.len()
            );
        }
    }
    Ok(())
}

/// Convert one file end to end, returning (words converted, words untouched).
fn convert_one(file: &Found) -> Result<(usize, usize), String> {
    match file.ext.as_str() {
        "docx" | "xlsx" | "pptx" => {
            let (_out, summary) = convert_office(&file.bytes, "Nirmala UI")
                .map_err(|e| format!("could not convert: {e}"))?;
            Ok((summary.words_converted, summary.words_untouched))
        }
        "doc" | "xls" | "ppt" => {
            let format =
                LegacyFormat::from_extension(&file.ext).expect("checked against SUPPORTED already");
            let outcome = convert_legacy_office(&file.bytes, format)
                .map_err(|e| format!("could not convert: {e}"))?;
            Ok((
                outcome.summary.words_converted,
                outcome.summary.words_untouched,
            ))
        }
        other => Err(format!("not a format this harness converts: {other}")),
    }
}

// ---------------------------------------------------------------------
// Mode 4: the noise floor
// ---------------------------------------------------------------------

fn noise_floor_mode(args: impl Iterator<Item = String>) -> Result<(), String> {
    let cfg = parse_config(args)?;
    let found = collect(&cfg.roots, cfg.limit)?;

    print_header("bench noise-floor — the same binary, measured twice");
    println!(
        "Runs the end-to-end conversion of {} files twice in a row and reports the spread \
         between the two totals. A later change smaller than this spread has not been shown \
         to do anything.\n",
        found.len()
    );

    let mut totals = Vec::new();
    for pass in 1..=2 {
        let started = Instant::now();
        for file in &found {
            let _ = convert_one(file);
        }
        let elapsed = started.elapsed().as_secs_f64();
        println!("pass {pass}: {elapsed:.6}s total");
        totals.push(elapsed);
    }
    let spread = (totals[0] - totals[1]).abs();
    let spread_pct = 100.0 * spread / totals[0].max(totals[1]).max(1e-9);
    println!("\nspread: {spread:.6}s ({spread_pct:.1}% of the larger total)");
    println!(
        "-> treat any later change under about {spread_pct:.1}% as unmeasured, not as a result"
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn median_of_an_odd_count_is_the_middle_value() {
        let (median, min, max) = median_min_max(vec![3.0, 1.0, 2.0]);
        assert_eq!(median, 2.0);
        assert_eq!(min, 1.0);
        assert_eq!(max, 3.0);
    }

    #[test]
    fn median_of_an_even_count_averages_the_middle_two() {
        let (median, min, max) = median_min_max(vec![4.0, 1.0, 3.0, 2.0]);
        assert_eq!(median, 2.5);
        assert_eq!(min, 1.0);
        assert_eq!(max, 4.0);
    }

    #[test]
    fn median_of_one_value_is_that_value() {
        let (median, min, max) = median_min_max(vec![7.5]);
        assert_eq!(median, 7.5);
        assert_eq!(min, 7.5);
        assert_eq!(max, 7.5);
    }

    #[test]
    fn one_outlier_does_not_move_the_median_the_way_it_would_move_a_mean() {
        // Nineteen ordinary runs and one that caught a core waking from
        // idle. The mean would be dragged well above 0.011; the median
        // should not move at all -- this is the whole reason median is
        // reported rather than mean.
        let mut v = vec![
            0.010, 0.011, 0.012, 0.011, 0.010, 0.011, 0.012, 0.010, 0.011, 0.012,
        ];
        v.push(5.0);
        let (median, _min, max) = median_min_max(v);
        assert!(
            median < 0.02,
            "the median should sit near the ordinary runs, not near the outlier: {median}"
        );
        assert_eq!(max, 5.0, "the outlier must still be visible as the max");
    }

    #[test]
    fn tier_boundaries_match_the_documented_thresholds() {
        assert_eq!(Tier::of(0), Tier::Text);
        assert_eq!(Tier::of(999_999), Tier::Text);
        assert_eq!(Tier::of(1_000_000), Tier::Mid);
        assert_eq!(Tier::of(49_999_999), Tier::Mid);
        assert_eq!(Tier::of(50_000_000), Tier::Media);
        assert_eq!(Tier::of(300_000_000), Tier::Media);
    }

    #[test]
    fn tier_all_clears_any_earlier_filter_and_does_not_swallow_the_rest_of_the_line() {
        // `"all" => return Ok(cfg)` once bailed out of parsing the moment it
        // was reached, so a folder named after `--tier all` on the command
        // line was silently dropped. Fixed to fall through like every other
        // value; this is the regression test for that.
        let args = ["--tier", "all", "a-folder"].map(str::to_owned);
        let cfg = parse_config(args.into_iter()).expect("a valid command line");
        assert_eq!(cfg.tier, None, "\"all\" must mean no filter");
        assert_eq!(
            cfg.roots,
            vec![PathBuf::from("a-folder")],
            "the folder named after --tier all must still be read"
        );
    }

    #[test]
    fn the_hash_is_sixty_four_hex_characters_and_stable() {
        let a = sha256_hex(b"the same bytes");
        let b = sha256_hex(b"the same bytes");
        let c = sha256_hex(b"different bytes");
        assert_eq!(a.len(), 64, "sha256 in hex is always 64 characters");
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, b, "identical input must hash identically");
        assert_ne!(a, c, "different input must not collide in this tiny sample");
    }

    #[test]
    fn the_inline_corpus_converts_without_panicking_and_changes_the_legacy_words() {
        // Not a correctness test for the converter -- mukti-core owns that --
        // but a guard that this harness's own fixture is a real word, not a
        // typo that would silently time zero conversions forever.
        let mut any_changed = false;
        for text in INLINE_CORPUS {
            for word in text.split_whitespace() {
                if convert(word) != word {
                    any_changed = true;
                }
            }
        }
        assert!(
            any_changed,
            "no word in the inline corpus converted -- it would time nothing"
        );
    }
}
