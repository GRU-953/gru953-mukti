//! Pull the legacy Bangla out of a PDF and convert it, as plain text.
//!
//! # Read-only, and why
//!
//! Everything else this crate does rewrites a document in place. A PDF cannot
//! be treated that way: Bijoy text and Unicode Bengali are different lengths
//! and different shapes, every glyph is positioned individually, and there is
//! no embedded Unicode Bengali font to draw the result with. Rewriting one
//! would mean re-laying-out the page, which is a typesetting job, not a
//! conversion. So a PDF goes in and text comes out.
//!
//! # Why this works at all, which was not obvious
//!
//! This was expected to be the hardest thing in the project and possibly
//! impossible: the usual worry is that a PDF stores glyph indexes into an
//! embedded font subset, from which no text can be recovered.
//!
//! That is not what these files do. Checked before any code was written: the
//! SutonnyMJ fonts in them declare `/Encoding /WinAnsiEncoding` and carry **no
//! `/ToUnicode` map at all**. The text operators therefore hold the original
//! Bijoy bytes, unchanged — `(Av)`, `(cwi)`, `(jU\xaav)`. And WinAnsi *is*
//! Windows-1252, the encoding [`crate::encoding`] already exists to read.
//!
//! So the bytes arrive exactly as a Bijoy `.txt` file would deliver them.
//!
//! # What is genuinely lost
//!
//! Layout. A PDF has no paragraphs, no words and no spaces — only glyphs at
//! coordinates. Spacing is inferred from the positioning operators, and that
//! inference is good rather than perfect: a wide gap becomes a space, a change
//! of line becomes a newline. Tables and columns come out as running text.
//! This is stated plainly here and in the tool's own output, because a user
//! who expects their layout back will otherwise think the conversion failed.

use lopdf::content::Content;
use lopdf::{Document, Object};

use crate::office::Summary;
use gru953_mukti::encoding::from_windows_1252;

/// How much of a gap counts as a space.
///
/// PDF measures these in thousandths of an em, negative meaning "move right".
/// Anything past this is a word gap rather than kerning. The value is the one
/// most PDF tools settle on; it is a threshold on a continuum, so it will
/// occasionally split a word or join two.
const SPACE_GAP: f64 = 180.0;

/// What we can do with text drawn in a particular font.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FontKind {
    /// A legacy Bangla font with no `/ToUnicode` map: the bytes ARE Bijoy, and
    /// the font itself is proof of it. Decode as Windows-1252 and convert.
    LegacyBijoy,
    /// An ordinary text font whose bytes are Latin. Decode and leave alone.
    PlainLatin,
    /// Anything whose bytes are glyph indexes rather than characters —
    /// a subsetted or symbolic font, or one using a `/Differences` array.
    ///
    /// **Skipped, deliberately.** Decoding these as Windows-1252 produces
    /// nonsense, and the nonsense then converts into plausible-looking Bengali
    /// that a reader cannot tell from real text. Dropping the text is bad;
    /// silently inventing Bengali is far worse.
    Unreadable,
}

fn classify_font(dictionary: &lopdf::Dictionary) -> FontKind {
    let base = dictionary
        .get(b"BaseFont")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).to_string())
        .unwrap_or_default();
    let lower = base.to_lowercase();
    let legacy = [
        "sutonny",
        "boishakhi",
        "sulekha",
        "bornosoft",
        "chandrabati",
        "adorsholipi",
    ]
    .iter()
    .any(|f| lower.contains(f));

    // A simple font with a named base encoding is byte-addressed, and that is
    // the only case we can read. Anything else — a Differences array, an
    // Identity CID encoding, a symbolic subset — is glyph indexes.
    let encoding_is_simple = match dictionary.get(b"Encoding") {
        Ok(lopdf::Object::Name(name)) => {
            matches!(
                name.as_slice(),
                b"WinAnsiEncoding" | b"MacRomanEncoding" | b"StandardEncoding"
            )
        }
        // No /Encoding at all: the font's built-in encoding. For the legacy
        // Bangla fonts that is the Bijoy layout, which is what we want.
        Err(_) => true,
        _ => false,
    };

    if !encoding_is_simple {
        return FontKind::Unreadable;
    }
    if legacy {
        FontKind::LegacyBijoy
    } else {
        FontKind::PlainLatin
    }
}

