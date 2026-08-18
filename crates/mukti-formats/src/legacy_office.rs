//! Read the pre-2007 binary Office formats and write modern ones.
//!
//! `.doc`, `.xls` and `.ppt` are not ZIP archives of XML like their modern
//! counterparts — they are OLE2 compound files, a small filesystem inside one
//! file, with the text held in binary records. There is no way to rewrite them
//! in place the way [`crate::office`] rewrites a `.docx`, so this module reads
//! the text out and writes a **new** modern document.
//!
//! # What carries over, and what does not
//!
//! The text, and nothing else. Not the fonts, sizes, colours, headings, tables,
//! images, headers, footers, comments or page layout. A converted `.doc` is the
//! words of the original in a plain modern Word file.
//!
//! That is a real limitation and it is stated rather than hidden: the reader
//! this module uses extracts plain text only. It also means old formats carry no
//! font information, so — unlike `.docx`, where a run in SutonnyMJ is known to
//! be legacy before a single letter is examined — the decision here rests on the
//! words alone, exactly as it does for a `.txt` file. Accuracy is the plain-text
//! accuracy, not the higher font-gated accuracy quoted for `.docx`.
//!
//! The original file is never modified.

use std::io::{Cursor, Write};

use office_oxide::{Document, DocumentFormat};
use zip::write::SimpleFileOptions;
use zip::{CompressionMethod, ZipWriter};

use crate::office::Summary;

/// Which of the three old formats this is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyFormat {
    Doc,
    Xls,
    Ppt,
}

impl LegacyFormat {
    /// Recognise one from a file extension, in any case.
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext.to_ascii_lowercase().as_str() {
            "doc" => Some(Self::Doc),
            "xls" => Some(Self::Xls),
            "ppt" => Some(Self::Ppt),
            _ => None,
        }
    }

    /// The modern format a converted file is written as.
    pub fn modern_extension(self) -> &'static str {
        match self {
            Self::Doc => "docx",
            Self::Xls => "xlsx",
            Self::Ppt => "pptx",
        }
    }

    fn reader_format(self) -> DocumentFormat {
        match self {
            Self::Doc => DocumentFormat::Doc,
            Self::Xls => DocumentFormat::Xls,
            Self::Ppt => DocumentFormat::Ppt,
        }
    }
}

/// What came of converting one old file.
#[derive(Debug, Clone)]
pub struct LegacyOutcome {
    /// The modern document, ready to write to disk.
    pub document: Vec<u8>,
    /// Word counts, the same shape the rest of the crate reports.
    pub summary: Summary,
    /// Paragraphs, or spreadsheet rows, carried across.
    pub blocks: usize,
    /// True when the file held no recoverable text at all.
    pub was_empty: bool,
}

/// The sentence to show a person after converting an old file.
///
/// Kept here beside the conversion so no two callers can
/// drift into describing the same limitation two different ways.
pub const PLAIN_TEXT_ONLY_NOTICE: &str =
    "Old .doc, .xls and .ppt files store no font information, so only the text \
     could be carried across — not the formatting, tables or images. Accuracy on \
     these files matches plain text rather than the higher figure quoted for \
     .docx files.";

