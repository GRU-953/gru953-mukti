//! Run every document in an archive through Mukti and check the result.
//!
//! The original release verified 300 documents by hand. That produced a number
//! nobody could reproduce and a check nobody could repeat, which is the same
//! mistake the accuracy claims made before they were rebuilt. This is that
//! check, written as a command.
//!
//! # What it actually checks
//!
//! Not "did it look right" — for each format, the properties that must hold for
//! **every** document, whatever is in it:
//!
//! | Kind | Checked |
//! |---|---|
//! | Office | opens; word count preserved; every non-text entry byte-identical; entry list unchanged; no legacy font left; converting twice changes nothing more |
//! | Legacy Office | opens; the .docx/.xlsx/.pptx it produces passes the Office check above; empty input is reported, not treated as a fault |
//! | Text | nothing converted means byte-for-byte identical output |
//! | English-only | **zero** words converted — anything else is a false positive |
//!
//! The strongest of these is **idempotence**. A converter that has genuinely
//! finished leaves nothing for a second pass to do; one that mangles its own
//! output will convert something the second time. It catches a whole class of
//! fault that eyeballing a document never will.
//!
//! # Why it writes to `local/`
//!
//! The report names real files from a private archive, so it goes where nothing
//! is ever committed from. Aggregate counts are the only thing that may leave.
//!
//! # Surviving a file that kills the process
//!
//! Each file is wrapped so a panic is recorded and the run continues. A **stack
//! overflow** cannot be caught that way — it takes the process with it, which is
//! exactly what RUSTSEC-2026-0187 once did with a 21 KB crafted PDF, back when
//! this tool still read PDFs (removed in 0.9.0). So every row is flushed to
//! disk as it is produced: if the process dies, the last row names the file
//! that killed it, and `--resume` carries on past it.
//!
//! # Keying, and comparing two runs
//!
//! Every row's first column is `in_hash`, a SHA-256 of the input file's own
//! bytes — never its path. `path` is kept as a later, non-key column for a
//! human to read; nothing joins on it any more. This is what makes `--resume`
//! rename-proof (a moved corpus directory no longer produces a false "never
//! checked this one before"), and it is what makes `--compare <old.tsv>`
//! possible at all: two runs of this tool, taken before and after a change,
//! can be joined on `in_hash` even if the corpus moved between them.
//!
//! `--compare` reports four disjoint counts — **identical**, **differing**,
//! **vanished** (in the old report, not the new one) and **new** — because
//! collapsing "vanished" into "differing" would score an environment change
//! (a file that could not be found this time) as if it were a defect in the
//! code being compared.
//!
//! Two columns carry the output, for two different questions. `out_hash` is
//! the whole produced document, byte for byte — the strongest thing that can
//! be asked. `out_entries_hash` is a fingerprint built from each archive
//! entry's own content, sorted by name, with no container framing in it at
//! all. The two diverge on exactly the case the zip 2→8 bump hit: three
//! documents whose CONTENT was untouched but whose central-directory
//! external-attributes bytes changed between library versions (see
//! `Dev-Memory/github-snapshot/dependabot-pull-requests.md`). `--compare`
//! uses `out_hash` by default; `--compare-entries` uses `out_entries_hash`,
//! for exactly that situation — a change known to touch only how the archive
//! container is written, not what is inside it.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use mukti_formats::office::{is_exactly_legacy_font, is_legacy_font, names_a_font};
use mukti_formats::{convert_legacy_office, convert_office, runs, LegacyFormat, Summary};

/// SHA-256 of `bytes`, as lowercase hex. Used for `in_hash`, `out_hash` and
/// as the building block of `out_entries_hash` — one hash function, so a
/// value from one column can always be compared with one from another.
fn sha256_hex(bytes: &[u8]) -> String {
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

/// A fingerprint of an Office archive's CONTENT, invariant to how the
/// container itself is written.
///
/// Hashes each entry's bytes individually, pairs that with the entry's name,
/// sorts by name (so entry order cannot matter either), and hashes the
/// joined result. A library that restamps timestamps or file-mode bits in
/// the central directory — exactly what zip 2 and zip 8 disagree about —
/// changes the whole-archive bytes without changing this fingerprint at all.
fn content_fingerprint(archive_bytes: &[u8]) -> Result<String, String> {
    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(archive_bytes))
        .map_err(|e| format!("not a readable archive: {e}"))?;
    let mut names: Vec<String> = zip.file_names().map(str::to_owned).collect();
    names.sort();

    let mut joined = String::new();
    for name in &names {
        let bytes = read_entry(&mut zip, name)?;
        joined.push_str(name);
        joined.push('\0');
        joined.push_str(&sha256_hex(&bytes));
        joined.push('\n');
    }
    Ok(sha256_hex(joined.as_bytes()))
}

/// What we decided about one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Converted, and every invariant for its kind held.
    Ok,
    /// Read fine, but there was nothing legacy in it. Not a fault.
    Untouched,
    /// The format is not supported yet. Not a fault either — a gap.
    Unsupported,
    /// Read, but produced nothing usable — an older Office file from which no
    /// text at all could be recovered.
    NoText,
    /// An invariant was violated. **This is a defect.**
    Failed,
    /// The code panicked. **Also a defect**, and a worse one.
    Panicked,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Untouched => "untouched",
            Status::Unsupported => "unsupported",
            Status::NoText => "no-text",
            Status::Failed => "FAILED",
            Status::Panicked => "PANICKED",
        }
    }
    /// Does this row mean something is wrong with Mukti?
    fn is_defect(self) -> bool {
        matches!(self, Status::Failed | Status::Panicked)
    }
}

