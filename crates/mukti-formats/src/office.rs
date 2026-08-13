//! Pull text runs, with the font each one declares, out of an Office file.
//!
//! `.docx`, `.xlsx` and `.pptx` are all ZIP archives of XML, and all three
//! spell a formatted run the same way once the namespace prefix is stripped:
//!
//! | Format | run | font | text |
//! |---|---|---|---|
//! | Word       | `w:r` | `w:rFonts` (`w:ascii`) | `w:t` |
//! | PowerPoint | `a:r` | `a:latin` (`typeface`) | `a:t` |
//! | Excel      | `r`   | `rFont` (`val`)        | `t`   |
//!
//! Their local names are `r`, one of `rFonts`/`latin`/`rFont`, and `t`. So one
//! parser handles all three, which is why this file is shorter than expected.

use std::io::{Read, Seek};

use quick_xml::events::Event;
use quick_xml::Reader;

// ---------------------------------------------------------------------------
// Limits against a hostile archive
// ---------------------------------------------------------------------------
//
// A `.docx` is a ZIP file, and ZIP has no ceiling on how much a small file can
// expand to. Deflate reaches about 1032:1, so a 10 MB entry can claim 10 GB. Until
// 13 August 2026 every XML part was read with an unbounded `read_to_string`, with
// no cap on part size, on how many entries an archive could have, or on how far
// one entry could expand. A document is something a stranger emails you.
//
// **These numbers were measured, not chosen.** Across all 1,377 documents in the
// project's own corpus:
//
//   uncompressed XML part     median   4 KB    p99  673 KB    max  91.5 MB
//   entries per archive       median  16       p99  259       max  528
//   compression ratio         median   4x      p99   27x      max   83x
//   total per archive         median 0.38 MB   p99 19.3 MB    max  94.7 MB
//
// That 91.5 MB part matters: it is a real document somebody uses. A limit chosen
// by intuition — 10 MB, say, which sounds generous — would have rejected it. This
// is why the corpus was measured first.
//
// Each limit sits well above the largest real value and far below anything that
// would exhaust a machine capable of running a desktop app. Raising one is a
// legitimate decision; do it by re-measuring, and record the new figures here.

/// The most one XML part may expand to. 2.8x the largest real part seen.
const MAX_PART_BYTES: u64 = 256 * 1024 * 1024;

/// The most an entire archive may expand to across all its parts. 5.4x the
/// largest real archive seen.
const MAX_TOTAL_BYTES: u64 = 512 * 1024 * 1024;

/// The most entries an archive may contain. 7.8x the largest real count seen.
const MAX_ENTRIES: usize = 4_096;

// There is deliberately NO compression-ratio limit, and that is a correction.
//
// One was written first, at 200:1 — 2.4x the highest ratio measured across the
// corpus (83:1) and a fifth of what deflate can achieve. It looked well justified
// and it was wrong. The test that a large but legitimate document must still be
// accepted failed immediately: 100 MB of ordinary repetitive Word markup
// compresses at **294:1**, because real XML repeats itself enormously.
//
// The corpus measurement was not wrong, it was answering a different question.
// Its biggest documents were not its most repetitive ones, so the observed maximum
// ratio said nothing about the ratio a legitimate *large* document can reach.
//
// And a ratio limit was never load-bearing anyway. What bounds memory is the
// absolute size, checked against the declared size before a byte is read and
// against the real size while reading. A ratio check adds no protection those two
// do not already give, and rejects real documents. So it is gone.

// Every limit must clear the largest value the real corpus contains — checked when
// this file is COMPILED, not when tests are run.
//
// These began as a `#[test]`, and clippy objected that an assertion between two
// constants has a constant value. It was right, and its suggestion is stronger than
// what it replaced: a compile-time assertion means a binary whose limits would
// reject real documents cannot be BUILT, so the guarantee does not depend on
// anybody remembering to run the tests.
//
// The figures come from measuring all 1,377 documents in the project's corpus on
// 13 August 2026. If the corpus grows and something exceeds a limit, re-measure and
// update BOTH the constant and the figure here — never only the constant.
const _: () = assert!(
    MAX_PART_BYTES > 91_500_191,
    "the part limit is below the largest real XML part measured (91.5 MB), so it \
     would reject a document somebody actually uses"
);
const _: () = assert!(
    MAX_TOTAL_BYTES > 94_680_000,
    "the archive limit is below the largest real archive measured (94.7 MB)"
);
const _: () = assert!(
    MAX_ENTRIES > 528,
    "the entry limit is below the most entries measured in one real archive (528)"
);
const _: () = assert!(
    MAX_TOTAL_BYTES >= MAX_PART_BYTES,
    "one legitimate maximum-size part could never be read at all"
);

/// What a refusal says. Plain English, because it is shown to a person, and
/// specific, because "invalid file" tells nobody anything.
fn too_big(what: &str, saw: u64, limit: u64) -> Box<dyn std::error::Error> {
    format!(
        "This file claims to contain far more data than any real document does \
         ({what}: {saw} against a limit of {limit}). It is either damaged or built \
         to exhaust the memory of whatever opens it, so it has not been read. \
         Nothing on your computer has been changed."
    )
    .into()
}

/// Check one entry before reading a byte of it, then read it under a hard cap.
///
/// The declared size is checked first because it costs nothing: a bomb announces
/// its intentions in the central directory. The `take` afterwards is the part that
/// actually protects anything, since a declared size can be a lie.
fn read_part_within_limits<R: Read>(
    entry: &mut R,
    name: &str,
    declared: u64,
    running_total: &mut u64,
) -> Result<String, Box<dyn std::error::Error>> {
    read_within(
        entry,
        name,
        declared,
        running_total,
        MAX_PART_BYTES,
        MAX_TOTAL_BYTES,
    )
}

/// The limit check itself, with the limits passed in.
///
/// Parameters rather than constants purely so this can be tested. Proving the
/// behaviour against the real 256 MB limit means compressing a quarter of a
/// gigabyte, which took minutes in a debug build — and a test suite slow enough
/// that people stop running it protects nothing at all. With the limits injected,
/// the same logic is exercised in microseconds against a limit of a few bytes.
///
/// The constants are then checked separately, and exactly, against the largest
/// sizes the real corpus contains. Between them the two tests cover more than one
/// enormous test ever did.
fn read_within<R: Read>(
    entry: &mut R,
    name: &str,
    declared: u64,
    running_total: &mut u64,
    max_part: u64,
    max_total: u64,
) -> Result<String, Box<dyn std::error::Error>> {
    // The declared size is checked first because it is free: a bomb announces its
    // intentions in the central directory before a byte is decompressed.
    if declared > max_part {
        return Err(too_big(&format!("the part {name}"), declared, max_part));
    }
    *running_total = running_total.saturating_add(declared);
    if *running_total > max_total {
        return Err(too_big("the whole archive", *running_total, max_total));
    }

    // Then read under a hard cap regardless of what the header claimed. The
    // declared size is an assertion by the file; this is the only guarantee.
    let mut xml = String::new();
    let read = entry.take(max_part + 1).read_to_string(&mut xml).map_err(
        |e| -> Box<dyn std::error::Error> {
            format!("The part {name} could not be read as text: {e}").into()
        },
    )?;
    if read as u64 > max_part {
        return Err(too_big(
            &format!("the part {name}, once expanded"),
            read as u64,
            max_part,
        ));
    }
    Ok(xml)
}

