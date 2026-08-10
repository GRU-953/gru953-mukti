//! A round-trip harness for GRU953-Scribe — a measuring instrument, not a fix.
//!
//! Scribe has never had large-scale test data with a **known correct answer**.
//! Hand-checking converted output does not scale, and every automated check so
//! far asks only "is this well-formed Bengali?" — a bar that a wrong-glyph
//! mapping sailed straight through, producing well-formed Bengali that was the
//! wrong word.
//!
//! This module manufactures the missing answer key. Take real **Unicode**
//! Bangla, encode it **into** Bijoy with [`to_bijoy`], run [`convert`] on the
//! result, and check the original text comes back. The source text is the
//! answer, so every difference is a candidate defect — in the reverse encoder
//! here, or in the converter itself.
//!
//! # Why this needs more than an inverted table
//!
//! Bijoy stores glyphs in the order they are **drawn**; Unicode stores letters
//! in the order they are **spoken**. So the reordering must be undone *before*
//! any character is mapped:
//!
//! * a pre-kar (`ি` `ে` `ৈ`) sits after its consonant cluster in Unicode and
//!   before it in Bijoy;
//! * `ো` is drawn as `ে` … `া` and `ৌ` as `ে` … `ৗ`, straddling the cluster;
//! * a reph (`র` + hasant) sits before its cluster in Unicode and after it in
//!   Bijoy, written with the single glyph `©`.
//!
//! # This is not a byte-exact encoder, by design
//!
//! Several Bijoy glyphs are true aliases — `Í` and `Z` both mean `ত`, `ø` and
//! `¬` both mean `্ল`, `†` and `‡` both mean `ে`. When the table is inverted,
//! one has to be chosen; the choice is the glyph appearing **first** in
//! `CONVERSION_MAP`, which is deterministic and nothing more. So `to_bijoy`
//! need not reproduce a document's original bytes. Only the round trip
//! *through Unicode* has to be faithful, and that is what is asserted.

use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

use crate::{convert, is_consonant, is_kar, tables};

const HALANT: char = '\u{09CD}';
const NUKTA: char = '\u{09BC}';
const RA: char = 'র';

/// One word that did not survive the round trip.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Mismatch {
    /// The word as it appeared in the source document.
    pub original: String,
    /// What [`to_bijoy`] made of it.
    pub via_bijoy: String,
    /// What [`convert`] gave back.
    pub got: String,
}

// ---------------------------------------------------------------------------
// The reverse table
// ---------------------------------------------------------------------------

/// `CONVERSION_MAP` inverted, bucketed by first character, longest key first.
///
/// Two rules make this deterministic:
///
/// * where several glyphs carry the same Unicode, the one appearing **first**
///   in `CONVERSION_MAP` wins;
/// * entries whose Unicode side is plain ASCII (`-`, `"`, `'`) are dropped.
///   Those characters are already themselves in Bijoy and pass through the
///   converter untouched, so rewriting them would only invent differences.
fn reverse_table() -> &'static HashMap<char, Vec<(&'static str, &'static str)>> {
    static TABLE: OnceLock<HashMap<char, Vec<(&'static str, &'static str)>>> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut claimed: Vec<&str> = Vec::new();
        let mut buckets: HashMap<char, Vec<(&'static str, &'static str)>> = HashMap::new();
        for (bijoy, unicode) in tables::CONVERSION_MAP {
            if unicode.is_ascii() || claimed.contains(unicode) {
                continue;
            }
            claimed.push(unicode);
            let Some(first) = unicode.chars().next() else { continue };
            buckets.entry(first).or_default().push((unicode, bijoy));
        }
        // Longest first, so a conjunct is never eaten by its own prefix. The
        // sort is stable, so equal-length keys keep CONVERSION_MAP's order.
        for entries in buckets.values_mut() {
            entries.sort_by_key(|(u, _)| std::cmp::Reverse(u.chars().count()));
        }
        buckets
    })
}

// ---------------------------------------------------------------------------
// Step 1 — undo the reordering
// ---------------------------------------------------------------------------

/// Where the two halves of a vowel sign are drawn, relative to their cluster.
///
/// `ো` and `ৌ` are single Unicode characters but two Bijoy glyphs, one on each
/// side of the consonant. That is the whole reason this function exists.
fn split_kar(k: char) -> (Option<char>, Option<char>) {
    match k {
        'ি' | 'ে' | 'ৈ' => (Some(k), None),
        'ো' => (Some('ে'), Some('া')),
        'ৌ' => (Some('ে'), Some('ৗ')),
        _ => (None, Some(k)),
    }
}