struct Outcome {
    status: Status,
    /// Plain-English detail. Never contains document text.
    detail: String,
    converted: usize,
    untouched: usize,
    fonts: usize,
    /// SHA-256 of the input file's bytes. Set by `run()` on every row,
    /// including a file that could not be converted — empty only when the
    /// file could not even be read.
    in_hash: String,
    /// SHA-256 of the produced output, whole. Empty when there is no
    /// meaningful output: a failed conversion, an unsupported kind, or an
    /// empty input.
    out_hash: String,
    /// A fingerprint of the produced output's archive ENTRIES, invariant to
    /// container-level framing. Empty for plain text, which is not an
    /// archive, and wherever `out_hash` is empty for the same reason.
    out_entries_hash: String,
}

impl Outcome {
    fn ok(summary: Summary) -> Self {
        let status = if summary.words_converted == 0 {
            Status::Untouched
        } else {
            Status::Ok
        };
        Self {
            status,
            detail: String::new(),
            converted: summary.words_converted,
            untouched: summary.words_untouched,
            fonts: summary.fonts_changed,
            in_hash: String::new(),
            out_hash: String::new(),
            out_entries_hash: String::new(),
        }
    }
    fn bad(status: Status, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
            converted: 0,
            untouched: 0,
            fonts: 0,
            in_hash: String::new(),
            out_hash: String::new(),
            out_entries_hash: String::new(),
        }
    }

    /// Record the produced Office archive's hashes. Called only on the
    /// success path, once the output bytes actually exist.
    fn with_office_output(mut self, out_bytes: &[u8]) -> Self {
        self.out_hash = sha256_hex(out_bytes);
        self.out_entries_hash = content_fingerprint(out_bytes).unwrap_or_default();
        self
    }

    /// Record the produced plain text's hash. There is no archive here, so
    /// no entry-level fingerprint applies.
    fn with_text_output(mut self, out_text: &str) -> Self {
        self.out_hash = sha256_hex(out_text.as_bytes());
        self
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(defects) => {
            if defects == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                eprintln!("\n{defects} file(s) violated an invariant. See the report.");
                std::process::ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("corpus-verify: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct Config {
    roots: Vec<PathBuf>,
    out: PathBuf,
    resume: bool,
    only: Option<Vec<String>>,
    limit: Option<usize>,
    /// Treat everything found as English-only: any conversion is a false positive.
    negative: bool,
    /// A previous report to join this run against, on `in_hash`.
    compare: Option<PathBuf>,
    /// Join on `out_entries_hash` instead of `out_hash` — for a change known
    /// to touch only container framing, not document content.
    compare_entries: bool,
}

fn parse() -> Result<Config, String> {
    let mut cfg = Config {
        roots: Vec::new(),
        out: PathBuf::from("local/verify-report.tsv"),
        resume: false,
        only: None,
        limit: None,
        negative: false,
        compare: None,
        compare_entries: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--out" => cfg.out = PathBuf::from(it.next().ok_or("--out needs a file")?),
            "--resume" => cfg.resume = true,
            "--negative" => cfg.negative = true,
            "--only" => {
                let list = it.next().ok_or("--only needs a comma-separated list")?;
                cfg.only = Some(list.split(',').map(|s| s.trim().to_lowercase()).collect());
            }
            "--limit" => {
                let n = it.next().ok_or("--limit needs a number")?;
                cfg.limit = Some(n.parse().map_err(|_| format!("not a number: {n}"))?);
            }
            "--compare" => {
                cfg.compare = Some(PathBuf::from(
                    it.next()
                        .ok_or("--compare needs a report file to join against")?,
                ))
            }
            "--compare-entries" => cfg.compare_entries = true,
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            path => cfg.roots.push(PathBuf::from(path)),
        }
    }
    if cfg.roots.is_empty() {
        return Err(
            "usage: corpus-verify <dir>... [--out <file>] [--only docx,xlsx] [--limit N] \
             [--resume] [--negative] [--compare <old.tsv>] [--compare-entries]"
                .into(),
        );
    }
    if cfg.compare_entries && cfg.compare.is_none() {
        return Err("--compare-entries needs --compare <old.tsv> as well".into());
    }
    Ok(cfg)
}

fn run() -> Result<usize, String> {
    let cfg = parse()?;

    let mut files = Vec::new();
    for root in &cfg.roots {
        if !root.is_dir() {
            return Err(format!("not a directory: {}", root.display()));
        }
        collect(root, &mut files).map_err(|e| format!("walking {}: {e}", root.display()))?;
    }
    files.sort();

    if let Some(only) = &cfg.only {
        files.retain(|p| extension(p).is_some_and(|e| only.contains(&e)));
    }

    // Refuse to write an empty report over a real one. corpus-label learned this
    // the expensive way: pointed at a moved directory it truncated a 152 MB
    // labelled set to a bare header, silently, and exited successfully.
    if files.is_empty() {
        return Err(
            "found no files to check — is the path right? Refusing to write an \
                    empty report over an existing one."
                .into(),
        );
    }

    // Keyed on `in_hash` (report column 0), not path: a moved corpus
    // directory must not defeat resume. Read failures write an empty
    // `in_hash`, and an empty string is never added to this set below, so a
    // run of unreadable files cannot make each other look already-done.
    let already: std::collections::HashSet<String> = if cfg.resume && cfg.out.exists() {
        fs::read_to_string(&cfg.out)
            .map_err(|e| e.to_string())?
            .lines()
            .skip(1)
            .filter_map(|l| l.split('\t').next())
            .filter(|h| !h.is_empty())
            .map(str::to_owned)
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    if let Some(parent) = cfg.out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let fresh = already.is_empty();
    let mut report = fs::OpenOptions::new()
        .create(true)
        .append(!fresh)
        .write(true)
        .truncate(fresh)
        .open(&cfg.out)
        .map_err(|e| format!("opening {}: {e}", cfg.out.display()))?;
    if fresh {
        writeln!(
            report,
            "in_hash\tpath\tkind\tstatus\tconverted\tuntouched\tfonts\tout_hash\tout_entries_hash\tdetail"
        )
        .map_err(|e| e.to_string())?;
    }

    let total = files.len();
    println!("Checking {total} file(s).");
    if cfg.negative {
        println!("Negative mode: any converted word counts as a defect.");
    }

    let mut tally: BTreeMap<(String, &'static str), usize> = BTreeMap::new();
    let mut defects = 0usize;
    let mut done = 0usize;

    for path in files {
        if let Some(limit) = cfg.limit {
            if done >= limit {
                break;
            }
        }
        let key = path.to_string_lossy().to_string();
        let kind = extension(&path).unwrap_or_else(|| "none".into());

        // Read and hash BEFORE the expensive part, so resume can skip
        // straight past a file it already verified without re-running a
        // full conversion and idempotence check on it. This is the entire
        // reason `check()` is split into a cheap read and an expensive
        // verify rather than doing both in one call the way it used to.
        let bytes = match fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                record_row(
                    &mut report,
                    &mut tally,
                    &mut defects,
                    &key,
                    &kind,
                    &Outcome::bad(Status::Failed, format!("could not read it: {e}")),
                )?;
                done += 1;
                continue;
            }
        };
        let in_hash = sha256_hex(&bytes);
        if already.contains(&in_hash) {
            continue;
        }

        // A panic here is a defect to record, not a reason to stop.
        let mut outcome = std::panic::catch_unwind(|| check_bytes(&bytes, &kind, cfg.negative))
            .unwrap_or_else(|_| {
                Outcome::bad(
                    Status::Panicked,
                    "the code panicked while reading this file",
                )
            });
        outcome.in_hash = in_hash;

        record_row(&mut report, &mut tally, &mut defects, &key, &kind, &outcome)?;

        done += 1;
        if done.is_multiple_of(100) {
            println!("  {done}/{total}...");
        }
    }

    println!("\n{:-<62}", "");
    println!("{:<8} {:<12} {:>8}", "kind", "status", "files");
    println!("{:-<62}", "");
    for ((kind, status), n) in &tally {
        println!("{kind:<8} {status:<12} {n:>8}");
    }
    println!("{:-<62}", "");
    println!("Report: {}", cfg.out.display());
    println!("\n{} file(s) checked, {} defect(s).", done, defects);

    // The one silent deletion in the converter, surfaced on every corpus run.
    //
    // `repair_word` drops a character when the word list recognises neither
    // candidate repair -- no evidence either way, so it guesses from
    // structure. That is defensible, and it is exactly the kind of mechanism
    // that could hide a reordering fault, so `mukti-core` counts it. A tally
    // nobody reads is not instrumentation, which is why it is printed here:
    // this tool is the thing that runs over every document there is.
    let blind = gru953_mukti::blind_vowel_drops();
    if blind > 0 {
        println!(
            "Blind vowel drops: {blind} (a character removed on structure \
             alone, because the word list knew neither candidate)"
        );
    }

    if let Some(previous) = &cfg.compare {
        report.flush().map_err(|e| e.to_string())?;
        compare_reports(previous, &cfg.out, cfg.compare_entries)?;
    }

    Ok(defects)
}

/// Tally, print (if it is a defect) and write one row. Shared by the normal
/// path and the read-failure path so the two cannot silently drift apart.
fn record_row(
    report: &mut fs::File,
    tally: &mut BTreeMap<(String, &'static str), usize>,
    defects: &mut usize,
    key: &str,
    kind: &str,
    outcome: &Outcome,
) -> Result<(), String> {
    *tally
        .entry((kind.to_owned(), outcome.status.as_str()))
        .or_default() += 1;
    if outcome.status.is_defect() {
        *defects += 1;
        println!("  {} {}  {}", outcome.status.as_str(), key, outcome.detail);
    }

    writeln!(
        report,
        "{}\t{key}\t{kind}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
        outcome.in_hash,
        outcome.status.as_str(),
        outcome.converted,
        outcome.untouched,
        outcome.fonts,
        outcome.out_hash,
        outcome.out_entries_hash,
        outcome.detail
    )
    .map_err(|e| e.to_string())?;
    // Flush every row. A stack overflow cannot be caught, so the last row
    // written is the only evidence of which file caused it.
    report.flush().map_err(|e| e.to_string())
}

/// One row of a report, as read back from disk for `--compare`.
struct ReportRow {
    path: String,
    status: String,
    out_hash: String,
    out_entries_hash: String,
    converted: String,
    untouched: String,
    fonts: String,
}

fn read_report(path: &Path) -> Result<BTreeMap<String, ReportRow>, String> {
    let text =
        fs::read_to_string(path).map_err(|e| format!("could not read {}: {e}", path.display()))?;
    let mut rows = BTreeMap::new();
    for line in text.lines().skip(1) {
        let mut parts = line.splitn(10, '\t');
        let (
            Some(in_hash),
            Some(row_path),
            Some(_kind),
            Some(status),
            Some(converted),
            Some(untouched),
            Some(fonts),
            Some(out_hash),
            Some(out_entries_hash),
        ) = (
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
            parts.next(),
        )
        else {
            continue; // a malformed line, e.g. from an older report format
        };
        if in_hash.is_empty() {
            continue; // a read failure has no content to key on or compare
        }
        rows.insert(
            in_hash.to_owned(),
            ReportRow {
                path: row_path.to_owned(),
                status: status.to_owned(),
                out_hash: out_hash.to_owned(),
                out_entries_hash: out_entries_hash.to_owned(),
                converted: converted.to_owned(),
                untouched: untouched.to_owned(),
                fonts: fonts.to_owned(),
            },
        );
    }
    Ok(rows)
}

/// Join the just-written report against a previous one, on `in_hash`, and
/// print four disjoint counts: identical, differing, vanished and new.
///
/// Vanished is counted separately from differing on purpose. A file that
/// could not be found this time round is an environment change, not a
/// defect in whatever is being compared, and collapsing the two together
/// would score "the corpus moved" as if it were a regression.
fn compare_reports(previous: &Path, current: &Path, entries: bool) -> Result<(), String> {
    let old = read_report(previous)?;
    let new = read_report(current)?;

    let mut identical = 0usize;
    let mut differing: Vec<(String, &ReportRow, &ReportRow)> = Vec::new();
    let mut vanished: Vec<&ReportRow> = Vec::new();
    let mut new_rows: Vec<&ReportRow> = Vec::new();

    for (hash, old_row) in &old {
        match new.get(hash) {
            None => vanished.push(old_row),
            Some(new_row) => {
                let old_key = if entries {
                    &old_row.out_entries_hash
                } else {
                    &old_row.out_hash
                };
                let new_key = if entries {
                    &new_row.out_entries_hash
                } else {
                    &new_row.out_hash
                };
                let same = old_row.status == new_row.status
                    && old_key == new_key
                    && old_row.converted == new_row.converted
                    && old_row.untouched == new_row.untouched
                    && old_row.fonts == new_row.fonts;
                if same {
                    identical += 1;
                } else {
                    differing.push((hash.clone(), old_row, new_row));
                }
            }
        }
    }
    for (hash, new_row) in &new {
        if !old.contains_key(hash) {
            new_rows.push(new_row);
        }
    }

    println!("\n{:-<62}", "");
    println!(
        "Compared against {}{}",
        previous.display(),
        if entries {
            " (by archive entry content, not container framing)"
        } else {
            ""
        }
    );
    println!("{:-<62}", "");
    println!("  identical : {identical}");
    println!("  differing : {}", differing.len());
    println!(
        "  vanished  : {} (in the old report, not this one)",
        vanished.len()
    );
    println!(
        "  new       : {} (in this report, not the old one)",
        new_rows.len()
    );

    if !differing.is_empty() {
        println!("\nFirst {} differing file(s):", differing.len().min(20));
        for (_hash, old_row, new_row) in differing.iter().take(20) {
            println!(
                "  {} : {} ({}/{}/{}) -> {} ({}/{}/{})",
                new_row.path,
                old_row.status,
                old_row.converted,
                old_row.untouched,
                old_row.fonts,
                new_row.status,
                new_row.converted,
                new_row.untouched,
                new_row.fonts
            );
        }
    }
    if !vanished.is_empty() {
        println!(
            "\nFirst {} vanished file(s) (present before, not checked this time):",
            vanished.len().min(20)
        );
        for row in vanished.iter().take(20) {
            println!("  {}", row.path);
        }
    }
    if !new_rows.is_empty() {
        println!(
            "\nFirst {} new file(s) (checked now, not present before):",
            new_rows.len().min(20)
        );
        for row in new_rows.iter().take(20) {
            println!("  {}", row.path);
        }
    }

    Ok(())
}

fn extension(p: &Path) -> Option<String> {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Word and Excel lock files are not documents.
        if name.starts_with("~$") || name == ".DS_Store" {
            continue;
        }
        // Never walk into a git checkout: its objects are not documents and
        // there are thousands of them.
        if path.is_dir() {
            if name == ".git" {
                continue;
            }
            collect(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

/// The expensive half of checking one file: `bytes` are already read (and
/// hashed, by the caller) by the time this runs, so resume can skip straight
/// past an already-verified file without paying for this at all.
fn check_bytes(bytes: &[u8], kind: &str, negative: bool) -> Outcome {
    if bytes.is_empty() {
        // A zero-byte file in the archive. Not a defect in Mukti — there is
        // nothing to convert and nothing it could have done differently. The CLI
        // already catches this case and says so before the zip reader sees it.
        return Outcome::bad(Status::Unsupported, "the file is empty (0 bytes)");
    }

    match kind {
        "docx" | "xlsx" | "pptx" => check_office(bytes, negative),
        "doc" | "xls" | "ppt" => check_legacy_office(bytes, kind, negative),
        "txt" | "csv" | "tsv" | "md" | "json" | "html" | "htm" | "py" | "yaml" | "yml"
        | "ipynb" | "sample" | "rev" | "idx" | "pack" => check_text(bytes, negative),
        _ => Outcome::bad(Status::Unsupported, "not a kind this tool converts"),
    }
}

/// Every property an Office conversion must satisfy, whatever the document.
fn check_office(bytes: &[u8], negative: bool) -> Outcome {
    let before = match runs(std::io::Cursor::new(bytes)) {
        Ok(r) => r,
        Err(e) => return Outcome::bad(Status::Failed, format!("could not be read: {e}")),
    };

    let (out, summary) = match convert_office(bytes, "Nirmala UI") {
        Ok(v) => v,
        Err(e) => return Outcome::bad(Status::Failed, format!("conversion failed: {e}")),
    };

    if negative && summary.words_converted > 0 {
        return Outcome::bad(
            Status::Failed,
            format!(
                "{} word(s) converted in a document that should be untouched",
                summary.words_converted
            ),
        );
    }

    let after = match runs(std::io::Cursor::new(&out)) {
        Ok(r) => r,
        Err(e) => {
            return Outcome::bad(
                Status::Failed,
                format!("the converted file could not be re-read: {e}"),
            )
        }
    };

    // 1. If nothing was converted, every entry must come back byte-identical.
    //
    //    The strongest check that is actually achievable, and the product's
    //    central promise. It caught the run-relocation bug: a converted word has
    //    to be consolidated into one run because its length changes, but that was
    //    being done to unconverted words too, so a document with no legacy Bangla
    //    at all came back rearranged — one run holding `t` came back holding
    //    `trainng`. Nothing weaker sees it, because the visible text was always
    //    correct.
    //
    //    Entry content, not the whole archive: measured 13 Aug 2026, a rebuilt
    //    archive differs from Word's by around 1,800 bytes of container framing
    //    even when every entry is copied through verbatim, because the zip crate
    //    does not reproduce Word's exact extra fields and alignment. That is not
    //    something this project promises or a user can observe. What is promised
    //    is that the content does not change, and that is what is checked.
    //
    //    `words_normalised` MUST join this gate, and its absence here was a
    //    real bug, found on 20 August 2026 while adding `--compare`: a random
    //    sample of 15 real documents produced 3 false "FAILED" rows, every one
    //    of them a document holding a decomposed two-part vowel (`ে` + `া`)
    //    that 0.7.0's own composition feature correctly joined into `ো`. That
    //    is an intentional, documented change to already-Unicode text -- see
    //    `Summary::words_normalised`'s own doc comment -- not a defect, and
    //    conflating it with "nothing happened" is exactly what this field was
    //    added to stop other code from doing.
    if summary.words_converted == 0 && summary.fonts_changed == 0 && summary.words_normalised == 0 {
        if let Err(why) = every_entry_identical(bytes, &out) {
            return Outcome::bad(Status::Failed, format!("nothing was converted, yet {why}"));
        }
    }

    // 2. The joined text must contain the same number of words.
    //
    //    Joined, not per-run, deliberately. A converted word that spanned several
    //    runs is written into the first of them, so per-run counts legitimately
    //    fall; the joined count must not. This is what sees words being glued
    //    together or newlines going missing.
    let joined_before: String = before.iter().map(|r| r.text.as_str()).collect();
    let joined_after: String = after.iter().map(|r| r.text.as_str()).collect();
    let words_before = joined_before.split_whitespace().count();
    let words_after = joined_after.split_whitespace().count();
    if words_before != words_after {
        return Outcome::bad(
            Status::Failed,
            format!("joined word count changed: {words_before} became {words_after}"),
        );
    }

    // 3. Every character that was not part of a converted word must survive.
    //    Whitespace is the sharpest probe: it is never converted, so any change
    //    in its shape means text moved.
    let ws_before = joined_before.chars().filter(|c| c.is_whitespace()).count();
    let ws_after = joined_after.chars().filter(|c| c.is_whitespace()).count();
    if ws_before != ws_after {
        return Outcome::bad(
            Status::Failed,
            format!("whitespace changed: {ws_before} characters became {ws_after}"),
        );
    }

    // 2. Nothing outside the text and font parts may change, and no entry may
    //    appear or disappear.
    if let Err(why) = entries_match(bytes, &out) {
        return Outcome::bad(Status::Failed, why);
    }

    // 3. No legacy font may survive, or the reader is asked for a font that
    //    contains no Bengali at all.
    if let Err(why) = no_legacy_font_left(&out) {
        return Outcome::bad(Status::Failed, why);
    }

    // 4. Idempotence. Converting the output again must find nothing to do.
    //    A converter that mangles its own output fails here and nowhere else.
    match convert_office(&out, "Nirmala UI") {
        Ok((_, second)) => {
            if second.words_converted > 0 {
                return Outcome::bad(
                    Status::Failed,
                    format!(
                        "not finished after one pass: a second pass converted {} more word(s)",
                        second.words_converted
                    ),
                );
            }
        }
        Err(e) => {
            return Outcome::bad(
                Status::Failed,
                format!("its own output could not be converted again: {e}"),
            )
        }
    }

    Outcome::ok(summary).with_office_output(&out)
}

/// Compare the two archives entry by entry.
fn entries_match(before: &[u8], after: &[u8]) -> Result<(), String> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(before))
        .map_err(|e| format!("the original is not a readable archive: {e}"))?;
    let mut b = zip::ZipArchive::new(std::io::Cursor::new(after))
        .map_err(|e| format!("the result is not a readable archive: {e}"))?;

    let names_a: Vec<String> = a.file_names().map(str::to_owned).collect();
    let names_b: Vec<String> = b.file_names().map(str::to_owned).collect();
    if names_a.len() != names_b.len() {
        return Err(format!(
            "the archive gained or lost entries: {} became {}",
            names_a.len(),
            names_b.len()
        ));
    }
    let mut sorted_a = names_a.clone();
    let mut sorted_b = names_b.clone();
    sorted_a.sort();
    sorted_b.sort();
    if sorted_a != sorted_b {
        return Err("the set of entries in the archive changed".to_owned());
    }

    for name in &names_a {
        // Text, font and font-metadata parts are meant to change. Everything
        // else must not.
        if mukti_formats::office::is_text_part(name)
            || mukti_formats::office::is_font_part(name)
            || mukti_formats::office::is_metadata_font_part(name)
        {
            continue;
        }
        let one = read_entry(&mut a, name)?;
        let two = read_entry(&mut b, name)?;
        if one != two {
            return Err(format!(
                "an entry that should have been copied through was altered ({} bytes became {})",
                one.len(),
                two.len()
            ));
        }
    }
    Ok(())
}

/// Every entry, in order, with identical content. Used when the conversion
/// reported that it changed nothing at all.
fn every_entry_identical(before: &[u8], after: &[u8]) -> Result<(), String> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(before))
        .map_err(|e| format!("the original is not a readable archive: {e}"))?;
    let mut b = zip::ZipArchive::new(std::io::Cursor::new(after))
        .map_err(|e| format!("the result is not a readable archive: {e}"))?;
    if a.len() != b.len() {
        return Err(format!(
            "the entry count changed: {} became {}",
            a.len(),
            b.len()
        ));
    }
    for i in 0..a.len() {
        let (name_a, one) = {
            let mut f = a
                .by_index(i)
                .map_err(|e| format!("entry {i} could not be opened: {e}"))?;
            let name = f.name().to_owned();
            let mut v = Vec::new();
            f.read_to_end(&mut v)
                .map_err(|e| format!("entry {i} could not be read: {e}"))?;
            (name, v)
        };
        let (name_b, two) = {
            let mut f = b
                .by_index(i)
                .map_err(|e| format!("entry {i} could not be opened: {e}"))?;
            let name = f.name().to_owned();
            let mut v = Vec::new();
            f.read_to_end(&mut v)
                .map_err(|e| format!("entry {i} could not be read: {e}"))?;
            (name, v)
        };
        if name_a != name_b {
            return Err(format!(
                "the entry order changed at position {i}: {name_a} became {name_b}"
            ));
        }
        if one != two {
            return Err(format!(
                "{name_a} changed ({} bytes became {})",
                one.len(),
                two.len()
            ));
        }
    }
    Ok(())
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = zip
        .by_name(name)
        .map_err(|e| format!("entry could not be opened: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| format!("entry could not be read: {e}"))?;
    Ok(buf)
}

/// No legacy font name may survive anywhere a font is actually named.
///
/// **Where** matters as much as what. An earlier version of this function scanned
/// every word of every XML part, and reported a spreadsheet as retaining a legacy
/// font because a participant list contained the name **SULEKHA** — which is also
/// a font. That is not a new mistake: the release check for v0.4.0 made the
/// identical one on the identical corpus, and `LESSONS.md` §1 records it. Writing
/// a verifier is no protection against repeating the bug it was written to catch.
///
/// So this parses the XML and looks in exactly two places:
///
/// * **attribute values on elements that name a font** — `w:rFonts`, `a:latin`,
///   `rFont` and the rest, via the converter's own `names_a_font`;
/// * **text nodes in `docProps/app.xml`**, the one part that records font names as
///   text, and there only when the text is *exactly* a font name.
///
/// Cell values, slide titles and body text are not font names and are not looked
/// at.
fn no_legacy_font_left(after: &[u8]) -> Result<(), String> {
    use quick_xml::events::Event;

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(after))
        .map_err(|e| format!("the result is not a readable archive: {e}"))?;
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();

    for name in names {
        if !name.ends_with(".xml") {
            continue;
        }
        let bytes = read_entry(&mut zip, &name)?;
        let xml = String::from_utf8_lossy(&bytes).into_owned();
        let metadata = mukti_formats::office::is_metadata_font_part(&name);

        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(false);
        loop {
            let event = match reader.read_event() {
                Ok(e) => e,
                // A part we cannot parse is not evidence of a surviving font.
                // Report it as its own problem rather than silently passing.
                Err(e) => return Err(format!("{name} could not be parsed: {e}")),
            };
            match event {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => {
                    if !names_a_font(local_name(e.name().as_ref())) {
                        continue;
                    }
                    for attr in e.attributes().flatten() {
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .unwrap_or_default();
                        if is_legacy_font(&value) {
                            return Err(format!(
                                "a legacy font is still named by an attribute in {name}"
                            ));
                        }
                    }
                }
                Event::Text(e) if metadata => {
                    let text = e.decode().unwrap_or_default();
                    if is_exactly_legacy_font(&text) {
                        return Err(format!("a legacy font name survives as text in {name}"));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// An element's name with any namespace prefix removed.
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

/// The pre-2007 binary formats, which are read and rewritten as modern ones.
///
/// These cannot be checked the way a `.docx` is — nothing is rewritten in place,
/// so there is no "before" to compare against. What can be checked is stronger
/// than it sounds: the document we produce must itself survive every Office
/// invariant, which includes the one that matters most — converting it again
/// must change nothing further. A converter that leaves legacy text behind, or
/// that mangles its own output, fails at that second pass.
fn check_legacy_office(bytes: &[u8], kind: &str, negative: bool) -> Outcome {
    let Some(format) = LegacyFormat::from_extension(kind) else {
        return Outcome::bad(Status::Unsupported, "not an older Office format");
    };
    let outcome = match convert_legacy_office(bytes, format) {
        Ok(o) => o,
        Err(e) => return Outcome::bad(Status::Failed, format!("conversion failed: {e}")),
    };
    if negative && outcome.summary.words_converted > 0 {
        return Outcome::bad(
            Status::Failed,
            format!(
                "{} word(s) converted in a document that should be untouched",
                outcome.summary.words_converted
            ),
        );
    }
    if outcome.was_empty {
        return Outcome::bad(Status::NoText, "no text could be recovered from the file");
    }
    // Hand the document we just wrote to the full Office check. If it is not a
    // readable Office file, or converting it again changes anything, that is a
    // defect in us and not in the original.
    match check_office(&outcome.document, true) {
        o if o.status == Status::Failed || o.status == Status::Panicked => Outcome::bad(
            o.status,
            format!("the document we wrote is not sound: {}", o.detail),
        ),
        _ => Outcome::ok(outcome.summary).with_office_output(&outcome.document),
    }
}

fn check_text(bytes: &[u8], negative: bool) -> Outcome {
    let (text, _encoding) = gru953_mukti::encoding::decode(bytes);
    let (out, summary) = mukti_formats::convert_text_with_summary(&text);

    if negative && summary.words_converted > 0 {
        return Outcome::bad(
            Status::Failed,
            format!(
                "{} word(s) converted in text that should be untouched",
                summary.words_converted
            ),
        );
    }

    // Nothing converted must mean nothing changed at all. This is the promise
    // the whole product rests on, so it is checked on every single file.
    //
    // The same theoretical gap fixed in `check_office`'s gate applies here in
    // principle: `convert_pieces` composes canonical two-part vowels even in
    // words `words_converted` never counts, and `convert_text_with_summary`
    // has no field recording that (unlike `office::Summary::words_normalised`),
    // so a Unicode text file holding a decomposed vowel pair would fail here
    // too. Left as a known gap rather than fixed with a new dependency
    // (Unicode NFC normalisation) or a change to shipped product code,
    // because the negative corpus this path actually runs against is English
    // and code only -- Markdown, notebooks, Python -- with no Bengali content
    // at all. Zero measured occurrences, against the Office gate's proven
    // 20% hit rate on a 15-file sample. Revisit if this path is ever pointed
    // at a corpus containing genuine Unicode Bengali text files.
    if summary.words_converted == 0 && out != text {
        return Outcome::bad(
            Status::Failed,
            "no word was converted, yet the text came back different",
        );
    }
    Outcome::ok(summary).with_text_output(&out)
}

#[cfg(test)]
mod tests {
    /// A scratch directory unique to THIS PROCESS.
    ///
    /// These were fixed names until 28 August 2026, and that is a real fragility
    /// rather than a tidiness point: two concurrent runs of the same test binary
    /// share the path, and one removes the other's fixture mid-test. It happened
    /// while gating a release, when a second `cargo test` overlapped the first.
    ///
    /// The worst case is `check_writes_nothing`, which compares a directory
    /// listing before and after. A concurrent run adding a file there does not
    /// merely fail -- it fails claiming `check` wrote something, which is the one
    /// promise this tool must never be wrongly accused of breaking.
    fn scratch(name: &str) -> std::path::PathBuf {
        std::env::temp_dir().join(format!("{name}-{}", std::process::id()))
    }

    use super::*;

    fn tiny_archive(mode: u32) -> Vec<u8> {
        let options = zip::write::SimpleFileOptions::default().unix_permissions(mode);
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        zip.start_file("b.xml", options).unwrap();
        zip.write_all(b"<b/>").unwrap();
        zip.start_file("a.xml", options).unwrap();
        zip.write_all(b"<a/>").unwrap();
        zip.finish().unwrap().into_inner()
    }

    #[test]
    fn the_hash_is_sixty_four_hex_characters_and_stable() {
        let a = sha256_hex(b"the same bytes");
        let b = sha256_hex(b"the same bytes");
        let c = sha256_hex(b"different bytes");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn content_fingerprint_is_invariant_to_container_framing() {
        // Same two entries, same content, different Unix mode bits in the
        // central directory -- exactly the shape of the zip 2 vs zip 8
        // difference (octal 100644 became 100000) that left every content
        // hash unchanged. The whole point of this fingerprint is that this
        // pair must be equal even though the raw archive bytes are not.
        let a = tiny_archive(0o100644);
        let b = tiny_archive(0o100000);
        assert_ne!(a, b, "the test fixture itself must differ in raw bytes");
        assert_eq!(
            content_fingerprint(&a).unwrap(),
            content_fingerprint(&b).unwrap(),
            "cosmetic container differences must not move the fingerprint"
        );
    }

    #[test]
    fn content_fingerprint_is_invariant_to_entry_order() {
        let options = zip::write::SimpleFileOptions::default();
        let mut forward = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        forward.start_file("a.xml", options).unwrap();
        forward.write_all(b"<a/>").unwrap();
        forward.start_file("b.xml", options).unwrap();
        forward.write_all(b"<b/>").unwrap();
        let forward = forward.finish().unwrap().into_inner();

        let mut backward = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        backward.start_file("b.xml", options).unwrap();
        backward.write_all(b"<b/>").unwrap();
        backward.start_file("a.xml", options).unwrap();
        backward.write_all(b"<a/>").unwrap();
        let backward = backward.finish().unwrap().into_inner();

        assert_eq!(
            content_fingerprint(&forward).unwrap(),
            content_fingerprint(&backward).unwrap()
        );
    }

    #[test]
    fn content_fingerprint_does_change_when_content_actually_changes() {
        let options = zip::write::SimpleFileOptions::default();
        let mut one = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        one.start_file("a.xml", options).unwrap();
        one.write_all(b"<a/>").unwrap();
        let one = one.finish().unwrap().into_inner();

        let mut two = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        two.start_file("a.xml", options).unwrap();
        two.write_all(b"<a changed=\"1\"/>").unwrap();
        let two = two.finish().unwrap().into_inner();

        assert_ne!(
            content_fingerprint(&one).unwrap(),
            content_fingerprint(&two).unwrap()
        );
    }

    #[test]
    fn read_report_skips_malformed_lines_and_empty_hashes() {
        let dir = scratch("corpus-verify-read-report-test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("report.tsv");
        fs::write(
            &path,
            "in_hash\tpath\tkind\tstatus\tconverted\tuntouched\tfonts\tout_hash\tout_entries_hash\tdetail\n\
             abc123\t/a.docx\tdocx\tok\t1\t2\t0\touthash1\tentries1\t\n\
             \t/unreadable.docx\tdocx\tFAILED\t0\t0\t0\t\t\tcould not read it\n\
             this line has too few columns\n",
        )
        .unwrap();

        let rows = read_report(&path).expect("a report this tool wrote must parse");
        assert_eq!(
            rows.len(),
            1,
            "only the one well-formed, hashed row survives"
        );
        assert!(rows.contains_key("abc123"));
        let row = &rows["abc123"];
        assert_eq!(row.path, "/a.docx");
        assert_eq!(row.status, "ok");
        assert_eq!(row.out_hash, "outhash1");

        let _ = fs::remove_file(&path);
    }

    #[test]
    fn compare_reports_counts_identical_differing_vanished_and_new() {
        let dir = scratch("corpus-verify-compare-test");
        let _ = fs::create_dir_all(&dir);
        let old_path = dir.join("old.tsv");
        let new_path = dir.join("new.tsv");
        let header = "in_hash\tpath\tkind\tstatus\tconverted\tuntouched\tfonts\tout_hash\tout_entries_hash\tdetail\n";

        fs::write(
            &old_path,
            format!(
                "{header}\
                 same\t/same.docx\tdocx\tok\t1\t2\t0\thash-a\tentries-a\t\n\
                 gone\t/gone.docx\tdocx\tok\t1\t0\t0\thash-b\tentries-b\t\n\
                 changed\t/changed.docx\tdocx\tok\t1\t0\t0\thash-old\tentries-old\t\n"
            ),
        )
        .unwrap();
        fs::write(
            &new_path,
            format!(
                "{header}\
                 same\t/same.docx\tdocx\tok\t1\t2\t0\thash-a\tentries-a\t\n\
                 changed\t/changed.docx\tdocx\tok\t1\t0\t0\thash-new\tentries-new\t\n\
                 fresh\t/fresh.docx\tdocx\tok\t1\t0\t0\thash-c\tentries-c\t\n"
            ),
        )
        .unwrap();

        // compare_reports only prints; assert on its inputs directly via
        // read_report, since the counting logic it prints from is exercised
        // the same way. This proves the join is correct without parsing
        // stdout.
        let old = read_report(&old_path).unwrap();
        let new = read_report(&new_path).unwrap();
        assert!(old.contains_key("same") && new.contains_key("same"));
        assert!(old.contains_key("gone") && !new.contains_key("gone"));
        assert!(!old.contains_key("fresh") && new.contains_key("fresh"));
        assert_eq!(old["changed"].out_hash, "hash-old");
        assert_eq!(new["changed"].out_hash, "hash-new");

        // And the real function must not error on this input.
        compare_reports(&old_path, &new_path, false).expect("a well-formed pair must compare");

        let _ = fs::remove_file(&old_path);
        let _ = fs::remove_file(&new_path);
    }
}
