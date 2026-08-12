//! Decide, word by word, what is legacy Bijoy and what must be left alone.
//!
//! # Why this is hard, stated plainly
//!
//! Bijoy is not a character set. It is ordinary ASCII drawn with Bengali
//! shapes. So `bvg` is the word নাম, and it is also three perfectly ordinary
//! Latin letters, and **nothing inside the word itself can tell you which**.
//! A word-level detector is therefore being asked a question that is sometimes
//! genuinely unanswerable, and the only honest design is one that says so.
//!
//! Hence three verdicts, not two. [`Verdict::Uncertain`] is not a failure of
//! nerve; it is the truthful answer for a word like `bvg` seen on its own, and
//! it is what lets the neighbouring words settle the matter instead.
//!
//! # The asymmetry that drives every threshold here
//!
//! Missing a legacy word leaves it unreadable — visible, annoying, and fixable
//! by running the file again with more context. Converting a word that was
//! *not* legacy silently destroys readable text, and the reader may never know
//! it happened. The two errors are not equally bad, so the thresholds are not
//! symmetric: **when the evidence runs out, the answer is "leave it alone".**
//!
//! # What replaced the guesswork
//!
//! The old detector asked "does this look like Bijoy?" — character density,
//! capitalisation, and a 150-stem word list matched by substring. Three
//! variations were tried and reverted before this.
//!
//! This asks a better question: **convert it and see whether the result is a
//! real Bengali word**, against 451,348 of them. Genuine Bijoy converts into
//! words. English forced through the same tables converts into Bengali-shaped
//! noise, which a dictionary rejects out of hand.

use crate::dictionary::Dictionary;
use crate::tokenise::{tokenise, Kind, Segment};
use crate::{convert, word_is_well_formed};

/// What the classifier concluded about one word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Verdict {
    /// Legacy Bijoy. Convert it.
    Legacy,
    /// Not legacy, or not judgeable. Leave it exactly as it is.
    NotLegacy,
    /// Genuinely ambiguous alone — pure ASCII that converts to a real Bengali
    /// word. Only the surrounding words can settle it.
    Uncertain,
}

/// Characters Bijoy uses to carry conjuncts, vowel signs and reph.
///
/// Plain English almost never contains these; Bijoy is dense in them. Their
/// presence is the single cheapest piece of evidence available.
fn is_bijoy_range(c: char) -> bool {
    let o = c as u32;
    (0x00A0..=0x024F).contains(&o) || (0x2010..=0x20FF).contains(&o)
}

/// What one word looks like, before any judgement is made about it.
#[derive(Debug, Clone)]
struct Features {
    /// Already Unicode Bengali. Converting would corrupt it — a hard stop.
    has_unicode_bengali: bool,
    /// No letters and no Bijoy-range characters: digits, punctuation, symbols.
    is_inert: bool,
    /// How many distinct Bijoy-range characters appear.
    distinct_exotic: usize,
    /// Share of the word's characters that are Bijoy-range.
    exotic_ratio: f32,
    /// What the word becomes if converted.
    converted_is_word: bool,
    /// Whether that conversion is even structurally possible Bengali.
    converted_plausible: bool,
    /// A common English word, which no amount of context should override.
    is_english: bool,
    /// How many letters and digits the word has. One is never enough.
    alphanumeric: usize,
}

