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
//!
//! # No file path ever crosses from the window
//!
//! Until 13 August 2026 the window handed Rust a path string and Rust opened
//! whatever it named — `convert_file(path)` and `save_text(path, text)`. With no
//! permission file in place at the time, those were the only commands that
//! worked, and they would read or overwrite anything on the machine.
//!
//! Rather than check paths against a list of allowed ones, the boundary moved:
//! **Rust owns the dialogs.** The window can ask to open a file or to save the
//! result, and Rust puts up the operating system's own picker and uses what the
//! person actually chose. Dropped files are handled the same way, in Rust's own
//! window event. So there is no path parameter left to abuse, rather than a
//! filter that has to be right.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use gru953_mukti::classify::{convert_pieces, count};
use gru953_mukti::encoding::{decode, TextEncoding};
use serde::Serialize;
use tauri::{Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

/// The Unicode font a converted Office document should ask for.
///
/// Present on Windows 10 and later, and the same default the command-line tool
/// uses — the two must agree or the same document converts differently
/// depending on which one you ran.
const UNICODE_FONT: &str = "Nirmala UI";

/// One piece of the result, ready to be drawn.
///
/// The `changed` flag is what makes the "what changed" view possible: the
/// window can show you precisely which words Mukti touched, so its judgement
/// is something you can check rather than something you have to trust.
#[derive(Serialize, Clone, Debug)]
struct Piece {
    text: String,
    changed: bool,
}

#[derive(Serialize, Clone, Debug)]
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
    /// What kind of thing was converted, so the window can describe it
    /// truthfully: a Word document is not editable text.
    kind: Kindness,
    /// The name of the file this came from, for the window to show. Never a full
    /// path: the folders someone keeps their documents in are their business.
    filename: Option<String>,
    /// Set when a PDF yielded no recoverable text at all, with the count of
    /// pieces that had to be skipped. 132 of 775 real PDFs do this.
    unreadable: Option<usize>,
    /// A limitation the person needs to know about this particular file, in
    /// finished English. Set for the older `.doc`, `.xls` and `.ppt` formats,
    /// which carry no formatting and no font information.
    notice: Option<String>,
}

/// What was converted. Decides what the window may offer to do next.
#[derive(Serialize, Clone, Copy, PartialEq, Eq, Debug)]
#[serde(rename_all = "lowercase")]
enum Kindness {
    /// Typed or pasted, or a plain-text file. Editable, saves as text.
    Text,
    /// Word, Excel or PowerPoint. Saves as the same kind of document.
    Document,
    /// A PDF. Read-only by design: saves as text, never back into the PDF.
    Pdf,
}

/// What `save` would write, held in Rust so the window never has to carry it.
///
/// A converted Word document is a whole rebuilt archive. Passing megabytes of it
/// out to JavaScript and back again to save it would be pointless work and one
/// more place for it to be altered on the way.
enum Payload {
    Text(String),
    Document { bytes: Vec<u8>, extension: String },
}

#[derive(Default)]
struct State {
    /// The most recent conversion, ready to save. Replaced on every conversion.
    last: Mutex<Option<Payload>>,
    /// The name of the file it came from, to suggest when saving.
    stem: Mutex<Option<String>>,
}

fn convert_str(input: &str, encoding: Option<String>) -> Conversion {
    let judged = convert_pieces(input);
    let (converted, untouched) = count(&judged);
    let text: String = judged.iter().map(|p| p.text.as_str()).collect();
    let pieces = judged
        .into_iter()
        .map(|p| Piece {
            text: p.text,
            changed: p.changed,
        })
        .collect();

    Conversion {
        pieces,
        text,
        source: input.to_owned(),
        converted,
        untouched,
        encoding,
        kind: Kindness::Text,
        filename: None,
        unreadable: None,
        notice: None,
    }
}

/// Convert whatever is in the box on the left.
#[tauri::command]
fn convert_text(text: String, state: tauri::State<'_, State>) -> Conversion {
    let result = convert_str(&text, None);
    *state.last.lock().expect("state lock") = Some(Payload::Text(result.text.clone()));
    *state.stem.lock().expect("state lock") = None;
    result
}