/// The literal text of a `Text` event.
///
/// From quick-xml 0.41 a `Text` event carries **no** entity references: `&amp;`
/// and `&#65;` arrive separately as [`Event::GeneralRef`], handled by
/// [`event_ref`]. So decoding bytes to characters is the whole job here.
///
/// The error is propagated rather than swallowed with `unwrap_or_default()`.
/// That matters more than it looks: an empty string in place of real text does
/// not merely lose a word, it shifts every character position after it, and the
/// two passes in `rewrite_part` anchor on those positions. Silence here is how a
/// document gets quietly corrupted instead of loudly refused.
fn event_text(e: &quick_xml::events::BytesText<'_>) -> Result<String, Box<dyn std::error::Error>> {
    Ok(e.decode()?.into_owned())
}

/// The text a `&…;` reference stands for.
///
/// quick-xml 0.41 split entity and character references out of `Text` into their
/// own event. That change is silent and dangerous: code written for 0.37 still
/// compiles, still passes a build, and simply **drops** every `&`, `<` and `>`
/// in the document — while shifting all following character positions, so the
/// damage spreads well past the ampersand itself. The test
/// `escaped_characters_come_back_unescaped` is what caught it.
///
/// Two kinds resolve:
///
/// * character references — `&#38;`, `&#x26;` — via `resolve_char_ref`;
/// * the five predefined names — `amp`, `lt`, `gt`, `quot`, `apos`.
///
/// Anything else is an entity declared in a document type definition, and this
/// **refuses** rather than guesses. Two reasons. Office documents are not
/// allowed to carry a document type definition at all, so one appearing means
/// the file is either malformed or probing for a parser that will follow it —
/// the classic way to make an XML reader fetch a file it should not. And a
/// dropped or invented entity would corrupt the text silently, which is the one
/// outcome this project treats as worse than failing.
fn event_ref(e: &quick_xml::events::BytesRef<'_>) -> Result<String, Box<dyn std::error::Error>> {
    if let Some(c) = e.resolve_char_ref()? {
        return Ok(c.to_string());
    }
    let name = e.decode()?;
    match quick_xml::escape::resolve_xml_entity(&name) {
        Some(text) => Ok(text.to_owned()),
        None => Err(format!(
            "this file refers to \"&{name};\", which is defined in the file's own \
             document type definition rather than by XML itself. Office files do \
             not use those, so this one is either damaged or not what it claims \
             to be. It has been left completely unchanged."
        )
        .into()),
    }
}

/// One run of text, together with the font it explicitly asked for.
pub struct Run {
    pub text: String,
    /// `None` when the run declares no font of its own.
    ///
    /// Such a run inherits from its paragraph style, the document defaults, or
    /// the theme — a chain this tool deliberately does **not** follow. An
    /// inherited font is a guess, and a guess has no place in a set of labels
    /// that later becomes the answer key. Precision over recall: an unlabelled
    /// run costs us a sample, a wrongly labelled one costs us the measurement.
    pub font: Option<String>,
}

/// The parts of each archive that actually hold document text.
///
/// The list is longer than the obvious three because text hides in places
/// that do not look like the document body, and every one of these was found
/// by converting a real archive and then asking what still contained Bijoy:
///
/// * **SmartArt diagrams** (`diagrams/data*.xml`) hold their own text. A
///   process diagram full of Bangla came back entirely unconverted while the
///   paragraphs around it were fine — the worst kind of half-done.
/// * **Charts** (`charts/chart*.xml`) hold axis labels and titles.
/// * **Speaker notes**, **comments**, **text boxes** and **headers** are all
///   text somebody wrote and expects to be converted.
pub fn is_text_part(name: &str) -> bool {
    let xml = name.ends_with(".xml");
    name == "word/document.xml"
        || name == "xl/sharedStrings.xml"
        || name == "word/comments.xml"
        || (xml && name.starts_with("ppt/slides/slide"))
        || (xml && name.starts_with("ppt/notesSlides/notesSlide"))
        || (xml && name.starts_with("word/footnotes"))
        || (xml && name.starts_with("word/endnotes"))
        || (xml && name.starts_with("word/header"))
        || (xml && name.starts_with("word/footer"))
        // SmartArt and charts, wherever the format keeps them.
        // SmartArt keeps TWO copies: `data` is the model, `drawing` is the
        // laid-out rendering. Both carry the text, so both must be converted
        // or the diagram disagrees with itself.
        || (xml && name.contains("/diagrams/data"))
        || (xml && name.contains("/diagrams/drawing"))
        || (xml && name.contains("/charts/chart"))
}

/// Parts that hold FONT settings but no document text.
///
/// Excel is the reason this exists. A spreadsheet keeps its cell text in
/// `sharedStrings.xml` and its fonts in `styles.xml`, so converting only the
/// text leaves the workbook still asking for SutonnyMJ — a font with no
/// Bengali codepoints at all. Word and PowerPoint keep style-level fonts
/// separately too, for the same reason.
///
/// These parts have their font names renamed and their text left alone,
/// because there is no text in them to speak of.
pub fn is_font_part(name: &str) -> bool {
    name == "xl/styles.xml"
        || name == "word/styles.xml"
        // Word 2010 writes a SECOND copy of the style sheet for older readers.
        // It was missed until a corpus-wide check found a legacy font name
        // surviving in it — the style sheet said Nirmala UI and its twin still
        // said SutonnyMJ, so which font the reader got depended on their version
        // of Word. Found 13 August 2026 by checking all 676 documents.
        || name == "word/stylesWithEffects.xml"
        // List bullets carry their own font, and a numbered list in a Bangla
        // document is numbered in Bangla.
        || name == "word/numbering.xml"
        || name == "word/fontTable.xml"
        || name == "ppt/presentation.xml"
        || name == "word/theme/theme1.xml"
        || (name.starts_with("ppt/slideMasters/") && name.ends_with(".xml"))
        || (name.starts_with("ppt/slideLayouts/") && name.ends_with(".xml"))
        || (name.starts_with("ppt/theme/") && name.ends_with(".xml"))
}

