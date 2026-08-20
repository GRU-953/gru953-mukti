//! Convert the Bangla inside an Office file, and change nothing else.
//!
//! `.docx`, `.xlsx` and `.pptx` are ZIP archives of XML. This crate opens one,
//! rewrites only the text runs, and copies every other entry through
//! untouched — images, styles, relationships, the lot. What comes out is the
//! same document with the same formatting, tables and layout, in which the
//! legacy Bangla has become Unicode.

pub mod legacy_office;
pub mod office;

pub use legacy_office::{
    convert_legacy_office, LegacyFormat, LegacyOutcome, PLAIN_TEXT_ONLY_NOTICE,
};
pub use office::{convert_office, runs, Run, Summary};

use gru953_mukti::classify::{convert_pieces, count};

/// Convert plain text word by word, counting what changed.
///
/// Shared so the legacy `.doc`/`.xls`/`.ppt` reader and anything else needing
/// plain text go through exactly the same classifier as the Office
/// rewriter — one decision-maker, so the accuracy figures describe all of
/// them. Not called by `mukti-cli` directly: the CLI reads only the six
/// Office formats, and this is `legacy_office`'s route into that shared
/// classifier for the three that carry no XML at all.
pub fn convert_text_with_summary(input: &str) -> (String, Summary) {
    let pieces = convert_pieces(input);
    let (converted, untouched) = count(&pieces);
    let summary = Summary {
        words_converted: converted,
        words_untouched: untouched,
        ..Summary::default()
    };
    (pieces.into_iter().map(|p| p.text).collect(), summary)
}