/// Modern office vocabulary that a 1934 dictionary cannot contain.
///
/// The bulk of the English guard is [`Dictionary::english`], 234,428 words
/// from Webster's Second International. Its blind spot is everything coined
/// since: email, website, dataset, spreadsheet. These documents are written
/// in precisely that register, so the gap matters and this list closes it.
///
/// Kept as a belt-and-braces addition rather than on measured evidence: the
/// Webster-only variant was never run on its own, so this list has not been
/// shown to be load-bearing. If a later pass wants to trim the code, measure
/// without it first rather than assuming either way.
const ENGLISH_GUARD: &[&str] = &[
    "the", "and", "for", "are", "but", "not", "you", "all", "any", "can", "had", "her", "was",
    "one", "our", "out", "day", "get", "has", "him", "his", "how", "its", "may", "new", "now",
    "old", "see", "two", "way", "who", "boy", "did", "man", "men", "run", "she", "too", "use",
    "add", "age", "ago", "air", "bad", "bag", "bar", "bed", "big", "box", "buy", "car", "cut",
    "end", "eye", "far", "few", "fit", "fix", "fly", "gas", "got", "gun", "hit", "hot", "job",
    "key", "kid", "law", "let", "lie", "lot", "low", "map", "mix", "net", "oil", "own", "pay",
    "per", "put", "red", "row", "sat", "say", "set", "sit", "six", "son", "sum", "tax", "ten",
    "top", "try", "war", "win", "yes", "yet", "act", "aid", "arm", "art", "ask", "ban", "bit",
    "bus", "cap", "cost", "data", "date", "each", "from", "have", "here", "into", "item", "list",
    "made", "make", "more", "most", "much", "must", "name", "need", "next", "note", "only", "over",
    "page", "part", "plan", "rate", "role", "same", "show", "site", "some", "such", "team", "text",
    "than", "that", "them", "then", "they", "this", "time", "type", "unit", "used", "user", "very",
    "week", "were", "what", "when", "will", "with", "work", "year", "your", "total", "report",
];

impl Features {
    fn of(word: &str, dictionary: &Dictionary) -> Features {
        let has_unicode_bengali = word.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c));
        let is_inert = !word
            .chars()
            .any(|c| c.is_alphanumeric() || is_bijoy_range(c));

        let mut seen: Vec<char> = Vec::new();
        let mut exotic = 0usize;
        let mut considered = 0usize;
        for c in word.chars() {
            considered += 1;
            if is_bijoy_range(c) {
                exotic += 1;
                if !seen.contains(&c) {
                    seen.push(c);
                }
            }
        }
        let exotic_ratio = if considered == 0 {
            0.0
        } else {
            exotic as f32 / considered as f32
        };

        // Trial conversion is skipped where it could only mislead: text that is
        // already Bengali, and text with nothing convertible in it.
        let (converted_is_word, converted_plausible) = if has_unicode_bengali || is_inert {
            (false, false)
        } else {
            let converted = convert(word);
            let trimmed = trim_to_bengali(&converted);
            (
                !trimmed.is_empty() && dictionary.contains(trimmed),
                word_is_well_formed(&converted),
            )
        };

        // Two lists, union. The 234,428-word Webster list carries the bulk;
        // the short guard adds the modern office vocabulary a 1934 dictionary
        // could not have — email, website, dataset — which is exactly the
        // register these documents are written in.
        let lower = word.to_ascii_lowercase();
        let bare = lower.trim_matches(|c: char| !c.is_ascii_alphabetic());
        let is_english = word.is_ascii()
            && (ENGLISH_GUARD.contains(&bare) || Dictionary::english().contains_english(word));

        Features {
            alphanumeric: word.chars().filter(|c| c.is_alphanumeric()).count(),
            has_unicode_bengali,
            is_inert,
            distinct_exotic: seen.len(),
            exotic_ratio,
            converted_is_word,
            converted_plausible,
            is_english,
        }
    }
}

/// Strip anything that is not Bengali from both ends.
///
/// Real text carries brackets, full stops and colons. `প্রতিবেদন,` is the word
/// with a comma stuck to it, and asking a dictionary about the comma would
/// fail every such word for no reason at all.
fn trim_to_bengali(s: &str) -> &str {
    s.trim_matches(|c: char| !('\u{0980}'..='\u{09FF}').contains(&c))
}