/// Convert the legacy Bangla in a PDF, returning plain text.
pub fn convert_pdf_to_text(bytes: &[u8]) -> Result<(String, Summary), Box<dyn std::error::Error>> {
    let document = Document::load_mem(bytes)?;
    let mut out = String::new();
    let mut summary = Summary::default();
    let mut skipped = 0usize;

    for (number, id) in document.get_pages() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let _ = number;
        let mut raw = String::new();
        let mut marks: Vec<FontKind> = Vec::new();
        page_text(&document, id, &mut raw, &mut marks, &mut skipped)?;
        out.push_str(&convert_marked(&raw, &marks, &mut summary));
    }
    summary.fonts_changed = skipped;
    Ok((out, summary))
}

/// One page's text, decoded according to the font each string is drawn in.
fn page_text(
    document: &Document,
    page: lopdf::ObjectId,
    out: &mut String,
    marks: &mut Vec<FontKind>,
    skipped: &mut usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let fonts = document.get_page_fonts(page)?;
    let kinds: std::collections::BTreeMap<Vec<u8>, FontKind> = fonts
        .iter()
        .map(|(name, dict)| (name.clone(), classify_font(dict)))
        .collect();

    // A cap on how much one page may expand to when decompressed.
    //
    // A PDF content stream is compressed drawing instructions, and the
    // compression ratio has no ceiling — so a small file can ask for an
    // unbounded amount of memory. lopdf 0.44 provides this limited variant for
    // exactly that reason; the unlimited `get_page_content` accumulates whatever
    // it is handed. A page of text is normally a few kilobytes, so 32 MiB is
    // roughly a thousand times generous and still bounded.
    //
    // Over the limit, the page is skipped and counted rather than aborting the
    // whole document: one hostile or broken page should not lose the other
    // ninety-nine. This matches how a page of unreadable fonts is treated.
    const MAX_PAGE_CONTENT: usize = 32 * 1024 * 1024;

    let Ok(data) = document.get_page_content_with_limit(page, MAX_PAGE_CONTENT) else {
        *skipped += 1;
        return Ok(());
    };
    let content = Content::decode(&data)?;
    // Until a font is selected nothing can be read safely.
    let mut current = FontKind::Unreadable;

    for operation in content.operations {
        match operation.operator.as_str() {
            "Tf" => {
                if let Some(lopdf::Object::Name(name)) = operation.operands.first() {
                    current = kinds
                        .get(name.as_slice())
                        .copied()
                        .unwrap_or(FontKind::Unreadable);
                }
            }
            "Td" | "TD" | "T*" | "TL" | "Tm" => push_break(out, marks),
            "Tj" | "'" | "\"" => {
                for object in &operation.operands {
                    push_string(out, marks, object, current, skipped);
                }
            }
            "TJ" => {
                if let Some(Object::Array(items)) = operation.operands.first() {
                    for item in items {
                        match item {
                            Object::String(..) => push_string(out, marks, item, current, skipped),
                            Object::Real(..) | Object::Integer(..) => {
                                let gap = match item {
                                    Object::Real(r) => f64::from(*r),
                                    Object::Integer(i) => *i as f64,
                                    _ => 0.0,
                                };
                                if -gap > SPACE_GAP && !out.ends_with(' ') && !out.ends_with('\n') {
                                    out.push(' ');
                                    marks.push(FontKind::PlainLatin);
                                }
                            }
                            _ => {}
                        }
                    }
                }
            }
            _ => {}
        }
    }
    Ok(())
}

fn push_break(out: &mut String, marks: &mut Vec<FontKind>) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
        marks.push(FontKind::PlainLatin);
    }
}

/// Emit one PDF string, recording which font each character came from.
///
/// The characters are NOT converted here. A PDF splits a word across as many
/// string operators as its kerning needs, and converting each fragment on its
/// own breaks the reordering that makes Bijoy readable: `mswk¬ó` arriving in
/// three pieces came out as `সংিশ্লষ্ট` instead of `সংশ্লিষ্ট`, with the vowel
/// sign stranded. The whole page is assembled first and converted after, which
/// is the same lesson the Office rewriter learned about runs.
fn push_string(
    out: &mut String,
    marks: &mut Vec<FontKind>,
    object: &Object,
    kind: FontKind,
    skipped: &mut usize,
) {
    let Object::String(bytes, _) = object else {
        return;
    };
    if kind == FontKind::Unreadable {
        // Nothing is emitted. See FontKind::Unreadable.
        if !bytes.iter().all(u8::is_ascii_whitespace) {
            *skipped += 1;
        }
        return;
    }
    let text = from_windows_1252(bytes);
    for c in text.chars() {
        out.push(c);
        marks.push(kind);
    }
}