/// Read an old Office file, convert the Bangla in it, and return a modern one.
pub fn convert_legacy_office(bytes: &[u8], format: LegacyFormat) -> Result<LegacyOutcome, String> {
    if bytes.is_empty() {
        return Err("the file is empty — there is nothing in it to convert".to_owned());
    }

    // The reader's own wording is not shown to anybody. It talks about CFB
    // errors, sector shifts and failing "to fill whole buffer", and a caller puts
    // whatever comes back here straight in front of a person. Found on 14 August 2026
    // by testing damaged files rather than only valid ones — every file in the
    // measurement archive is well-formed, so the archive could never have shown
    // this.
    //
    // Every failure here means the same thing to a person, whatever the parser
    // called it: these bytes are not a readable file of this kind.
    let document = Document::from_reader(Cursor::new(bytes.to_vec()), format.reader_format())
        .map_err(|_| {
            format!(
                "the file could not be read as an older .{} — it may be damaged or \
                 incomplete, or it may be a different kind of file with that name",
                match format {
                    LegacyFormat::Doc => "doc",
                    LegacyFormat::Xls => "xls",
                    LegacyFormat::Ppt => "ppt",
                }
            )
        })?;

    // PowerPoint keeps its own path, because the old format records where each
    // slide starts and which text was a title. Throwing that away and working
    // from one flat string would lose structure the file actually has.
    if format == LegacyFormat::Ppt {
        let (slides, summary, any_text) = convert_slides(&document);
        let (blocks, bytes) = write_pptx(&slides)?;
        return Ok(LegacyOutcome {
            document: bytes,
            summary,
            blocks,
            was_empty: !any_text,
        });
    }

    let text = document.plain_text();
    let (converted, summary) = crate::convert_text_with_summary(&text);
    let was_empty = converted.trim().is_empty();

    let (blocks, bytes) = match format {
        LegacyFormat::Xls => write_xlsx(&converted),
        _ => write_docx(&converted),
    }?;

    let (document, summary) = settle(bytes, summary)?;
    Ok(LegacyOutcome {
        document,
        summary,
        blocks,
        was_empty,
    })
}

/// The font a converted document asks for. The same one the command-line tool
/// uses, so every caller produces identical documents.
const UNICODE_FONT: &str = "Nirmala UI";

/// Run the finished document through the ordinary Office converter once, so
/// what comes out cannot be improved by converting it again.
///
/// **Why this is needed.** The text of an old file is classified all at once,
/// while the Office rewriter classifies a paragraph at a time. Context changes
/// the verdict, so the two can disagree — and they did, on one document in the
/// archive: the `.docx` we had just written still contained a word the Office
/// pass would convert. That breaks the invariant this project cares most about,
/// that converting twice is the same as converting once, and a user who
/// converted their `.doc` and then converted the result would have seen the file
/// change again.
///
/// Running the Office pass here makes the output a fixed point by construction.
/// It costs one extra pass over a document we already hold in memory, and it
/// reuses the converter that is already property-tested for idempotence rather
/// than adding a second opinion about what a legacy word is.
fn settle(bytes: Vec<u8>, mut summary: Summary) -> Result<(Vec<u8>, Summary), String> {
    match crate::office::convert_office(&bytes, UNICODE_FONT) {
        Ok((settled, second)) => {
            // The second pass judges the same words again, so only what it
            // *changed* is new; everything else was already counted.
            summary.words_converted += second.words_converted;
            summary.words_untouched = summary
                .words_untouched
                .saturating_sub(second.words_converted);
            Ok((settled, summary))
        }
        // If the document we just wrote cannot be read back, that is worth
        // knowing about rather than papering over.
        Err(e) => Err(format!("the converted document could not be re-read: {e}")),
    }
}

/// Convert every text run on every slide, keeping titles apart from bodies.
fn convert_slides(document: &Document) -> (Vec<Slide>, Summary, bool) {
    use office_oxide::ppt::TextType;

    let mut out = Vec::new();
    let mut total = Summary::default();
    let mut any_text = false;

    let Some(ppt) = document.as_ppt() else {
        return (out, total, any_text);
    };

    for slide in &ppt.slides {
        let mut title = String::new();
        let mut body = Vec::new();
        for run in &slide.text_runs {
            // Speaker notes have nowhere honest to go on a slide.
            if run.text_type == TextType::Notes {
                continue;
            }
            let (converted, summary) = crate::convert_text_with_summary(&run.text);
            total.words_converted += summary.words_converted;
            total.words_untouched += summary.words_untouched;
            for line in converted.split('\n') {
                if !line.trim().is_empty() {
                    any_text = true;
                }
                if matches!(run.text_type, TextType::Title | TextType::CenterTitle)
                    && title.is_empty()
                    && !line.trim().is_empty()
                {
                    title = line.to_owned();
                } else {
                    body.push(line.to_owned());
                }
            }
        }
        out.push(Slide { title, body });
    }
    (out, total, any_text)
}