/// The one part that records font names as element **text** rather than as an
/// attribute.
///
/// `docProps/app.xml` carries PowerPoint's "Fonts Used" summary, written as
/// `<vt:lpstr>SutonnyMJ</vt:lpstr>` inside `TitlesOfParts`. The attribute-renaming
/// path cannot see it, and the code used to say so and leave it — until a check
/// across the whole archive found the stale name still sitting in **29** of 142
/// presentations after conversion. Cosmetic, in that no text renders from it, but
/// it is a claim the file makes about itself that is no longer true.
///
/// Handled separately, and strictly: only text that is *exactly* a legacy font
/// name is replaced, because this same vector also holds slide titles.
pub fn is_metadata_font_part(name: &str) -> bool {
    name == "docProps/app.xml"
}

/// Elements whose END marks the end of a line of text.
///
/// Pass one inserts a newline here so the last word of one paragraph and the
/// first of the next are not read as a single word. Pass two must skip over
/// that same newline, or every character position after the first paragraph is
/// wrong — which is exactly the bug that ate the spaces between runs on a real
/// document. One predicate, used by both, so they cannot disagree.
pub(crate) fn ends_a_line(name: &[u8]) -> bool {
    matches!(name, b"p" | b"tc" | b"br" | b"si")
}

/// Elements that name a font.
///
/// More than the obvious one per format, and the extras are not decoration.
/// PowerPoint names a font once per script — `a:latin` for Latin, `a:ea` for
/// East Asian, `a:cs` for complex script, `a:sym` for symbols — and Bengali
/// typists routinely set the complex-script one. Handling only `a:latin` left
/// SutonnyMJ behind on every slide of a real deck.
///
/// `name` is Excel's, inside `styles.xml`, and `font` is Word's font manifest
/// entry in `fontTable.xml`. Both are generic words, but nothing is renamed
/// unless its value is actually a legacy Bangla font, so an element of either
/// name meaning something else is untouched.
///
/// One thing this mechanism cannot reach, stated rather than hidden:
/// `docProps/app.xml` lists the fonts a document uses as element *text*, not
/// as attributes. It is metadata — Word and PowerPoint rebuild it on save —
/// and nothing renders from it, so a stale name there changes nothing a
/// reader sees.
pub fn names_a_font(name: &[u8]) -> bool {
    matches!(
        name,
        b"rFonts"
            | b"latin"
            | b"rFont"
            | b"name"
            | b"ea"
            | b"cs"
            | b"sym"
            | b"font"
            // The fallback name Word records beside a font it may not find.
            | b"altName"
    )
}

/// Is this element a run of actual text?
///
/// **The prefix matters here, unlike everywhere else in this file.** Word uses
/// `w:t`, DrawingML — slides, charts, SmartArt — uses `a:t`, and Excel uses a
/// bare `t`. But SmartArt also has `dgm:t`, which is a text *container*, not
/// text. Matching on the local name alone read that container as a run, which
/// double-counted its contents and shifted the word count of every SmartArt
/// document. Measured: sixteen files out of three hundred.
fn is_text_element(qualified: &[u8]) -> bool {
    matches!(qualified, b"t" | b"w:t" | b"a:t")
}

/// Strip a namespace prefix: `w:rFonts` becomes `rFonts`.
fn local_name(raw: &[u8]) -> &[u8] {
    match raw.iter().position(|b| *b == b':') {
        Some(i) => &raw[i + 1..],
        None => raw,
    }
}

/// Every run of text in one Office file, with its declared font.
pub fn runs<R: Read + Seek>(archive: R) -> Result<Vec<Run>, Box<dyn std::error::Error>> {
    let mut zip = zip::ZipArchive::new(archive)?;
    if zip.len() > MAX_ENTRIES {
        return Err(too_big("entries", zip.len() as u64, MAX_ENTRIES as u64));
    }
    let parts: Vec<String> = zip
        .file_names()
        .filter(|n| is_text_part(n))
        .map(str::to_owned)
        .collect();

    let mut out = Vec::new();
    let mut total = 0u64;
    for part in parts {
        let declared = zip.by_name(&part)?.size();
        let mut entry = zip.by_name(&part)?;
        let xml = read_part_within_limits(&mut entry, &part, declared, &mut total)?;
        drop(entry);
        collect_runs(&xml, &mut out)?;
    }
    Ok(out)
}

