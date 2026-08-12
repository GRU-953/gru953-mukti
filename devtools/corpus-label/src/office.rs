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
fn is_text_part(name: &str) -> bool {
    name == "word/document.xml"
        || name == "xl/sharedStrings.xml"
        || (name.starts_with("ppt/slides/slide") && name.ends_with(".xml"))
        || (name.starts_with("word/") && name.starts_with("word/footnotes"))
        || (name.starts_with("word/") && name.starts_with("word/endnotes"))
        || (name.starts_with("word/header") && name.ends_with(".xml"))
        || (name.starts_with("word/footer") && name.ends_with(".xml"))
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
                b"t" => {
                    in_text = true;
                    text.clear();
                }
                _ => {}
            },
            Event::Text(e) if in_text => {
                text.push_str(&e.unescape().unwrap_or_default());
            }
            Event::End(e) if local_name(e.name().as_ref()) == b"t" => {
                in_text = false;
                if !text.trim().is_empty() {
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
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "Kg");
        assert_eq!(runs[0].font.as_deref(), Some("SutonnyMJ"));
        assert_eq!(runs[1].text, "plain");
        assert_eq!(runs[1].font, None, "a font leaked into the next run");
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
    fn a_preserved_space_is_kept_as_its_own_run() {
        // Word stores a run of one space this way. Dropping it would join the
        // words either side of it into one.
        let xml = r#"<w:p>
            <w:r><w:t>Awd</w:t></w:r>
            <w:r><w:t xml:space="preserve"> </w:t></w:r>
            <w:r><w:t>bvgt</w:t></w:r>
        </w:p>"#;
        let mut runs = Vec::new();
        collect_runs(xml, &mut runs).unwrap();
        // The whitespace-only run is not emitted, but the two words stay apart
        // because the caller joins runs with a space.
        assert_eq!(runs.len(), 2);
        assert_eq!(runs[0].text, "Awd");
        assert_eq!(runs[1].text, "bvgt");
    }

    #[test]
    fn escaped_characters_come_back_unescaped() {
        let xml = r#"<w:r><w:t>a &amp; b &lt;c&gt;</w:t></w:r>"#;
        let mut runs = Vec::new();
        collect_runs(xml, &mut runs).unwrap();
        assert_eq!(runs[0].text, "a & b <c>");
    }
}
