//! Measure what GRU953 Mukti actually does, before anyone tries to improve it.
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

use gru953_mukti::classify::{classify_words, Verdict};
use gru953_mukti::dictionary::Dictionary;
use gru953_mukti::roundtrip::{is_testable_word, normalise_nukta, to_bijoy};
use gru953_mukti::{convert, detect, word_is_well_formed, LegacyEncoding};
use stats::{edit_distance, thousands, Proportion};

fn main() -> std::process::ExitCode {
    match run() {
        Ok(missed) if missed.is_empty() => std::process::ExitCode::SUCCESS,
        Ok(missed) => {
            eprintln!("\n{} target(s) NOT MET:", missed.len());
            for one in &missed {
                eprintln!("  - {one}");
            }
            eprintln!(
                "\nThis command now fails when a target is missed. Until 13 August 2026\n\
                 it printed \"NOT MET\" and exited successfully, so every target could be\n\
                 missed and anything asking whether it passed was told yes."
            );
            std::process::ExitCode::FAILURE
        }
        Err(e) => {
            eprintln!("eval: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

fn run() -> Result<Vec<String>, Box<dyn std::error::Error>> {
    let cfg = Config::parse()?;
    let mut gates = Gates::default();

    println!("GRU953 Mukti — accuracy report");
    println!("================================\n");

    m1_round_trip(&cfg, &mut gates)?;
    m2_character_grid(&cfg, &mut gates)?;
    m3_dictionary_on_real_documents(&cfg)?;
    m4_vowel_preservation(&cfg)?;
    detection(&cfg, &mut gates)?;
    detection_with_context(&cfg, &mut gates)?;

    println!("\nEvery figure above is measured, not estimated. Sample sizes are");
    println!("stated because a percentage without one means nothing.");
    Ok(gates.missed)
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
fn m1_round_trip(cfg: &Config, gates: &mut Gates) -> Result<(), Box<dyn std::error::Error>> {
    heading("M1", "Round trip: Unicode -> Bijoy -> Unicode");

    let bytes = fs::read(&cfg.extended_fst)?;
    let set = fst::Set::new(bytes)?;
    let mut stream = set.stream();

    let mut words_exact = 0usize;
    let mut words_ok = 0usize;
    let mut words_total = 0usize;
    let mut skipped = 0usize;
    let mut malformed = 0usize;
    let mut chars_total = 0usize;
    let mut chars_wrong = 0usize;
    let mut patterns: BTreeMap<String, (usize, String)> = BTreeMap::new();

    while let Some(key) = fst::Streamer::next(&mut stream) {
        let word = std::str::from_utf8(key)?;
        // Bijoy *is* ASCII, so a word mixing scripts is ambiguous by
        // construction; testing it would measure the ambiguity, not the code.
        if !is_testable_word(word) {
            skipped += 1;
            continue;
        }
        // The word list has typing slips in it — a vowel sign struck twice, two
        // signs on one consonant, `আ` typed as `অ` plus a matra. Mukti's repair
        // passes turn these into the correct spelling, which is what they are
        // for, and the round trip then scores that correction as a failure.
        //
        // They are set aside on an orthographic test that knows nothing about
        // Mukti: each is a sequence Bengali does not permit at all. The count
        // is printed, so this is never a silent exclusion of whatever failed.
        if !word_is_well_formed(word) {
            malformed += 1;
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
            let e = patterns.entry(difference(&a, &b)).or_default();
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!("{a} -> {b}");
            }
        }
    }

    let words = Proportion::new(words_ok, words_total);
    let exact = Proportion::new(words_exact, words_total);
    let chars = Proportion::new(chars_total - chars_wrong.min(chars_total), chars_total);
    println!("  Word accuracy       {}", words.describe());
    println!("  Character accuracy  {}", chars.describe());
    // Printed even when it matches, because a match is itself the finding: it
    // says Mukti's output is already canonically spelled, so normalising
    // before comparison is not quietly doing work to flatter the figure.
    println!("  Same, comparing exact bytes rather than canonical spellings:");
    println!("    Word accuracy     {}", exact.describe());
    if words_exact == words_ok {
        println!("    Identical, which is the point: Mukti's output is already canonically");
        println!("    spelled, so normalising before comparison changes nothing here.");
    } else {
        println!("    The gap is canonically equivalent spellings — a two-part vowel written");
        println!("    as its two halves rather than as one character. See `canonical`.");
    }
    println!("  Excluded, and why:");
    println!(
        "    {:>6}  mix scripts, so Bijoy cannot be told from plain ASCII in them",
        thousands(skipped)
    );
    println!(
        "    {:>6}  not well-formed Bengali in the source: a vowel sign struck twice, two",
        thousands(malformed)
    );
    println!("            on one consonant, or `আ` typed as `অ` plus a matra. Mukti repairs");
    println!("            these to the correct spelling; the word list, not Mukti, is wrong.");
    gates.at_least("word accuracy", words.rate(), 0.99);
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
fn m2_character_grid(cfg: &Config, gates: &mut Gates) -> Result<(), Box<dyn std::error::Error>> {
    heading(
        "M2",
        "Character grid: every consonant x every vowel and conjunct",
    );

    let text = fs::read_to_string(cfg.corpus.join("BengaliCharacterCombinations.txt"))?;
    let mut ok = 0usize;
    let mut total = 0usize;
    let mut legend = 0usize;
    let mut mistyped = 0usize;
    let mut patterns: BTreeMap<String, (usize, String)> = BTreeMap::new();

    for token in text.split_whitespace() {
        if !is_testable_word(token) {
            continue;
        }
        // Two kinds of entry in this file are not cells of the grid, and both
        // are counted and named below rather than quietly dropped. Excluding
        // whatever fails is how a harness flatters itself, so each exclusion
        // has to answer to a test that has nothing to do with Mukti.
        //
        // First, the file's own legend. Lines 3 and 4 list the vowel signs
        // (`া ি ী ু ৃ ে ৈ ো ৌ`) and the phala forms (`্য ্র ্ন ্ম ্ল ্ব`) — the
        // headings the grid is indexed by, not words. They are recognised by
        // opening with a mark rather than a consonant, which no word does.
        if token.starts_with(is_leading_mark) {
            legend += 1;
            continue;
        }
        // Second, five mistyped cells. Each is a vowel sign next to a hasant,
        // which Bengali orthography does not permit because a vowel closes its
        // syllable and leaves the hasant nothing to join. In every case the
        // other seven cells of the same row are spelled correctly, and Mukti
        // converts these to the spelling the row implies.
        if !word_is_well_formed(token) {
            mistyped += 1;
            continue;
        }
        total += 1;
        let got = convert(&to_bijoy(token));
        let (a, b) = (canonical(token), canonical(&got));
        if a == b {
            ok += 1;
        } else {
            let e = patterns.entry(difference(&a, &b)).or_default();
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!("{a} -> {b}");
            }
        }
    }

    let p = Proportion::new(ok, total);
    println!("  Combinations correct  {}", p.describe());
    println!("  Excluded, and why:");
    println!("    {legend:>3}  legend entries — the grid's own headings, not words");
    println!("    {mistyped:>3}  cells mistyped in the source file: a vowel sign beside a hasant,");
    println!("         which Bengali does not permit. The rest of each such row is spelled");
    println!("         correctly and Mukti produces that spelling. The grid is wrong here,");
    println!("         not Mukti — and it is judged so by orthography, not by agreeing with us.");
    gates.at_least("character grid", p.rate(), 1.0);
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
        // `split` was discarded here with `..`, which made M3 the one measurement
        // that read the TUNING half even when asked for the held-out half. A figure
        // measured partly on the data used to tune is not a held-out figure.
        let Row {
            split,
            label,
            token,
            ..
        } = row?;
        if split != cfg.split || label != "legacy" {
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
/// swapped for দীর্ঘ-ঈ, হ্রস্ব-উ for দীর্ঘ-ঊ, hasanta dropped. Mukti carries a
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
    let mut patterns: BTreeMap<String, (usize, String)> = BTreeMap::new();

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
            let e = patterns.entry(difference(&a, &b)).or_default();
            e.0 += 1;
            if e.1.is_empty() {
                e.1 = format!("{a} -> {b}");
            }
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

/// Does Mukti convert **only** what is genuinely legacy?
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
fn detection(cfg: &Config, gates: &mut Gates) -> Result<(), Box<dyn std::error::Error>> {
    heading("D", "Detection on held-out documents, one word at a time");

    let mut counts: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();
    for row in rows(&cfg.labels)? {
        let Row {
            split,
            label,
            token,
            ..
        } = row?;
        // Was `split != "test"`, hard-coded — so `--split tune` still reported the
        // held-out half, quietly defeating the one discipline the split exists for.
        if split != cfg.split {
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
    gates.at_most("false-positive rate (no context)", fpr.rate(), 0.001);

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
// Detection, word level, with context — the Phase 3 classifier
// ---------------------------------------------------------------------------

/// The same question as `detection`, put to the word-level classifier.
///
/// Two differences from the baseline, and both matter:
///
/// * words are judged **in document order**, so the context pass has the
///   neighbours it was built to use;
/// * the split is chosen by the caller. Thresholds are tuned against `tune`
///   and the figure that gets quoted comes from `test`, which is never looked
///   at while anything is being adjusted. Tuning on the data you report on is
///   how a classifier scores 99% on paper and fails on the first real file.
fn detection_with_context(
    cfg: &Config,
    gates: &mut Gates,
) -> Result<(), Box<dyn std::error::Error>> {
    heading(
        "D2",
        &format!(
            "Word-level detection with context, on the {} split",
            cfg.split
        ),
    );

    let dictionary = Dictionary::shipped();
    let mut counts: BTreeMap<&'static str, (usize, usize)> = BTreeMap::new();

    // One document at a time, in order. Anything else would deny the context
    // pass the only thing it has to work with.
    let mut doc = usize::MAX;
    let mut words: Vec<String> = Vec::new();
    let mut labels: Vec<&'static str> = Vec::new();
    // What is actually being got wrong, by frequency. Frequency is the point:
    // a token that recurs is ordinary vocabulary, not somebody's name or a
    // one-off reference, so this can be looked at without reading documents.
    let mut wrong_english: BTreeMap<String, usize> = BTreeMap::new();
    let mut missed_legacy: BTreeMap<String, usize> = BTreeMap::new();

    let flush = |words: &mut Vec<String>,
                 labels: &mut Vec<&'static str>,
                 counts: &mut BTreeMap<&'static str, (usize, usize)>,
                 wrong: &mut BTreeMap<String, usize>,
                 missed: &mut BTreeMap<String, usize>| {
        if words.is_empty() {
            return;
        }
        let refs: Vec<&str> = words.iter().map(String::as_str).collect();
        let verdicts = classify_words(&refs, dictionary);
        for ((verdict, label), word) in verdicts.iter().zip(labels.iter()).zip(refs.iter()) {
            let entry = counts.entry(label).or_default();
            entry.0 += usize::from(*verdict == Verdict::Legacy);
            entry.1 += 1;
            if *verdict == Verdict::Legacy && *label == "english" {
                *wrong.entry((*word).to_owned()).or_default() += 1;
            }
            // And the other direction: legacy words we failed to convert.
            // Recall is the open gate, so what is being missed matters as
            // much as what is wrongly taken.
            if *verdict != Verdict::Legacy && *label == "legacy" {
                *missed.entry((*word).to_owned()).or_default() += 1;
            }
        }
        words.clear();
        labels.clear();
    };

    for row in rows(&cfg.labels)? {
        let row = row?;
        if row.split != cfg.split {
            continue;
        }
        let name: &'static str = match row.label.as_str() {
            "legacy" => "legacy",
            "legacy_ascii" => "legacy_ascii",
            "unicode" => "unicode",
            "english" => "english",
            "inert" => "inert",
            _ => continue,
        };
        if row.doc != doc {
            flush(
                &mut words,
                &mut labels,
                &mut counts,
                &mut wrong_english,
                &mut missed_legacy,
            );
            doc = row.doc;
        }
        words.push(row.token);
        labels.push(name);
    }
    flush(
        &mut words,
        &mut labels,
        &mut counts,
        &mut wrong_english,
        &mut missed_legacy,
    );

    let get = |k: &str| counts.get(k).copied().unwrap_or((0, 0));
    let (legacy_hit, legacy_n) = get("legacy");
    let recall = Proportion::new(legacy_hit, legacy_n);

    let must_not: usize = ["unicode", "english", "inert"]
        .iter()
        .map(|k| get(k).1)
        .sum();
    let wrongly: usize = ["unicode", "english", "inert"]
        .iter()
        .map(|k| get(k).0)
        .sum();
    let fpr = Proportion::new(wrongly, must_not);
    let english = Proportion::new(get("english").0, get("english").1);

    let precision = Proportion::new(legacy_hit, legacy_hit + wrongly);
    let f1 = if precision.rate() + recall.rate() > 0.0 {
        2.0 * precision.rate() * recall.rate() / (precision.rate() + recall.rate())
    } else {
        0.0
    };

    println!("  Recall on legacy words     {}", recall.describe());
    println!("  Precision                  {}", precision.describe());
    println!("  F1                         {f1:.4}");
    println!("  False positives, aggregate {}", fpr.describe());
    gates.at_least("recall", recall.rate(), 0.99);
    gates.at_most("false positives, aggregate", fpr.rate(), 0.001);

    println!("\n  Wrongly converted, by what the text actually was:");
    for key in ["unicode", "english", "inert"] {
        let (hit, n) = get(key);
        println!("    {key:<14} {}", Proportion::new(hit, n).describe());
    }
    // The gate that actually binds. The aggregate is diluted by the enormous,
    // perfectly clean Unicode class; English is where readable text gets
    // destroyed, so English is what has to clear the bar.
    gates.at_most("false positives on ENGLISH", english.rate(), 0.001);

    let (amb_hit, amb_n) = get("legacy_ascii");
    println!("\n  Pure-ASCII legacy — genuinely ambiguous, reported on its own:");
    println!("    {}", Proportion::new(amb_hit, amb_n).describe());
    println!("    These are real legacy words that carry no evidence of it. Every one");
    println!("    recovered here was recovered from its neighbours alone.");

    let mut miss: Vec<_> = missed_legacy.iter().collect();
    miss.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\n  Legacy words most often MISSED (recall is the open gate):");
    for (token, count) in miss.iter().take(20) {
        println!(
            "    {:>6}x  {token}  would have been  {}",
            thousands(**count),
            convert(token)
        );
    }

    let mut worst: Vec<_> = wrong_english.iter().collect();
    worst.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    println!("\n  English tokens most often converted in error:");
    for (token, count) in worst.iter().take(20) {
        println!(
            "    {:>6}x  {token}  ->  {}",
            thousands(**count),
            convert(token)
        );
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Plumbing
// ---------------------------------------------------------------------------

struct Config {
    corpus: PathBuf,
    labels: PathBuf,
    extended_fst: PathBuf,
    /// Which half of the labelled set D2 reports on. `tune` while adjusting
    /// anything; `test` only when the figure is going to be quoted.
    split: String,
}

impl Config {
    fn parse() -> Result<Self, String> {
        let mut corpus = None;
        let mut labels = PathBuf::from("local/labelled-corpus.tsv");
        let mut extended_fst = PathBuf::from("local/extended-words.fst");
        let mut split = String::from("test");
        let mut it = std::env::args().skip(1);
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--corpus" => corpus = it.next().map(PathBuf::from),
                "--labels" => labels = it.next().map(PathBuf::from).unwrap_or(labels),
                "--words" => extended_fst = it.next().map(PathBuf::from).unwrap_or(extended_fst),
                "--split" => split = it.next().unwrap_or(split),
                "--report" => {}
                other => return Err(format!("unknown argument {other:?}")),
            }
        }
        Ok(Config {
            corpus: corpus.ok_or("--corpus <Bangla Word Collection dir> is required")?,
            labels,
            extended_fst,
            split,
        })
    }
}

/// Stream the labelled set. 152 MB on disk, so never all at once.
fn rows(path: &Path) -> Result<impl Iterator<Item = std::io::Result<Row>>, std::io::Error> {
    let file = BufReader::new(fs::File::open(path)?);
    Ok(file.lines().skip(1).filter_map(|line| match line {
        Err(e) => Some(Err(e)),
        Ok(line) => {
            let mut parts = line.splitn(4, '\t');
            match (parts.next(), parts.next(), parts.next(), parts.next()) {
                (Some(s), Some(d), Some(l), Some(t)) => Some(Ok(Row {
                    split: s.to_owned(),
                    doc: d.parse().unwrap_or(0),
                    label: l.to_owned(),
                    token: t.to_owned(),
                })),
                _ => None,
            }
        }
    }))
}

/// One labelled token, with the document it came from.
struct Row {
    split: String,
    doc: usize,
    label: String,
    token: String,
}

/// A combining mark that cannot begin a word: a vowel sign or a hasant.
///
/// Used to recognise the character grid's legend entries, which are the marks
/// themselves rather than words made with them.
fn is_leading_mark(c: char) -> bool {
    matches!(
        c,
        'া' | 'ি' | 'ী' | 'ু' | 'ূ' | 'ৃ' | 'ে' | 'ৈ' | 'ো' | 'ৌ' | 'ৗ' | '\u{09CD}'
    )
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
///   words the decomposed way; Mukti composes them, which is the normalised
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

/// The commonest failures, each with one whole word showing it in context.
///
/// Only ever called on measurements drawn from the public word corpus, never
/// from the private document archive — the examples are ordinary dictionary
/// vocabulary, and the difference patterns alone are too short to identify
/// anything anyway.
fn top_patterns(patterns: &BTreeMap<String, (usize, String)>, title: &str) {
    if patterns.is_empty() {
        return;
    }
    let mut sorted: Vec<_> = patterns.iter().collect();
    sorted.sort_by(|a, b| b.1 .0.cmp(&a.1 .0).then(a.0.cmp(b.0)));
    println!("{title} ({} distinct):", thousands(patterns.len()));
    for (pattern, (count, example)) in sorted.iter().take(12) {
        println!(
            "    {:>8}x  {pattern:<22} e.g. {example}",
            thousands(*count)
        );
    }
}

fn heading(id: &str, title: &str) {
    println!("\n{id} — {title}");
    println!("{}", "-".repeat(72));
}

/// Every unmet target, collected so the command can fail on them.
///
/// Until 13 August 2026 a gate only *printed* "NOT MET" and `eval` still exited
/// successfully. So the six targets this project measures itself against could
/// every one of them be missed, and any script or pipeline asking "did it pass?"
/// would be told yes. A target that cannot fail is a wish.
#[derive(Default)]
struct Gates {
    missed: Vec<String>,
}

impl Gates {
    /// A floor: the value must be at least the target.
    fn at_least(&mut self, what: &str, value: f64, target: f64) {
        let met = value >= target;
        println!(
            "  Target {what} >= {:.1}%: {}",
            target * 100.0,
            if met { "MET" } else { "NOT MET" }
        );
        if !met {
            self.missed.push(format!(
                "{what}: {:.4}%, needed at least {:.1}%",
                value * 100.0,
                target * 100.0
            ));
        }
    }

    /// A ceiling: the value must be no more than the limit.
    fn at_most(&mut self, what: &str, value: f64, limit: f64) {
        let met = value <= limit;
        println!(
            "  Target {what} <= {:.2}%: {}",
            limit * 100.0,
            if met { "MET" } else { "NOT MET" }
        );
        if !met {
            self.missed.push(format!(
                "{what}: {:.4}%, must be no more than {:.2}%",
                value * 100.0,
                limit * 100.0
            ));
        }
    }
}
