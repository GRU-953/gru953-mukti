//! GRU953 Mukti on the command line.
//!
//! Converts legacy Bijoy/SutonnyMJ Bangla into Unicode, **word by word**, so
//! English, numbers and Bengali that is already Unicode come through exactly
//! as they went in.
//!
//! # Two rules this tool will not break
//!
//! **It never writes over a file unless you ask it to.** The default is a new
//! file beside the original, and if that new name is already taken the run
//! stops rather than destroying whatever was there. `--in-place` overwrites the
//! original and has to be typed; `--out` names a file, and naming it is asking;
//! `--force` allows the derived name to be replaced.
//!
//! That wording used to say only "your file", which was true of the original and
//! false of everything else: the derived `.unicode.txt` sibling and any `--out`
//! target were truncated without a word.
//!
//! **It never claims to have done more than it did.** Every run says how many
//! words changed, and `check` shows you that without writing anything at all.
//!
//! No arguments parser dependency: the options are few and hand-rolled parsing
//! keeps this crate's dependency list at one entry, the converter itself.

use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use gru953_mukti::classify::{convert_pieces, count};
use gru953_mukti::encoding::{decode, TextEncoding};
use mukti_formats::{
    convert_legacy_office, convert_office, convert_pdf_to_text, LegacyFormat,
    PLAIN_TEXT_ONLY_NOTICE,
};

const USAGE: &str = "\
GRU953 Mukti — convert legacy Bangla text to Unicode.

  mukti convert <file>...    convert files, writing a new file beside each one
                             (.txt .csv .md, .docx .xlsx .pptx,
                              .doc .xls .ppt, and .pdf)
  mukti check <file>...      say what would change, and write nothing
  mukti convert -            read from the keyboard or a pipe, write to screen

Options
  --font <name>     the Bengali font to set in Office files (default: Nirmala UI)
  --in-place        overwrite the original file instead of writing a new one
  --force           allow the new file to replace one that is already there
  --out <file>      write the result to this file (one input file only)
  --quiet           print nothing but errors
  --version         print the version
  --help            print this

Examples
  mukti convert report.txt              writes report.unicode.txt
  mukti convert notes.doc               writes notes.unicode.docx (text only)
  mukti check *.txt                     shows what would change, changes nothing
  cat old.txt | mukti convert -         converts a pipe
";

fn main() -> ExitCode {
    match run() {
        Ok(code) => code,
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<ExitCode, String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if args.is_empty() || args.iter().any(|a| a == "--help" || a == "-h") {
        print!("{USAGE}");
        return Ok(ExitCode::SUCCESS);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        println!("GRU953 Mukti {}", env!("CARGO_PKG_VERSION"));
        return Ok(ExitCode::SUCCESS);
    }

    let mut mode = None;
    let mut files: Vec<PathBuf> = Vec::new();
    let mut in_place = false;
    let mut quiet = false;
    let mut force = false;
    let mut out: Option<PathBuf> = None;
    // Nirmala UI ships with Windows and covers Bengali; it is the safest
    // default for a document that will most likely be opened in Word. Anyone
    // who prefers SolaimanLipi or Kalpurush can say so.
    let mut font = String::from("Nirmala UI");

    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "convert" if mode.is_none() => mode = Some(Mode::Convert),
            "check" if mode.is_none() => mode = Some(Mode::Check),
            "--font" => {
                font = it
                    .next()
                    .ok_or_else(|| "--font needs a font name after it.".to_owned())?
            }
            "--in-place" => in_place = true,
            "--force" => force = true,
            "--quiet" | "-q" => quiet = true,
            "--out" => {
                out = Some(it.next().map(PathBuf::from).ok_or_else(|| {
                    "--out needs a file name after it, for example: --out result.txt".to_owned()
                })?)
            }
            other if other.starts_with("--") => {
                return Err(format!("I do not know the option {other}.\n\n{USAGE}"))
            }
            other => files.push(PathBuf::from(other)),
        }
    }

    let Some(mode) = mode else {
        return Err(format!(
            "Start with either `convert` or `check`.\n\n{USAGE}"
        ));
    };
    if files.is_empty() {
        return Err(format!(
            "No files given. Name a file, or use `-` to read from a pipe.\n\n{USAGE}"
        ));
    }
    if out.is_some() && files.len() > 1 {
        return Err(
            "--out writes a single file, but several were given.\nConvert them one at a time, or drop --out to write a new file beside each."
                .to_owned(),
        );
    }
    if in_place && files.iter().any(|f| f.as_os_str() == "-") {
        return Err("--in-place cannot be used with `-`: a pipe is not a file.".to_owned());
    }

    let mut any_failed = false;
    let mut total = Tally::default();

    for path in &files {
        match handle(path, mode, in_place, out.as_deref(), quiet, force, &font) {
            Ok(tally) => total.add(tally),
            Err(message) => {
                eprintln!("{message}");
                any_failed = true;
            }
        }
    }

    if !quiet && files.len() > 1 {
        println!("\nAcross {} files: {}", files.len(), total.describe(mode));
    }
    Ok(if any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Convert,
    Check,
}

