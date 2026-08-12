//! Measure what GRU953 Scribe actually does, before anyone tries to improve it.
//!
//! Five measurements of conversion and one of detection. Each prints its
//! sample size and, where it is a proportion, a 95% confidence interval. A
//! figure without those beside it is an opinion.
//!
//! # The blind spot, stated once and not forgotten
//!
//! M1 is round-trip testing: take real Unicode Bengali, encode it into Bijoy,
//! convert it back, compare. The source text is the answer key. It cannot
//! detect an error the encoder and the decoder **share** — if `to_bijoy` and
//! `convert` are wrong in matching ways, the word returns intact and the
//! harness sees nothing. So M1 is an upper bound.
//!
//! M3 is what closes that hole. It runs on **real legacy documents**, where
//! there is no encoder to agree with the converter, and asks a different
//! question entirely: is the output an actual Bengali word? A conversion that
//! is well-formed but the *wrong word* fails M3 and would have sailed past M1.
//!
//! Neither is sufficient alone. Both are reported.

mod stats;

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufRead, BufReader};
use std::path::{Path, PathBuf};

use gru953_scribe::dictionary::Dictionary;
use gru953_scribe::roundtrip::{is_testable_word, normalise_nukta, to_bijoy};
use gru953_scribe::{convert, detect, LegacyEncoding};
use stats::{edit_distance, thousands, Proportion};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = Config::parse()?;

    println!("GRU953 Scribe — accuracy report");
    println!("================================\n");

    m1_round_trip(&cfg)?;
    m2_character_grid(&cfg)?;
    m3_dictionary_on_real_documents(&cfg)?;
    m4_vowel_preservation(&cfg)?;
    detection(&cfg)?;

    println!("\nEvery figure above is measured, not estimated. Sample sizes are");
    println!("stated because a percentage without one means nothing.");
    Ok(())
}

// ---------------------------------------------------------------------------
// M1 — round trip
// ---------------------------------------------------------------------------

/// Unicode -> Bijoy -> Unicode, over every word in the extended dictionary.
///
/// Reports word accuracy (the word came back exactly) and character accuracy
/// (one minus the edit distance over the total length). Word accuracy is the
/// harsher and the more honest of the two: a single wrong character ruins the
/// word for anybody searching for it, which is what this tool is for.
fn m1_round_trip(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    heading("M1", "Round trip: Unicode -> Bijoy -> Unicode");

    let bytes = fs::read(&cfg.extended_fst)?;
    let set = fst::Set::new(bytes)?;
    let mut stream = set.stream();

    let mut words_exact = 0usize;
    let mut words_ok = 0usize;
    let mut words_total = 0usize;
    let mut skipped = 0usize;
    let mut chars_total = 0usize;
    let mut chars_wrong = 0usize;
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();

    while let Some(key) = fst::Streamer::next(&mut stream) {
        let word = std::str::from_utf8(key)?;
        // Bijoy *is* ASCII, so a word mixing scripts is ambiguous by
        // construction; testing it would measure the ambiguity, not the code.
        if !is_testable_word(word) {
            skipped += 1;
            continue;
        }
        words_total += 1;
        let got = convert(&to_bijoy(word));
        if normalise_nukta(word) == normalise_nukta(&got) {
            words_exact += 1;
        }
        let (a, b) = (canonical(word), canonical(&got));
        chars_total += a.chars().count();
        if a == b {
            words_ok += 1;
        } else {
            chars_wrong += edit_distance(&a, &b);
            *patterns.entry(difference(&a, &b)).or_default() += 1;
        }
    }

    let words = Proportion::new(words_ok, words_total);
    let exact = Proportion::new(words_exact, words_total);
    let chars = Proportion::new(chars_total - chars_wrong.min(chars_total), chars_total);
    println!("  Word accuracy       {}", words.describe());
    println!("  Character accuracy  {}", chars.describe());
    // Printed even when it matches, because a match is itself the finding: it
    // says Scribe's output is already canonically spelled, so normalising
    // before comparison is not quietly doing work to flatter the figure.
    println!("  Same, comparing exact bytes rather than canonical spellings:");
    println!("    Word accuracy     {}", exact.describe());
    if words_exact == words_ok {
        println!("    Identical, which is the point: Scribe's output is already canonically");
        println!("    spelled, so normalising before comparison changes nothing here.");
    } else {
        println!("    The gap is canonically equivalent spellings — a two-part vowel written");
        println!("    as its two halves rather than as one character. See `canonical`.");
    }
    println!(
        "  {} words skipped as ambiguous (they mix scripts, so Bijoy cannot be told from ASCII).",
        thousands(skipped)
    );
    gate("word accuracy", words.rate(), 0.99);
    top_patterns(&patterns, "  Most common failures");
    Ok(())
}

