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
pub(crate) fn names_a_font(name: &[u8]) -> bool {
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
    let parts: Vec<String> = zip
        .file_names()
        .filter(|n| is_text_part(n))
        .map(str::to_owned)
        .collect();

    let mut out = Vec::new();
    for part in parts {
        let mut xml = String::new();
        zip.by_name(&part)?.read_to_string(&mut xml)?;
        collect_runs(&xml, &mut out)?;
    }
    Ok(out)
}

fn collect_runs(xml: &str, out: &mut Vec<Run>) -> Result<(), quick_xml::Error> {
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
                            if let Ok(v) = attr.unescape_value() {
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
                text.push_str(&e.unescape().unwrap_or_default());
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

use gru953_scribe::classify::{classify_words, Verdict};
use gru953_scribe::convert;
use gru953_scribe::dictionary::Dictionary;
use gru953_scribe::tokenise::{tokenise, Kind};
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
const LEGACY_FONTS: &[&str] = &[
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

fn is_legacy_font(name: &str) -> bool {
    let lower = name.to_lowercase();
    LEGACY_FONTS.iter().any(|f| lower.contains(f))
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
    let mut out = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let mut summary = Summary::default();

    for i in 0..archive.len() {
        let entry = archive.by_index(i)?;
        let name = entry.name().to_owned();
        if !is_text_part(&name) && !is_font_part(&name) {
            // Neither text nor fonts: copy it across exactly as it is.
            out.raw_copy_file(entry)?;
            continue;
        }
        let mut xml = String::new();
        let mut entry = entry;
        std::io::Read::read_to_string(&mut entry, &mut xml)?;
        // A font-only part gets its names swapped and its text left alone.
        let (rewritten, part_summary) = if is_text_part(&name) {
            rewrite_part(&xml, unicode_font)?
        } else {
            rewrite_fonts_only(&xml, unicode_font)?
        };
        summary.words_converted += part_summary.words_converted;
        summary.words_untouched += part_summary.words_untouched;
        summary.fonts_changed += part_summary.fonts_changed;

        out.start_file(
            name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated),
        )?;
        out.write_all(rewritten.as_bytes())?;
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
                is_word: false,
            }),
            Kind::Word => {
                let text = if verdicts[w] == Verdict::Legacy {
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
                    is_word: true,
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
                original.push_str(&e.unescape().unwrap_or_default());
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
        let value = attr.unescape_value().unwrap_or_default();
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
    /// The characters to emit: the converted word, or the whitespace itself.
    text: String,
    is_word: bool,
}

/// The converted form of the characters between `from` and `to`.
///
/// A word is emitted whole by the run in which it **starts**. A word split
/// across runs by a mid-word formatting change therefore lands entirely in the
/// first of them and the rest contribute nothing — which is right, because
/// `Kg` + `©m~wP` is one word, `কর্মসূচি`, and it has to be written somewhere.
///
/// Whitespace is copied character for character from the position it occupied,
/// so the document's spacing and line breaks survive exactly however the runs
/// happen to be divided. Getting this wrong is not subtle: an earlier,
/// count-based version glued words together and lost newlines across a real
/// 781-word document, taking it down to 465 words.
fn span(pieces: &[Placed], from: usize, to: usize) -> String {
    let mut out = String::new();
    for piece in pieces {
        if piece.end <= from || piece.start >= to {
            continue;
        }
        if piece.is_word {
            // A word belongs to the run it starts in, and to no other.
            if piece.start >= from {
                out.push_str(&piece.text);
            }
        } else {
            // Whitespace: copy exactly the part that falls inside this run.
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