/// Rewrite logical (Unicode) order into visual (Bijoy) order.
///
/// Works one syllable at a time. A syllable is an optional reph, a consonant
/// cluster `C (্C)*`, and an optional vowel sign; it is emitted as
/// `[pre-kar] cluster [©] [post-kar]`.
///
/// The reph glyph goes immediately after its cluster and **before** any vowel
/// sign, because that is what real documents overwhelmingly contain: counted
/// over a large body of Bijoy text, a vowel sign follows the reph glyph about
/// five times as often as it precedes it. Both spellings exist, only one can be
/// generated, and generating the rarer one would let the harness quietly avoid
/// faults that real documents walk straight into.
fn to_visual_order(unicode: &str) -> Vec<char> {
    let s: Vec<char> = unicode.chars().collect();
    let n = s.len();
    let mut out: Vec<char> = Vec::with_capacity(n + 4);
    let mut i = 0usize;

    while i < n {
        // A reph is `র` + hasant + consonant, where the `র` is not itself
        // preceded by a hasant. That last clause is what separates a reph from
        // the `র` of a ra-phala — the distinction that breaks words like
        // `ব্র্যান্ড` when it is got wrong.
        let reph = s[i] == RA
            && i + 2 < n
            && s[i + 1] == HALANT
            && is_consonant(s[i + 2])
            && (i == 0 || s[i - 1] != HALANT);
        let start = if reph { i + 2 } else { i };

        if !is_consonant(s[start]) {
            out.push(s[i]);
            i += 1;
            continue;
        }

        let mut end = start + 1;
        while end + 1 < n && s[end] == HALANT && is_consonant(s[end + 1]) {
            end += 2;
        }
        let kar = if end < n && is_kar(s[end]) { Some(s[end]) } else { None };
        let (pre, post) = kar.map_or((None, None), split_kar);

        out.extend(pre);
        out.extend_from_slice(&s[start..end]);
        if reph {
            out.push(RA);
            out.push(HALANT);
        }
        out.extend(post);
        i = end + usize::from(kar.is_some());
    }
    out
}

// ---------------------------------------------------------------------------
// Step 2 — map the characters
// ---------------------------------------------------------------------------

/// Encode Unicode Bangla into Bijoy (SutonnyMJ).
///
/// The reverse of [`convert`], to the extent that a lossy mapping can be
/// reversed. Characters with no Bijoy form — and anything that is not Bengali —
/// are passed through unchanged.
///
/// The input is nukta-composed first ([`normalise_nukta`]). Bijoy has a single
/// glyph for each of `ড়`, `ঢ়` and `য়` and no combining nukta at all, so an
/// encoder has no choice. Skipping this step also breaks the syllable walk:
/// `য` + U+09BC is two characters, so the cluster ends early and the following
/// vowel sign is left where it stood.
pub fn to_bijoy(unicode: &str) -> String {
    let unicode = &normalise_nukta(unicode);
    let visual = to_visual_order(unicode);
    let table = reverse_table();
    let mut out = String::with_capacity(unicode.len());
    let mut i = 0usize;

    while i < visual.len() {
        let mut taken = 0usize;
        if let Some(candidates) = table.get(&visual[i]) {
            for (unicode_form, glyph) in candidates {
                let len = unicode_form.chars().count();
                if len <= visual.len() - i && visual[i..i + len].iter().copied().eq(unicode_form.chars())
                {
                    out.push_str(glyph);
                    taken = len;
                    break;
                }
            }
        }
        if taken == 0 {
            out.push(visual[i]);
            taken = 1;
        }
        i += taken;
    }
    out
}

// ---------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------

/// Settle the two legal spellings of `ড়`, `ঢ়` and `য়` on the precomposed one.
///
/// Each of these exists in Unicode as a single character (U+09DC, U+09DD,
/// U+09DF) **and** as a base consonant plus a combining nukta (U+09BC). Both
/// are correct, both are common in real documents, and they look identical.
/// Comparing without settling them first buries every genuine defect under
/// thousands of differences that are not differences at all — an ambiguity that
/// has already cost this project four separate defects.
pub fn normalise_nukta(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        let composed = match (c, chars.peek()) {
            ('ড', Some(&NUKTA)) => Some('\u{09DC}'),
            ('ঢ', Some(&NUKTA)) => Some('\u{09DD}'),
            ('য', Some(&NUKTA)) => Some('\u{09DF}'),
            _ => None,
        };
        match composed {
            Some(single) => {
                out.push(single);
                chars.next();
            }
            None => out.push(c),
        }
    }
    out
}

/// Every character the Bijoy tables read as a letter rather than as itself.
///
/// Read off `CONVERSION_MAP`'s keys rather than listed by hand, so it cannot
/// drift when the table changes. The backslash is added because `PRE_MAP`
/// deletes it outright; every other `PRE_MAP` key character is either
/// whitespace or already a `CONVERSION_MAP` key.
fn bijoy_significant() -> &'static HashSet<char> {
    static CHARS: OnceLock<HashSet<char>> = OnceLock::new();
    CHARS.get_or_init(|| {
        tables::CONVERSION_MAP
            .iter()
            .flat_map(|(key, _)| key.chars())
            .chain(std::iter::once('\\'))
            .filter(|c| !c.is_whitespace())
            .collect()
    })
}

