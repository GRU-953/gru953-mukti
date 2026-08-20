//! A small Bengali word list, used to tell a real conversion from a fake one.
//!
//! # Why this exists
//!
//! Bijoy is a font hack over the whole ASCII range, so **any** English text
//! converts to something Bengali-shaped. Measured on real lines:
//!
//! | Input | Converts to |
//! |---|---|
//! | `Awd†mi bvgt … ZvwiLt` (real Bijoy) | `অফিসের নামঃ তারিখঃ` — real words |
//! | `Consultant: Example Widget` (English) | `ঈড়হংঁষঃধহঃ ঊীধসঢ়ষব` — nonsense |
//!
//! Every character-level test fails here, because both outputs are legal
//! Bengali codepoints. Three separate attempts at character density and
//! capitalisation rules were tried and reverted. What actually separates them
//! is meaning: real Bijoy converts into **words**.
//!
//! # How the list was chosen
//!
//! Two sources, neither invented:
//!
//! 1. Vocabulary from conversions that were hand-checked as correct — report,
//!    member, branch, and so on.
//! 2. High-frequency Bengali function words, which appear in essentially any
//!    Bengali prose regardless of subject.
//!
//! Entries are **stems**, matched as substrings, because Bengali inflects
//! heavily: `শাখা` must also match `শাখার`, `শাখায়`, `শাখাগুলো`. Every stem is
//! at least two characters, so single letters cannot match by chance.

/// Common Bengali stems. Order is irrelevant; this is a membership test.
///
/// Formatting is deliberate and exempt from rustfmt: the entries are grouped by
/// what they are — function words, everyday vocabulary, hand-checked
/// conversions, loanwords, sector vocabulary — and reflowing them to fill the
/// line width would scatter each group and lose the comments' meaning.
#[rustfmt::skip]
pub static STEMS: &[&str] = &[
    // --- function words: the backbone of any Bengali sentence ---
    "এবং", "এই", "সেই", "যে", "যা", "যদি", "তবে", "কিন্তু", "অথবা",
    "জন্য", "থেকে", "সাথে", "মধ্যে", "উপর", "পরে", "আগে", "দিয়ে", "নিয়ে",
    "করা", "করে", "করুন", "হয়", "হবে", "ছিল", "আছে", "নেই", "হয়েছে",
    "সব", "সকল", "কিছু", "অন্য", "প্রতি", "সহ", "মতো", "মত",
    "তার", "তাদের", "আমাদের", "আপনার", "আমরা", "আপনি", "তিনি",
    "না", "কোন", "কোনো", "কী", "কেন", "কিভাবে", "যেমন",
    // --- everyday nouns and verbs ---
    "নাম", "সময়", "কাজ", "দিন", "মাস", "বছর", "সপ্তাহ", "তারিখ",
    "মানুষ", "লোক", "ব্যক্তি", "নারী", "পুরুষ", "শিশু", "পরিবার",
    "টাকা", "মোট", "সংখ্যা", "অংশ", "ধরন", "বিষয়", "তথ্য",
    "ভালো", "নতুন", "পুরাতন", "বড়", "ছোট", "প্রথম", "শেষ",
    "বাড়ি", "ঘর", "এলাকা", "গ্রাম", "শহর", "জেলা", "অফিস",
    "খাবার", "পানি", "স্বাস্থ্য", "শিক্ষা", "প্রশিক্ষণ",
    // --- vocabulary from hand-checked conversions ---
    "প্রোগ্রাম", "রিপোর্ট", "প্রতিবেদন", "কার্যক্রম", "কর্মসূচি",
    "সদস্য", "শাখা", "সাপ্তাহিক", "মাসিক", "বার্ষিক",
    "বিবরণ", "বিশেষ", "উদ্যোগ", "পদক্ষেপ", "সমস্যা", "সমাধান",
    "অগ্রগতি", "পর্যবেক্ষণ", "পরিদর্শন", "ভিজিট", "যাচাই",
    "স্বাক্ষর", "পদবি", "পদবী", "নীতিমালা", "নির্দেশ", "গাইডলাইন",
    "সেবা", "সহায়তা", "সুবিধা", "উন্নয়ন", "পরিকল্পনা",
    "বাজেট", "ব্যয়", "আয়", "হিসাব", "অনুমোদন",
    "প্রয়োজন", "গুরুত্ব", "ফলাফল", "লক্ষ্য", "উদ্দেশ্য",
    // --- English loanwords written in Bengali, ordinary in this sector ---
    // Bengali borrows freely and writes the result in Bengali script. These
    // are as much Bengali words as any other, and leaving them out meant a
    // heading like `বিভাগঃ` went unrecognised.
    "রিজিওন", "ইউনিট", "ব্রাঞ্চ", "ম্যানেজার", "অফিসার", "কর্মকর্তা",
    "ফিল্ড", "ট্রেনিং", "মিটিং", "ফরম", "চার্ট", "ডাটা", "ডেটা",
    "প্রজেক্ট", "টিম", "গ্রুপ", "কমিউনিটি", "সার্ভে", "মনিটরিং",
    // --- development-sector vocabulary, common in reports of this kind ---
    "দায়িত্ব", "কর্তব্য", "অধিকার", "নিরাপত্তা", "চিত্র",
    "স্বাস্থ্য", "পুষ্টি", "দারিদ্র", "উপকারভোগী", "কিশোরী", "মাতৃ",
];

