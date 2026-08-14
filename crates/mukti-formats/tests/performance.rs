//! Cost must grow with the size of a document, not with its square.
//!
//! Until 14 August 2026 rewriting a document cost the number of runs times the
//! number of pieces, because the function handing each run its share of the text
//! scanned the whole document to find it. Nothing in the test suite could see
//! that: every fixture was small, and small squared is still small.
//!
//! Converting an 8.1 GB archive found it at once — five spreadsheets that never
//! finished inside 300 seconds and twelve that took over thirty, every one an
//! `.xlsx`. One real 62 MB workbook took **61 ms to read** and **131 seconds to
//! convert**, and converted nothing at all, having no legacy Bangla in it.
//!
//! Shared strings are what made it visible. Excel stores each distinct cell
//! string once in `xl/sharedStrings.xml` and has cells refer to it by index, so a
//! spreadsheet with a lot of text has a lot of runs in one part. The same text
//! written inline was instant, which is why no fixture here had ever caught it.

use std::io::Write;
use std::time::Instant;

use mukti_formats::convert_office;

/// A workbook holding `n` distinct shared strings, the way Excel writes one.
fn shared_string_workbook(n: usize) -> Vec<u8> {
    let mut strings = format!(
        r#"<?xml version="1.0"?><sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="{n}" uniqueCount="{n}">"#
    );
    let mut sheet = String::from(
        r#"<?xml version="1.0"?><worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><sheetData>"#,
    );
    for i in 0..n {
        strings.push_str(&format!("<si><t>Programme review row {i}</t></si>"));
        sheet.push_str(&format!(
            r#"<row r="{}"><c r="A{}" t="s"><v>{i}</v></c></row>"#,
            i + 1,
            i + 1
        ));
    }
    strings.push_str("</sst>");
    sheet.push_str("</sheetData></worksheet>");

    let content_types = concat!(
        r#"<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">"#,
        r#"<Default Extension="xml" ContentType="application/xml"/>"#,
        r#"<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>"#,
        r#"<Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/>"#,
        r#"<Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/>"#,
        r#"<Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/>"#,
        r#"</Types>"#
    );
    let root_rels = concat!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/>"#,
        r#"</Relationships>"#
    );
    let workbook = concat!(
        r#"<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" "#,
        r#"xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships">"#,
        r#"<sheets><sheet name="S" sheetId="1" r:id="rId1"/></sheets></workbook>"#
    );
    let workbook_rels = concat!(
        r#"<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">"#,
        r#"<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/>"#,
        r#"</Relationships>"#
    );

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let options = zip::write::SimpleFileOptions::default();
    for (name, body) in [
        ("[Content_Types].xml", content_types.to_string()),
        ("_rels/.rels", root_rels.to_string()),
        ("xl/workbook.xml", workbook.to_string()),
        ("xl/_rels/workbook.xml.rels", workbook_rels.to_string()),
        ("xl/sharedStrings.xml", strings),
        ("xl/worksheets/sheet1.xml", sheet),
    ] {
        zip.start_file(name, options).expect("start the part");
        zip.write_all(body.as_bytes()).expect("write the part");
    }
    zip.finish().expect("finish the archive").into_inner()
}

fn seconds_to_convert(strings: usize) -> f64 {
    let bytes = shared_string_workbook(strings);
    let started = Instant::now();
    let (_out, summary) = convert_office(&bytes, "Nirmala UI").expect("it should convert");
    assert_eq!(
        summary.words_converted, 0,
        "the text is English; nothing should have been converted"
    );
    started.elapsed().as_secs_f64()
}

#[test]
fn a_spreadsheet_does_not_get_quadratically_slower_as_it_grows() {
    // A ratio, not a wall-clock limit. A slower machine changes both timings by
    // the same factor, so an absolute bound would be either flaky or useless.
    // Four times the strings should cost roughly four times the time; the old
    // behaviour cost about sixteen.
    let _warm = seconds_to_convert(2_000);
    let small = seconds_to_convert(8_000).max(0.001);
    let large = seconds_to_convert(32_000);
    let ratio = large / small;

    assert!(
        ratio < 8.0,
        "four times the shared strings cost {ratio:.1} times the time. Linear is \
         about 4 and quadratic about 16, so the cost has gone quadratic again: \
         {small:.3}s for 8,000 strings against {large:.3}s for 32,000."
    );
}