/// Is this word worth round-tripping?
///
/// It must contain Bengali, and nothing that Bijoy would read as a letter.
/// Bijoy *is* ASCII wearing Bengali shapes: `ABC` and `2026` convert to
/// Bengali because that is genuinely what the encoding says they mean, and the
/// same is true of an en-dash (the u-kar glyph) and an underscore (`থ`). A word
/// holding one of those is ambiguous by construction, so testing it would
/// measure the ambiguity rather than the converter. Ordinary punctuation —
/// `,` `.` `(` `)` `-` `%` `:` — is not in the tables and is kept.
pub fn is_testable_word(word: &str) -> bool {
    let significant = bijoy_significant();
    let mut bengali = false;
    for c in word.chars() {
        if ('\u{0980}'..='\u{09FF}').contains(&c) {
            bengali = true;
        } else if significant.contains(&c) {
            return false;
        }
    }
    bengali
}

/// Every whitespace-separated word in `text` that [`is_testable_word`] accepts.
pub fn testable_words(text: &str) -> impl Iterator<Item = &str> {
    text.split_whitespace().filter(|w| is_testable_word(w))
}

/// Round-trip each testable word and return only the ones that came back wrong.
///
/// A word appears here when `convert(to_bijoy(word))` differs from `word`.
/// That means **either** the reverse encoder or the converter is wrong; the
/// harness cannot tell which, and does not pretend to.
pub fn round_trip_report(unicode: &str) -> Vec<Mismatch> {
    testable_words(unicode)
        .filter_map(|word| {
            let via_bijoy = to_bijoy(word);
            let got = convert(&via_bijoy);
            if normalise_nukta(&got) == normalise_nukta(word) {
                return None;
            }
            Some(Mismatch {
                original: word.to_owned(),
                via_bijoy,
                got,
            })
        })
        .collect()
}

impl Mismatch {
    /// The differing part alone, with the shared prefix and suffix stripped.
    ///
    /// Whole words are too specific to count: the same one-character fault
    /// appears in a thousand different words. Reducing each mismatch to just
    /// what changed makes the faults countable — and keeps whole phrases of
    /// document text out of anything printed to a terminal.
    pub fn pattern(&self) -> String {
        let a: Vec<char> = normalise_nukta(&self.original).chars().collect();
        let b: Vec<char> = normalise_nukta(&self.got).chars().collect();
        let head = a
            .iter()
            .zip(&b)
            .take_while(|(x, y)| x == y)
            .count();
        let tail = a[head..]
            .iter()
            .rev()
            .zip(b[head..].iter().rev())
            .take_while(|(x, y)| x == y)
            .count();
        let from: String = a[head..a.len() - tail].iter().collect();
        let to: String = b[head..b.len() - tail].iter().collect();
        format!("{} → {}", clip(&from), clip(&to))
    }
}