fn collect_runs(xml: &str, out: &mut Vec<Run>) -> Result<(), Box<dyn std::error::Error>> {
    let mut reader = Reader::from_str(xml);
    let config = reader.config_mut();
    // Word writes `<w:t xml:space="preserve"> </w:t>` for a single space, and
    // dropping those would silently glue two words together.
    config.trim_text(false);

    let mut font: Option<String> = None;
    let mut in_text = false;
    let mut text = String::new();

    loop {
        match reader.read_event()? {
            Event::Eof => break,
            Event::Start(e) | Event::Empty(e) => match local_name(e.name().as_ref()) {
                // A new run: whatever font the previous one declared is gone.
                b"r" => {
                    font = None;
                }
                b"rFonts" | b"latin" | b"rFont" => {
                    // Word names the font once per script — Latin, high-ANSI
                    // and complex-script. A Bijoy font is normally set on all
                    // three; taking the first that is present is enough, and
                    // arguing about which one wins would add nothing.
                    for attr in e.attributes().flatten() {
                        if matches!(
                            local_name(attr.key.as_ref()),
                            b"ascii" | b"hAnsi" | b"cs" | b"typeface" | b"val"
                        ) {
                            // `normalized_value(Implicit1_0)` is precisely what
                            // the removed `unescape_value()` did — quick-xml's
                            // own deprecated body forwarded to exactly this.
                            if let Ok(v) = attr.normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            {
                                if !v.is_empty() {
                                    font = Some(v.into_owned());
                                    break;
                                }
                            }
                        }
                    }
                }
                _ if is_text_element(e.name().as_ref()) => {
                    in_text = true;
                    text.clear();
                }
                _ => {}
            },
            Event::Text(e) if in_text => {
                text.push_str(&event_text(&e)?);
            }
            // `&amp;` and `&#65;` arrive here, not in the Text event above.
            // Pass two has the matching arm; they must stay in step.
            Event::GeneralRef(e) if in_text => {
                text.push_str(&event_ref(&e)?);
            }
            // A paragraph, table cell or slide line ends the word that was in
            // progress. Without this the last word of one paragraph and the
            // first of the next are rejoined into a single nonsense token.
            Event::End(e) if ends_a_line(local_name(e.name().as_ref())) => {
                out.push(Run {
                    text: "\n".to_owned(),
                    font: None,
                });
            }
            Event::End(e) if is_text_element(e.name().as_ref()) => {
                in_text = false;
                // Whitespace-only runs are KEPT. Word stores a single space
                // as its own run, and dropping those glued adjacent words
                // together when the text was rejoined.
                if !text.is_empty() {
                    out.push(Run {
                        text: std::mem::take(&mut text),
                        font: font.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_word_run_carries_its_font_forward_and_no_further() {
        // Two runs: the first declares SutonnyMJ, the second declares nothing
        // and must NOT inherit it.
        let xml = r#"<w:p>
            <w:r><w:rPr><w:rFonts w:ascii="SutonnyMJ" w:hAnsi="SutonnyMJ"/></w:rPr><w:t>Kg</w:t></w:r>
            <w:r><w:t>plain</w:t></w:r>
        </w:p>"#;
        let mut runs = Vec::new();
        collect_runs(xml, &mut runs).unwrap();
        let text: Vec<&Run> = runs.iter().filter(|r| r.text.trim() != "").collect();
        assert_eq!(text.len(), 2);
        assert_eq!(text[0].text, "Kg");
        assert_eq!(text[0].font.as_deref(), Some("SutonnyMJ"));
        assert_eq!(text[1].text, "plain");
        assert_eq!(text[1].font, None, "a font leaked into the next run");
        // The paragraph end is emitted so words either side of it are never
        // rejoined into one when the runs are concatenated.
        assert_eq!(runs.last().unwrap().text, "\n");
    }

    #[test]
    fn powerpoint_and_excel_runs_parse_with_the_same_code() {
        let pptx = r#"<a:p><a:r><a:rPr><a:latin typeface="SutonnyOMJ"/></a:rPr><a:t>bvg</a:t></a:r></a:p>"#;
        let mut runs = Vec::new();
        collect_runs(pptx, &mut runs).unwrap();
        assert_eq!(runs[0].font.as_deref(), Some("SutonnyOMJ"));
        assert_eq!(runs[0].text, "bvg");

        let xlsx = r#"<si><r><rPr><rFont val="SutonnyMJ"/></rPr><t>ZvwiLt</t></r></si>"#;
        let mut runs = Vec::new();
        collect_runs(xlsx, &mut runs).unwrap();
        assert_eq!(runs[0].font.as_deref(), Some("SutonnyMJ"));
        assert_eq!(runs[0].text, "ZvwiLt");
    }

    #[test]
    fn a_preserved_space_keeps_two_words_apart() {
        // Word stores a run of one space this way. Dropping it would join the
        // words either side of it into one.
        let xml = r#"<w:p>
            <w:r><w:t>Awd</w:t></w:r>
            <w:r><w:t xml:space="preserve"> </w:t></w:r>
            <w:r><w:t>bvgt</w:t></w:r>
        </w:p>"#;
        let mut runs = Vec::new();
        collect_runs(xml, &mut runs).unwrap();
        // Concatenating every run must reproduce the text with its spacing
        // intact, because that is what the caller tokenises.
        let joined: String = runs.iter().map(|r| r.text.as_str()).collect();
        assert_eq!(
            joined.trim_end(),
            "Awd bvgt",
            "spacing was lost: {joined:?}"
        );
        assert_eq!(
            joined.split_whitespace().collect::<Vec<_>>(),
            vec!["Awd", "bvgt"],
            "the two words were glued together"
        );
    }

    #[test]
    fn escaped_characters_come_back_unescaped() {
        let xml = r#"<w:r><w:t>a &amp; b &lt;c&gt;</w:t></w:r>"#;
        let mut runs = Vec::new();
        collect_runs(xml, &mut runs).unwrap();
        assert_eq!(runs[0].text, "a & b <c>");
    }
}

// ---------------------------------------------------------------------------
// Rewriting
// ---------------------------------------------------------------------------

use std::io::{Cursor, Write as _};

use gru953_mukti::classify::{classify_words, Verdict};
use gru953_mukti::convert;
use gru953_mukti::dictionary::Dictionary;
use gru953_mukti::tokenise::{tokenise, Kind};
use quick_xml::events::{BytesText, Event as XmlEvent};
use quick_xml::Writer;

/// What converting one document did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Summary {
    pub words_converted: usize,
    pub words_untouched: usize,
    /// Runs whose font was changed from a legacy one to a Unicode one.
    pub fonts_changed: usize,
}

/// Legacy Bangla font names, lower-cased.
///
/// Public so that anything checking "did a legacy font survive the conversion?"
/// asks **this** list rather than keeping its own copy. There are already two
/// other font lists in the workspace (`pdf.rs` and `corpus-label`) that drifted
/// apart; a verifier with a fourth would be able to pass while the converter
/// failed, which is worse than having no verifier.
pub const LEGACY_FONTS: &[&str] = &[
    "sutonnymj",
    "sutonnyomj",
    "sutonnyemj",
    "boishakhi",
    "bornosoft",
    "sulekha",
    "chandrabati",
    "modhumatimj",
    "adorsholipi",
    "nikoshban",
    "ekushey",
];

/// Does this font name belong to a legacy Bangla font?
///
/// Deliberately a substring test, because a real font attribute reads
/// `SutonnyMJ Bold` or `NikoshBAN` as often as the bare name. Safe **only** where
/// the value is known to be a font name — an attribute on an element that
/// [`names_a_font`]. Never use it on arbitrary document text: `sulekha` is a
/// Bengali given name, and a participant list containing "SULEKHA" is not a
/// document with a legacy font in it. That exact false positive has now been hit
/// twice on this corpus, once by a release check and once by a verifier written
/// afterwards to catch what the release check missed.
pub fn is_legacy_font(name: &str) -> bool {
    let lower = name.to_lowercase();
    LEGACY_FONTS.iter().any(|f| lower.contains(f))
}

/// Is this text *exactly* the name of a legacy font, and nothing else?
///
/// The strict counterpart to [`is_legacy_font`], for the one place a font name
/// appears as element text rather than as an attribute: `docProps/app.xml`, whose
/// "Fonts Used" list PowerPoint writes as `<vt:lpstr>SutonnyMJ</vt:lpstr>`. An
/// exact match is required precisely so that a cell or a slide title merely
/// *containing* one of these words is left alone.
pub fn is_exactly_legacy_font(text: &str) -> bool {
    let lower = text.trim().to_lowercase();
    LEGACY_FONTS.iter().any(|f| &lower == f)
}

/// Convert every Bangla word inside an Office file.
///
/// `unicode_font` replaces the legacy font name on any run that was rewritten.
/// Without that the document would still ask for SutonnyMJ, which has no
/// Bengali codepoints at all, and the reader would be relying on their word
/// processor's font fallback to see anything sensible.
///
/// Every ZIP entry that is not document text is copied through **without being
/// decompressed and recompressed**, so images, styles and relationships come
/// out byte-identical rather than merely equivalent.
pub fn convert_office(
    bytes: &[u8],
    unicode_font: &str,
) -> Result<(Vec<u8>, Summary), Box<dyn std::error::Error>> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))?;
    if archive.len() > MAX_ENTRIES {
        return Err(too_big("entries", archive.len() as u64, MAX_ENTRIES as u64));
    }
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut summary = Summary::default();
    let mut total_read = 0u64;

    // Work out what each text or font part should become, before writing
    // anything. Deciding first means a part that turns out not to need changing
    // can be copied across byte-for-byte instead of rebuilt — see below.
    let mut replacements: std::collections::BTreeMap<String, String> =
        std::collections::BTreeMap::new();
    for i in 0..archive.len() {
        let name = archive.by_index(i)?.name().to_owned();
        if !is_text_part(&name) && !is_font_part(&name) && !is_metadata_font_part(&name) {
            continue;
        }
        let xml = {
            let mut entry = archive.by_index(i)?;
            let declared = entry.size();
            read_part_within_limits(&mut entry, &name, declared, &mut total_read)?
        };
        let (rewritten, part_summary) = if is_text_part(&name) {
            rewrite_part(&xml, unicode_font)?
        } else if is_metadata_font_part(&name) {
            // Font names recorded as element text, not as attributes.
            rewrite_font_names_in_text(&xml, unicode_font)?
        } else {
            // A font-only part gets its names swapped and its text left alone.
            rewrite_fonts_only(&xml, unicode_font)?
        };
        summary.words_converted += part_summary.words_converted;
        summary.words_untouched += part_summary.words_untouched;
        summary.fonts_changed += part_summary.fonts_changed;

        // Keep the rewrite ONLY if this part actually needed one.
        //
        // The test is what the rewrite *did* — words converted, fonts renamed —
        // not whether the resulting string differs. Those are not the same
        // question, and the difference matters:
        //
        // Reading an XML part and writing it back re-serialises the markup, and a
        // faithful re-serialisation is not the same text. quick-xml escapes a
        // carriage return as `&#13;`, because an XML parser would otherwise
        // silently turn it into a newline — correct, and four characters longer.
        // Word writes bare carriage returns in places, so parts nobody had edited
        // came back slightly larger. Comparing strings would take that grown
        // version; asking what changed rejects it.
        //
        // So a part that converted nothing and renamed nothing is treated exactly
        // like an image: copied through, byte for byte, still compressed as Word
        // left it. Every re-serialisation is an opportunity to differ from the
        // original in a way nobody has thought to check, and the cheapest way to
        // take that risk to zero is not to do it.
        if part_summary.words_converted > 0 || part_summary.fonts_changed > 0 {
            replacements.insert(name, rewritten);
        }
    }

    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_owned();
        match replacements.get(&name) {
            None => out.raw_copy_file(entry)?,
            Some(rewritten) => {
                out.start_file(
                    &name,
                    zip::write::SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )?;
                out.write_all(rewritten.as_bytes())?;
            }
        }
    }

    Ok((out.finish()?.into_inner(), summary))
}

/// Rewrite one XML part: decide first, then edit.
///
/// Two passes, and the first is not optional. The classifier reads a word's
/// neighbours to settle the ambiguous ones — `bvg` is নাম and it is also three
/// Latin letters — so the whole part's text has to be assembled and judged
/// before a single character of it can be rewritten. Editing run by run as we
/// met them would deny it exactly the evidence it needs.
fn rewrite_part(
    xml: &str,
    unicode_font: &str,
) -> Result<(String, Summary), Box<dyn std::error::Error>> {
    // Pass one: what does this part say, and what should happen to it?
    let mut collected = Vec::new();
    collect_runs(xml, &mut collected)?;
    let joined: String = collected.iter().map(|r| r.text.as_str()).collect();

    let dictionary = Dictionary::shipped();
    let segments = tokenise(&joined);
    let words: Vec<&str> = segments
        .iter()
        .filter(|s| s.kind == Kind::Word)
        .map(|s| s.text)
        .collect();
    let verdicts = classify_words(&words, dictionary);

    // What each word becomes, tied to WHERE it sits in the joined text.
    //
    // Position, not order-of-arrival. An earlier version handed each run a
    // share of the converted stream counted in words, and it drifted: a run
    // holding two spaces in a row contributed one word but two pieces, so
    // every run after it took the wrong share. On a real 781-word document
    // that silently glued words together and lost newlines — `KzB‡Ri m¤¢ve¨`
    // came out as one word. Anchoring to character positions cannot drift,
    // because nothing is being counted.
    let mut pieces: Vec<Placed> = Vec::new();
    let mut summary = Summary::default();
    let mut at = 0usize;
    let mut w = 0usize;
    for segment in &segments {
        let len = segment.text.chars().count();
        match segment.kind {
            Kind::Gap => pieces.push(Placed {
                start: at,
                end: at + len,
                text: segment.text.to_owned(),
                changed: false,
            }),
            Kind::Word => {
                let changed = verdicts[w] == Verdict::Legacy;
                let text = if changed {
                    summary.words_converted += 1;
                    convert(segment.text)
                } else {
                    summary.words_untouched += 1;
                    segment.text.to_owned()
                };
                pieces.push(Placed {
                    start: at,
                    end: at + len,
                    text,
                    changed,
                });
                w += 1;
            }
        }
        at += len;
    }

    // Pass two: walk the XML again, giving each run back exactly the span of
    // the document it contributed.
    let mut cursor = 0usize;

    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::new()));

    let mut in_text = false;
    let mut original = String::new();

    loop {
        match reader.read_event()? {
            XmlEvent::Eof => break,
            XmlEvent::Start(e) if is_text_element(e.name().as_ref()) => {
                in_text = true;
                original.clear();
                writer.write_event(XmlEvent::Start(e))?;
            }
            XmlEvent::Text(e) if in_text => {
                original.push_str(&event_text(&e)?);
            }
            // The matching arm to pass one's. Buffered, not written: the whole
            // run is re-emitted at the closing tag via `BytesText::new`, which
            // escapes, so a `&` resolved here becomes `&amp;` again on the way
            // out. A reference OUTSIDE a text element falls to the catch-all and
            // is written through untouched.
            XmlEvent::GeneralRef(e) if in_text => {
                original.push_str(&event_ref(&e)?);
            }
            XmlEvent::End(e) if is_text_element(e.name().as_ref()) => {
                if !original.is_empty() {
                    let len = original.chars().count();
                    let replacement = span(&pieces, cursor, cursor + len);
                    cursor += len;
                    writer.write_event(XmlEvent::Text(BytesText::new(&replacement)))?;
                }
                in_text = false;
                writer.write_event(XmlEvent::End(e))?;
            }
            // Step over the newline pass one inserted here.
            //
            // It is structural: it already exists in the XML as this very tag,
            // so nothing is written — but the cursor must move past it or every
            // character position after the first paragraph is off by one. That
            // is not an abstract worry: it shifted every run by one place on a
            // real document, so each run emitted the previous run's trailing
            // space and the words ran together.
            XmlEvent::End(e) if ends_a_line(local_name(e.name().as_ref())) => {
                cursor += 1;
                writer.write_event(XmlEvent::End(e))?;
            }
            // A legacy font name on a run becomes a Unicode one, so the
            // converted text is asked for in a font that can actually draw it.
            // SutonnyMJ contains no Bengali codepoints whatsoever, so leaving
            // the name alone would leave the reader relying on their word
            // processor's font fallback to see anything at all.
            //
            // `<w:rFonts/>` is an empty element in every file seen, but a
            // start tag is handled too and re-emitted AS a start tag: turning
            // one into an empty element would orphan its end tag and produce
            // a document Word refuses to open.
            XmlEvent::Empty(e) if names_a_font(local_name(e.name().as_ref())) => {
                let (replaced, changed) = rename_legacy_font(&e, unicode_font)?;
                summary.fonts_changed += usize::from(changed);
                writer.write_event(XmlEvent::Empty(replaced))?;
            }
            XmlEvent::Start(e) if names_a_font(local_name(e.name().as_ref())) => {
                let (replaced, changed) = rename_legacy_font(&e, unicode_font)?;
                summary.fonts_changed += usize::from(changed);
                writer.write_event(XmlEvent::Start(replaced))?;
            }
            other => writer.write_event(other)?,
        }
    }
    Ok((
        String::from_utf8(writer.into_inner().into_inner())?,
        summary,
    ))
}