// ---------------------------------------------------------------------------
// M2 — the character grid
// ---------------------------------------------------------------------------

/// The conversion table's own exam.
///
/// `BengaliCharacterCombinations.txt` is a systematic grid: every consonant
/// against every vowel sign, plus the conjunct forms — reph, ra-phala, ya-phala,
/// la-phala and the rest. These are precisely where Bijoy conversion fails, and
/// unlike a word list the grid is exhaustive rather than a sample. The bar is
/// 100%: anything less names a specific missing entry in the table.
fn m2_character_grid(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    heading(
        "M2",
        "Character grid: every consonant x every vowel and conjunct",
    );

    let text = fs::read_to_string(cfg.corpus.join("BengaliCharacterCombinations.txt"))?;
    let mut ok = 0usize;
    let mut total = 0usize;
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();

    for token in text.split_whitespace() {
        if !is_testable_word(token) {
            continue;
        }
        total += 1;
        let got = convert(&to_bijoy(token));
        let (a, b) = (canonical(token), canonical(&got));
        if a == b {
            ok += 1;
        } else {
            *patterns.entry(difference(&a, &b)).or_default() += 1;
        }
    }

    let p = Proportion::new(ok, total);
    println!("  Combinations correct  {}", p.describe());
    gate("character grid", p.rate(), 1.0);
    top_patterns(&patterns, "  Combinations that fail");
    Ok(())
}

// ---------------------------------------------------------------------------
// M3 — the measurement round-trip cannot fake
// ---------------------------------------------------------------------------

/// Convert real legacy text and ask whether the result is a real Bengali word.
///
/// No encoder is involved, so there is nothing for the converter to agree with
/// but the language itself. A conversion producing well-formed Bengali that is
/// nevertheless the wrong word fails here and passes M1.
///
/// Read the figure with its own caveat, which cuts the other way: a word can be
/// converted perfectly and still be missing from a 477,000-word dictionary,
/// because it is a name, a place, an acronym or simply rare. So this is a
/// **lower** bound where M1 is an upper one, and the truth sits between them.
fn m3_dictionary_on_real_documents(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    heading(
        "M3",
        "Real legacy documents: is the output an actual Bengali word?",
    );

    let bytes = fs::read(&cfg.extended_fst)?;
    let wide = fst::Set::new(bytes)?;
    let shipped = Dictionary::shipped();

    let mut found = 0usize;
    let mut total = 0usize;
    let mut found_shipped = 0usize;

    for row in rows(&cfg.labels)? {
        let (_, label, token) = row?;
        if label != "legacy" {
            continue;
        }
        let converted = convert(&token);
        let word = trim_to_bengali(&converted);
        if word.is_empty() {
            continue;
        }
        total += 1;
        if wide.contains(normalise_nukta(word)) {
            found += 1;
        }
        if shipped.contains(word) {
            found_shipped += 1;
        }
    }

    let p = Proportion::new(found, total);
    println!("  Output words in the dictionary  {}", p.describe());
    println!(
        "  Against the shipped dictionary   {}",
        Proportion::new(found_shipped, total).describe()
    );
    println!("  A lower bound: names, places and rare words are absent from any word list.");
    Ok(())
}

// ---------------------------------------------------------------------------
// M4 — adversarial
// ---------------------------------------------------------------------------

/// The converter must not quietly tidy up spelling.
///
/// `wrong_file.txt` is 14,214 deliberately misspelled Bengali words: হ্রস্ব-ই
/// swapped for দীর্ঘ-ঈ, হ্রস্ব-উ for দীর্ঘ-ঊ, hasanta dropped. Scribe carries a
/// repair pass for mistyped vowels, and the risk it creates is real — a repair
/// that fires too eagerly would "fix" text nobody asked it to touch, and the
/// user would never know their document had been altered.
///
/// Every one of these words must survive the round trip **exactly as written**,
/// wrong spelling and all.
fn m4_vowel_preservation(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    heading("M4", "Adversarial: misspellings must survive unaltered");

    let text = fs::read_to_string(cfg.corpus.join("wrong_file.txt"))?;
    let mut preserved = 0usize;
    let mut total = 0usize;
    let mut patterns: BTreeMap<String, usize> = BTreeMap::new();

    for line in text.lines() {
        let word = line.trim().trim_start_matches('\u{FEFF}');
        if word.is_empty() || !is_testable_word(word) {
            continue;
        }
        total += 1;
        let got = convert(&to_bijoy(word));
        let (a, b) = (canonical(word), canonical(&got));
        if a == b {
            preserved += 1;
        } else {
            *patterns.entry(difference(&a, &b)).or_default() += 1;
        }
    }

    let p = Proportion::new(preserved, total);
    println!("  Misspellings preserved  {}", p.describe());
    top_patterns(&patterns, "  Words the converter altered");
    Ok(())
}