/// Convert every word that came from a legacy font, and only those.
///
/// A word counts as legacy if ANY of its characters were drawn in a legacy
/// font, because a word split across operators may change font mid-way — a
/// bold first letter is enough to do it. The font is authority here, so no
/// guessing is needed and the word-level classifier is not consulted at all.
fn convert_marked(text: &str, marks: &[FontKind], summary: &mut Summary) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let legacy = marks[start..i].contains(&FontKind::LegacyBijoy);
        if legacy {
            out.push_str(&gru953_mukti::convert(&word));
            summary.words_converted += 1;
        } else {
            out.push_str(&word);
            summary.words_untouched += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(base: &str, encoding: Option<&str>) -> lopdf::Dictionary {
        let mut d = lopdf::Dictionary::new();
        d.set("BaseFont", lopdf::Object::Name(base.as_bytes().to_vec()));
        if let Some(e) = encoding {
            d.set("Encoding", lopdf::Object::Name(e.as_bytes().to_vec()));
        }
        d
    }

    #[test]
    fn a_legacy_font_with_a_simple_encoding_is_readable() {
        for name in ["BCDEEE+SutonnyMJ-Bold", "SutonnyMJ", "Boishakhi"] {
            assert_eq!(
                classify_font(&font(name, Some("WinAnsiEncoding"))),
                FontKind::LegacyBijoy,
                "{name} was not recognised as legacy"
            );
        }
        // No /Encoding at all means the font's built-in one, which for these
        // fonts is the Bijoy layout.
        assert_eq!(
            classify_font(&font("SutonnyMJ", None)),
            FontKind::LegacyBijoy
        );
    }

    #[test]
    fn an_ordinary_text_font_is_read_but_not_converted() {
        assert_eq!(
            classify_font(&font("ArialMT", Some("WinAnsiEncoding"))),
            FontKind::PlainLatin
        );
    }

    /// The case that matters most: refusing to guess.
    #[test]
    fn a_font_whose_bytes_are_glyph_indexes_is_skipped_not_guessed_at() {
        // A Differences array, a symbolic subset, an Identity CID encoding —
        // none of these hold characters. Decoding them as Windows-1252 and
        // converting produced plausible-looking Bengali nonsense on a real
        // circular, which is worse than dropping the text.
        let mut differences = lopdf::Dictionary::new();
        differences.set("BaseFont", lopdf::Object::Name(b"ABCDEF+SymbolMT".to_vec()));
        differences.set(
            "Encoding",
            lopdf::Object::Dictionary(lopdf::Dictionary::new()),
        );
        assert_eq!(classify_font(&differences), FontKind::Unreadable);

        assert_eq!(
            classify_font(&font("ABCDEF+SutonnyMJ", Some("Identity-H"))),
            FontKind::Unreadable,
            "a CID-encoded legacy font must not be trusted either"
        );
    }

    #[test]
    fn a_legacy_string_is_read_as_windows_1252_and_converted() {
        let mut out = String::new();
        let mut marks = Vec::new();
        let mut summary = Summary::default();
        let mut skipped = 0;
        // 0x86 is `†` in Windows-1252 and the vowel sign ে in Bijoy. Note the
        // word arrives in TWO pieces, as a real PDF delivers it.
        for piece in [b"Awd\x86".as_slice(), b"mi".as_slice()] {
            push_string(
                &mut out,
                &mut marks,
                &Object::String(piece.to_vec(), lopdf::StringFormat::Literal),
                FontKind::LegacyBijoy,
                &mut skipped,
            );
        }
        assert_eq!(skipped, 0);
        // Converted only after the whole word is assembled, or the vowel sign
        // ends up in the wrong place.
        assert_eq!(convert_marked(&out, &marks, &mut summary), "অফিসের");
        assert_eq!(summary.words_converted, 1);
    }

    #[test]
    fn an_unreadable_string_emits_nothing_and_is_counted() {
        let mut out = String::new();
        let mut marks = Vec::new();
        let mut skipped = 0;
        push_string(
            &mut out,
            &mut marks,
            &Object::String(b"\x03\x11\x42".to_vec(), lopdf::StringFormat::Literal),
            FontKind::Unreadable,
            &mut skipped,
        );
        assert_eq!(out, "", "glyph indexes were turned into text");
        assert_eq!(skipped, 1, "the skip was not reported");
    }

    #[test]
    fn line_breaks_are_not_doubled_up() {
        let mut out = String::from("one");
        let mut marks = vec![FontKind::PlainLatin; 3];
        push_break(&mut out, &mut marks);
        push_break(&mut out, &mut marks);
        assert_eq!(out, "one\n");
        assert_eq!(marks.len(), out.chars().count(), "marks drifted from text");
    }
}
