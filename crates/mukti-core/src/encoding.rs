//! Decide whether a run of bytes is UTF-8 or Windows-1252, and decode it.
//!
//! # Nothing the `mukti` command does reaches this module
//!
//! **This is not on the shipped path, and that sentence is a fact rather than an
//! aspiration.** Version 0.9.0 narrowed the tool to six Office formats, all of
//! which are Zip archives holding XML that declares its own encoding, so the
//! guess this module makes is one the converter no longer has to make. `.txt`,
//! `.csv` and `.md` are refused by the six-format gate in `mukti-cli` before a
//! byte is read.
//!
//! It is kept, deliberately, for one caller: `devtools/corpus-verify` uses
//! [`decode`] to read the English-only negative corpus — 311 Markdown files, 66
//! notebooks and 8 Python files that must come through a conversion completely
//! untouched. A single converted word in there is a false positive and a real
//! defect, which makes that check one of this project's better safety nets, and
//! it needs to read those files as text to run at all.
//!
//! # Why the guess is needed where it IS still made
//!
//! A Bijoy document saved as plain text is almost never UTF-8. It was typed on
//! Windows, so it is **Windows-1252**, and the glyphs Bijoy leans on live in
//! exactly the byte range where Windows-1252 and Unicode disagree:
//!
//! | Byte | Windows-1252 | What Bijoy means by it |
//! |---|---|---|
//! | `0x86` | `†` | the vowel sign `ে` |
//! | `0x87` | `‡` | the same sign, another form |
//! | `0xA9` | `©` | reph |
//! | `0x93` | `“` | part of a conjunct |
//!
//! Read those bytes as UTF-8 and they either fail to decode outright or come
//! back as replacement characters — and text that has become replacement
//! characters cannot be checked for having been wrongly converted, because the
//! damage the check is looking for is no longer distinguishable from the damage
//! the decode did.
//!
//! # How the choice is made
//!
//! UTF-8 first, always. It is self-validating: a byte sequence that decodes
//! cleanly as UTF-8 is overwhelmingly unlikely to have been meant as anything
//! else, so a modern Unicode file is never mangled. Only when that fails does
//! Windows-1252 get its turn, and being a single-byte encoding it cannot fail
//! — every byte maps to something.

/// What the bytes turned out to be.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextEncoding {
    /// Decoded cleanly as UTF-8.
    Utf8,
    /// Not valid UTF-8, so read as Windows-1252. Ordinary for legacy Bangla.
    Windows1252,
}

/// The 0x80–0x9F range, where Windows-1252 differs from Latin-1.
///
/// Latin-1 leaves these as invisible control codes; Windows-1252 puts
/// printable characters there — and they are precisely the ones Bijoy uses for
/// its vowel signs and conjuncts, which is why guessing Latin-1 instead would
/// silently produce control characters where the letters should be.
///
/// `'\u{FFFD}'` marks the five positions Windows-1252 leaves undefined.
#[rustfmt::skip]
const CP1252_HIGH: [char; 32] = [
    '\u{20AC}', '\u{FFFD}', '\u{201A}', '\u{0192}', '\u{201E}', '\u{2026}', '\u{2020}', '\u{2021}',
    '\u{02C6}', '\u{2030}', '\u{0160}', '\u{2039}', '\u{0152}', '\u{FFFD}', '\u{017D}', '\u{FFFD}',
    '\u{FFFD}', '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2022}', '\u{2013}', '\u{2014}',
    '\u{02DC}', '\u{2122}', '\u{0161}', '\u{203A}', '\u{0153}', '\u{FFFD}', '\u{017E}', '\u{0178}',
];

/// Decode bytes to text, choosing the encoding and saying which it chose.
pub fn decode(bytes: &[u8]) -> (String, TextEncoding) {
    match std::str::from_utf8(bytes) {
        // A byte-order mark carries no text and would otherwise become part of
        // the first word.
        Ok(text) => (
            text.strip_prefix('\u{FEFF}').unwrap_or(text).to_owned(),
            TextEncoding::Utf8,
        ),
        Err(_) => (from_windows_1252(bytes), TextEncoding::Windows1252),
    }
}

/// Windows-1252 to text. Cannot fail: every byte means something.
pub fn from_windows_1252(bytes: &[u8]) -> String {
    bytes
        .iter()
        .map(|b| match b {
            0x80..=0x9F => CP1252_HIGH[(b - 0x80) as usize],
            _ => char::from(*b),
        })
        .collect()
}

/// Text back to Windows-1252, for writing a file in the encoding it arrived in.
///
/// Any character with no Windows-1252 form is written as `?`, which is what
/// every other tool does. In practice this never fires on converted output:
/// Unicode Bengali is written as UTF-8, and only untouched legacy text is ever
/// written back this way.
pub fn to_windows_1252(text: &str) -> Vec<u8> {
    text.chars()
        .map(|c| {
            let o = c as u32;
            if o < 0x80 || (0xA0..=0xFF).contains(&o) {
                o as u8
            } else {
                match CP1252_HIGH.iter().position(|h| *h == c) {
                    Some(i) => 0x80 + i as u8,
                    None => b'?',
                }
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modern_unicode_files_are_read_as_utf8() {
        let (text, enc) = decode("কর্মসূচি এবং প্রতিবেদন".as_bytes());
        assert_eq!(enc, TextEncoding::Utf8);
        assert_eq!(text, "কর্মসূচি এবং প্রতিবেদন");
    }

    #[test]
    fn a_byte_order_mark_does_not_become_part_of_the_first_word() {
        let (text, _) = decode("\u{FEFF}Kg".as_bytes());
        assert_eq!(text, "Kg");
    }

    /// The case this module exists for.
    #[test]
    fn a_windows_typed_bijoy_file_decodes_to_the_right_glyphs() {
        // `Awd†mi bvgt` — অফিসের নামঃ — as a Windows text editor stores it.
        // 0x86 is the byte; `†` is what it means.
        let raw: Vec<u8> = b"Awd\x86mi bvgt".to_vec();
        assert!(
            std::str::from_utf8(&raw).is_err(),
            "this test is pointless if the bytes are valid UTF-8"
        );
        let (text, enc) = decode(&raw);
        assert_eq!(enc, TextEncoding::Windows1252);
        assert_eq!(text, "Awd\u{2020}mi bvgt");
        // And the whole point: it now converts.
        assert_eq!(crate::convert(&text), "অফিসের নামঃ");
    }

    #[test]
    fn every_byte_bijoy_uses_survives_a_round_trip() {
        // The full 0x80-0xFF range, which is where Bijoy keeps its conjuncts.
        let raw: Vec<u8> = (0x80u8..=0xFF).collect();
        let text = from_windows_1252(&raw);
        let back = to_windows_1252(&text);
        // The five undefined Windows-1252 positions cannot round-trip; every
        // other byte must.
        let undefined = [0x81u8, 0x8D, 0x8F, 0x90, 0x9D];
        for (original, returned) in raw.iter().zip(back.iter()) {
            if undefined.contains(original) {
                continue;
            }
            assert_eq!(original, returned, "byte {original:#04x} did not survive");
        }
    }

    #[test]
    fn plain_english_is_unaffected_by_either_path() {
        let english = "Programme operations and budget review for 2026.";
        let (text, enc) = decode(english.as_bytes());
        assert_eq!(enc, TextEncoding::Utf8);
        assert_eq!(text, english);
        assert_eq!(from_windows_1252(english.as_bytes()), english);
    }
}