/// Swap any legacy font name on this element for the Unicode one.
///
/// Returns the rebuilt element and whether anything actually changed. Every
/// other attribute is copied across verbatim, so a run that names Times New
/// Roman for Latin and SutonnyMJ for Bengali keeps the first and loses only
/// the second.
fn rename_legacy_font<'a>(
    element: &quick_xml::events::BytesStart<'a>,
    unicode_font: &str,
) -> Result<(quick_xml::events::BytesStart<'a>, bool), Box<dyn std::error::Error>> {
    let attrs: Vec<_> = element.attributes().flatten().collect();
    let mut rebuilt = element.clone().into_owned();
    rebuilt.clear_attributes();
    let mut changed = false;
    for attr in attrs {
        let key = std::str::from_utf8(attr.key.as_ref())?.to_owned();
        let value = attr
            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
            .unwrap_or_default();
        if is_legacy_font(&value) {
            changed = true;
            rebuilt.push_attribute((key.as_str(), unicode_font));
        } else {
            rebuilt.push_attribute((key.as_str(), value.as_ref()));
        }
    }
    Ok((rebuilt, changed))
}

/// Rename legacy fonts in a part that holds no document text.
///
/// Deliberately does not touch a single character of text: a style sheet's
/// strings are names of styles, and converting those would rename the user's
/// formatting.
/// Replace font names that appear as element **text**, changing nothing else.
///
/// For `docProps/app.xml` only — see [`is_metadata_font_part`]. PowerPoint writes
/// its "Fonts Used" list as text nodes, so the attribute path never reaches it and
/// a converted presentation went on claiming to use SutonnyMJ.
///
/// Strict by design. The same `TitlesOfParts` vector holds slide titles, so only
/// text that is *exactly* a legacy font name is touched. A slide titled
/// "SutonnyMJ conversion notes" keeps its title; one whose entire content is
/// `SutonnyMJ` is a font entry and is renamed. That is the correct trade: the
/// alternative once flagged a participant named Sulekha as a font.
fn rewrite_font_names_in_text(
    xml: &str,
    unicode_font: &str,
) -> Result<(String, Summary), Box<dyn std::error::Error>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut summary = Summary::default();

    loop {
        match reader.read_event()? {
            XmlEvent::Eof => break,
            XmlEvent::Text(e) => {
                let text = event_text(&e)?;
                if is_exactly_legacy_font(&text) {
                    summary.fonts_changed += 1;
                    writer.write_event(XmlEvent::Text(BytesText::new(unicode_font)))?;
                } else {
                    writer.write_event(XmlEvent::Text(e))?;
                }
            }
            other => writer.write_event(other)?,
        }
    }
    Ok((
        String::from_utf8(writer.into_inner().into_inner())?,
        summary,
    ))
}

