//! Convert the Bangla inside an Office file, and change nothing else.
//!
//! `.docx`, `.xlsx` and `.pptx` are ZIP archives of XML. This crate opens one,
//! rewrites only the text runs, and copies every other entry through
//! untouched — images, styles, relationships, the lot. What comes out is the
//! same document with the same formatting, tables and layout, in which the
//! legacy Bangla has become Unicode.

pub mod office;
pub mod pdf;

pub use office::{convert_office, runs, Run, Summary};
pub use pdf::convert_pdf_to_text;

use gru953_mukti::classify::{classify_words, Verdict};
use gru953_mukti::dictionary::Dictionary;
use gru953_mukti::tokenise::{tokenise, Kind};

/// Convert plain text word by word, counting what changed.
///
/// Shared so the PDF reader and anything else needing plain text go through
/// exactly the same classifier as the Office rewriter — one decision-maker,
/// so the accuracy figures describe all of them.
pub fn convert_text_with_summary(input: &str) -> (String, Summary) {
    let dictionary = Dictionary::shipped();
    let segments = tokenise(input);
    let words: Vec<&str> = segments
        .iter()
        .filter(|s| s.kind == Kind::Word)
        .map(|s| s.text)
        .collect();
    let verdicts = classify_words(&words, dictionary);

    let mut out = String::with_capacity(input.len());
    let mut summary = Summary::default();
    let mut w = 0usize;
    for segment in &segments {
        match segment.kind {
            Kind::Gap => out.push_str(segment.text),
            Kind::Word => {
                if verdicts[w] == Verdict::Legacy {
                    out.push_str(&gru953_mukti::convert(segment.text));
                    summary.words_converted += 1;
                } else {
                    out.push_str(segment.text);
                    summary.words_untouched += 1;
                }
                w += 1;
            }
        }
    }
    (out, summary)
}