#[derive(Default)]
struct Tally {
    converted: usize,
    untouched: usize,
}

impl Tally {
    fn add(&mut self, other: Tally) {
        self.converted += other.converted;
        self.untouched += other.untouched;
    }

    fn describe(&self, mode: Mode) -> String {
        let verb = match mode {
            Mode::Convert => "converted",
            Mode::Check => "would be converted",
        };
        format!(
            "{} of {} words {verb}; {} left exactly as they were.",
            self.converted,
            self.converted + self.untouched,
            self.untouched
        )
    }
}

/// Word, Excel and PowerPoint files, which are converted in place inside the
/// document rather than turned into plain text.
fn is_office(path: &Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref(),
        Some("docx" | "xlsx" | "pptx")
    )
}

fn handle(
    path: &Path,
    mode: Mode,
    in_place: bool,
    out: Option<&Path>,
    quiet: bool,
    force: bool,
    font: &str,
) -> Result<Tally, String> {
    let from_pipe = path.as_os_str() == "-";
    if is_office(path) && !from_pipe {
        return handle_office(path, mode, in_place, out, quiet, force, font);
    }
    if !from_pipe {
        if let Some(legacy) = path
            .extension()
            .and_then(|e| e.to_str())
            .and_then(LegacyFormat::from_extension)
        {
            return handle_legacy(path, legacy, mode, in_place, out, quiet, force);
        }
    }
    // JSON is refused outright, and the reason is worth stating in full.
    //
    // It was listed as supported from 0.3.0 and never once tested. When it finally was,
    // on 19 August 2026, **5 of 13 real JSON files came out invalid** -- the conversion
    // tables map Bijoy's curly double quotes (0xD2 and 0xD3) to a plain ASCII `"`,
    // which ends a JSON string value early and makes the file unloadable.
    //
    // Converting it properly means parsing the JSON, converting only the string
    // contents and re-serialising with correct escaping. That needs a JSON parser as a
    // dependency, and the owner chose on 19 August 2026 to drop the format instead
    // rather than add one for a format nobody had asked for.
    //
    // Refusing is the honest implementation of that choice. There is no list of
    // supported text extensions to remove JSON from -- anything unrecognised falls
    // through to the plain-text path below -- so removing it from the documentation
    // alone would have left the corruption in place while claiming otherwise.
    //
    // A pipe cannot be checked this way, because a pipe has no name. `cat x.json |
    // mukti convert -` is still treated as plain text, and the message below says so.
    if !from_pipe
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref()
            == Some("json")
    {
        return Err(format!(
            "{} is a JSON file, and Mukti no longer converts JSON.\n\
             Converting it could produce a file that no longer loads: a Bijoy quotation \
             mark becomes a plain \" character, which ends a JSON value early. That was \
             happening to real files, so the format was withdrawn rather than left to \
             corrupt them.\n\
             If the Bangla inside it needs converting, copy the text into a .txt file \
             and convert that, then put it back.",
            path.display()
        ));
    }

    if !from_pipe
        && path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_lowercase)
            .as_deref()
            == Some("pdf")
    {
        return handle_pdf(path, mode, out, quiet, force);
    }

    let bytes = if from_pipe {
        let mut buf = Vec::new();
        io::stdin()
            .read_to_end(&mut buf)
            .map_err(|e| format!("Could not read from the pipe: {e}"))?;
        buf
    } else {
        fs::read(path).map_err(|e| {
            format!(
                "Could not open {}: {e}\nCheck the name is right and that the file is not open in another programme.",
                path.display()
            )
        })?
    };

    let (text, encoding) = decode(&bytes);
    let (converted, tally) = convert_and_count(&text);

    match mode {
        Mode::Check => {
            if !quiet {
                let name = if from_pipe {
                    "(from the pipe)".to_owned()
                } else {
                    path.display().to_string()
                };
                println!("{name}: {}", tally.describe(mode));
                if encoding == TextEncoding::Windows1252 {
                    println!("  Read as Windows-1252, which is normal for a legacy Bangla file.");
                }
            }
        }
        Mode::Convert if from_pipe => {
            io::stdout()
                .write_all(converted.as_bytes())
                .map_err(|e| format!("Could not write the result: {e}"))?;
        }
        Mode::Convert => {
            let destination = match out {
                Some(o) => o.to_path_buf(),
                None if in_place => path.to_path_buf(),
                None => {
                    let ours = beside(path);
                    refuse_to_clobber(&ours, force)?;
                    ours
                }
            };
            // Converted text is Bengali, so it is always written as UTF-8 —
            // the encoding it arrived in cannot hold it.
            fs::write(&destination, converted.as_bytes()).map_err(|e| {
                format!(
                    "Could not write {}: {e}\nCheck you have permission to write to that folder.",
                    destination.display()
                )
            })?;
            if !quiet {
                println!("{} -> {}", path.display(), destination.display());
                println!("  {}", tally.describe(mode));
                if encoding == TextEncoding::Windows1252 {
                    println!("  Read as Windows-1252 and written as UTF-8.");
                }
            }
        }
    }
    Ok(tally)
}