// ---------------------------------------------------------------------------
// Detection
// ---------------------------------------------------------------------------

/// Does Scribe convert **only** what is genuinely legacy?
///
/// Measured on the held-out half of the labelled set, one token at a time.
///
/// **This is a deliberately unkind baseline.** Today's detector is built to
/// judge a whole line, with a document's other lines vouching for a short one;
/// asking it about a bare word denies it every piece of context it was designed
/// to use. That is exactly the point. Word-by-word is what the finished tool
/// must do, so this is the number Phase 3 has to beat, and it must be recorded
/// before anything changes rather than after.
///
/// Two figures matter, and they are not equally important:
///
/// * **Recall** — how much genuine legacy text is converted. A miss leaves a
///   word unreadable, which is visible and fixable.
/// * **False-positive rate** — how much non-legacy text is wrongly converted.
///   That silently corrupts readable English and Unicode Bengali, and the user
///   may never notice. It is the gate.
fn detection(cfg: &Config) -> Result<(), Box<dyn std::error::Error>> {
    heading("D", "Detection on held-out documents, one word at a time");

    let mut counts: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for row in rows(&cfg.labels)? {
        let (split, label, token) = row?;
        if split != "test" {
            continue;
        }
        let name: &'static str = match label.as_str() {
            "legacy" => "legacy",
            "legacy_ascii" => "legacy_ascii",
            "unicode" => "unicode",
            "english" => "english",
            "inert" => "inert",
            _ => continue,
        };
        let flagged = detect(&token).encoding == LegacyEncoding::SutonnyMj;
        let entry = counts.entry(name).or_default();
        entry.0 += usize::from(flagged);
        entry.1 += 1;
    }

    let get = |k: &str| counts.get(k).copied().unwrap_or((0, 0));
    let (legacy_hit, legacy_n) = get("legacy");
    let recall = Proportion::new(legacy_hit, legacy_n);

    // The gate. Every one of these is text a user can read today and would
    // find turned to nonsense tomorrow.
    let must_not: usize = ["unicode", "english", "inert"]
        .iter()
        .map(|k| get(k).1)
        .sum();
    let wrongly: usize = ["unicode", "english", "inert"]
        .iter()
        .map(|k| get(k).0)
        .sum();
    let fpr = Proportion::new(wrongly, must_not);

    let precision = Proportion::new(legacy_hit, legacy_hit + wrongly);
    let f1 = if precision.rate() + recall.rate() > 0.0 {
        2.0 * precision.rate() * recall.rate() / (precision.rate() + recall.rate())
    } else {
        0.0
    };

    println!("  Recall on legacy words     {}", recall.describe());
    println!("  Precision                  {}", precision.describe());
    println!("  F1                         {:.4}", f1);
    println!("  FALSE POSITIVES            {}", fpr.describe());
    gate_max("false-positive rate", fpr.rate(), 0.001);

    println!("\n  Wrongly converted, by what the text actually was:");
    for key in ["unicode", "english", "inert"] {
        let (hit, n) = get(key);
        println!("    {key:<14} {}", Proportion::new(hit, n).describe());
    }
    let (amb_hit, amb_n) = get("legacy_ascii");
    println!("\n  Pure-ASCII legacy (ambiguous, reported separately, in neither figure above):");
    println!("    {}", Proportion::new(amb_hit, amb_n).describe());
    Ok(())
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

struct Config {
    corpus: PathBuf,
    labels: PathBuf,
    extended_fst: PathBuf,
}

