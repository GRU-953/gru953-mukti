//! GRU953 Mukti — the desktop app.
//!
//! A thin shell around the same converter the command-line tool uses. Every
//! accuracy figure quoted anywhere in this project describes exactly this
//! code, because there is only one copy of it.
//!
//! # Why the window draws itself with HTML
//!
//! Bengali is a complex script: conjuncts join, vowel signs move around the
//! consonant they belong to, and a reph rides above the cluster. Getting that
//! right needs a real text-shaping engine. Tauri renders through the operating
//! system's own web view — WebKit on macOS, WebView2 on Windows, WebKitGTK on
//! Ubuntu — so the shaping is done by the same engine that draws Bengali
//! everywhere else on the machine. A Bangla tool that renders Bangla wrongly
//! is not a tool.
//!
//! The front end is plain HTML, CSS and JavaScript. No framework, no bundler,
//! no npm: the brand kit is already CSS, and there is nothing here that a
//! build step would make better.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use gru953_mukti::classify::{classify_words, Verdict};
use gru953_mukti::convert;
use gru953_mukti::dictionary::Dictionary;
use gru953_mukti::encoding::{decode, TextEncoding};
use gru953_mukti::tokenise::{tokenise, Kind};
use serde::Serialize;

/// One piece of the result, ready to be drawn.
///
/// The `changed` flag is what makes the "what changed" view possible: the
/// window can show you precisely which words Mukti touched, so its judgement
/// is something you can check rather than something you have to trust.
#[derive(Serialize)]
struct Piece {
    text: String,
    changed: bool,
}

#[derive(Serialize)]
struct Conversion {
    pieces: Vec<Piece>,
    text: String,
    /// The text exactly as it arrived, so the window can show the original
    /// beside the result when a file is opened.
    source: String,
    converted: usize,
    untouched: usize,
    /// Only set when a file was read, so the window can say when it found a
    /// Windows-1252 file — which is what a genuine legacy document usually is.
    encoding: Option<String>,
}

fn convert_str(input: &str, encoding: Option<String>) -> Conversion {
    let dictionary = Dictionary::shipped();
    let segments = tokenise(input);
    let words: Vec<&str> = segments
        .iter()
        .filter(|s| s.kind == Kind::Word)
        .map(|s| s.text)
        .collect();
    let verdicts = classify_words(&words, dictionary);

    let mut pieces = Vec::with_capacity(segments.len());
    let mut text = String::with_capacity(input.len());
    let (mut converted, mut untouched) = (0usize, 0usize);
    let mut w = 0usize;

    for segment in &segments {
        match segment.kind {
            Kind::Gap => {
                text.push_str(segment.text);
                pieces.push(Piece {
                    text: segment.text.to_owned(),
                    changed: false,
                });
            }
            Kind::Word => {
                let changed = verdicts[w] == Verdict::Legacy;
                let out = if changed {
                    converted += 1;
                    convert(segment.text)
                } else {
                    untouched += 1;
                    segment.text.to_owned()
                };
                text.push_str(&out);
                pieces.push(Piece { text: out, changed });
                w += 1;
            }
        }
    }

    Conversion {
        pieces,
        text,
        source: input.to_owned(),
        converted,
        untouched,
        encoding,
    }
}

/// Convert whatever is in the box on the left.
#[tauri::command]
fn convert_text(text: String) -> Conversion {
    convert_str(&text, None)
}

/// Convert a file the user dropped on the window.
///
/// Reads bytes rather than text on purpose: a legacy file is usually
/// Windows-1252, and `decode` is what works that out. Nothing is written —
/// saving is a separate, deliberate step the user takes.
#[tauri::command]
fn convert_file(path: String) -> Result<Conversion, String> {
    let bytes = std::fs::read(&path).map_err(|e| {
        format!("Could not open that file: {e}. Check it is not open in another programme.")
    })?;
    let (text, encoding) = decode(&bytes);
    let label = match encoding {
        TextEncoding::Utf8 => "UTF-8",
        TextEncoding::Windows1252 => "Windows-1252",
    };
    Ok(convert_str(&text, Some(label.to_owned())))
}

/// Save the converted text where the user asked.
#[tauri::command]
fn save_text(path: String, text: String) -> Result<(), String> {
    // Always UTF-8: Bengali does not fit in the encoding the file arrived in.
    std::fs::write(&path, text.as_bytes())
        .map_err(|e| format!("Could not save to that place: {e}. Check you can write to it."))
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            convert_text,
            convert_file,
            save_text
        ])
        .run(tauri::generate_context!())
        .expect("the window could not be opened");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_pieces_always_rebuild_the_result_exactly() {
        for input in [
            "Report: Kg\u{a9}m~wP for 2026 এবং done",
            "Programme operations, unchanged.",
            "  spaces\tand\ttabs  \n\n",
            "",
        ] {
            let c = convert_str(input, None);
            let rebuilt: String = c.pieces.iter().map(|p| p.text.as_str()).collect();
            assert_eq!(rebuilt, c.text, "the drawn pieces disagree with the result");
        }
    }

    #[test]
    fn nothing_but_legacy_words_is_ever_marked_as_changed() {
        let c = convert_str("Report: Kg\u{a9}m~wP for 2026 এবং done", None);
        let changed: Vec<&str> = c
            .pieces
            .iter()
            .filter(|p| p.changed)
            .map(|p| p.text.as_str())
            .collect();
        assert_eq!(changed, vec!["কর্মসূচি"]);
        assert_eq!(c.converted, 1);
    }

    #[test]
    fn text_with_nothing_legacy_comes_back_byte_for_byte() {
        let input = "Programme operations and budget review for 2026.";
        let c = convert_str(input, None);
        assert_eq!(c.text, input);
        assert_eq!(c.converted, 0);
    }
}