/// Judge one word with no help from its neighbours.
fn judge_alone(word: &str, dictionary: &Dictionary) -> Verdict {
    let f = Features::of(word, dictionary);

    // Hard stops. Nothing below may overturn these.
    if f.has_unicode_bengali || f.is_inert {
        return Verdict::NotLegacy;
    }
    // A common English word stays English whatever else is true of it. This
    // is the guard on the error that matters: silently wrecking readable text.
    if f.is_english {
        return Verdict::NotLegacy;
    }

    // The strong signal: a Bijoy-range character AND a conversion that is a
    // real Bengali word. Noise does not clear both.
    if f.converted_is_word && f.distinct_exotic >= 1 {
        return Verdict::Legacy;
    }

    // Dense in Bijoy's own characters, and converts to something structurally
    // possible. This carries words the dictionary has never heard of — names,
    // places, technical terms — which are common and must not be abandoned
    // merely for being rare.
    if f.exotic_ratio >= 0.10 && f.distinct_exotic >= 2 && f.converted_plausible {
        return Verdict::Legacy;
    }

    // Pure ASCII that converts to a real word. Genuinely undecidable alone:
    // this is `bvg` (নাম) and it is also three Latin letters. Ask the
    // neighbours.
    //
    // Two characters minimum. A single letter converts to a single Bengali
    // letter, and single Bengali letters are words — so `I` becomes `ও` and
    // the English first person singular disappears from the middle of a
    // sentence. Measured: 24 occurrences in one half of the corpus, the
    // largest single source of genuine false positives. One character is not
    // evidence, however good its neighbours look.
    if f.converted_is_word && f.alphanumeric >= 2 {
        return Verdict::Uncertain;
    }

    // A single Bijoy-range character and a possible conversion. Weak on its
    // own; context may yet carry it.
    if f.distinct_exotic >= 1 && f.converted_plausible {
        return Verdict::Uncertain;
    }

    Verdict::NotLegacy
}

/// How far either side an uncertain word looks for help.
const CONTEXT_WINDOW: usize = 6;

/// Judge every word in a document, letting neighbours settle the doubtful ones.
///
/// Two passes. The first judges each word alone. The second revisits only the
/// `Uncertain` ones and asks whether they sit among words already confirmed as
/// legacy — a word surrounded by Bijoy is Bijoy, and the same word surrounded
/// by English is English.
///
/// An `Uncertain` word is **never** promoted on the strength of other uncertain
/// words. Only confirmed evidence counts, or a page of ambiguous ASCII would
/// talk itself into being Bengali.
pub fn classify_words(words: &[&str], dictionary: &Dictionary) -> Vec<Verdict> {
    let mut verdicts: Vec<Verdict> = words.iter().map(|w| judge_alone(w, dictionary)).collect();

    let confirmed: Vec<bool> = verdicts.iter().map(|v| *v == Verdict::Legacy).collect();

    for (i, verdict) in verdicts.iter_mut().enumerate() {
        if *verdict != Verdict::Uncertain {
            continue;
        }
        let lo = i.saturating_sub(CONTEXT_WINDOW);
        let hi = (i + CONTEXT_WINDOW + 1).min(confirmed.len());
        let nearby = confirmed[lo..hi].iter().filter(|c| **c).count();
        *verdict = if nearby > 0 {
            Verdict::Legacy
        } else {
            Verdict::NotLegacy
        };
    }
    verdicts
}

