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
    // Superscripts and subscripts (U+2070-U+209F) sit inside the punctuation
    // block but Bijoy uses none of them. Counting them made `(SO₂)` and
    // `(CH₄)` look like dense Bijoy, and chemical formulae in an environmental
    // report became Bengali.
    if (0x2070..=0x209F).contains(&o) {
        return false;
    }
    (0x00A0..=0x024F).contains(&o) || (0x2010..=0x20FF).contains(&o)
}

/// A Roman-numeral list marker: `iv)`, `iii)`, `II.`, `(vi)`.
///
/// These are ordinary English document furniture, and each is also a valid
/// Bijoy string that converts to a real Bengali word — `iv)` becomes `রা)`.
///
/// **The list punctuation is required, and that is the whole subtlety.** A
/// first version matched any short word spelled from Roman-numeral letters,
/// and cost 2.2% of recall in one measurement: `cv` is পা, `ci` is পর, `mi`
/// is সর — common Bengali syllables built from exactly those letters. Fifty-two
/// false positives were fixed and roughly 3,900 real conversions lost, which
/// is a bad trade in the direction that matters least.
///
/// So the marker must LOOK like a list marker: numerals, then `)`, `.` or `:`,
/// optionally wrapped in brackets. Bare `iv` stays convertible.
fn is_roman_numeral_marker(word: &str) -> bool {
    if !word.is_ascii() {
        return false;
    }
    let trimmed = word.trim_start_matches('(');
    let Some(body) = trimmed
        .strip_suffix(')')
        .or_else(|| trimmed.strip_suffix('.'))
        .or_else(|| trimmed.strip_suffix(':'))
    else {
        return false;
    };
    !body.is_empty()
        && body.len() <= 4
        && body.chars().all(|c| {
            matches!(
                c.to_ascii_lowercase(),
                'i' | 'v' | 'x' | 'l' | 'c' | 'd' | 'm'
            )
        })
}

/// Does this carry a subscript or superscript digit?
///
/// `SO₂`, `CH₄`, `m³`. Bijoy has no such glyph, so their presence says the
/// token is scientific or mathematical notation rather than Bengali — and
/// leaving it merely *uncertain* is not enough, because a chemical formula in
/// an environmental report sits surrounded by Bengali and context would then
/// convert it.
fn has_sub_or_superscript(word: &str) -> bool {
    // ONLY the true sub/superscript block. The Latin-1 forms `¹ ² ³` look like
    // they belong here and do not: the conversion table uses all three as
    // Bijoy glyphs — `j²x` is লক্ষ্মী. Including them cost 2.2% of recall, which
    // is how they came to be checked against the table rather than assumed.
    word.chars()
        .any(|c| (0x2070..=0x209F).contains(&(c as u32)))
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
    /// The trial conversion itself, wherever one was computed. Carried so a
    /// caller that ends up needing this word converted for real -- because
    /// its verdict is `Legacy` -- can reuse this rather than calling
    /// `convert` a second time. `None` wherever no trial conversion was
    /// computed at all: see the hard-stop gate around where this is set.
    converted: Option<String>,
    /// A common English word, which no amount of context should override.
    is_english: bool,
    /// A short bracket/period/colon list marker: `iv)`, `II.`. Computed once
    /// here rather than a second time in `judge_alone`, since it is also
    /// needed early to decide whether the trial conversion below is worth
    /// running at all.
    is_roman_numeral_marker: bool,
    /// A true Unicode sub/superscript character is present. Same reason.
    has_sub_or_superscript: bool,
    /// How many letters and digits the word has. One is never enough.
    alphanumeric: usize,
}

