//! Compile a Bengali word list into the compressed dictionary Scribe embeds.
//!
//! Run once, by hand, when the word list changes. The output is checked in;
//! building Scribe itself never needs the corpus, a network, or this tool.
//!
//! # Two dictionaries, for two different reasons
//!
//! **Shipped** (`--mode shipped`, the default) is built from `words.txt` alone
//! — 454,649 words, released into the public domain under the Unlicense at
//! `github.com/tahmid02016/bangla-wordlist`. It is the only file in the corpus
//! whose licence is known, which is what makes it the only one Scribe may
//! redistribute. It also happens to be the largest, covering 97.6% of every
//! distinct word in the whole collection.
//!
//! **Extended** (`--mode extended`) merges every word list in the corpus,
//! including files of unknown provenance. It is written to `local/` and is
//! **never committed and never shipped** — it exists only so the accuracy
//! harness can judge a converted word against the widest vocabulary available.
//! Using it to *measure* redistributes nothing.
//!
//! # Why a finite-state transducer
//!
//! Half a million Bengali words is roughly 14 MB of plain UTF-8 — too much to
//! embed. An FST stores them as a shared-prefix, shared-suffix automaton, which
//! suits an inflecting language like Bengali particularly well: `শাখা`, `শাখার`,
//! `শাখায়` and `শাখাগুলো` share almost all of their structure. Lookup is
//! O(length of the word) and needs no decompression.

use std::collections::BTreeSet;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

use gru953_scribe::roundtrip::normalise_nukta;

/// The only word list whose licence is known, and so the only one that ships.
const SHIPPED_SOURCE: &str = "words.txt";

/// Everything usable as vocabulary for local measurement.
///
/// The dictionary files are excluded on purpose: their Bengali column holds
/// multi-word glosses ("মেজাজের রুক্ষতা"), not headwords, so they would fill the
/// set with phrases pretending to be words.
const EXTENDED_SOURCES: &[&str] = &[
    "words.txt",
    "BengaliWordList_439.txt",
    "BengaliWordList_112.txt",
    "BengaliWordList_48.txt",
    "BengaliWordList_40.txt",
    "bangla_word_huge_dataset.csv", // single column, no header
    "right_file.txt",               // proofread correct spellings
];

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse(std::env::args().skip(1))?;

    let sources: &[&str] = match args.mode {
        Mode::Shipped => &[SHIPPED_SOURCE],
        Mode::Extended => EXTENDED_SOURCES,
    };

    let mut words = BTreeSet::new();
    let mut read_total = 0usize;
    let mut rejected = 0usize;

    for name in sources {
        let path = args.corpus.join(name);
        let text = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read {}: {e}", path.display()))?;
        let before = words.len();
        let mut lines = 0usize;
        for line in text.lines() {
            lines += 1;
            match clean(line) {
                Some(word) => {
                    words.insert(word);
                }
                None => rejected += 1,
            }
        }
        read_total += lines;
        println!(
            "  {name:<32} {lines:>9} lines  {:>9} new",
            words.len() - before
        );
    }

    if let Some(parent) = args.out.parent() {
        fs::create_dir_all(parent)?;
    }
    let writer = BufWriter::new(fs::File::create(&args.out)?);
    let mut builder = fst::SetBuilder::new(writer)?;
    // BTreeSet iterates in byte order, which is exactly the order an FST
    // builder demands. Sorting is therefore not a separate step.
    for word in &words {
        builder.insert(word)?;
    }
    builder.into_inner()?.flush()?;

    let bytes = fs::metadata(&args.out)?.len();
    let plain: usize = words.iter().map(|w| w.len() + 1).sum();
    println!(
        "\n{} lines read, {} rejected as not plain Bengali, {} distinct words.",
        read_total,
        rejected,
        words.len()
    );
    println!(
        "{} -> {} KiB, down from {} KiB as plain text ({:.1}x smaller).",
        args.out.display(),
        bytes / 1024,
        plain / 1024,
        plain as f64 / bytes as f64
    );
    Ok(())
}