/// Convert one file, choosing how to read it from its name.
///
/// This mirrors `mukti-cli`'s dispatch exactly, and deliberately so: the app and
/// the command must not disagree about what a `.docx` is. Before this, the app
/// did not depend on `mukti-formats` at all — it read every file as text, so a
/// Word document arrived as mojibake, while the README claimed the window
/// handled Word, Excel, PowerPoint and PDF.
fn convert_path(path: &Path, state: &State) -> Result<Conversion, String> {
    let bytes = std::fs::read(path).map_err(|e| {
        format!("That file could not be opened: {e}. Check it has not been moved, and is not open in another programme.")
    })?;
    if bytes.is_empty() {
        return Err("That file is empty, so there is nothing to convert.".to_owned());
    }

    let filename = path.file_name().map(|n| n.to_string_lossy().to_string());
    let stem = path.file_stem().map(|n| n.to_string_lossy().to_string());
    let extension = path
        .extension()
        .map(|e| e.to_string_lossy().to_lowercase())
        .unwrap_or_default();

    let mut conversion = match extension.as_str() {
        "docx" | "xlsx" | "pptx" => {
            let (document, summary) = mukti_formats::convert_office(&bytes, UNICODE_FONT)
                .map_err(|e| format!("That document could not be read: {e}. It may be damaged, or not really a {extension} file."))?;
            // Show the text so the conversion can be checked, but keep the
            // rebuilt document as what gets saved: the formatting, tables and
            // images only survive in the document itself.
            let runs = mukti_formats::runs(std::io::Cursor::new(&document))
                .map_err(|e| format!("The converted document could not be read back: {e}"))?;
            let text: String = runs.iter().map(|r| r.text.as_str()).collect();
            *state.last.lock().expect("state lock") = Some(Payload::Document {
                bytes: document,
                extension: extension.clone(),
            });
            let mut c = convert_str(&text, None);
            // The counts come from the document rewrite, not from re-judging the
            // text we just extracted, which would count everything twice.
            c.converted = summary.words_converted;
            c.untouched = summary.words_untouched;
            c.kind = Kindness::Document;
            c
        }
        "doc" | "xls" | "ppt" => {
            let format = mukti_formats::LegacyFormat::from_extension(&extension)
                .expect("the match arm above names exactly these three");
            let outcome = mukti_formats::convert_legacy_office(&bytes, format).map_err(|e| {
                format!(
                    "That older {extension} file could not be read: {e}. It may be damaged, or it may be a newer file that has been given an old name."
                )
            })?;
            // Read the text back out of what we just wrote, so what is shown is
            // what will be saved rather than a separate guess at it.
            let text = mukti_formats::runs(std::io::Cursor::new(&outcome.document))
                .map(|runs| runs.iter().map(|r| r.text.as_str()).collect::<String>())
                .unwrap_or_default();
            *state.last.lock().expect("state lock") = Some(Payload::Document {
                bytes: outcome.document,
                extension: format.modern_extension().to_owned(),
            });
            let mut c = convert_str(&text, None);
            c.converted = outcome.summary.words_converted;
            c.untouched = outcome.summary.words_untouched;
            c.kind = Kindness::Document;
            // The one thing a person must be told about these files: only the
            // words came across, and the decision was made without the font.
            c.notice = Some(mukti_formats::PLAIN_TEXT_ONLY_NOTICE.to_owned());
            c
        }
        "pdf" => {
            let (text, summary) = mukti_formats::convert_pdf_to_text(&bytes)
                .map_err(|e| format!("That PDF could not be read: {e}. Some PDFs are images of pages rather than text."))?;
            *state.last.lock().expect("state lock") = Some(Payload::Text(text.clone()));
            let mut c = convert_str(&text, None);
            c.kind = Kindness::Pdf;
            if text.trim().is_empty() {
                c.unreadable = Some(summary.fonts_changed);
            }
            c
        }
        _ => {
            let (text, encoding) = decode(&bytes);
            let label = match encoding {
                TextEncoding::Utf8 => "UTF-8",
                TextEncoding::Windows1252 => "Windows-1252",
            };
            let c = convert_str(&text, Some(label.to_owned()));
            *state.last.lock().expect("state lock") = Some(Payload::Text(c.text.clone()));
            c
        }
    };

    conversion.filename = filename;
    *state.stem.lock().expect("state lock") = stem;
    Ok(conversion)
}