/// Modern office vocabulary that a 1934 dictionary cannot contain.
///
/// The bulk of the English guard is [`Dictionary::english`], 465,971 words
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

        // Two lists, union. The 465,971-word Webster list carries the bulk;
        // the short guard adds the modern office vocabulary a 1934 dictionary
        // could not have — email, website, dataset — which is exactly the
        // register these documents are written in.
        let lower = word.to_ascii_lowercase();
        let bare = lower.trim_matches(|c: char| !c.is_ascii_alphabetic());
        // The raw word must be ASCII, and the big dictionary is asked about the
        // raw word — both deliberately, both measured.
        //
        // The obvious-looking improvement is to test `bare` instead, so that
        // `(Owners’` keeps its English protection: Word curled the apostrophe,
        // the word is therefore not ASCII, and it converts to `(ঙহিবৎং্থ`.
        // That was tried on 14 August 2026 and **rejected on measurement**.
        // Trimming the ends exposes short Bijoy cores that land on English
        // words, and detection recall fell from 99.962% to 98.936% — through
        // the 99% gate — while the English false-positive rate did not move at
        // all, staying at 0.014%. A thousand-odd real conversions lost for no
        // measurable gain. Words like `(Owners’` remain a known residue inside
        // the measured 0.014%; see R16l. Do not change this without re-running
        // `eval` and reading both numbers.
        // The `is_ascii` test is relaxed for a TRAILING run of word-processor
        // typography, and for nothing else.
        //
        // This is a much narrower change than the one rejected above, and the
        // difference is the whole point. That one made the entire gate use `bare`,
        // which trims **both** ends of **any** non-alphabetic character, exposing
        // short Bijoy cores everywhere. This only stops six specific characters --
        // the curled quotes and the en/em dashes Word substitutes -- from defeating
        // the ASCII test when they sit at the END of a token. `Harm’` was converting
        // to `ঐধৎস্থ` purely because Word had curled the apostrophe: 11 such tokens in
        // 7 of 1,059 real documents on 19 August 2026, three of them inside live
        // spreadsheet formulas, where a transliterated identifier breaks the formula
        // rather than merely looking wrong.
        //
        // The residual risk is real and bounded: genuine Bijoy that happens to end in
        // one of these six characters, and whose trimmed core happens to be an
        // English word, would now be protected and so missed. That is why this ships
        // only with `eval` re-run and both recall and the false-positive rate read.
        let without_typography = word.trim_end_matches(|c: char| {
            matches!(
                c,
                '\u{2018}' | '\u{2019}' | '\u{201C}' | '\u{201D}' | '\u{2013}' | '\u{2014}'
            )
        });
        // Note what this does NOT do. `contains_english` already trims both ends of
        // whatever it is given, so the both-ends trimming the rejected fix was blamed
        // for is *already* how an ASCII word is looked up today. The only thing that
        // changes here is that a trailing curl no longer stops the lookup happening
        // at all -- `contains_english` bails out on its own `is_ascii` check before it
        // ever reaches the dictionary.
        // A FOUR-character floor on the typography path, and it is not arbitrary.
        //
        // Without it the relaxation costs recall in exactly the way the wider fix did.
        // Compared against v0.6.1 over 400 real documents: `Mi“` -- genuine Bijoy for
        // গরু -- trimmed to `Mi`, whose lower-cased core `mi` is a Webster's headword,
        // so the word was protected as English and stopped converting. Short cores are
        // where Bijoy collides with English; the words this fix exists for are not
        // short (`Harm`, `Owners`, `judgment`). Four characters keeps every one of
        // those and excludes the collisions found.
        let typography_core_is_long_enough = without_typography.len() != word.len()
            && without_typography
                .chars()
                .filter(|c| c.is_ascii_alphabetic())
                .count()
                >= 4;
        let is_english = if without_typography.len() == word.len() {
            // No typography was trimmed: the original behaviour, unchanged.
            word.is_ascii()
                && (ENGLISH_GUARD.contains(&bare) || Dictionary::english().contains_english(word))
        } else {
            typography_core_is_long_enough
                && Dictionary::english().contains_english(without_typography)
        };

        let is_roman_numeral_marker = is_roman_numeral_marker(word);
        let has_sub_or_superscript = has_sub_or_superscript(word);

        // Trial conversion is skipped wherever `judge_alone`'s two hard stops
        // would discard it unread: text that is already Bengali or has
        // nothing convertible in it (hard stop 1), and a common English
        // word, a roman-numeral list marker, or a sub/superscript (hard
        // stop 2). Every one of these ends `judge_alone` before
        // `converted_is_word`/`converted_plausible` is ever consulted, so
        // computing them was pure waste on exactly the words most documents
        // are mostly made of. The trial conversion is the single most
        // expensive thing this function can do -- the full 223-entry
        // table-scan `convert()` pipeline, plus a `word_is_well_formed`
        // structural check -- so skipping it here is where the payoff is.
        //
        // Provably output-identical: `judge_alone` returns at one of the two
        // hard stops before it ever reads `converted_is_word` or
        // `converted_plausible`, so whatever value they hold on that path
        // cannot affect the verdict. Moving the computation earlier or later
        // changes nothing but how much of it happens.
        let hard_stop = has_unicode_bengali
            || is_inert
            || is_english
            || is_roman_numeral_marker
            || has_sub_or_superscript;
        let (converted_is_word, converted_plausible, converted) = if hard_stop {
            (false, false, None)
        } else {
            let converted = convert(word);
            let trimmed = trim_to_bengali(&converted);
            let is_word = !trimmed.is_empty() && dictionary.contains(trimmed);
            let plausible = word_is_well_formed(&converted);
            (is_word, plausible, Some(converted))
        };

        Features {
            alphanumeric: word.chars().filter(|c| c.is_alphanumeric()).count(),
            has_unicode_bengali,
            is_inert,
            distinct_exotic: seen.len(),
            exotic_ratio,
            converted_is_word,
            converted_plausible,
            converted,
            is_english,
            is_roman_numeral_marker,
            has_sub_or_superscript,
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
///
/// `classify_words_with_conversions` no longer calls this directly -- it
/// needs the `Features` itself, to keep the trial conversion alongside the
/// verdict -- so this now exists for tests, which want the verdict alone and
/// do not care that `verdict_from_features` is one call away.
#[cfg(test)]
fn judge_alone(word: &str, dictionary: &Dictionary) -> Verdict {
    verdict_from_features(&Features::of(word, dictionary))
}

/// The decision itself, read from `Features` alone.
///
/// Extracted from `judge_alone` so that `classify_words_with_conversions` can
/// compute `Features` once per word, keep the trial conversion it carries,
/// and still call exactly this same judgement -- one place the verdict logic
/// is written down, however many things end up wanting the `Features` behind
/// it.
fn verdict_from_features(f: &Features) -> Verdict {
    // Hard stops. Nothing below may overturn these.
    if f.has_unicode_bengali || f.is_inert {
        return Verdict::NotLegacy;
    }
    // A common English word stays English whatever else is true of it. This
    // is the guard on the error that matters: silently wrecking readable text.
    //
    // Read from `Features`, not recomputed: `Features::of` already needed
    // both of these, earlier, to decide whether the trial conversion was
    // worth running at all.
    if f.is_english || f.is_roman_numeral_marker || f.has_sub_or_superscript {
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
    // Two distinct glyphs, not one. Relaxing this to one was tried and
    // reverted: it bought 0.27% recall and cost a twentyfold rise in false
    // positives on digits and punctuation (0.040% -> 0.797%), breaking the
    // aggregate gate. The bar stays where the measurement put it.
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

/// Which rule inside `verdict_from_features` decided a word's verdict, judged
/// alone — for diagnostics only. Never called from the shipped classifier;
/// `eval`'s Step 0 breakdown of *why* a `legacy_ascii` token was refused is
/// the reason this exists, replacing an estimate with a fact.
///
/// Mirrors `verdict_from_features`'s branches exactly, in the same order, so
/// the two cannot silently drift apart. A word whose alone-verdict is
/// `Uncertain` and stays that way is reported by which `Uncertain` branch
/// produced it — "no confirmed neighbour rescued it" is `classify_words`'s
/// job to know, not this function's, since only the full document gives that
/// answer.
pub fn diagnose_alone(word: &str, dictionary: &Dictionary) -> &'static str {
    let f = Features::of(word, dictionary);
    if f.has_unicode_bengali || f.is_inert {
        return "hard_stop_unicode_or_inert";
    }
    if f.is_english {
        return "hard_stop_english";
    }
    if f.is_roman_numeral_marker {
        return "hard_stop_roman_numeral_marker";
    }
    if f.has_sub_or_superscript {
        return "hard_stop_sub_or_superscript";
    }
    if f.converted_is_word && f.distinct_exotic >= 1 {
        return "legacy_byte_and_dictionary";
    }
    if f.exotic_ratio >= 0.10 && f.distinct_exotic >= 2 && f.converted_plausible {
        return "legacy_density";
    }
    if f.converted_is_word && f.alphanumeric >= 2 {
        return "uncertain_dictionary_word";
    }
    if f.distinct_exotic >= 1 && f.converted_plausible {
        return "uncertain_plausible";
    }
    if f.converted_is_word {
        // A single-character dictionary hit: reachable only by the `>= 2`
        // floor turning it away, since a lone alphanumeric character cannot
        // reach here any other way.
        "alphanumeric_floor"
    } else {
        "not_a_dictionary_word_and_not_plausible"
    }
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
    classify_words_with_conversions(words, dictionary)
        .into_iter()
        .map(|(verdict, _converted)| verdict)
        .collect()
}

/// `classify_words`, but also returning each word's trial conversion
/// wherever `Features::of` computed one.
///
/// Reusing this string is what closes the double conversion this classifier
/// used to cost every legacy word: once here, as the trial that decides the
/// verdict, and once more by the caller -- `convert_pieces`,
/// `office::rewrite_part` -- to actually rewrite the word. `None` wherever no
/// trial conversion was computed at all: a hard stop was hit first (already
/// Unicode Bengali or inert, a common English word, a roman-numeral list
/// marker, a sub/superscript).
///
/// The conversion survives context promotion untouched. A word promoted from
/// `Uncertain` to `Legacy` by its neighbours must itself have passed both
/// hard stops to reach `Uncertain` at all -- see `verdict_from_features` --
/// so its trial conversion was already computed in the first pass below and
/// needs no second look.
pub fn classify_words_with_conversions(
    words: &[&str],
    dictionary: &Dictionary,
) -> Vec<(Verdict, Option<String>)> {
    let features: Vec<Features> = words.iter().map(|w| Features::of(w, dictionary)).collect();
    let mut verdicts: Vec<Verdict> = features.iter().map(verdict_from_features).collect();

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
        .into_iter()
        .zip(features)
        .map(|(verdict, f)| (verdict, f.converted))
        .collect()
}

/// Convert a document, rewriting only the words that are genuinely legacy.
///
/// Every other byte — words left alone, spaces, tabs, line endings — is
/// reproduced exactly.
/// One piece of a converted document, and whether the converter changed it.
///
/// Gaps — spaces, punctuation, line breaks — are pieces too, and never changed.
/// Keeping them means the pieces can be joined back into the original document
/// exactly, and lets an interface show precisely which words were touched.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Piece {
    pub text: String,
    pub changed: bool,
    /// Whether this piece was a word at all. Gaps are pieces too, and counting
    /// them as untouched words would inflate every total this project prints.
    pub word: bool,
}

/// Convert every legacy word in the text, and say which ones changed.
///
/// **The one place this decision is made.** Until 14 August 2026 this loop was
/// written out four times — here, in `mukti_formats::convert_text_with_summary`,
/// in the command-line tool's `convert_and_count` and in every other
/// `convert_str`. Four copies of a judgement is four chances for the tool and
/// caller to disagree about the same file, and every accuracy figure this
/// project publishes is a claim about *one* judgement. Everything else now
/// counts or joins these pieces; nothing else decides.
pub fn convert_pieces(input: &str) -> Vec<Piece> {
    let dictionary = Dictionary::shipped();
    let segments: Vec<Segment> = tokenise(input);
    let words: Vec<&str> = segments
        .iter()
        .filter(|s| s.kind == Kind::Word)
        .map(|s| s.text)
        .collect();
    // `_with_conversions`: reuses each `Legacy` word's trial conversion
    // below instead of calling `convert` on it a second time.
    let judged = classify_words_with_conversions(&words, dictionary);
    let mut judged = judged.into_iter();

    let mut pieces = Vec::with_capacity(segments.len());
    for segment in &segments {
        match segment.kind {
            Kind::Gap => pieces.push(Piece {
                text: segment.text.to_owned(),
                changed: false,
                word: false,
            }),
            Kind::Word => {
                let (verdict, converted) = judged.next().expect("one verdict per word segment");
                let changed = verdict == Verdict::Legacy;
                pieces.push(Piece {
                    text: if changed {
                        // Reuse the trial conversion computed while judging
                        // this word. Available whenever the verdict is
                        // `Legacy`; see `classify_words_with_conversions`.
                        // The fallback is a defence that must never actually
                        // run.
                        converted.unwrap_or_else(|| convert(segment.text))
                    } else {
                        // A word that is NOT legacy is returned as it came, with one
                        // exception: Bengali's two-part vowel signs are composed.
                        //
                        // This is Unicode NFC and nothing else -- `ে`+`া` becomes `ো`,
                        // which is the same character by definition, so the meaning
                        // cannot change and neither can what a reader sees. What does
                        // change is that the word becomes findable: a search for `ো`
                        // does not match `ে`+`া` in Word, a browser or a database, and
                        // being searchable is the whole point of converting at all.
                        // A 1,059-document run on 19 August 2026 found 1,688 words
                        // written the decomposed way, none of them Mukti's doing.
                        //
                        // It is NOT counted as a conversion, because it is not one --
                        // no legacy text was involved. The count a user is shown stays
                        // the count of legacy words converted.
                        crate::compose_canonical_vowels(segment.text)
                    },
                    changed,
                    word: true,
                });
            }
        }
    }
    pieces
}

/// How many words changed, and how many were left alone.
pub fn count(pieces: &[Piece]) -> (usize, usize) {
    let converted = pieces.iter().filter(|p| p.changed).count();
    let untouched = pieces.iter().filter(|p| p.word && !p.changed).count();
    (converted, untouched)
}

pub fn convert_words(input: &str) -> String {
    convert_pieces(input).into_iter().map(|p| p.text).collect()
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

    /// Numbered lists and chemical formulae are English document furniture.
    #[test]
    fn list_markers_and_formulae_are_left_alone() {
        // Every one of these converts to a real Bengali word, which is exactly
        // why they need naming: `iv)` becomes `রা)`.
        for marker in ["i)", "iv)", "iii)", "(vi)", "II.", "x.", "(iv)"] {
            assert_eq!(judge(marker), Verdict::NotLegacy, "converted {marker}");
        }
        // Subscripts are not Bijoy glyphs.
        for formula in ["(SO\u{2082})", "(CH\u{2084})", "(O\u{2083})"] {
            assert_eq!(judge(formula), Verdict::NotLegacy, "converted {formula}");
        }
        // Bijoy syllables are spelled from these very letters. Without the
        // list punctuation they must NOT be taken for numerals — an earlier
        // version did, and cost 2.2% of recall.
        for bijoy in ["cv", "ci", "mi", "iv", "vi", "dv"] {
            assert!(
                !is_roman_numeral_marker(bijoy),
                "{bijoy} taken for a numeral"
            );
        }
        for word in ["did", "mild", "civil", "climax"] {
            assert!(!is_roman_numeral_marker(word), "{word} taken for a numeral");
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

    /// Word's curled apostrophe must not strip an English word of its protection.
    ///
    /// `Harm’` was converting to `ঐধৎস্থ` for one reason only: the curl makes the
    /// token non-ASCII, the ASCII gate then refused to consult the English
    /// dictionary, and the forced conversion looked plausible enough to accept.
    /// Measured on 19 August 2026 over 1,059 real documents: 11 such tokens in 7
    /// files, three of them inside live spreadsheet formulas.
    ///
    /// The fix relaxes the ASCII test for a trailing run of six typographic
    /// characters and nothing else. `eval` was re-run against it and every figure
    /// held -- detection recall stayed at 99.962% and the English false-positive rate
    /// at 0.014% -- which is what the earlier, wider version of this fix could not
    /// manage: it cost a full point of recall.
    #[test]
    fn english_keeps_its_protection_through_word_processor_typography() {
        // Each of these ends in punctuation Word substitutes automatically.
        //
        // `Owners’` -- the example the source comment above has named since 14 August
        // -- is deliberately NOT here, and the reason is a separate defect. The
        // embedded English list is Webster's 1934 headwords, which are singular only:
        // `owner` is present and `owners` is not, as are `member`/`members` and
        // `meeting`/`meetings`. So English plurals get no dictionary protection at
        // all, whatever their punctuation. That is a dictionary gap, not a gate gap,
        // and fixing it here would mean guessing at morphology instead.
        for word in [
            "Harm\u{2019}", // right single quote
            "Decide\u{2019}",
            "position\u{2014}", // em dash
            "report\u{201D}",   // right double quote
            "budget\u{2013}",   // en dash
        ] {
            // Concatenation, not format!: a \u{..} escape inside a format string
            // cannot be brace-escaped, because Rust lexes the escape first.
            let line = word.to_string() + " Kg\u{a9}m~wP";
            let out = convert_words(&line);
            assert!(
                out.starts_with(word),
                "an English word lost its protection to trailing typography: \
                 {word:?} became {out:?}"
            );
            assert!(
                out.contains("কর্মসূচি"),
                "the legacy word beside it should still convert: {out:?}"
            );
        }
    }

    /// Already-Unicode Bengali is composed to NFC, and otherwise left alone.
    ///
    /// This is the one exception to "text that is not legacy comes back exactly as it
    /// went in", added 19 August 2026 and deliberately confined to Unicode canonical
    /// equivalence: `ে`+`া` becomes `ো`, the same character by definition. 1,688 words
    /// in a 1,059-document run arrived written the decomposed way -- not Mukti's doing
    /// -- and were invisible to a search for the composed spelling.
    #[test]
    fn already_unicode_bengali_is_normalised_but_not_otherwise_altered() {
        // মোট and লোক, spelled the decomposed way.
        let decomposed = "\u{09AE}\u{09C7}\u{09BE}\u{099F} \u{09B2}\u{09C7}\u{09BE}\u{0995}";
        let out = convert_words(decomposed);
        assert_eq!(
            out, "মোট লোক",
            "the decomposed spelling was not composed: {out:?}"
        );
        assert!(
            !out.contains("\u{09C7}\u{09BE}"),
            "a decomposed vowel survived: {out:?}"
        );

        // Nothing else about a non-legacy word may change.
        for untouched in [
            "Report 2026",
            "এবং সদস্য", // already-composed Bengali
            "email@example.com",
            "কর্মসূচি",
        ] {
            assert_eq!(
                convert_words(untouched),
                untouched,
                "text with nothing to normalise was altered"
            );
        }

        // And a normalisation is NOT reported as a conversion.
        let pieces = convert_pieces(decomposed);
        let (converted, _) = count(&pieces);
        assert_eq!(
            converted, 0,
            "composing a vowel is not a legacy conversion and must not be counted as one"
        );
    }
}