fn rewrite_fonts_only(
    xml: &str,
    unicode_font: &str,
) -> Result<(String, Summary), Box<dyn std::error::Error>> {
    let mut reader = quick_xml::Reader::from_str(xml);
    reader.config_mut().trim_text(false);
    let mut writer = Writer::new(Cursor::new(Vec::new()));
    let mut summary = Summary::default();

    loop {
        match reader.read_event()? {
            XmlEvent::Eof => break,
            XmlEvent::Empty(e) if names_a_font(local_name(e.name().as_ref())) => {
                let (replaced, changed) = rename_legacy_font(&e, unicode_font)?;
                summary.fonts_changed += usize::from(changed);
                writer.write_event(XmlEvent::Empty(replaced))?;
            }
            XmlEvent::Start(e) if names_a_font(local_name(e.name().as_ref())) => {
                let (replaced, changed) = rename_legacy_font(&e, unicode_font)?;
                summary.fonts_changed += usize::from(changed);
                writer.write_event(XmlEvent::Start(replaced))?;
            }
            other => writer.write_event(other)?,
        }
    }
    Ok((
        String::from_utf8(writer.into_inner().into_inner())?,
        summary,
    ))
}

/// One tokenised piece of the joined text, and where it sat.
struct Placed {
    start: usize,
    end: usize,
    /// The characters to emit: the converted word, or the original text itself.
    text: String,
    /// Did conversion actually alter this piece?
    ///
    /// Only `true` for a word that was converted. That is the one case where the
    /// emitted text is a different length from the original, so it can no longer
    /// be cut at the original character positions. Everything else — untouched
    /// words, whitespace, punctuation — keeps its exact original length and is
    /// therefore sliced rather than moved. See `span`.
    changed: bool,
}

/// The converted form of the characters between `from` and `to`.
///
/// Two cases, and the distinction is the whole point of this function.
///
/// **A piece whose text is unchanged** is copied character for character from
/// the position it occupied, so the document's spacing, line breaks and — this
/// is the part that was wrong until 13 August 2026 — the division of text
/// between runs all survive exactly.
///
/// **A converted word is emitted whole by the run in which it starts**, and the
/// other runs it spans contribute nothing. There is no alternative: `Kg` +
/// `©m~wP` is one word, `কর্মসূচি`, of a different length, and it cannot be cut
/// at the old character offsets. A word split by mid-word formatting therefore
/// loses that internal split when it converts. That is a real cost, accepted
/// knowingly, and it now applies **only** to words that actually change.
///
/// The bug this replaced: consolidation was applied to *every* word. Word splits
/// words across runs constantly — revision marks, spell-check state, a change of
/// language mid-word — so a document with **no legacy Bangla at all** still came
/// back with its runs rearranged. On one real document a run holding `t` came
/// back holding `trainng`, having stolen the rest of the word from the runs
/// after it. Visible text was always correct, which is exactly why this survived
/// a 300-document check that compared the joined text: joined text cannot see
/// which run a character came from. Found by checking that a document with
/// nothing to convert comes back byte-identical.
///
/// Getting the whitespace half wrong is not subtle either: an earlier,
/// count-based version glued words together and lost newlines across a real
/// 781-word document, taking it down to 465 words.
fn span(pieces: &[Placed], from: usize, to: usize) -> String {
    let mut out = String::new();
    for piece in pieces {
        if piece.end <= from || piece.start >= to {
            continue;
        }
        if piece.changed {
            // A converted word belongs to the run it starts in, and no other.
            if piece.start >= from {
                out.push_str(&piece.text);
            }
        } else {
            // Unchanged: copy exactly the part that falls inside this run, so
            // the original distribution across runs is preserved.
            let skip = from.saturating_sub(piece.start);
            let take = piece.end.min(to) - piece.start.max(from);
            out.extend(piece.text.chars().skip(skip).take(take));
        }
    }
    out
}