/// Ask the person for a file, then convert it.
///
/// Rust puts the picker up and uses what came back, so no path is ever supplied
/// by the window. `Ok(None)` means they closed the dialog without choosing.
#[tauri::command]
fn open_and_convert(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<Option<Conversion>, String> {
    let picked = app
        .dialog()
        .file()
        .add_filter(
            "Everything Mukti can read",
            &[
                "txt", "csv", "tsv", "md", "json", "docx", "xlsx", "pptx", "doc", "xls", "ppt",
                "pdf",
            ],
        )
        .add_filter("Word, Excel, PowerPoint", &["docx", "xlsx", "pptx"])
        .add_filter("Older Word, Excel, PowerPoint", &["doc", "xls", "ppt"])
        .add_filter("PDF", &["pdf"])
        .add_filter("Plain text", &["txt", "csv", "tsv", "md", "json"])
        .blocking_pick_file();

    let Some(picked) = picked else {
        return Ok(None);
    };
    let path = picked
        .into_path()
        .map_err(|e| format!("That file could not be located on disk: {e}"))?;
    convert_path(&path, &state).map(Some)
}

/// Save the most recent conversion, asking where.
///
/// Takes no path and no text: both come from Rust, which is what makes this safe
/// by construction rather than by validation. `Ok(None)` means the dialog was
/// closed without choosing.
#[tauri::command]
fn save_result(
    app: tauri::AppHandle,
    state: tauri::State<'_, State>,
) -> Result<Option<String>, String> {
    let stem = state
        .stem
        .lock()
        .expect("state lock")
        .clone()
        .unwrap_or_else(|| "converted".to_owned());

    // Decide the suggested name and filter from what is actually being saved,
    // then release the lock before the dialog blocks on a person.
    let (suggested, filter_name, extensions): (String, &str, Vec<String>) = {
        let held = state.last.lock().expect("state lock");
        match held.as_ref() {
            None => return Err("There is nothing converted to save yet.".to_owned()),
            Some(Payload::Text(t)) if t.is_empty() => {
                return Err("There is nothing converted to save yet.".to_owned())
            }
            Some(Payload::Text(_)) => (
                format!("{stem}.unicode.txt"),
                "Plain text",
                vec!["txt".to_owned()],
            ),
            Some(Payload::Document { extension, .. }) => (
                format!("{stem}.unicode.{extension}"),
                "Document",
                vec![extension.clone()],
            ),
        }
    };

    let extensions: Vec<&str> = extensions.iter().map(String::as_str).collect();
    let picked = app
        .dialog()
        .file()
        .set_file_name(&suggested)
        .add_filter(filter_name, &extensions)
        .blocking_save_file();

    let Some(picked) = picked else {
        return Ok(None);
    };
    let path: PathBuf = picked
        .into_path()
        .map_err(|e| format!("That location could not be used: {e}"))?;

    let held = state.last.lock().expect("state lock");
    match held.as_ref() {
        None => Err("There is nothing converted to save yet.".to_owned()),
        // Always UTF-8: Bengali does not fit in the encoding the file arrived in.
        Some(Payload::Text(t)) => std::fs::write(&path, t.as_bytes())
            .map(|()| Some(display_name(&path)))
            .map_err(|e| format!("It could not be saved there: {e}. Check you are allowed to write to that folder.")),
        Some(Payload::Document { bytes, .. }) => std::fs::write(&path, bytes)
            .map(|()| Some(display_name(&path)))
            .map_err(|e| format!("It could not be saved there: {e}. Check you are allowed to write to that folder.")),
    }
}

/// Just the file's own name, for a message. The folders someone keeps their
/// documents in are not something to echo back at them.
fn display_name(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().to_string())
        .unwrap_or_else(|| "the chosen file".to_owned())
}

/// Say plainly that there is no desktop to draw on, rather than panicking.
///
/// Found on 14 August 2026 by installing the published `.deb` in a container and
/// running it — the first time any Linux artefact of this project had been run at
/// all. With no display, GTK cannot start and the window layer panics:
///
/// ```text
/// thread 'main' panicked at /home/runner/.cargo/registry/.../tao-0.35.3/...
/// Failed to initialize gtk backend!: BoolError { message: "Failed to initialize GTK" ... }
/// ```
///
/// Two things wrong with that. It is a panic where a sentence would do — the
/// same rule as every other error path here — and it prints the file paths of the
/// machine that built the release, which are nobody's business and mean nothing
/// to the person reading them.
///
/// Only Linux needs this. macOS and Windows always have a window server.
#[cfg(target_os = "linux")]
fn refuse_without_a_desktop() {
    let has_display = std::env::var_os("DISPLAY").is_some_and(|v| !v.is_empty())
        || std::env::var_os("WAYLAND_DISPLAY").is_some_and(|v| !v.is_empty());
    if has_display {
        return;
    }
    eprintln!(
        "GRU953 Mukti is a desktop app and there is no desktop here — neither \
         DISPLAY nor WAYLAND_DISPLAY is set.\n\
         \n\
         If you are connected over SSH, either run it on the machine itself or \
         forward its display with `ssh -X`.\n\
         \n\
         To convert files without a desktop, use the command-line tool instead:\n\
         \n\
         \x20   mukti convert yourfile.docx\n"
    );
    std::process::exit(1);
}

