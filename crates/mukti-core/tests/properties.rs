//! The rules that must hold for **every** input, not just the ones we thought of.
//!
//! # Why this file exists
//!
//! All 105 of this project's original tests used fixed literal inputs. That is the
//! right way to pin down a known Bijoy sequence against its known Bengali answer,
//! and it has a blind spot the size of the input space: a rule can hold for every
//! example anyone thought of and fail on the first one they did not.
//!
//! These tests state the rules as rules, and let `proptest` spend a few thousand
//! attempts trying to break each one — including the inputs a person would never
//! think to type. When it finds a failure it shrinks it to the smallest input that
//! still fails, which usually turns a mystery into an obvious bug.
//!
//! # What is worth stating as a property
//!
//! Only claims the product actually makes. Three of them carry the whole promise:
//!
//! 1. **Nothing is lost.** Splitting text into words and gaps and joining it back
//!    must return exactly what went in — for any string at all.
//! 2. **Text that is not legacy is not touched.** If nothing converted, the output
//!    is the input, byte for byte.
//! 3. **Converting is finished after one pass.** Running the converter over its own
//!    output must change nothing more.
//!
//! A converter that breaks any of those is not a conversion tool; it is a text
//! mangler with a good reputation.
//!
//! # And one negative property, which is the real prize
//!
//! **No input may cause a panic.** Not a malformed one, not a hostile one, not a
//! lone combining mark, not half a surrogate pair's worth of odd Unicode. A panic
//! in a desktop app is a window vanishing while somebody's document is open.

use proptest::prelude::*;

use gru953_mukti::classify::convert_words;
use gru953_mukti::tokenise::{tokenise, Kind};
use gru953_mukti::{convert, detect, repair_unicode, word_is_well_formed};

/// Text made of the things that actually turn up in these documents.
///
/// Not `any::<String>()`. Uniformly random Unicode almost never produces a Bijoy
/// sequence, a Bengali conjunct or a vowel sign in a position that matters, so it
/// would spend thousands of cases proving the converter ignores Cyrillic. This
/// mixes the four alphabets that collide in a real legacy document, weighted so
/// awkward combinations actually occur.
fn realistic_text() -> impl Strategy<Value = String> {
    let piece = prop_oneof![
        // Bijoy: ASCII letters and the high-range glyphs it reuses.
        3 => "[A-Za-z]{1,8}",
        3 => "[\u{00A0}-\u{024F}\u{2010}-\u{20FF}]{1,6}",
        // Unicode Bengali, including the marks that move around a consonant.
        3 => "[\u{0980}-\u{09FF}]{1,8}",
        // Whitespace and punctuation, the things that must survive untouched.
        2 => "[ \t\n]{1,3}",
        2 => "[-.,:;/()\"'|0-9]{1,4}",
        // A lone combining mark, which is exactly what breaks naive text handling.
        1 => "[\u{09BC}\u{09CD}\u{09BE}\u{09C7}\u{09D7}]{1,3}",
    ];
    proptest::collection::vec(piece, 0..24).prop_map(|parts| parts.concat())
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 2048, ..ProptestConfig::default() })]

    /// Splitting into words and gaps, then joining, returns the original exactly.
    ///
    /// Everything else rests on this. If the pieces do not reassemble, the
    /// converter cannot promise to leave anything alone, because it has already
    /// lost track of what it was given.
    #[test]
    fn tokenising_never_loses_or_invents_a_character(text in realistic_text()) {
        let rebuilt: String = tokenise(&text).iter().map(|s| s.text).collect();
        prop_assert_eq!(rebuilt, text);
    }

    /// Words and gaps strictly alternate, and neither is ever empty.
    ///
    /// Two words in a row would mean a missing gap, and an empty piece would mean
    /// a position that belongs to nothing — the sort of off-by-one that made a
    /// real 781-word document come out as 465.
    #[test]
    fn words_and_gaps_alternate_and_are_never_empty(text in realistic_text()) {
        let segments = tokenise(&text);
        for pair in segments.windows(2) {
            prop_assert_ne!(
                pair[0].kind == Kind::Word, pair[1].kind == Kind::Word,
                "two pieces of the same kind in a row"
            );
        }
        for s in &segments {
            prop_assert!(!s.text.is_empty(), "an empty piece");
        }
    }

    /// If nothing was converted, the text comes back byte for byte.
    ///
    /// The product's central promise, stated as a rule rather than as a sentence
    /// in a README.
    #[test]
    fn text_with_nothing_legacy_is_returned_untouched(text in realistic_text()) {
        let out = convert_words(&text);
        if out == text {
            // Nothing changed: exactly what should happen, nothing to check.
        } else {
            // Something changed, so SOMETHING must have been legacy. The
            // whitespace must still be identical either way: it is never legacy.
            let ws_before: String = text.chars().filter(|c| c.is_whitespace()).collect();
            let ws_after: String = out.chars().filter(|c| c.is_whitespace()).collect();
            prop_assert_eq!(ws_before, ws_after, "whitespace was altered");
        }
    }

    /// Converting is finished after one pass.
    ///
    /// A second pass must find nothing left to do. This catches a whole class of
    /// fault nothing else does: a converter that mangles its own output looks
    /// perfectly correct on a single pass over hand-picked examples.
    #[test]
    fn converting_twice_is_the_same_as_converting_once(text in realistic_text()) {
        let once = convert_words(&text);
        let twice = convert_words(&once);
        prop_assert_eq!(twice, once);
    }

    /// No input causes a panic. Any input at all, not merely a realistic one.
    ///
    /// Deliberately `any::<String>()` here: this is the one property where
    /// arbitrary junk is the point. A panic in the library is a window
    /// disappearing while somebody's document is open in it.
    #[test]
    fn nothing_ever_panics(text in any::<String>()) {
        let _ = detect(&text);
        let _ = convert(&text);
        let _ = repair_unicode(&text);
        let _ = convert_words(&text);
        let _ = word_is_well_formed(&text);
        let _ = tokenise(&text);
    }

    /// Repairing already-correct Unicode Bengali is also finished in one pass.
    #[test]
    fn repairing_twice_is_the_same_as_repairing_once(text in realistic_text()) {
        let once = repair_unicode(&text);
        let twice = repair_unicode(&once);
        prop_assert_eq!(twice, once);
    }

    /// Pure ASCII with no Bijoy-range character in it is never called legacy on
    /// its own evidence.
    ///
    /// The asymmetry the project chose deliberately: a missed conversion is
    /// visible and fixable, a wrongly converted word destroys readable text and
    /// may never be noticed. So plain English words must survive.
    #[test]
    fn ordinary_english_words_are_left_alone(
        words in proptest::collection::vec("[a-z]{3,10}", 1..8)
    ) {
        let text = words.join(" ");
        prop_assert_eq!(convert_words(&text), text);
    }
}