/// Normalise one line into a dictionary entry, or reject it.
///
/// Rejects anything that is not **entirely** Bengali letters. A dictionary is
/// used to answer "did this convert into a real Bengali word?", and an entry
/// holding a digit, a Latin letter or a hyphen could answer that question
/// wrongly: `অ-কার` would vouch for output that still contained a stray hyphen.
///
/// The nukta is composed first. `য়`, `ড়` and `ঢ়` are each legal in Unicode two
/// ways — one character, or a base letter plus U+09BC — and they look identical.
/// A dictionary that held both spellings would still miss the third caller who
/// looked it up the other way. Settling on the precomposed form here means
/// every lookup asks the same question. This distinction has already cost this
/// codebase four separate defects.
fn clean(line: &str) -> Option<String> {
    let word = normalise_nukta(line.trim().trim_start_matches('\u{FEFF}'))
        // Two-part vowels, for exactly the same reason as the nukta. `ো` is one
        // character (U+09CB) and also, equally legally, `ে` followed by `া`
        // (U+09C7 U+09BE); Unicode calls them canonically equivalent and they
        // render identically. The source list spells 3,700 of its words the
        // decomposed way. A dictionary holding both spellings would still tell
        // the next caller their perfectly ordinary word does not exist, so it
        // settles on the composed form — which is what Scribe itself produces.
        .replace("\u{09C7}\u{09BE}", "\u{09CB}")
        .replace("\u{09C7}\u{09D7}", "\u{09CC}");
    if word.is_empty() {
        return None;
    }
    if word.chars().all(|c| ('\u{0980}'..='\u{09FF}').contains(&c)) {
        Some(word)
    } else {
        None
    }
}

enum Mode {
    Shipped,
    Extended,
}

struct Args {
    corpus: PathBuf,
    out: PathBuf,
    mode: Mode,
}

impl Args {
    fn parse(mut it: impl Iterator<Item = String>) -> Result<Self, String> {
        let mut corpus = None;
        let mut out = None;
        let mut mode = Mode::Shipped;
        while let Some(arg) = it.next() {
            match arg.as_str() {
                "--corpus" => corpus = it.next().map(PathBuf::from),
                "--out" => out = it.next().map(PathBuf::from),
                "--mode" => {
                    mode = match it.next().as_deref() {
                        Some("shipped") => Mode::Shipped,
                        Some("extended") => Mode::Extended,
                        other => return Err(format!("unknown mode {other:?}")),
                    }
                }
                other => return Err(format!("unknown argument {other:?}\n\n{USAGE}")),
            }
        }
        let corpus = corpus.ok_or_else(|| format!("--corpus is required\n\n{USAGE}"))?;
        let out = out.unwrap_or_else(|| match mode {
            Mode::Shipped => default_shipped_out(),
            Mode::Extended => PathBuf::from("local/extended-words.fst"),
        });
        Ok(Args { corpus, out, mode })
    }
}

fn default_shipped_out() -> PathBuf {
    Path::new("crates/scribe-core/data/bengali-words.fst").to_path_buf()
}

const USAGE: &str = "\
usage: lexicon-build --corpus <dir> [--mode shipped|extended] [--out <file>]

  --corpus   the Bangla Word Collection directory
  --mode     shipped  (default) words.txt only, public domain, safe to commit
             extended every word list, unknown provenance, local use only
  --out      where to write the compiled dictionary";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_wholly_bengali_entries_survive() {
        assert_eq!(clean("  শাখা \n").as_deref(), Some("শাখা"));
        // A byte-order mark on the first line of a list is ordinary and must
        // not become part of the first word.
        assert_eq!(clean("\u{FEFF}অংশ").as_deref(), Some("অংশ"));

        for rejected in ["", "   ", "অ-কার", "ঢাকা2026", "Dhaka", "প্রতিবেদন।"]
        {
            assert!(clean(rejected).is_none(), "accepted {rejected:?}");
        }
    }

    #[test]
    fn both_spellings_of_a_two_part_vowel_land_on_one_entry() {
        // `গুলো` written with the composed o-kar and with its two halves.
        // Unicode calls these the same word; so must the dictionary.
        let composed = "গুল\u{09CB}";
        let decomposed = "গুল\u{09C7}\u{09BE}";
        assert_ne!(composed, decomposed, "the two spellings differ as bytes");
        assert_eq!(clean(composed), clean(decomposed));
        assert_eq!(clean(decomposed).as_deref(), Some(composed));
    }

    #[test]
    fn both_spellings_of_ya_with_nukta_land_on_one_entry() {
        let precomposed = "সম\u{09DF}";
        let decomposed = "সম\u{09AF}\u{09BC}";
        assert_ne!(precomposed, decomposed, "the two spellings differ as bytes");
        assert_eq!(clean(precomposed), clean(decomposed));
        assert!(
            clean(decomposed).is_some(),
            "the decomposed form was dropped"
        );
    }
}
