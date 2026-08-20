//! Nothing panics, whatever the bytes.
//!
//! The conversion core already states this as a property and lets `proptest`
//! spend thousands of cases on it. The pre-2007 Office reader had no such
//! cover when it shipped in v0.5.0: it hands bytes to `office_oxide`, a crate
//! four months old at the time. Its read path has no production panic sites —
//! that was checked by reading it — but every file it had ever been given
//! here was **valid**. All 141 in the measurement archive are well-formed, so
//! the archive proves nothing at all about damaged input.
//!
//! A panic in a library is not a tidy error. In the desktop app it is the window
//! disappearing while somebody has their document open, so "it returns an error"
//! and "it panics" are completely different outcomes and only the first is
//! acceptable.
//!
//! The PDF reader once had three tests here too. PDF support was removed in
//! 0.9.0 along with the reader itself, so they went with it.

use std::panic;

use mukti_formats::{convert_legacy_office, LegacyFormat};

/// Run `f` and report whether it panicked, without letting the panic escape.
///
/// The hook is silenced first: these tests provoke panics deliberately if the
/// code is wrong, and a page of backtrace per case makes the real failure hard
/// to find.
fn panicked<T>(f: impl FnOnce() -> T + panic::UnwindSafe) -> bool {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(|_| {}));
    let outcome = panic::catch_unwind(f);
    panic::set_hook(previous);
    outcome.is_err()
}

/// The OLE2 signature every `.doc`, `.xls` and `.ppt` begins with.
const OLE2_MAGIC: [u8; 8] = [0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1];

/// Inputs designed to be wrong in a different way each time.
fn hostile_bytes() -> Vec<(&'static str, Vec<u8>)> {
    let mut cases: Vec<(&'static str, Vec<u8>)> = Vec::new();

    cases.push(("empty", Vec::new()));
    cases.push(("one byte", vec![0xD0]));
    cases.push(("the signature and nothing else", OLE2_MAGIC.to_vec()));

    // The signature, then a header of zeroes. Every offset and count a reader
    // trusts is now zero, which is the classic way a container parser divides by
    // nothing or indexes an empty vector.
    let mut zeroed = OLE2_MAGIC.to_vec();
    zeroed.extend(std::iter::repeat_n(0u8, 512));
    cases.push(("signature then all zeroes", zeroed));

    // The same, but every count and offset is the largest it can be. A reader
    // that multiplies a sector number by a sector size overflows here.
    let mut maxed = OLE2_MAGIC.to_vec();
    maxed.extend(std::iter::repeat_n(0xFFu8, 512));
    cases.push(("signature then all ones", maxed));

    // A header claiming an absurd sector size. 2^0xFFFF is not a size.
    let mut absurd_sector = OLE2_MAGIC.to_vec();
    absurd_sector.extend(std::iter::repeat_n(0u8, 22));
    absurd_sector.extend_from_slice(&[0xFF, 0xFF]); // sector shift
    absurd_sector.extend(std::iter::repeat_n(0u8, 488));
    cases.push(("absurd sector size", absurd_sector));

    // Truncated part-way through the header, which is where an eager reader
    // slices past the end of what it was given.
    cases.push((
        "truncated mid-header",
        OLE2_MAGIC.iter().copied().chain(0u8..=200).collect(),
    ));

    // Not an OLE2 file at all, but claiming to be one by its extension. A zip,
    // which is what the *modern* formats are — an easy way to send a reader
    // down the wrong path.
    cases.push(("a zip pretending to be a .doc", b"PK\x03\x04rest".to_vec()));

    // Text, which is what a mislabelled file usually is.
    cases.push(("plain text", b"This is not a document at all.\n".repeat(4)));

    // A directory entry loop is the pathological case for any tree walk: the
    // bytes below are a header followed by entries whose sibling pointers can
    // only be read as pointing back at themselves.
    let mut looped = OLE2_MAGIC.to_vec();
    looped.extend(std::iter::repeat_n(0u8, 40));
    looped.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    looped.extend(std::iter::repeat_n(0x01u8, 1024));
    cases.push(("self-referencing directory bytes", looped));

    cases
}

#[test]
fn a_damaged_old_format_file_is_refused_and_never_panics() {
    for format in [LegacyFormat::Doc, LegacyFormat::Xls, LegacyFormat::Ppt] {
        for (name, bytes) in hostile_bytes() {
            let bytes = bytes.clone();
            let crashed = panicked(move || convert_legacy_office(&bytes, format));
            assert!(
                !crashed,
                "converting {name:?} as {format:?} panicked; it must return an error"
            );
        }
    }
}

#[test]
fn the_refusal_is_written_for_a_person_not_a_parser() {
    // Every error a user can provoke has to read like a sentence. The reader
    // underneath talks about EOCD records and sector shifts, and that wording
    // must not reach anybody.
    let leaks = [
        "EOCD",
        "sector shift",
        "unwrap",
        "panicked",
        "Err(",
        "None",
        "CFB",
        "I/O error",
        "buffer",
        "OfficeError",
        "UnsupportedFormat",
    ];
    for (name, bytes) in hostile_bytes() {
        if let Err(message) = convert_legacy_office(&bytes, LegacyFormat::Doc) {
            for leak in leaks {
                assert!(
                    !message.contains(leak),
                    "the message for {name:?} leaks parser wording {leak:?}: {message}"
                );
            }
            assert!(
                message.chars().next().is_some_and(|c| !c.is_uppercase())
                    || message.starts_with("the "),
                "the message for {name:?} should read as a clause: {message}"
            );
        }
    }
}

#[test]
fn a_damaged_modern_office_file_is_refused_in_plain_english_too() {
    // The same lens, pointed at the older path. `convert_office` has shipped
    // since 0.3.0 and the app puts whatever it returns into the window.
    use mukti_formats::convert_office;
    let cases: &[(&str, &[u8])] = &[
        ("empty", b""),
        ("not a zip", b"this is not a zip at all"),
        ("a truncated zip", b"PK\x03\x04\x14\x00\x00\x00"),
        ("a zip with no document part", b"PK\x05\x06\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00"),
    ];
    let leaks = [
        "EOCD",
        "unwrap",
        "panicked",
        "Err(",
        "InvalidArchive",
        "Io(",
        "quick_xml",
        "SyntaxError",
        "ZipError",
    ];
    for (name, bytes) in cases {
        let owned = bytes.to_vec();
        assert!(
            !panicked(move || convert_office(&owned, "Nirmala UI")),
            "a {name} .docx panicked"
        );
        if let Err(e) = convert_office(bytes, "Nirmala UI") {
            let message = e.to_string();
            for leak in leaks {
                assert!(
                    !message.contains(leak),
                    "the message for {name:?} leaks parser wording {leak:?}: {message}"
                );
            }
        }
    }
}