/// Escape the five characters XML reserves, and drop the ones it forbids.
///
/// A control character inside a `<w:t>` element makes Word refuse the whole
/// file, and old binary formats do carry stray control bytes.
fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 16);
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            // Tab, newline and carriage return are the only control characters
            // XML 1.0 permits. Everything below 0x20 otherwise is invalid, and
            // so are the two unpaired-surrogate replacements.
            c if (c as u32) < 0x20 && c != '\t' && c != '\n' && c != '\r' => {}
            '\u{FFFE}' | '\u{FFFF}' => {}
            c => out.push(c),
        }
    }
    out
}

/// Store every part deflated, which is what Word itself writes.
fn options() -> SimpleFileOptions {
    SimpleFileOptions::default().compression_method(CompressionMethod::Deflated)
}

fn finish(parts: Vec<(&str, String)>) -> Result<Vec<u8>, String> {
    let mut buffer = Cursor::new(Vec::new());
    {
        let mut zip = ZipWriter::new(&mut buffer);
        for (name, body) in parts {
            zip.start_file(name, options())
                .map_err(|e| format!("could not start {name}: {e}"))?;
            zip.write_all(body.as_bytes())
                .map_err(|e| format!("could not write {name}: {e}"))?;
        }
        zip.finish().map_err(|e| format!("could not finish: {e}"))?;
    }
    Ok(buffer.into_inner())
}

const XML_DECLARATION: &str = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>"#;

/// Word.
fn write_docx(text: &str) -> Result<(usize, Vec<u8>), String> {
    let paragraphs: Vec<&str> = text.split('\n').collect();
    let mut body = String::new();
    for line in &paragraphs {
        // Tabs inside a paragraph become real Word tabs, so a table row read out
        // of the old file still lines up when it is opened.
        let mut runs = String::new();
        for (i, cell) in line.split('\t').enumerate() {
            if i > 0 {
                runs.push_str("<w:r><w:tab/></w:r>");
            }
            if !cell.is_empty() {
                runs.push_str(&format!(
                    r#"<w:r><w:t xml:space="preserve">{}</w:t></w:r>"#,
                    xml_escape(cell)
                ));
            }
        }
        body.push_str(&format!("<w:p>{runs}</w:p>"));
    }

    let content_types = format!(
        r#"{XML_DECLARATION}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/></Types>"#
    );
    let rels = format!(
        r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>"#
    );
    let document = format!(
        r#"{XML_DECLARATION}<w:document xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"><w:body>{body}<w:sectPr><w:pgSz w:w="11906" w:h="16838"/></w:sectPr></w:body></w:document>"#
    );

    let bytes = finish(vec![
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("word/document.xml", document),
    ])?;
    Ok((paragraphs.len(), bytes))
}