/// Convert a document, rewriting only the words that are genuinely legacy.
///
/// Every other byte — words left alone, spaces, tabs, line endings — is
/// reproduced exactly.
pub fn convert_words(input: &str) -> String {
    let dictionary = Dictionary::shipped();
    let segments: Vec<Segment> = tokenise(input);
    let words: Vec<&str> = segments
        .iter()
        .filter(|s| s.kind == Kind::Word)
        .map(|s| s.text)
        .collect();
    let verdicts = classify_words(&words, dictionary);

    let mut out = String::with_capacity(input.len());
    let mut w = 0usize;
    for segment in &segments {
        match segment.kind {
            Kind::Gap => out.push_str(segment.text),
            Kind::Word => {
                if verdicts[w] == Verdict::Legacy {
                    out.push_str(&convert(segment.text));
                } else {
                    out.push_str(segment.text);
                }
                w += 1;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn judge(word: &str) -> Verdict {
        judge_alone(word, Dictionary::shipped())
    }

    #[test]
    fn unicode_bengali_is_never_touched() {
        for word in ["প্রতিবেদন", "কর্মসূচি", "সাপ্তাহিক", "এবং"]
        {
            assert_eq!(
                judge(word),
                Verdict::NotLegacy,
                "would have converted {word}"
            );
        }
    }

    #[test]
    fn digits_and_punctuation_are_never_touched() {
        for word in ["2026", "12.5%", "(a)", "—", "...", "£40"] {
            assert_eq!(
                judge(word),
                Verdict::NotLegacy,
                "would have converted {word}"
            );
        }
    }

    #[test]
    fn genuine_bijoy_words_are_recognised_alone() {
        // Each carries a Bijoy-range character and converts to a real word.
        for (word, means) in [
            ("Kg\u{a9}m~wP", "কর্মসূচি"),
            ("cÖwZ\u{2020}e`b", "প্রতিবেদন"),
            ("\u{af}^v\u{ff}i", "স্বাক্ষর"),
        ] {
            assert_eq!(judge(word), Verdict::Legacy, "missed {word} ({means})");
        }
    }

    #[test]
    fn common_english_words_are_protected_whatever_else_is_true() {
        for word in [
            "the", "and", "report", "total", "data", "year", "The", "Report,",
        ] {
            assert_eq!(
                judge(word),
                Verdict::NotLegacy,
                "would have converted {word}"
            );
        }
    }

    /// The case the whole three-verdict design exists for.
    #[test]
    fn pure_ascii_bijoy_is_undecidable_alone_and_context_decides_it() {
        // `bvg` is নাম. It is also three Latin letters. Alone, it is honestly
        // uncertain.
        assert_eq!(judge("bvg"), Verdict::Uncertain);

        // Among Bijoy, it is Bijoy.
        let among_bijoy = ["Kg\u{a9}m~wP", "bvg", "cÖwZ\u{2020}e`b"];
        let v = classify_words(&among_bijoy, Dictionary::shipped());
        assert_eq!(v[1], Verdict::Legacy, "context failed to carry it");

        // Among English, it is left alone.
        let among_english = ["Programme", "operations", "bvg", "review", "team"];
        let v = classify_words(&among_english, Dictionary::shipped());
        assert_eq!(
            v[2],
            Verdict::NotLegacy,
            "English context did not protect it"
        );
    }

    /// Uncertain words must never vouch for each other.
    #[test]
    fn ambiguous_words_cannot_talk_themselves_into_being_bengali() {
        let all_ambiguous = ["bvg", "bvg", "bvg", "bvg", "bvg"];
        let v = classify_words(&all_ambiguous, Dictionary::shipped());
        assert!(
            v.iter().all(|x| *x == Verdict::NotLegacy),
            "a page of ambiguous ASCII convinced itself: {v:?}"
        );
    }

    /// The guarantee the whole feature rests on.
    #[test]
    fn everything_not_converted_is_reproduced_byte_for_byte() {
        for input in [
            "Programme operations and budget review for the 2026 cycle.",
            "সম্পূর্ণ ইউনিকোড বাংলা লেখা",
            "Region\tTotal\tBalance\nDhaka\t1200\t340",
            "  leading and trailing  \n\n",
            "",
        ] {
            assert_eq!(convert_words(input), input, "text was altered: {input:?}");
        }
    }

    /// A line that mixes all three, which is what real documents look like.
    #[test]
    fn only_the_legacy_words_change_in_a_mixed_line() {
        let input = "Report: Kg\u{a9}m~wP for 2026 এবং done";
        let out = convert_words(input);
        assert!(
            out.contains("কর্মসূচি"),
            "the legacy word was missed: {out:?}"
        );
        assert!(out.contains("Report:"), "English was altered: {out:?}");
        assert!(out.contains("2026"), "a number was altered: {out:?}");
        assert!(out.contains("এবং"), "Unicode Bengali was altered: {out:?}");
        assert!(out.contains("done"), "English was altered: {out:?}");
    }
}