/// Read a PDF and write the converted text beside it.
///
/// A PDF is the one format that cannot be converted in place: its glyphs are
/// individually positioned and it carries no Unicode Bengali font to draw the
/// result with, so rewriting one would be a typesetting job. The user is told
/// this rather than left to discover it — the layout does not survive, and
/// somebody expecting their tables back would otherwise think it had failed.
fn handle_pdf(
    path: &Path,
    mode: Mode,
    out: Option<&Path>,
    quiet: bool,
    force: bool,
) -> Result<Tally, String> {
    let bytes = fs::read(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    // The reader's own wording is deliberately not shown. `legacy_office.rs` states
    // this policy for the old Office formats -- a message about cross-reference
    // tables and invalid file headers tells a person nothing they can act on -- and
    // the PDF path was the last one still echoing a library string at the user.
    let (text, summary) = convert_pdf_to_text(&bytes).map_err(|_| {
        format!(
            "Could not read {} as a PDF. The file may be damaged or incomplete, or it \
             may not be a PDF at all despite its name.\nIf it is a scanned image rather \
             than text, there are no letters in it to convert.",
            path.display()
        )
    })?;
    let tally = Tally {
        converted: summary.words_converted,
        untouched: summary.words_untouched,
    };
    if mode == Mode::Check {
        if !quiet {
            println!("{}: {}", path.display(), tally.describe(mode));
            // `check` exists to say what converting would do, and for a PDF the two
            // things a person most needs to know are exactly these: the layout will
            // not survive, and some text may be unreadable. Until 19 August 2026
            // `convert` reported both and `check` reported neither. Measured over 253
            // real PDFs: every one of the 253 check lines was silent about it, while
            // converting the same files reported 1,196,732 unreadable pieces, with 22
            // of them yielding no text at all.
            println!("  Would be written as plain text: a PDF's layout cannot be carried over.");
            if summary.fonts_changed > 0 {
                println!(
                    "  {} pieces of text could NOT be read and would be left out: they are",
                    summary.fonts_changed
                );
                println!("  drawn with fonts that store glyph shapes rather than letters.");
                if tally.converted == 0 && tally.untouched == 0 {
                    println!("  Nothing at all would be recovered from this file.");
                }
            }
        }
        return Ok(tally);
    }
    let destination = match out {
        Some(o) => o.to_path_buf(),
        None => {
            let ours = path.with_extension("unicode.txt");
            refuse_to_clobber(&ours, force)?;
            ours
        }
    };
    fs::write(&destination, text.as_bytes())
        .map_err(|e| format!("Could not write {}: {e}", destination.display()))?;
    if !quiet {
        println!("{} -> {}", path.display(), destination.display());
        println!("  {}", tally.describe(mode));
        println!("  Written as plain text: a PDF's layout cannot be carried over.");
        if summary.fonts_changed > 0 {
            println!(
                "  {} pieces of text could NOT be read and were left out: they are drawn",
                summary.fonts_changed
            );
            println!("  with fonts that store glyph shapes rather than letters. Guessing at");
            println!("  those would produce convincing nonsense, so they are skipped instead.");
        }
    }
    Ok(tally)
}

/// Convert a Word, Excel or PowerPoint file, keeping its formatting.
fn handle_office(
    path: &Path,
    mode: Mode,
    in_place: bool,
    out: Option<&Path>,
    quiet: bool,
    force: bool,
    font: &str,
) -> Result<Tally, String> {
    let bytes = fs::read(path).map_err(|e| {
        format!(
            "Could not open {}: {e}\nCheck the name is right and that the file is not open in another programme.",
            path.display()
        )
    })?;
    // An empty file is not a broken document, and saying "invalid Zip archive:
    // could not find EOCD" to somebody who has just dragged a file in is not
    // an explanation of anything. Found by running the tool over a real
    // archive of 1,399 files, exactly one of which was zero bytes.
    if bytes.is_empty() {
        return Err(format!(
            "{} is empty — there is nothing in it to convert.",
            path.display()
        ));
    }
    let (converted, summary) = convert_office(&bytes, font).map_err(|e| {
        format!(
            "Could not read {} as a Word, Excel or PowerPoint file.\nIf it is an older .doc, .xls or .ppt, open it and save it as .docx, .xlsx or .pptx first.\n(The technical reason: {e})",
            path.display()
        )
    })?;
    let tally = Tally {
        converted: summary.words_converted,
        untouched: summary.words_untouched,
    };

    if mode == Mode::Check {
        if !quiet {
            println!("{}: {}", path.display(), tally.describe(mode));
            println!(
                "  {} font settings would change to {font}.",
                summary.fonts_changed
            );
        }
        return Ok(tally);
    }

    let destination = match out {
        Some(o) => o.to_path_buf(),
        None if in_place => path.to_path_buf(),
        None => {
            let ours = beside(path);
            refuse_to_clobber(&ours, force)?;
            ours
        }
    };
    fs::write(&destination, &converted).map_err(|e| {
        format!(
            "Could not write {}: {e}\nCheck you have permission to write to that folder.",
            destination.display()
        )
    })?;
    if !quiet {
        println!("{} -> {}", path.display(), destination.display());
        println!("  {}", tally.describe(mode));
        println!(
            "  {} font settings changed to {font}; formatting and images untouched.",
            summary.fonts_changed
        );
    }
    Ok(tally)
}

/// The old binary formats: `.doc`, `.xls`, `.ppt`.
///
/// These cannot be rewritten in place the way a `.docx` can — there is no XML to
/// edit, and only the text can be recovered — so a **new** modern document is
/// written beside the original and the original is never touched.
fn handle_legacy(
    path: &Path,
    format: LegacyFormat,
    mode: Mode,
    in_place: bool,
    out: Option<&Path>,
    quiet: bool,
    force: bool,
) -> Result<Tally, String> {
    // `--in-place` cannot mean anything here: the file that comes out is a
    // different format from the one that went in. Saying so is better than
    // quietly writing a .docx into a name ending .doc.
    if in_place {
        return Err(format!(
            "{} is an older {} file, so the converted copy has to be a .{} — a different format.\n\
             --in-place cannot be used for these. Leave it off to write a new file beside the original, or use --out to choose a name.",
            path.display(),
            path.extension().unwrap_or_default().to_string_lossy(),
            format.modern_extension()
        ));
    }

    let bytes = fs::read(path).map_err(|e| {
        format!(
            "Could not open {}: {e}\nCheck the name is right and that the file is not open in another programme.",
            path.display()
        )
    })?;

    let outcome = convert_legacy_office(&bytes, format).map_err(|e| {
        format!(
            "Could not read {} as an older Word, Excel or PowerPoint file.\n\
             It may be damaged, or it may be a newer file that has simply been given an old name — try renaming it to .{}x.\n\
             (The technical reason: {e})",
            path.display(),
            path.extension().unwrap_or_default().to_string_lossy()
        )
    })?;

    let tally = Tally {
        converted: outcome.summary.words_converted,
        untouched: outcome.summary.words_untouched,
    };

    if mode == Mode::Check {
        if !quiet {
            println!("{}: {}", path.display(), tally.describe(mode));
            println!("  {}", PLAIN_TEXT_ONLY_NOTICE);
        }
        return Ok(tally);
    }

    let destination = match out {
        Some(o) => o.to_path_buf(),
        None => {
            let ours = beside_as(path, format.modern_extension());
            refuse_to_clobber(&ours, force)?;
            ours
        }
    };
    fs::write(&destination, &outcome.document).map_err(|e| {
        format!(
            "Could not write {}: {e}\nCheck you have permission to write to that folder.",
            destination.display()
        )
    })?;

    if !quiet {
        println!("{} -> {}", path.display(), destination.display());
        println!("  {}", tally.describe(mode));
        if outcome.was_empty {
            println!("  No text could be recovered from this file, so the new document is empty.");
        }
        println!("  {}", PLAIN_TEXT_ONLY_NOTICE);
    }
    Ok(tally)
}

/// Refuse to destroy a file the user never named.
///
/// The derived name is ours, not theirs. If something is already sitting there
/// — an earlier conversion, or a file that happens to share the name — writing
/// over it is not something this tool was asked to do.
fn refuse_to_clobber(destination: &Path, force: bool) -> Result<(), String> {
    if force || !destination.exists() {
        return Ok(());
    }
    Err(format!(
        "{} already exists, and I chose that name rather than you.\n\
         Move or delete it, use --out to pick a different name, or add --force to replace it.",
        destination.display()
    ))
}

/// `notes.doc` becomes `notes.unicode.docx` — a new name **and** a new format.
fn beside_as(path: &Path, extension: &str) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    path.with_file_name(format!("{stem}.unicode.{extension}"))
}