/// Excel.
fn write_xlsx(text: &str) -> Result<(usize, Vec<u8>), String> {
    let rows: Vec<&str> = text.split('\n').collect();
    let mut sheet_rows = String::new();
    for (r, line) in rows.iter().enumerate() {
        let mut cells = String::new();
        for (c, cell) in line.split('\t').enumerate() {
            if cell.is_empty() {
                continue;
            }
            // Inline strings, so no shared-string table is needed. Every value
            // is written as text on purpose: a legacy spreadsheet's numbers are
            // already text by the time they reach here, and guessing which ones
            // are numbers would change the data.
            cells.push_str(&format!(
                r#"<c r="{}{}" t="inlineStr"><is><t xml:space="preserve">{}</t></is></c>"#,
                column_name(c),
                r + 1,
                xml_escape(cell)
            ));
        }
        sheet_rows.push_str(&format!(r#"<row r="{}">{cells}</row>"#, r + 1));
    }

    let content_types = format!(
        r#"{XML_DECLARATION}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/></Types>"#
    );
    let rels = format!(
        r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"#
    );
    let workbook = format!(
        r#"{XML_DECLARATION}<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><sheets><sheet name="Sheet1" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = format!(
        r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/></Relationships>"#
    );
    let sheet = format!(
        r#"{XML_DECLARATION}<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>{sheet_rows}</sheetData></worksheet>"#
    );

    let bytes = finish(vec![
        ("[Content_Types].xml", content_types),
        ("_rels/.rels", rels),
        ("xl/workbook.xml", workbook),
        ("xl/_rels/workbook.xml.rels", workbook_rels),
        ("xl/worksheets/sheet1.xml", sheet),
    ])?;
    Ok((rows.len(), bytes))
}

/// `A`, `B`, … `Z`, `AA`, `AB`, … — a spreadsheet column's name from its index.
fn column_name(mut index: usize) -> String {
    let mut name = Vec::new();
    loop {
        name.push(b'A' + (index % 26) as u8);
        if index < 26 {
            break;
        }
        index = index / 26 - 1;
    }
    name.reverse();
    String::from_utf8(name).expect("A-Z is always valid UTF-8")
}

/// One slide's worth of recovered text.
struct Slide {
    title: String,
    body: Vec<String>,
}

/// PowerPoint.
///
/// The old format does record where one slide ends and the next begins, and
/// which text was a title, so this rebuilds real slides rather than pouring
/// everything into one. Speaker notes are **not** carried across — there is
/// nowhere honest to put them on a slide, and silently mixing them into the body
/// would change what the slide says.
fn write_pptx(slides: &[Slide]) -> Result<(usize, Vec<u8>), String> {
    // A presentation with no slides at all will not open. An empty one will.
    let count = slides.len().max(1);
    let empty = Slide {
        title: String::new(),
        body: Vec::new(),
    };

    let mut parts: Vec<(String, String)> = Vec::new();
    let mut slide_overrides = String::new();
    let mut slide_ids = String::new();
    let mut presentation_rels = String::from(
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="slideMasters/slideMaster1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="theme/theme1.xml"/>"#,
    );

    for i in 0..count {
        let slide = slides.get(i).unwrap_or(&empty);
        let n = i + 1;
        let rid = n + 2;
        parts.push((format!("ppt/slides/slide{n}.xml"), slide_xml(slide)));
        parts.push((
            format!("ppt/slides/_rels/slide{n}.xml.rels"),
            format!(
                r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/></Relationships>"#
            ),
        ));
        slide_overrides.push_str(&format!(
            r#"<Override PartName="/ppt/slides/slide{n}.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slide+xml"/>"#
        ));
        slide_ids.push_str(&format!(r#"<p:sldId id="{}" r:id="rId{rid}"/>"#, 255 + n));
        presentation_rels.push_str(&format!(
            r#"<Relationship Id="rId{rid}" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slide" Target="slides/slide{n}.xml"/>"#
        ));
    }

    let content_types = format!(
        r#"{XML_DECLARATION}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/ppt/presentation.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"/><Override PartName="/ppt/slideMasters/slideMaster1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideMaster+xml"/><Override PartName="/ppt/slideLayouts/slideLayout1.xml" ContentType="application/vnd.openxmlformats-officedocument.presentationml.slideLayout+xml"/><Override PartName="/ppt/theme/theme1.xml" ContentType="application/vnd.openxmlformats-officedocument.theme+xml"/>{slide_overrides}</Types>"#
    );

    let mut all: Vec<(String, String)> = vec![
        ("[Content_Types].xml".to_owned(), content_types),
        (
            "_rels/.rels".to_owned(),
            format!(
                r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="ppt/presentation.xml"/></Relationships>"#
            ),
        ),
        (
            "ppt/presentation.xml".to_owned(),
            format!(
                r#"{XML_DECLARATION}<p:presentation xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:sldMasterIdLst><p:sldMasterId id="2147483648" r:id="rId1"/></p:sldMasterIdLst><p:sldIdLst>{slide_ids}</p:sldIdLst><p:sldSz cx="12192000" cy="6858000"/><p:notesSz cx="6858000" cy="9144000"/></p:presentation>"#
            ),
        ),
        (
            "ppt/_rels/presentation.xml.rels".to_owned(),
            format!(
                r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">{presentation_rels}</Relationships>"#
            ),
        ),
        (
            "ppt/slideMasters/slideMaster1.xml".to_owned(),
            slide_master_xml(),
        ),
        (
            "ppt/slideMasters/_rels/slideMaster1.xml.rels".to_owned(),
            format!(
                r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideLayout" Target="../slideLayouts/slideLayout1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/theme" Target="../theme/theme1.xml"/></Relationships>"#
            ),
        ),
        (
            "ppt/slideLayouts/slideLayout1.xml".to_owned(),
            slide_layout_xml(),
        ),
        (
            "ppt/slideLayouts/_rels/slideLayout1.xml.rels".to_owned(),
            format!(
                r#"{XML_DECLARATION}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/slideMaster" Target="../slideMasters/slideMaster1.xml"/></Relationships>"#
            ),
        ),
        ("ppt/theme/theme1.xml".to_owned(), theme_xml()),
    ];
    all.extend(parts);

    let borrowed: Vec<(&str, String)> = all.iter().map(|(n, b)| (n.as_str(), b.clone())).collect();
    let bytes = finish(borrowed)?;
    Ok((count, bytes))
}

/// A text box, positioned in English Metric Units (914,400 to the inch).
fn text_box(
    id: u32,
    name: &str,
    x: i64,
    y: i64,
    cx: i64,
    cy: i64,
    size: u32,
    lines: &[String],
) -> String {
    let mut paragraphs = String::new();
    for line in lines {
        if line.is_empty() {
            paragraphs.push_str(&format!(
                r#"<a:p><a:pPr/><a:endParaRPr lang="en-GB" sz="{size}"/></a:p>"#
            ));
        } else {
            paragraphs.push_str(&format!(
                r#"<a:p><a:r><a:rPr lang="bn-BD" sz="{size}" dirty="0"/><a:t>{}</a:t></a:r></a:p>"#,
                xml_escape(line)
            ));
        }
    }
    if paragraphs.is_empty() {
        paragraphs = format!(r#"<a:p><a:endParaRPr lang="en-GB" sz="{size}"/></a:p>"#);
    }
    format!(
        r#"<p:sp><p:nvSpPr><p:cNvPr id="{id}" name="{name}"/><p:cNvSpPr txBox="1"/><p:nvPr/></p:nvSpPr><p:spPr><a:xfrm><a:off x="{x}" y="{y}"/><a:ext cx="{cx}" cy="{cy}"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></p:spPr><p:txBody><a:bodyPr wrap="square"><a:normAutofit/></a:bodyPr><a:lstStyle/>{paragraphs}</p:txBody></p:sp>"#
    )
}

fn slide_xml(slide: &Slide) -> String {
    let mut shapes = String::new();
    if !slide.title.is_empty() {
        shapes.push_str(&text_box(
            2,
            "Title",
            838200,
            365125,
            10515600,
            1325563,
            3200,
            std::slice::from_ref(&slide.title),
        ));
    }
    if !slide.body.is_empty() {
        shapes.push_str(&text_box(
            3,
            "Content",
            838200,
            1825625,
            10515600,
            4351338,
            1800,
            &slide.body,
        ));
    }
    format!(
        r#"{XML_DECLARATION}<p:sld xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld><p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr>{shapes}</p:spTree></p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sld>"#
    )
}

const EMPTY_TREE: &str = r#"<p:spTree><p:nvGrpSpPr><p:cNvPr id="1" name=""/><p:cNvGrpSpPr/><p:nvPr/></p:nvGrpSpPr><p:grpSpPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="0" cy="0"/><a:chOff x="0" y="0"/><a:chExt cx="0" cy="0"/></a:xfrm></p:grpSpPr></p:spTree>"#;

fn slide_master_xml() -> String {
    format!(
        r#"{XML_DECLARATION}<p:sldMaster xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main"><p:cSld>{EMPTY_TREE}</p:cSld><p:clrMap bg1="lt1" tx1="dk1" bg2="lt2" tx2="dk2" accent1="accent1" accent2="accent2" accent3="accent3" accent4="accent4" accent5="accent5" accent6="accent6" hlink="hlink" folHlink="folHlink"/><p:sldLayoutIdLst><p:sldLayoutId id="2147483649" r:id="rId1"/></p:sldLayoutIdLst></p:sldMaster>"#
    )
}

fn slide_layout_xml() -> String {
    format!(
        r#"{XML_DECLARATION}<p:sldLayout xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:p="http://schemas.openxmlformats.org/presentationml/2006/main" type="blank" preserve="1"><p:cSld name="Blank">{EMPTY_TREE}</p:cSld><p:clrMapOvr><a:masterClrMapping/></p:clrMapOvr></p:sldLayout>"#
    )
}

/// The minimum theme PowerPoint will accept: twelve colours, three font slots
/// and three format lists. Every one of these elements is required — leave any
/// out and the file is refused rather than degraded.
fn theme_xml() -> String {
    let fill = r#"<a:solidFill><a:schemeClr val="phClr"/></a:solidFill>"#;
    let fill_styles = format!("<a:fillStyleLst>{fill}{fill}{fill}</a:fillStyleLst>");
    let line = format!(
        r#"<a:ln w="9525" cap="flat" cmpd="sng" algn="ctr">{fill}<a:prstDash val="solid"/></a:ln>"#
    );
    let line_styles = format!("<a:lnStyleLst>{line}{line}{line}</a:lnStyleLst>");
    let effect = r#"<a:effectStyle><a:effectLst/></a:effectStyle>"#;
    let effect_styles = format!("<a:effectStyleLst>{effect}{effect}{effect}</a:effectStyleLst>");
    let bg_styles = format!("<a:bgFillStyleLst>{fill}{fill}{fill}</a:bgFillStyleLst>");
    let colours = r#"<a:dk1><a:sysClr val="windowText" lastClr="000000"/></a:dk1><a:lt1><a:sysClr val="window" lastClr="FFFFFF"/></a:lt1><a:dk2><a:srgbClr val="44546A"/></a:dk2><a:lt2><a:srgbClr val="E7E6E6"/></a:lt2><a:accent1><a:srgbClr val="4472C4"/></a:accent1><a:accent2><a:srgbClr val="ED7D31"/></a:accent2><a:accent3><a:srgbClr val="A5A5A5"/></a:accent3><a:accent4><a:srgbClr val="FFC000"/></a:accent4><a:accent5><a:srgbClr val="5B9BD5"/></a:accent5><a:accent6><a:srgbClr val="70AD47"/></a:accent6><a:hlink><a:srgbClr val="0563C1"/></a:hlink><a:folHlink><a:srgbClr val="954F72"/></a:folHlink>"#;
    // Noto Sans Bengali is named for the complex-script slot so a slide of
    // Bangla is laid out with a font that can actually draw it.
    let font =
        r#"<a:latin typeface="Calibri"/><a:ea typeface=""/><a:cs typeface="Noto Sans Bengali"/>"#;
    format!(
        r#"{XML_DECLARATION}<a:theme xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" name="Mukti"><a:themeElements><a:clrScheme name="Mukti">{colours}</a:clrScheme><a:fontScheme name="Mukti"><a:majorFont>{font}</a:majorFont><a:minorFont>{font}</a:minorFont></a:fontScheme><a:fmtScheme name="Mukti">{fill_styles}{line_styles}{effect_styles}{bg_styles}</a:fmtScheme></a:themeElements><a:objectDefaults/><a:extraClrSchemeLst/></a:theme>"#
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_map_both_ways() {
        assert_eq!(LegacyFormat::from_extension("DOC"), Some(LegacyFormat::Doc));
        assert_eq!(LegacyFormat::from_extension("xls"), Some(LegacyFormat::Xls));
        assert_eq!(LegacyFormat::from_extension("ppt"), Some(LegacyFormat::Ppt));
        assert_eq!(LegacyFormat::from_extension("docx"), None);
        assert_eq!(LegacyFormat::Doc.modern_extension(), "docx");
        assert_eq!(LegacyFormat::Xls.modern_extension(), "xlsx");
    }

    #[test]
    fn column_names_carry_past_z() {
        assert_eq!(column_name(0), "A");
        assert_eq!(column_name(25), "Z");
        assert_eq!(column_name(26), "AA");
        assert_eq!(column_name(27), "AB");
        assert_eq!(column_name(51), "AZ");
        assert_eq!(column_name(52), "BA");
        assert_eq!(column_name(701), "ZZ");
        assert_eq!(column_name(702), "AAA");
    }

    #[test]
    fn xml_escaping_covers_the_five_and_drops_control_bytes() {
        assert_eq!(xml_escape("a<b>&\"c\'"), "a&lt;b&gt;&amp;&quot;c&apos;");
        // A stray control byte out of a binary record must not reach the file:
        // Word refuses to open a document containing one.
        assert_eq!(xml_escape("a\u{1}b\u{7}c"), "abc");
        // Tab, newline and return are legal and must survive.
        assert_eq!(xml_escape("a\tb\nc\r"), "a\tb\nc\r");
        // Bengali passes through untouched.
        assert_eq!(xml_escape("কৃষি"), "কৃষি");
    }

    #[test]
    fn an_empty_file_is_refused_in_plain_english() {
        let e = convert_legacy_office(&[], LegacyFormat::Doc).unwrap_err();
        assert!(e.contains("empty"), "unhelpful message: {e}");
        assert!(!e.contains("EOCD"), "raw parser wording leaked: {e}");
    }

    #[test]
    fn a_word_document_is_a_readable_archive_with_the_text_in_it() {
        let (blocks, bytes) = write_docx("Hello\tworld\nদ্বিতীয় লাইন").unwrap();
        assert_eq!(blocks, 2);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a valid zip");
        let names: Vec<String> = zip.file_names().map(|n| n.to_owned()).collect();
        for wanted in ["[Content_Types].xml", "_rels/.rels", "word/document.xml"] {
            assert!(names.iter().any(|n| n == wanted), "missing {wanted}");
        }
        let mut body = String::new();
        std::io::Read::read_to_string(&mut zip.by_name("word/document.xml").unwrap(), &mut body)
            .unwrap();
        assert!(body.contains("Hello"), "the text is missing");
        assert!(body.contains("দ্বিতীয় লাইন"), "the Bengali is missing");
        assert!(body.contains("<w:tab/>"), "the tab became nothing");
        assert_eq!(body.matches("<w:p>").count(), 2, "wrong paragraph count");
    }

    #[test]
    fn a_spreadsheet_puts_each_cell_in_its_own_reference() {
        let (rows, bytes) = write_xlsx("one\ttwo\nথ্রি\tfour").unwrap();
        assert_eq!(rows, 2);
        let mut zip = zip::ZipArchive::new(Cursor::new(bytes)).expect("a valid zip");
        let mut sheet = String::new();
        std::io::Read::read_to_string(
            &mut zip.by_name("xl/worksheets/sheet1.xml").unwrap(),
            &mut sheet,
        )
        .unwrap();
        assert!(sheet.contains(r#"r="A1""#), "A1 missing: {sheet}");
        assert!(sheet.contains(r#"r="B1""#), "B1 missing");
        assert!(sheet.contains(r#"r="A2""#), "A2 missing");
        assert!(sheet.contains("থ্রি"), "the Bengali cell is missing");
    }
}