fn main() {
    #[cfg(target_os = "linux")]
    refuse_without_a_desktop();

    tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .manage(State::default())
        .invoke_handler(tauri::generate_handler![
            convert_text,
            open_and_convert,
            save_result
        ])
        // Dropped files are handled here rather than in the window, for the same
        // reason the dialogs are: the path stays on this side of the boundary.
        .on_window_event(|window, event| {
            if let tauri::WindowEvent::DragDrop(drag) = event {
                match drag {
                    tauri::DragDropEvent::Enter { .. } | tauri::DragDropEvent::Over { .. } => {
                        let _ = window.emit("drop-state", "over");
                    }
                    tauri::DragDropEvent::Drop { paths, .. } => {
                        let _ = window.emit("drop-state", "idle");
                        if let Some(first) = paths.first() {
                            let state = window.state::<State>();
                            match convert_path(first, &state) {
                                Ok(conversion) => {
                                    let _ = window.emit("file-converted", conversion);
                                }
                                Err(why) => {
                                    let _ = window.emit("file-failed", why);
                                }
                            }
                        }
                    }
                    _ => {
                        let _ = window.emit("drop-state", "idle");
                    }
                }
            }
        })
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

    #[test]
    fn a_word_document_is_read_as_a_document_and_kept_for_saving() {
        // A minimal .docx with one SutonnyMJ run, built here so the test needs
        // no fixture on disk.
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default();
        for (name, body) in [
            ("[Content_Types].xml", "<Types/>"),
            ("_rels/.rels", "<Relationships/>"),
            (
                "word/document.xml",
                concat!(
                    "<w:document><w:body><w:p><w:r>",
                    "<w:rPr><w:rFonts w:ascii=\"SutonnyMJ\"/></w:rPr>",
                    "<w:t>Kg\u{a9}m~wP</w:t></w:r></w:p></w:body></w:document>"
                ),
            ),
        ] {
            zip.start_file(name, opts).unwrap();
            std::io::Write::write_all(&mut zip, body.as_bytes()).unwrap();
        }
        let docx = zip.finish().unwrap().into_inner();

        let dir = std::env::temp_dir().join("mukti-app-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("one.docx");
        std::fs::write(&path, &docx).unwrap();

        let state = State::default();
        let c = convert_path(&path, &state).expect("the document should convert");

        assert_eq!(c.kind, Kindness::Document, "a .docx is not plain text");
        assert_eq!(c.converted, 1, "the one legacy word should have converted");
        assert!(
            c.text.contains("কর্মসূচি"),
            "the converted Bangla should be shown: {:?}",
            c.text
        );
        assert_eq!(c.filename.as_deref(), Some("one.docx"));

        // What gets saved must be the rebuilt DOCUMENT, not the text, or the
        // formatting, tables and images are all thrown away on save.
        match &*state.last.lock().unwrap() {
            Some(Payload::Document { bytes, extension }) => {
                assert_eq!(extension, "docx");
                assert!(
                    zip::ZipArchive::new(std::io::Cursor::new(bytes)).is_ok(),
                    "the saved payload should be a readable archive"
                );
            }
            _ => panic!("a document conversion must keep the document for saving"),
        }

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn a_plain_text_file_keeps_its_text_for_saving() {
        let dir = std::env::temp_dir().join("mukti-app-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("note.txt");
        std::fs::write(&path, "Kg\u{a9}m~wP and English").unwrap();

        let state = State::default();
        let c = convert_path(&path, &state).expect("the file should convert");
        assert_eq!(c.kind, Kindness::Text);
        assert!(c.text.contains("কর্মসূচি"));
        assert!(matches!(
            &*state.last.lock().unwrap(),
            Some(Payload::Text(_))
        ));

        std::fs::remove_file(&path).ok();
    }

    #[test]
    fn an_empty_file_is_refused_with_a_readable_reason() {
        let dir = std::env::temp_dir().join("mukti-app-test");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("empty.txt");
        std::fs::write(&path, b"").unwrap();

        let state = State::default();
        let why = convert_path(&path, &state).expect_err("an empty file has nothing to convert");
        assert!(
            why.contains("empty"),
            "the reason should say so plainly: {why}"
        );
        // No Rust error type should ever reach the person reading it.
        assert!(!why.contains("Os {"), "a raw system error leaked: {why}");

        std::fs::remove_file(&path).ok();
    }
}