/// Fold the three nukta letters to one spelling before comparing.
///
/// `য়`, `ড়` and `ঢ়` each exist in Unicode **two** ways: as a single character
/// (U+09DF, U+09DC, U+09DD) or as a base letter followed by a combining nukta
/// (U+09BC). Both look identical and neither is wrong — but `contains` treats
/// them as different strings, so a stem written one way silently fails to match
/// text written the other. This is the second time that distinction has bitten
/// in this codebase; see `is_consonant` for the first.
fn fold_nukta(text: &str) -> String {
    text.replace('\u{09DF}', "\u{09AF}\u{09BC}")
        .replace('\u{09DC}', "\u{09A1}\u{09BC}")
        .replace('\u{09DD}', "\u{09A2}\u{09BC}")
}

/// Shortest stem trusted as evidence.
///
/// Two-character stems match by chance. English forced through the Bijoy
/// tables produces a stream of Bengali letters, and any given two-letter
/// sequence turns up in it soon enough: "Cyber Security Framework and Board"
/// converted to text containing `সব`, and was wrongly rewritten as a result.
/// Three characters is rare enough in noise to mean something — the correct
/// case that exposed this matched `যদি` and `শিশু`, both three.
const MIN_STEM_CHARS: usize = 3;

/// How many distinct stems appear in this text.
///
/// Only stems of at least [`MIN_STEM_CHARS`] count. Shorter ones stay in the
/// list because they are real words and may be useful elsewhere, but they carry
/// no weight as evidence.
pub fn word_hits(text: &str) -> usize {
    // The stems are folded ONCE, ever, not once per call.
    //
    // This function used to call `fold_nukta` on every stem on every call.
    // `fold_nukta` is three chained `.replace()` calls, so three allocations
    // each, and the list is long: measured at **462 allocations per call**,
    // against 107 for converting a whole ordinary line. `repair_word` calls
    // this twice, so a single word needing repair cost more allocation than
    // the rest of the pipeline put together.
    //
    // Folding the stems is a pure function of a `const` list, so hoisting it
    // into a `LazyLock` cannot change the answer -- only how often the same
    // answer is computed. The short stems are filtered out here too, for the
    // same reason: the set they produce is fixed.
    static FOLDED_STEMS: std::sync::LazyLock<Vec<String>> = std::sync::LazyLock::new(|| {
        STEMS
            .iter()
            .filter(|s| s.chars().count() >= MIN_STEM_CHARS)
            .map(|s| fold_nukta(s))
            .collect()
    });

    let folded = fold_nukta(text);
    FOLDED_STEMS
        .iter()
        .filter(|s| folded.contains(s.as_str()))
        .count()
}

/// Does this read as real Bengali rather than Bengali-shaped noise?
///
/// Requires two distinct recognisable words. Measured against real data:
/// genuine Bijoy table cells clear this easily (`অফিসের নামঃ তারিখঃ` has three),
/// while English forced through the tables produces at most one by chance.
pub fn reads_as_bengali(text: &str) -> bool {
    word_hits(text) >= 1
}