/// `report.txt` becomes `report.unicode.txt`.
///
/// A new name rather than the original, because overwriting somebody's only
/// copy of a document on the strength of a guess is not a thing this tool does
/// without being told to.
fn beside(path: &Path) -> PathBuf {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let name = match path.extension() {
        Some(ext) => format!("{stem}.unicode.{}", ext.to_string_lossy()),
        None => format!("{stem}.unicode"),
    };
    path.with_file_name(name)
}

/// Convert, and count what changed, in one pass over the text.
fn convert_and_count(input: &str) -> (String, Tally) {
    let pieces = convert_pieces(input);
    let (converted, untouched) = count(&pieces);
    (
        pieces.into_iter().map(|p| p.text).collect(),
        Tally {
            converted,
            untouched,
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_name_is_chosen_beside_the_original() {
        assert_eq!(
            beside(Path::new("report.txt")),
            Path::new("report.unicode.txt")
        );
        assert_eq!(
            beside(Path::new("/a/b/notes.md")),
            Path::new("/a/b/notes.unicode.md")
        );
        assert_eq!(beside(Path::new("README")), Path::new("README.unicode"));
    }

    #[test]
    fn text_with_nothing_legacy_in_it_comes_back_byte_for_byte() {
        for input in [
            "Programme operations and budget review for the 2026 cycle.",
            "সম্পূর্ণ ইউনিকোড বাংলা লেখা",
            "Region\tTotal\tBalance\nDhaka\t1200\t340\n",
            "",
        ] {
            let (out, tally) = convert_and_count(input);
            assert_eq!(out, input, "text was altered: {input:?}");
            assert_eq!(tally.converted, 0);
        }
    }

    #[test]
    fn only_the_legacy_words_change_and_the_count_says_so() {
        let input = "Report: Kg\u{a9}m~wP for 2026 এবং done\n";
        let (out, tally) = convert_and_count(input);
        assert!(
            out.contains("কর্মসূচি"),
            "the legacy word was missed: {out:?}"
        );
        assert!(out.starts_with("Report:"), "English was altered: {out:?}");
        assert!(out.ends_with("done\n"), "the ending changed: {out:?}");
        assert_eq!(tally.converted, 1);
        assert_eq!(tally.untouched, 5);
    }

    #[test]
    fn the_tally_reads_as_plain_english() {
        let tally = Tally {
            converted: 3,
            untouched: 7,
        };
        assert_eq!(
            tally.describe(Mode::Convert),
            "3 of 10 words converted; 7 left exactly as they were."
        );
        assert_eq!(
            tally.describe(Mode::Check),
            "3 of 10 words would be converted; 7 left exactly as they were."
        );
    }
}

#[cfg(test)]
mod clobber_tests {
    use super::*;

    #[test]
    fn a_name_we_chose_is_not_written_over_without_being_asked() {
        let dir = std::env::temp_dir().join("mukti-clobber-test");
        let _ = fs::create_dir_all(&dir);
        let taken = dir.join("already-here.txt");
        fs::write(&taken, b"precious").unwrap();

        // The derived name is ours, not theirs, so an existing file stops the run.
        let refused = refuse_to_clobber(&taken, false).unwrap_err();
        assert!(refused.contains("already exists"), "unhelpful: {refused}");
        assert!(
            refused.contains("--force"),
            "the way out is not offered: {refused}"
        );
        assert_eq!(
            fs::read(&taken).unwrap(),
            b"precious",
            "it was written over anyway"
        );

        // Asking is allowed.
        assert!(
            refuse_to_clobber(&taken, true).is_ok(),
            "--force was refused"
        );

        // A name nothing occupies is fine either way.
        let free = dir.join("not-here.txt");
        let _ = fs::remove_file(&free);
        assert!(refuse_to_clobber(&free, false).is_ok());

        let _ = fs::remove_file(&taken);
    }

    /// A `.json` file is refused, and nothing is written.
    ///
    /// JSON was advertised as supported from 0.3.0 and never tested. When it finally
    /// was, on 19 August 2026, 5 of 13 real files came out invalid: the tables map
    /// Bijoy's curly double quotes to a plain `"`, which ends a JSON string value
    /// early. Converting it properly needs a JSON parser as a dependency, and the
    /// format was dropped instead.
    ///
    /// The test asserts BOTH halves of that: the refusal happens, and no output file
    /// appears. Refusing while still writing something would be worse than either.
    #[test]
    fn a_json_file_is_refused_and_nothing_is_written() {
        let dir = std::env::temp_dir().join("mukti-json-refusal-test");
        let _ = fs::create_dir_all(&dir);
        let input = dir.join("m.json");
        // Bijoy with the curly quote that breaks the JSON, plus enough context that
        // the detector would otherwise act on it.
        fs::write(
            &input,
            b"{\n  \"a\": \"Kg\xa9m~wP cP\xd6wZ\xa1e`b\",\n  \"b\": \"\xd3Kg\xa9m~wP\xd2\"\n}\n",
        )
        .expect("write the fixture");
        let produced = dir.join("m.unicode.json");
        let _ = fs::remove_file(&produced);

        let result = handle(
            &input,
            Mode::Convert,
            false,
            None,
            true,
            false,
            "Nirmala UI",
        );

        let message = match result {
            Err(m) => m,
            Ok(_) => panic!("a .json file must be refused, not converted"),
        };
        assert!(
            message.contains("no longer converts JSON"),
            "the refusal should say plainly that JSON is not converted: {message}"
        );
        assert!(
            !produced.exists(),
            "a file was written despite the refusal, which is worse than either \
             converting or refusing cleanly"
        );

        let _ = fs::remove_file(&input);
    }
}