/// Keep a printed fragment short enough that it can never be a whole phrase.
fn clip(s: &str) -> String {
    let mut out: String = s.chars().take(12).collect();
    if s.chars().count() > 12 {
        out.push('…');
    }
    if out.is_empty() {
        out.push_str("(nothing)");
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Hand-verified pairs of Bijoy bytes and their correct Unicode.
    ///
    /// Asserted as a round trip rather than byte equality, because glyph
    /// aliases make several Bijoy spellings of the same word equally correct:
    /// `ev¯Íevqb` and `ev¯Zevqb` differ only in which of the two glyphs for `ত`
    /// is used, and both convert to `বাস্তবায়ন`.
    const KNOWN: &[(&str, &str)] = &[
        ("Awd†mi bvgt", "অফিসের নামঃ"),
        ("ev¯Íevqb", "বাস্তবা\u{9df}ন"),
        ("D\u{2021}j\u{f8}L\u{a8}", "উল্লেখ্য"),
        ("mswk\u{f8}\u{f3}", "সংশ্লিষ্ট"),
    ];

    #[test]
    fn known_words_survive_the_round_trip() {
        for (bijoy, unicode) in KNOWN {
            let there = to_bijoy(unicode);
            let back = convert(&there);
            assert_eq!(
                normalise_nukta(&back),
                normalise_nukta(unicode),
                "round trip failed for {unicode:?} (reference Bijoy {bijoy:?}, ours {there:?})"
            );
        }
    }

    /// The encoder must agree with real documents, not merely with itself.
    /// Converting our Bijoy and a real document's Bijoy must give the same
    /// Unicode.
    #[test]
    fn our_bijoy_means_the_same_as_a_real_documents_bijoy() {
        for (bijoy, unicode) in KNOWN {
            assert_eq!(
                normalise_nukta(&convert(&to_bijoy(unicode))),
                normalise_nukta(&convert(bijoy)),
                "our encoding of {unicode:?} does not mean what {bijoy:?} means"
            );
        }
    }

    /// Where a word uses no aliased glyph, the bytes must match what a real
    /// SutonnyMJ document holds — otherwise the round trip could be passing by
    /// accident.
    #[test]
    fn an_unaliased_word_is_encoded_byte_for_byte() {
        assert_eq!(to_bijoy("অফিসের নামঃ"), "Awd†mi bvgt");
        assert_eq!(to_bijoy("কর্মসূচি"), "Kg\u{a9}m~wP");
        assert_eq!(to_bijoy("ব্র্যান্ড"), "e\u{aa}\u{a8}v\u{db}");
        assert_eq!(to_bijoy("আমি"), "Avwg");
    }

    #[test]
    fn the_reordering_is_undone_before_the_characters_are_mapped() {
        // Pre-kar moves in front of its whole cluster.
        assert_eq!(to_visual_order("প্রি").iter().collect::<String>(), "িপ্র");
        // Two-part vowel signs straddle it.
        assert_eq!(to_visual_order("পো").iter().collect::<String>(), "েপা");
        assert_eq!(to_visual_order("পৌ").iter().collect::<String>(), "েপৗ");
        // Reph moves behind it; ra-phala does not move at all.
        assert_eq!(to_visual_order("কর্ম").iter().collect::<String>(), "কমর্");
        assert_eq!(to_visual_order("প্রা").iter().collect::<String>(), "প্রা");
        // A reph goes before its vowel sign, which is how documents spell it.
        assert_eq!(to_visual_order("সার্বিক").iter().collect::<String>(), "সািবর্ক");
        assert_eq!(to_visual_order("কর্মী").iter().collect::<String>(), "কমর্ী");
    }

    #[test]
    fn a_word_that_round_trips_is_not_reported() {
        assert!(round_trip_report("কর্মসূচি এবং প্রতিবেদন").is_empty());
    }

    #[test]
    fn both_spellings_of_ya_with_nukta_compare_equal() {
        let precomposed = "বাস্তবা\u{9df}ন";
        let decomposed = "বাস্তবায\u{9bc}ন";
        assert_ne!(precomposed, decomposed, "the two spellings must differ as text");
        assert_eq!(normalise_nukta(precomposed), normalise_nukta(decomposed));
        assert!(round_trip_report(decomposed).is_empty(), "nukta noise leaked through");
    }

    #[test]
    fn words_that_cannot_be_judged_are_not_tested() {
        // Bijoy *is* ASCII, so an ASCII fragment inside Bengali is ambiguous.
        assert!(!is_testable_word("ঢাকা-Dhaka"));
        assert!(!is_testable_word("২০২৬সালে2026"));
        assert!(!is_testable_word("Programme"));
        // An en-dash is the u-kar glyph and an underscore is `থ`. Both look
        // like punctuation and are not.
        assert!(!is_testable_word("১৮\u{2013}৩০"));
        assert!(!is_testable_word("_সহজ"));
        // Ordinary punctuation is in neither table and must stay testable.
        assert!(is_testable_word("ঢাকা,"));
        assert!(is_testable_word("(প্রতিবেদন)।"));
    }

    /// The filter must match what the converter actually does, not a guess.
    /// Every character it excludes must really be rewritten, and every
    /// character it keeps must really survive untouched.
    #[test]
    fn the_filter_agrees_with_the_converter() {
        for c in bijoy_significant() {
            let one = c.to_string();
            assert_ne!(convert(&one), one, "{c:?} is excluded but converts to itself");
        }
        for c in [',', '.', '(', ')', '-', '%', ':', ';', '?', '!', '/', '\u{0964}'] {
            let one = c.to_string();
            assert_eq!(convert(&one), one, "{c:?} is kept but the converter rewrites it");
        }
    }

    #[test]
    fn a_mismatch_reports_only_what_changed() {
        let m = Mismatch {
            original: "বিবরণ".into(),
            via_bijoy: "weeiY".into(),
            got: "বিবরন".into(),
        };
        assert_eq!(m.pattern(), "ণ → ন");
    }

    /// Aliases must resolve to the glyph that appears first in the table, so
    /// two runs of the harness never disagree about what a word encodes to.
    #[test]
    fn aliased_glyphs_resolve_to_the_first_one_in_the_table() {
        assert_eq!(to_bijoy("তু"), "Zy", "ত and ু each have two glyphs");
        assert_eq!(to_bijoy("ল্ল"), "j\u{ac}", "্ল has three glyphs");
        assert_eq!(to_bijoy("কে"), "\u{2020}K", "ে has two glyphs");
    }
}