#[cfg(test)]
mod rewrite_tests {
    use super::*;

    /// The smallest thing Word will call a document, plus an image so there is
    /// something non-text to prove is copied through untouched.
    fn build_docx(document_xml: &str) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("[Content_Types].xml", "<Types/>"),
            ("_rels/.rels", "<Relationships/>"),
            ("word/document.xml", document_xml),
        ] {
            zip.start_file(name, opts).unwrap();
            zip.write_all(body.as_bytes()).unwrap();
        }
        zip.start_file("word/media/image1.png", opts).unwrap();
        zip.write_all(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A])
            .unwrap();
        zip.finish().unwrap().into_inner()
    }

    fn document_text(bytes: &[u8]) -> String {
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).unwrap();
        let mut xml = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut xml)
            .unwrap();
        xml
    }

    /// Every `<w:t>` in a document, joined — what the reader actually sees.
    fn visible(bytes: &[u8]) -> String {
        let mut runs = Vec::new();
        collect_runs(&document_text(bytes), &mut runs).unwrap();
        runs.iter().map(|r| r.text.as_str()).collect()
    }

    const MIXED: &str = concat!(
        "<w:document><w:body><w:p>",
        "<w:r><w:rPr><w:rFonts w:ascii=\"SutonnyMJ\" w:hAnsi=\"SutonnyMJ\"/></w:rPr>",
        "<w:t>Kg\u{a9}m~wP</w:t></w:r>",
        "<w:r><w:t xml:space=\"preserve\"> </w:t></w:r>",
        "<w:r><w:rPr><w:rFonts w:ascii=\"Calibri\"/></w:rPr><w:t>Programme review 2026</w:t></w:r>",
        "</w:p></w:body></w:document>"
    );

    #[test]
    fn the_bangla_converts_and_the_english_does_not() {
        let (out, summary) = convert_office(&build_docx(MIXED), "Nirmala UI").unwrap();
        let xml = document_text(&out);
        assert!(xml.contains("কর্মসূচি"), "the Bangla did not convert: {xml}");
        assert!(
            xml.contains("Programme review 2026"),
            "the English was altered: {xml}"
        );
        assert_eq!(summary.words_converted, 1);
    }

    /// An ampersand and an angle bracket sit **before** the Bangla on purpose.
    ///
    /// `&amp;` is five characters in the file and one on the page. If the two
    /// passes of `rewrite_part` disagree about which, every position after it
    /// shifts — and the Bangla, being downstream, is what visibly breaks. So this
    /// is not really a test about ampersands. It is a test that the two passes
    /// still count the same characters, using the cheapest available way to make
    /// them disagree.
    ///
    /// It exists because upgrading quick-xml 0.37 → 0.41 moved entity references
    /// out of the `Text` event into an event of their own. The old code still
    /// compiled and still built cleanly; it simply dropped every `&`, `<` and `>`
    /// and shifted everything after them.
    #[test]
    fn entities_before_bangla_do_not_shift_the_conversion() {
        const WITH_ENTITIES: &str = concat!(
            "<w:document><w:body><w:p>",
            "<w:r><w:t>Q1 &amp; Q2 &lt;draft&gt;</w:t></w:r>",
            "<w:r><w:t xml:space=\"preserve\"> </w:t></w:r>",
            "<w:r><w:rPr><w:rFonts w:ascii=\"SutonnyMJ\"/></w:rPr>",
            "<w:t>Kg\u{a9}m~wP</w:t></w:r>",
            "</w:p></w:body></w:document>"
        );

        let (out, summary) = convert_office(&build_docx(WITH_ENTITIES), "Nirmala UI").unwrap();
        let xml = document_text(&out);

        // The Bangla still converts, which is only possible if the character
        // positions survived the entities in front of it.
        assert!(
            xml.contains("কর্মসূচি"),
            "the Bangla after the entities did not convert — positions drifted: {xml}"
        );
        assert_eq!(summary.words_converted, 1);

        // The English is byte-for-byte what it was, once entities are resolved.
        //
        // The trailing newline is not incidental — pass one appends one for the
        // paragraph end, and pass two must step over it. It is spelled out here
        // rather than trimmed away, because that newline is exactly the sort of
        // invisible character whose accounting the two passes have to agree on.
        assert_eq!(
            visible(&out),
            "Q1 & Q2 <draft> কর্মসূচি\n",
            "the escaped characters were lost or altered"
        );

        // And they went back out escaped, so the file is still valid XML rather
        // than a document with a bare ampersand in it.
        assert!(
            xml.contains("&amp;") && xml.contains("&lt;") && xml.contains("&gt;"),
            "the entities were not re-escaped on the way out: {xml}"
        );
    }

    /// A reference this tool cannot resolve must stop the conversion, not be
    /// quietly dropped.
    ///
    /// Office files are not permitted to declare their own entities, so one
    /// appearing means the file is damaged or is probing for a parser that will
    /// go and fetch something. Either way, guessing would corrupt the text and
    /// dropping it would corrupt every position after it. Refusing leaves the
    /// user's original untouched, which is the only safe answer.
    #[test]
    fn an_entity_we_cannot_resolve_is_refused_rather_than_dropped() {
        const CUSTOM_ENTITY: &str = concat!(
            "<w:document><w:body><w:p>",
            "<w:r><w:t>before &mystery; after</w:t></w:r>",
            "</w:p></w:body></w:document>"
        );

        let result = convert_office(&build_docx(CUSTOM_ENTITY), "Nirmala UI");
        assert!(
            result.is_err(),
            "an unresolvable entity produced a document instead of an error"
        );
        let message = result.unwrap_err().to_string();
        assert!(
            message.contains("mystery"),
            "the error should name the reference it could not resolve, so the user \
             knows what is wrong with their file; got: {message}"
        );
    }

    #[test]
    fn the_legacy_font_is_replaced_and_others_are_left_alone() {
        let (out, summary) = convert_office(&build_docx(MIXED), "Nirmala UI").unwrap();
        let xml = document_text(&out);
        assert!(!xml.contains("SutonnyMJ"), "a legacy font survived: {xml}");
        assert!(xml.contains("Nirmala UI"), "no Unicode font was set: {xml}");
        assert!(
            xml.contains("Calibri"),
            "an unrelated font was changed: {xml}"
        );
        assert_eq!(summary.fonts_changed, 1);
    }

    #[test]
    fn images_and_other_parts_are_copied_through_untouched() {
        let source = build_docx(MIXED);
        let (out, _) = convert_office(&source, "Nirmala UI").unwrap();

        let mut before = zip::ZipArchive::new(Cursor::new(&source[..])).unwrap();
        let mut after = zip::ZipArchive::new(Cursor::new(&out[..])).unwrap();
        assert_eq!(before.len(), after.len(), "an entry was lost or added");

        for name in [
            "word/media/image1.png",
            "[Content_Types].xml",
            "_rels/.rels",
        ] {
            let mut a = Vec::new();
            let mut b = Vec::new();
            std::io::Read::read_to_end(&mut before.by_name(name).unwrap(), &mut a).unwrap();
            std::io::Read::read_to_end(&mut after.by_name(name).unwrap(), &mut b).unwrap();
            assert_eq!(a, b, "{name} was altered");
        }
    }

    /// **The whitespace guarantee.** Found the hard way: a count-based version
    /// of the rewriter glued words together and dropped newlines on a real
    /// document, taking 781 words down to 465. The word COUNT and the
    /// whitespace SHAPE must both survive, whatever the run boundaries are.
    #[test]
    fn spacing_and_line_breaks_survive_exactly() {
        let awkward = concat!(
            "<w:document><w:body><w:p>",
            // Two spaces in a row, which is what defeated the earlier version.
            "<w:r><w:rPr><w:rFonts w:ascii=\"SutonnyMJ\"/></w:rPr>",
            "<w:t xml:space=\"preserve\">KzB\u{2021}Ri  m¤¢ve¨</w:t></w:r>",
            "<w:r><w:t xml:space=\"preserve\">\nKg\u{a9}m~wP  ok</w:t></w:r>",
            "</w:p></w:body></w:document>"
        );
        let source = build_docx(awkward);
        let (out, _) = convert_office(&source, "Nirmala UI").unwrap();

        let before = visible(&source);
        let after = visible(&out);
        assert_eq!(
            before.split_whitespace().count(),
            after.split_whitespace().count(),
            "words were glued together or split apart:\n  {before:?}\n  {after:?}"
        );
        // The whitespace shape, with every word blanked out, must be identical.
        let shape = |s: &str| {
            s.split_whitespace().count().to_string()
                + &s.chars().filter(|c| c.is_whitespace()).collect::<String>()
        };
        assert_eq!(shape(&before), shape(&after), "the spacing changed");
    }

    #[test]
    fn a_document_with_no_legacy_bangla_keeps_its_text_exactly() {
        let english = concat!(
            "<w:document><w:body><w:p><w:r><w:rPr><w:rFonts w:ascii=\"Calibri\"/></w:rPr>",
            "<w:t>Programme operations and budget review for 2026.</w:t></w:r>",
            "</w:p></w:body></w:document>"
        );
        let source = build_docx(english);
        let (out, summary) = convert_office(&source, "Nirmala UI").unwrap();
        assert_eq!(visible(&out), visible(&source), "English text was altered");
        assert_eq!(summary.words_converted, 0);
        assert_eq!(summary.fonts_changed, 0);
    }

    /// A word split across runs by a formatting change must still convert.
    #[test]
    fn a_word_split_across_runs_still_converts() {
        let split = concat!(
            "<w:document><w:body><w:p>",
            "<w:r><w:rPr><w:rFonts w:ascii=\"SutonnyMJ\"/></w:rPr><w:t>Kg</w:t></w:r>",
            "<w:r><w:rPr><w:rFonts w:ascii=\"SutonnyMJ\"/></w:rPr><w:t>\u{a9}m~wP</w:t></w:r>",
            "<w:r><w:t xml:space=\"preserve\"> ok</w:t></w:r>",
            "</w:p></w:body></w:document>"
        );
        let (out, _) = convert_office(&build_docx(split), "Nirmala UI").unwrap();
        let seen = visible(&out);
        assert!(
            seen.contains("কর্মসূচি"),
            "the split word did not convert: {seen:?}"
        );
        assert!(seen.contains("ok"), "the following text was lost: {seen:?}");
    }
}