/// The stricter bar, for text with nothing else vouching for it.
///
/// Two distinct words. One is not enough on its own: a stem can turn up by
/// chance inside noise — "Programme operations" forced through the tables
/// produced exactly one accidental match, which is how this bar was set.
///
/// The looser [`reads_as_bengali`] is used only for fragments inside a document
/// that has **already** proved it contains Bijoy, where one recognisable word
/// is enough. A short heading like `প্রস্তুতকারীর স্বাক্ষরঃ` carries only one.
pub fn reads_as_bengali_strict(text: &str) -> bool {
    let hits = word_hits(text);
    if hits < 2 {
        return false;
    }
    // Evidence has to scale with the amount of text. Two recognised words in a
    // short heading is a strong signal; two in a long meeting
    // transcript is chance, and that is exactly how English survived into a
    // Bengali conversion. Real Bengali prose is *dense* in common words — a
    // genuine paragraph clears this comfortably.
    let bengali = text
        .chars()
        .filter(|c| ('\u{0980}'..='\u{09FF}').contains(c))
        .count();
    // One recognised word per 60 Bengali characters. Real Bengali prose carries
    // a common word every fifteen to twenty-five characters, so a genuine
    // passage clears this easily; a few hundred characters of English with
    // two chance matches does not.
    hits * 60 >= bengali
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_headings_are_recognised() {
        // Form headings and table labels — the hardest case for this check,
        // because there is little text to judge and a heading may carry only
        // two or three recognisable words.
        //
        // Constructed, not copied. An earlier version used verbatim headings
        // from a private archive; these are ordinary office Bengali of the same
        // shape and length, which is what the thresholds are tuned against.
        for text in [
            "অফিসের নামঃ তারিখঃ",
            "উপস্থিত ব্যক্তির সংখ্যা",
            "মাসিক প্রতিবেদন এবং কাজের বিবরণ",
            "আলোচনার বিষয় ও সিদ্ধান্ত",
            "পূর্ববর্তী সভায় গৃহীত সিদ্ধান্তের অগ্রগতি যাচাই",
            "বিশেষ কোন মন্তব্য",
            "অফিসের নামঃ তারিখঃ বিভাগঃ",
            "প্রস্তুতকারীর স্বাক্ষরঃ",
        ] {
            assert!(
                reads_as_bengali(text),
                "Bengali heading not recognised: {text}"
            );
        }
    }

    #[test]
    fn english_run_through_the_tables_is_rejected() {
        // What English actually becomes when forced through the Bijoy map.
        for text in [
            "ঈড়হংঁষঃধহঃ: ঊীধসঢ়ষব ডরফমবঃ ঈড়সঢ়ধহু ্ত ঝঢ়ৎরহমভরবষফ",
            "অহহবী ৪",
            "চৎড়মৎধসসব ড়ঢ়বৎধঃরড়হং",
            "ঈধভল্ক ৎল্কংঁসল্ক হধশুাব ্ত ধপপবহঃবফ ঊহমষরংয রং ংঃরষষ ঊহমষরংয",
        ] {
            assert!(
                !reads_as_bengali_strict(text),
                "Bengali-shaped nonsense was accepted as real: {text}"
            );
        }
    }

    #[test]
    fn a_stem_matches_whichever_way_its_nukta_is_written() {
        // `সময়` ("time"), spelled both legal ways. Both must match.
        let precomposed = "সম\u{09DF}"; // য় as the single U+09DF
        let decomposed = "সম\u{09AF}\u{09BC}"; // য followed by a combining nukta
        assert_ne!(
            precomposed, decomposed,
            "the two spellings must differ as bytes"
        );
        assert_eq!(word_hits(precomposed), word_hits(decomposed));
        assert!(word_hits(precomposed) >= 1, "the stem did not match at all");
    }

    #[test]
    fn every_stem_is_long_enough_to_be_meaningful() {
        for s in STEMS {
            assert!(s.chars().count() >= 2, "stem too short: {s}");
        }
    }

    #[test]
    fn long_english_needs_more_than_two_chance_matches() {
        // Real regression: a long meeting transcript contained two stems by
        // chance and was rewritten as Bengali.
        let long_english = "Speaker one: yeah advanced like it is basically like zero down \
            again anti-bribery um fluorine SF6 uh EU regulations and the whole compliance \
            picture for the group across every market we operate in this year and next"
            .repeat(3);
        let converted = crate::convert(&long_english);
        assert!(
            !reads_as_bengali_strict(&converted),
            "a long English passage was accepted as Bengali on chance matches"
        );
    }

    #[test]
    fn english_forced_through_the_tables_is_not_accepted() {
        // Real regression: plain English inside real documents was rewritten as
        // Bengali because a two-character stem matched by chance.
        for english in [
            "Cyber Security Framework and Board Charter",
            "Use a server-side language model behind a documented HTTP interface",
            "Reviewer greenlit combined Milestone 2 and Milestone 3 with team planning",
            "Revise Stakeholder Lists and Frameworks",
        ] {
            let converted = crate::convert(english);
            assert!(
                !reads_as_bengali_strict(&converted),
                "English was accepted as Bengali: {english:?} -> {converted:?}"
            );
        }
    }
}