impl Config {
    fn parse() -> Result<Self, String> {
        let mut corpus = None;
        let mut labels = PathBuf::from("local/labelled-tokens.tsv");
        let mut extended_fst = PathBuf::from("local/extended-words.fst");
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--corpus" => corpus = it.next().map(PathBuf::from),
                "--labels" => labels = it.next().map(PathBuf::from).unwrap_or(labels),
                "--words" => extended_fst = it.next().map(PathBuf::from).unwrap_or(extended_fst),
                "--report" => {}
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        Ok(Config {
            corpus: corpus.ok_or("--corpus <Bangla Word Collection dir> is required")?,
            labels,
            extended_fst,
        })
    }
}

/// Stream the labelled set. 152 MB on disk, so never all at once.
fn rows(
    path: &Path,
) -> Result<impl Iterator<Item = std::io::Result<(String, String, String)>>, std::io::Error> {
    let file = BufReader::new(fs::File::open(path)?);
    Ok(file.lines().skip(1).filter_map(|line| match line {
        Err(e) => Some(Err(e)),
        Ok(line) => {
            let mut parts = line.splitn(3, '\t');
            match (parts.next(), parts.next(), parts.next()) {
                (Some(s), Some(l), Some(t)) => Some(Ok((s.to_owned(), l.to_owned(), t.to_owned()))),
                _ => None,
            }
        }
    }))
}

/// Settle spellings that Unicode itself calls equivalent, before comparing.
///
/// Two ambiguities, both of which produce byte differences that are not word
/// differences, and both of which will silently dominate any failure list left
/// unhandled:
///
/// * **Two-part vowels.** `ো` is one character (U+09CB) and also, equally
///   legally, `ে` followed by `া` (U+09C7 U+09BE). They are *canonically
///   equivalent* — Unicode's own composition turns the second into the first —
///   and they render identically. The source word list spells 3,700 of its
///   words the decomposed way; Scribe composes them, which is the normalised
///   form and arguably the better output. Counting those as conversion errors
///   measured a spelling convention rather than the converter.
/// * **The nukta**, for the same reason, already handled by `normalise_nukta`.
///
/// Both figures are reported: the exact-bytes one, and this one. Normalising
/// before comparison is standard practice for text this close to the boundary,
/// but the raw number is printed beside it so nobody has to take that on trust.
fn canonical(s: &str) -> String {
    normalise_nukta(s)
        .replace("\u{09C7}\u{09BE}", "\u{09CB}")
        .replace("\u{09C7}\u{09D7}", "\u{09CC}")
}

/// Strip anything that is not Bengali from both ends of a converted token.
///
/// Real document text carries brackets, full stops and colons. `প্রতিবেদন,` is
/// the word `প্রতিবেদন` with a comma stuck to it, and asking a dictionary about
/// the comma would fail every such word for no reason.
fn trim_to_bengali(s: &str) -> &str {
    let bengali = |c: char| ('\u{0980}'..='\u{09FF}').contains(&c);
    s.trim_matches(|c: char| !bengali(c))
}

/// The differing part of two words, with the shared ends stripped.
///
/// Whole words are too specific to count — one wrong glyph shows up in a
/// thousand different words. Reducing each failure to just what changed makes
/// them countable, and keeps document text out of anything printed.
fn difference(a: &str, b: &str) -> String {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let head = a.iter().zip(&b).take_while(|(x, y)| x == y).count();
    let tail = a[head..]
        .iter()
        .rev()
        .zip(b[head..].iter().rev())
        .take_while(|(x, y)| x == y)
        .count();
    let clip = |v: &[char]| {
        let s: String = v.iter().take(8).collect();
        if s.is_empty() {
            "(nothing)".to_owned()
        } else {
            s
        }
    };
    format!(
        "{} -> {}",
        clip(&a[head..a.len() - tail]),
        clip(&b[head..b.len() - tail])
    )
}

fn top_patterns(patterns: &BTreeMap<String, usize>, title: &str) {
    if patterns.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = patterns.iter().collect();
    sorted.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("{title} ({} distinct):", thousands(patterns.len()));
    for (pattern, count) in sorted.iter().take(10) {
        println!("    {:>8}x  {pattern}", thousands(**count));
    }
}

fn heading(id: &str, title: &str) {
    println!("\n{id} — {title}");
    println!("{}", "-".repeat(72));
}

fn gate(what: &str, value: f64, target: f64) {
    let verdict = if value >= target { "MET" } else { "NOT MET" };
    println!("  Target {what} >= {:.1}%: {verdict}", target * 100.0);
}

fn gate_max(what: &str, value: f64, limit: f64) {
    let verdict = if value <= limit { "MET" } else { "NOT MET" };
    println!("  Target {what} <= {:.2}%: {verdict}", limit * 100.0);
}
