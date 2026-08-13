//! Convert the Bangla inside an Office file, and change nothing else.
//!
//! `.docx`, `.xlsx` and `.pptx` are ZIP archives of XML. This crate opens one,
//! rewrites only the text runs, and copies every other entry through
//! untouched — images, styles, relationships, the lot. What comes out is the
//! same document with the same formatting, tables and layout, in which the
//! legacy Bangla has become Unicode.

pub mod legacy_office;
pub mod office;
pub mod pdf;

pub use legacy_office::{
    convert_legacy_office, LegacyFormat, LegacyOutcome, PLAIN_TEXT_ONLY_NOTICE,
};
pub use office::{convert_office, runs, Run, Summary};
pub use pdf::convert_pdf_to_text;

use gru953_mukti::classify::{convert_pieces, count};

/// Convert plain text word by word, counting what changed.
///
/// Shared so the PDF reader and anything else needing plain text go through
/// exactly the same classifier as the Office rewriter — one decision-maker,
/// so the accuracy figures describe all of them.
pub fn convert_text_with_summary(input: &str) -> (String, Summary) {
    let pieces = convert_pieces(input);
    let (converted, untouched) = count(&pieces);
    let mut summary = Summary::default();
    summary.words_converted = converted;
    summary.words_untouched = untouched;
    (pieces.into_iter().map(|p| p.text).collect(), summary)
}
