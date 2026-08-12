//! Convert the Bangla inside an Office file, and change nothing else.
//!
//! `.docx`, `.xlsx` and `.pptx` are ZIP archives of XML. This crate opens one,
//! rewrites only the text runs, and copies every other entry through
//! untouched — images, styles, relationships, the lot. What comes out is the
//! same document with the same formatting, tables and layout, in which the
//! legacy Bangla has become Unicode.

pub mod office;

pub use office::{convert_office, runs, Run, Summary};