#[cfg(test)]
mod limit_tests {
    use super::*;

    /// A part that declares more than the limit is refused before it is read.
    ///
    /// The reader here is INFINITE. If the declared size were not checked first,
    /// this would read until it filled memory — so the test would hang rather than
    /// fail, which is its own kind of proof.
    #[test]
    fn a_part_declaring_too_much_is_refused_without_reading_it() {
        let mut total = 0u64;
        let mut endless = std::io::repeat(b'A');
        let err = read_within(
            &mut endless,
            "word/document.xml",
            5_000,
            &mut total,
            1_000,
            10_000,
        )
        .expect_err("5,000 declared against a 1,000 limit must be refused");
        assert!(err.to_string().contains("more data than any real document"));
        assert!(err.to_string().contains("word/document.xml"), "{err}");
    }

    /// A part that LIES about its size is still capped while being read.
    ///
    /// The declared size is an assertion by the file. The read cap is the only
    /// part that is a guarantee, and this is what proves it.
    #[test]
    fn a_part_that_understates_its_size_is_capped_as_it_is_read() {
        let mut total = 0u64;
        let mut endless = std::io::repeat(b'A');
        let err = read_within(
            &mut endless,
            "xl/sharedStrings.xml",
            10,
            &mut total,
            1_000,
            10_000,
        )
        .expect_err("a part supplying more than it declared must still be capped");
        assert!(err.to_string().contains("once expanded"), "{err}");
    }

    /// Several parts, each within the limit, may not add up past the archive total.
    ///
    /// A per-part limit alone is not a limit: a thousand parts just under it would
    /// still exhaust the machine.
    #[test]
    fn parts_that_are_each_small_enough_can_still_exhaust_the_archive_budget() {
        let mut total = 0u64;
        for i in 0..9 {
            let mut data = std::io::Cursor::new(vec![b'x'; 100]);
            let outcome = read_within(&mut data, "part", 100, &mut total, 1_000, 800);
            if i < 8 {
                assert!(
                    outcome.is_ok(),
                    "part {i} of 100 bytes should fit within 800"
                );
            } else {
                let err = outcome.expect_err("the ninth part takes the total past 800");
                assert!(err.to_string().contains("the whole archive"), "{err}");
            }
        }
    }

    /// A part comfortably inside the limits is read, and returns its content.
    ///
    /// The limits must let ordinary work through, which is easy to forget when
    /// every other test is about refusing things.
    #[test]
    fn an_ordinary_part_is_read_normally() {
        let mut total = 0u64;
        let mut data = std::io::Cursor::new(b"<w:document/>".to_vec());
        let xml = read_within(
            &mut data,
            "word/document.xml",
            13,
            &mut total,
            1_000,
            10_000,
        )
        .expect("13 bytes against a 1,000 limit is ordinary");
        assert_eq!(xml, "<w:document/>");
        assert_eq!(total, 13);
    }
}
