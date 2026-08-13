//! What happens when the file was built to break us, rather than to be read.
//!
//! A document is something a stranger emails you. Until 13 August 2026 this crate
//! read every XML part with an unbounded `read_to_string` and no cap on part size,
//! entry count or expansion ratio — so a small file could ask for all the memory
//! in the machine and get it. Two of the three vulnerabilities fixed that day were
//! in exactly this area, published by other people about the libraries underneath.
//!
//! The limits these tests check were **measured, not chosen.** Across all 1,377
//! documents in the project's own corpus the largest real XML part is 91.5 MB, the
//! most entries in one archive is 528, and the highest real compression ratio is
//! 83x. A limit picked by intuition — 10 MB sounds generous — would have rejected
//! a document somebody actually uses. That is why the corpus was measured first,
//! and it is the difference between a limit and a guess.
//!
//! Each test builds a hostile file in memory, so there is nothing to check in and
//! nothing that goes stale.

use std::io::Write;

use mukti_formats::convert_office;

/// A minimal but structurally valid `.docx`, with one entry replaced by whatever
/// we want to be nasty with.
fn docx_with(extra_name: &str, extra_body: &[u8]) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (name, body) in [
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
        (
            "word/document.xml",
            "<w:document><w:body><w:p><w:r><w:t>hello</w:t></w:r></w:p></w:body></w:document>",
        ),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(body.as_bytes()).unwrap();
    }
    if !extra_name.is_empty() {
        zip.start_file(extra_name, opts).unwrap();
        zip.write_all(extra_body).unwrap();
    }
    zip.finish().unwrap().into_inner()
}

/// An archive with far more entries than any real document has.
#[test]
fn an_absurd_number_of_entries_is_refused() {
    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for i in 0..5_000 {
        zip.start_file(format!("word/junk{i}.xml"), opts).unwrap();
        zip.write_all(b"<x/>").unwrap();
    }
    let many = zip.finish().unwrap().into_inner();

    let err = convert_office(&many, "Nirmala UI")
        .expect_err("5,000 entries is nine times the largest real document");
    assert!(err.to_string().contains("entries"), "{}", err);
}

/// The limits must not reject anything a real document does.
///
/// 1 MB here, and the size was arrived at the hard way. 100 MB first, then 12 MB,
/// and both took MINUTES — not because of the zip, but because converting that much
/// text in a debug build means millions of dictionary lookups without optimisation.
/// A test suite slow enough that people stop running it protects nothing.
///
/// 1 MB is still 1.5x the corpus p99 of 673 KB, so it exercises the real path with a
/// larger part than 99 documents in 100 contain. What it does NOT prove is that the
/// 256 MB limit clears the 91.5 MB largest real part — that is asserted exactly, and
/// for nothing, by `the_limits_clear_every_size_the_corpus_contains` beside the
/// constants. Two cheap tests covering more than one enormous one did.
#[test]
fn a_large_but_legitimate_document_is_still_accepted() {
    let mut body = String::from("<w:document><w:body>");
    let paragraph = "<w:p><w:r><w:t>Programme operations and budget review.</w:t></w:r></w:p>";
    while body.len() < 1024 * 1024 {
        body.push_str(paragraph);
    }
    body.push_str("</w:body></w:document>");

    let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default();
    for (name, content) in [
        ("[Content_Types].xml", "<Types/>"),
        ("_rels/.rels", "<Relationships/>"),
    ] {
        zip.start_file(name, opts).unwrap();
        zip.write_all(content.as_bytes()).unwrap();
    }
    zip.start_file("word/document.xml", opts).unwrap();
    zip.write_all(body.as_bytes()).unwrap();
    let big = zip.finish().unwrap().into_inner();

    let (out, summary) = convert_office(&big, "Nirmala UI")
        .expect("a 1 MB part is ordinary for a real document and must be read");
    assert!(!out.is_empty());
    assert_eq!(
        summary.words_converted, 0,
        "there is no legacy Bangla in it, so nothing should convert"
    );
}

/// Truncated, empty and nonsense files fail as errors rather than panics.
///
/// A panic in the library is a window vanishing while somebody's document is open.
/// An error is a sentence they can read.
#[test]
fn damaged_files_produce_an_error_and_never_a_panic() {
    let good = docx_with("", b"");
    let cases: Vec<(&str, Vec<u8>)> = vec![
        ("empty", Vec::new()),
        ("one byte", vec![b'P']),
        ("zip header only", b"PK\x03\x04".to_vec()),
        ("truncated halfway", good[..good.len() / 2].to_vec()),
        ("truncated to a third", good[..good.len() / 3].to_vec()),
        (
            "plain text pretending",
            b"this is not a document at all".to_vec(),
        ),
        ("all zeroes", vec![0u8; 4096]),
        ("high bytes", vec![0xFFu8; 4096]),
    ];

    for (what, bytes) in cases {
        // Any outcome is acceptable except a panic, which is what this asserts.
        let outcome = std::panic::catch_unwind(|| {
            let _ = convert_office(&bytes, "Nirmala UI");
            let _ = mukti_formats::runs(std::io::Cursor::new(&bytes));
        });
        assert!(outcome.is_ok(), "reading a {what} file panicked");
    }
}
