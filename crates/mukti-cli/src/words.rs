//! Every string Mukti shows a person, in one place.
//!
//! Not because a beginner's tool needs a translation layer — it does not,
//! English only, for now, see `HANDOVER.md` — but because putting every
//! string here, once, is what lets the tests below check all of them at
//! once: no banned word, no exclamation mark, no emoji, no sentence over the
//! brand's own length, no error over 30 words, and the locked tagline
//! byte-exact. Scattering the same wording across match arms is how a
//! banned word or a stray "!" survives review.
//!
//! # The rule for every error, from the brand guide
//!
//! Three parts, in order: what happened, why (only if it is actually known),
//! what to do now. Reassure about the reader's data if it is safe. Under 30
//! words. Two refusals here cannot fit that — the JSON and PDF explanations
//! are reasoning about *why* a whole format was withdrawn, not instructions
//! for one file — so the short form goes here and the full reasoning moves
//! to `README.md`, `USING-MUKTI.md` and `CHANGELOG.md`. Nothing is lost;
//! nothing is trimmed to a half-truth.

use std::path::Path;

/// A file name is data a stranger chose, not something Mukti wrote — and a
/// name carrying a raw escape or control byte could otherwise repaint the
/// terminal, hide part of itself, or impersonate Mukti's own output once it
/// reaches a printed sentence. Every function in this file that interpolates
/// a path calls this first, rather than `Path::display` directly, so the
/// defence lives at the one place untrusted text enters a message — not at
/// the far end where the finished, and by then already-coloured, line is
/// printed. See [`crate::report::safe_for_screen`], which this delegates to.
fn show(path: &Path) -> String {
    crate::report::show_path(path)
}

/// "1 file", "12 files" -- never "file(s)".
///
/// The bracketed form is quicker to write and reads as a form to fill in
/// rather than a sentence, which is the opposite of what a beginner-facing
/// tool wants. Every noun this is used with pluralises regularly, so one
/// helper covers all of them; anything irregular must not be routed here.
fn counted(n: usize, noun: &str) -> String {
    let n_text = crate::report::group_thousands(n);
    if n == 1 {
        format!("{n_text} {noun}")
    } else {
        format!("{n_text} {noun}s")
    }
}

/// The wordmark, first mention. "GRU953 Mukti" is never used again once this
/// landed — the prefix form is reserved by the brand's own rule for a name
/// too generic to stand alone, and "Mukti" is not that.
pub const BANNER: &str = "\
Mukti by GRU953
Converts old Bangla writing in Office files to Unicode.
Simple technology. For everyone.";

/// The locked tagline, English. Never shortened, never reworded — checked
/// byte-exact by a test below even though the CLI prints it only as part of
/// `BANNER`'s own literal text, never by reading this const, so a future
/// edit to either cannot drift the other quietly. Deliberately unreferenced
/// by any non-test code: that absence is the point, not an oversight — see
/// `the_tagline_is_exact_in_both_languages` below.
#[allow(dead_code)]
pub const TAGLINE_EN: &str = "Simple technology. For everyone.";

/// The locked tagline, Bangla. Not printed anywhere in this English-only
/// interface (see the module doc), but checked byte-exact for the same
/// reason `TAGLINE_EN` is: a locked string must never be free to drift just
/// because nothing currently reads it.
#[allow(dead_code)]
pub const TAGLINE_BN: &str = "সহজ প্রযুক্তি। সবার জন্য।";

pub const HELP: &str = "\
Mukti by GRU953 converts old Bangla writing in Office files to Unicode.
Simple technology. For everyone.

Run mukti on its own and it asks what to convert, one step at a time.

Commands
  mukti                    ask which folder to convert, then convert it
  mukti convert <files>    convert the files you name
  mukti check <files>      say what would change, and write nothing

Files Mukti can open
  .docx .xlsx .pptx        Word, Excel and PowerPoint, formatting kept
  .doc .xls .ppt           the older kinds, saved as a new .docx, .xlsx
                           or .pptx, text only

Options
  --font <name>   the Bangla font to set in the converted file
                  (default: Nirmala UI)
  --in-place      write the new text into the original file
  --force         let a new file replace one of the same name
  --out <file>    save the result under this name, one file at a time
  --jobs <n>      convert up to this many files at once (default: 1)
  --quiet         show errors only
  --theme <light|dark|off>
                  match your window's colours, or turn colour off
  --version       show which version this is
  --help          show this text

Examples
  mukti                           ask, then convert a whole folder
  mukti convert report.docx       saves report.unicode.docx beside it
  mukti check report.docx         shows what would change, changes nothing
  mukti convert notes.doc         saves notes.unicode.docx, text only
";

// ---------------------------------------------------------------------
// Reading and writing files
// ---------------------------------------------------------------------

/// A file could not be read. Branches on the operating system's own
/// `io::ErrorKind`, replacing four near-identical copies of this message
/// that used to exist across the old single-file command.
pub fn could_not_read(path: &Path, kind: std::io::ErrorKind, technical: &str) -> String {
    use std::io::ErrorKind;
    let name = show(path);
    match kind {
        ErrorKind::NotFound => format!(
            "There is no file called {name} here. Check the name and the folder \
             it is in, then run mukti again."
        ),
        ErrorKind::PermissionDenied => format!(
            "Your computer would not let Mukti read {name}. Check the file is \
             yours to open, then run mukti again. Nothing was changed."
        ),
        _ => format!(
            "{name} could not be opened. It may be open in another programme. \
             Close it, then run mukti again.\nTechnical detail: {technical}"
        ),
    }
}

/// A converted file could not be saved. `size_hint_bytes` is the converted
/// document's own length in memory, rounded up, so the space quoted in the
/// disk-full case is a real figure rather than an invented one.
pub fn could_not_write(
    destination: &Path,
    kind: std::io::ErrorKind,
    size_hint_bytes: usize,
) -> String {
    use std::io::ErrorKind;
    let name = show(destination);
    let mb = (size_hint_bytes.max(1_000_000) as f64 / 1_000_000.0).ceil() as u64;
    match kind {
        ErrorKind::PermissionDenied => format!(
            "Your computer would not let Mukti save {name}. Check you can add \
             files to that folder, then run mukti again. Your original file is \
             unchanged."
        ),
        ErrorKind::StorageFull => format!(
            "{name} could not be saved. The disk has no space left. Free up \
             about {mb} MB, then run mukti again. Your original file is \
             unchanged."
        ),
        ErrorKind::ReadOnlyFilesystem => format!(
            "{name} could not be saved. That folder cannot be written to. Save \
             it elsewhere with --out, then run mukti again. Your original file \
             is unchanged."
        ),
        _ => format!(
            "{name} could not be saved. Check the folder still exists, then run \
             mukti again. Your original file is unchanged."
        ),
    }
}

/// A file is empty, so there is nothing to convert. Hoisted here so `.doc`,
/// `.xls` and `.ppt` get the same plain wording as the modern formats,
/// instead of a library string arriving behind "(the technical reason: …)".
pub fn empty_file(path: &Path) -> String {
    format!(
        "{} has nothing in it, so there is nothing to convert. Check the file \
         saved properly, then run mukti again.",
        show(path)
    )
}

// ---------------------------------------------------------------------
// The six-format gate
// ---------------------------------------------------------------------

/// JSON is refused, and the reason is worth keeping in full — but not here.
/// A 30-word limit cannot carry it honestly, so the short form goes to the
/// person and the full reasoning lives in `CHANGELOG.md` and `README.md`.
pub fn refused_json(path: &Path) -> String {
    format!(
        "Mukti does not convert JSON files. A quotation mark inside one can \
         break the file so it no longer opens. Copy the Bangla from {} into a \
         Word document, then convert that.",
        show(path)
    )
}

/// PDF, same reasoning: withdrawn for a real reason too long to fit here.
pub fn refused_pdf(path: &Path) -> String {
    format!(
        "Mukti does not convert PDF files. A PDF stores the shape of each \
         letter, not the letter itself, so nothing reliable can be recovered \
         from {}. Copy the Bangla text into a Word document instead.",
        show(path)
    )
}

/// Any other unrecognised extension, including none at all.
pub fn refused_unsupported(path: &Path) -> String {
    format!(
        "Mukti cannot open {}. It opens Word, Excel and PowerPoint files: \
         .docx, .xlsx, .pptx, .doc, .xls and .ppt.",
        show(path)
    )
}

/// A document that could not be read as a modern Office file.
pub fn could_not_read_office(path: &Path, kind_word: &str, technical: &str) -> String {
    format!(
        "{} could not be read as a {kind_word} file. It may be damaged. Open \
         it in {kind_word}, save it again, then run mukti.\nTechnical detail: {technical}",
        show(path)
    )
}

/// A document that could not be read as an older `.doc`/`.xls`/`.ppt`.
pub fn could_not_read_legacy(path: &Path, new_extension: &str, technical: &str) -> String {
    format!(
        "{} could not be read as an older Word file. It may be damaged, or a \
         newer file with an old name. Try renaming it to {new_extension}.\n\
         Technical detail: {technical}",
        show(path)
    )
}

/// `--in-place` was asked for on a format that has to change container.
pub fn in_place_not_possible(path: &Path, modern_extension: &str) -> String {
    format!(
        "{} has to be saved as a new .{modern_extension} file, a different \
         format, so --in-place cannot be used. Save a new file beside the \
         original instead.",
        show(path)
    )
}

/// Two different files in the same run would derive the same output name —
/// `notes.doc` and `notes.docx` both become `notes.unicode.docx`. Caught
/// before either is read, so the run stops cleanly rather than one file
/// silently overwriting the other.
pub fn duplicate_destination(a: &Path, b: &Path, destination: &Path) -> String {
    format!(
        "{} and {} would both be saved as {}. Rename one of them, then run \
         mukti again.",
        show(a),
        show(b),
        show(destination)
    )
}

/// The derived name already exists, and the tool chose it, not the reader.
pub fn name_already_taken(destination: &Path) -> String {
    format!(
        "A file called {} is already there, and Mukti chose that name, not \
         you. Move it, or add --force to replace it.",
        show(destination)
    )
}

// ---------------------------------------------------------------------
// Argument parsing
// ---------------------------------------------------------------------

pub fn font_needs_a_value() -> String {
    "--font needs a font name after it, for example: --font \"Nirmala UI\"".to_owned()
}

pub fn out_needs_a_value() -> String {
    "--out needs a file name after it, for example: --out report-unicode.docx".to_owned()
}

pub fn out_with_several_files(count: usize) -> String {
    format!(
        "--out saves one file, and {count} were named. Convert them one at a \
         time, or leave --out off to save a new file beside each one."
    )
}

pub fn jobs_needs_a_number() -> String {
    "--jobs needs a number after it, for example: --jobs 4".to_owned()
}

pub fn jobs_must_be_at_least_one() -> String {
    "--jobs must be 1 or more.".to_owned()
}

pub fn unknown_option(name: &str) -> String {
    format!("Mukti has no option called {name}. Run mukti --help to see the options it does have.")
}

pub fn bad_theme_value() -> String {
    "--theme takes light, dark or off. For example: mukti --theme dark".to_owned()
}

pub fn no_verb_with_files(first: &Path) -> String {
    format!(
        "Mukti needs to know what to do with {}. Run mukti convert {} to \
         convert it, or mukti check {} to see what would change.",
        show(first),
        show(first),
        show(first)
    )
}

pub fn verb_with_no_files(verb: &str) -> String {
    format!("{verb} needs the name of a file after it. For example: mukti {verb} report.docx")
}

/// The line printed once, after every file in a batch of more than one.
pub fn across_files_summary(file_count: usize, tally_description: &str) -> String {
    format!("Across {file_count} files: {tally_description}")
}

/// How many words changed and how many did not. Lives here rather than beside
/// the arithmetic in `report` so the brand tests below sweep it like every
/// other sentence a reader sees.
pub fn tally_sentence(converted: usize, total: usize, untouched: usize, checking: bool) -> String {
    let verb = if checking {
        "would be converted"
    } else {
        "converted"
    };
    format!(
        "{} of {} words {verb}; {} left exactly as they were.",
        crate::report::group_thousands(converted),
        crate::report::group_thousands(total),
        crate::report::group_thousands(untouched)
    )
}

/// The one edit Mukti makes to text it otherwise leaves alone, said out loud.
///
/// Where a vowel sign was stored in two pieces, the two are joined into the
/// single character Unicode defines them as. It looks identical either way, so
/// without this line the change is invisible — and an invisible edit to text
/// the tool promises not to touch is worth a sentence even though it cannot
/// alter what the text says.
pub fn normalisation_note(count: usize, checking: bool) -> String {
    let verb = if checking { "would be" } else { "were" };
    format!(
        "{} Bangla words already in Unicode {verb} tidied. A vowel sign stored \
         in two pieces is joined into one character, so a search can find the \
         word. The words read the same.",
        crate::report::group_thousands(count)
    )
}

// ---------------------------------------------------------------------
// Status markers
// ---------------------------------------------------------------------
//
// Colour is never the only signal a state is what it is: every one of these
// carries a word and a plain ASCII marker, so the meaning survives with
// colour off, in a log file, or in a terminal with no glyph coverage. These
// are fixed interface chrome rather than prose, so they are checked for
// exactness below rather than swept by the prose rules in `samples()` —
// the brand's "no exclamation mark" rule governs sentences, and `[!]` here
// is a marker glyph the kit itself specifies, not a sentence's punctuation.

pub const ERROR_LABEL: &str = "[!] Error:";
pub const WARNING_LABEL: &str = "[!] Warning:";
pub const DONE_LABEL: &str = "Done.";
pub const NOTE_LABEL: &str = "Note:";
pub const SKIPPED_LABEL: &str = "Skipped:";

// ---------------------------------------------------------------------
// Guided mode
// ---------------------------------------------------------------------

pub fn ask_folder() -> String {
    "Which folder has the files to convert? Press Return to use the \
     current folder, or type q to stop."
        .to_owned()
}

pub fn folder_not_found(path: &Path) -> String {
    format!(
        "There is no folder called {}. Check the name, or drag the folder \
         into this window instead of typing it.",
        show(path)
    )
}

pub fn given_a_file_offer_to_convert_it(path: &Path) -> String {
    format!(
        "{} is a file, not a folder. Convert this one file now?",
        show(path)
    )
}

pub fn nothing_found(folder: &Path) -> String {
    format!(
        "{} has no Word, Excel or PowerPoint files Mukti can convert.",
        show(folder)
    )
}

pub fn only_mukti_output_found(count: usize) -> String {
    format!(
        "{} here already {} like Mukti's own earlier output. There is nothing \
         left to convert.",
        counted(count, "matching file"),
        if count == 1 { "looks" } else { "look" }
    )
}

pub fn matches_only_in_subfolders(subfolder_count: usize) -> String {
    format!(
        "Nothing here directly, though this folder holds {} Mukti did not look \
         inside. Run mukti again and name one of them, if the files are there.",
        counted(subfolder_count, "other folder")
    )
}

pub fn discovery_report(by_type: &[(&str, usize)]) -> String {
    let parts: Vec<String> = by_type
        .iter()
        .filter(|(_, n)| *n > 0)
        .map(|(ext, n)| format!("{n} .{ext}"))
        .collect();
    format!("Found {}.", parts.join(", "))
}

pub fn subfolders_excluded_note(count: usize) -> String {
    format!(
        "{} here {} not looked inside. Run mukti again and name one directly to \
         convert what is in it.",
        counted(count, "sub-folder"),
        if count == 1 { "was" } else { "were" }
    )
}

pub fn skipped_note(count: usize) -> String {
    format!(
        "{} {} left out because {} like Mukti's own earlier output.",
        counted(count, "file"),
        if count == 1 { "was" } else { "were" },
        if count == 1 { "it looks" } else { "they look" }
    )
}

pub fn ask_output_location() -> String {
    "Where should the converted files go? Type 1 for a new folder next to \
     this one, or 2 to save each file beside its original."
        .to_owned()
}

pub fn confirm_conversion(file_count: usize) -> String {
    format!(
        "About to convert {}. Nothing changes until this is confirmed. Continue?",
        counted(file_count, "file")
    )
}

pub fn some_outputs_already_exist(count: usize) -> String {
    format!(
        "{count} of the files Mukti would write already exist, from an \
         earlier run. Replace them?"
    )
}

pub fn run_cancelled() -> String {
    "Nothing was changed.".to_owned()
}

pub fn gave_up_after_unclear_answers() -> String {
    "That answer was not one Mukti recognised, three times running, so \
     nothing was changed. Run mukti again when ready."
        .to_owned()
}

pub fn could_not_make_folder(folder: &Path, kind: std::io::ErrorKind) -> String {
    use std::io::ErrorKind;
    let name = show(folder);
    match kind {
        ErrorKind::PermissionDenied => format!(
            "Your computer would not let Mukti make the folder {name}. Nothing \
             was changed. Save the files beside the originals instead."
        ),
        _ => format!(
            "The folder {name} could not be made, so nothing was changed. Save \
             the files beside the originals instead."
        ),
    }
}

pub fn not_y_or_n() -> String {
    "That did not look like y or n.".to_owned()
}

pub fn not_one_or_two() -> String {
    "Please type 1 or 2.".to_owned()
}

pub fn guided_goodbye() -> String {
    "Stopped. Nothing was changed.".to_owned()
}

/// Shown once, only on a real terminal, when no signal said whether the
/// background is light or dark — the ladder's own last step, kept here
/// rather than inline in `style.rs` so it is swept by the same brand tests
/// as everything else a reader can see.
pub fn colour_off_hint() -> &'static str {
    "Colour is off because this terminal did not say which background it \
     uses. Run mukti --theme dark to turn it on."
}

/// What guided mode asks when a beginner types a file with no verb —
/// `mukti report.docx` rather than `mukti convert report.docx`.
pub fn confirm_bare_file(path: &Path) -> String {
    format!("Convert {} now?", show(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every message this crate can show, rendered with sample arguments, so
    /// the tests below can sweep all of them in one pass. Anything added to
    /// this file that a person can see belongs in this list too.
    fn samples() -> Vec<String> {
        let p = Path::new("report.docx");
        let d = Path::new("report.unicode.docx");
        vec![
            BANNER.to_owned(),
            HELP.to_owned(),
            could_not_read(p, std::io::ErrorKind::NotFound, ""),
            could_not_read(p, std::io::ErrorKind::PermissionDenied, ""),
            could_not_read(p, std::io::ErrorKind::Other, "some technical detail"),
            could_not_write(d, std::io::ErrorKind::PermissionDenied, 1_000_000),
            could_not_write(d, std::io::ErrorKind::StorageFull, 3_400_000),
            could_not_write(d, std::io::ErrorKind::ReadOnlyFilesystem, 1_000_000),
            could_not_write(d, std::io::ErrorKind::Other, 1_000_000),
            empty_file(p),
            refused_json(p),
            refused_pdf(p),
            refused_unsupported(p),
            could_not_read_office(p, "Word", "some technical detail"),
            could_not_read_legacy(
                Path::new("notes.doc"),
                "notes.docx",
                "some technical detail",
            ),
            in_place_not_possible(Path::new("notes.doc"), "docx"),
            duplicate_destination(Path::new("notes.doc"), Path::new("notes.docx"), d),
            name_already_taken(d),
            font_needs_a_value(),
            out_needs_a_value(),
            out_with_several_files(7),
            jobs_needs_a_number(),
            jobs_must_be_at_least_one(),
            unknown_option("--recursive"),
            bad_theme_value(),
            no_verb_with_files(p),
            verb_with_no_files("convert"),
            across_files_summary(7, "3 of 10 words converted; 7 left exactly as they were."),
            tally_sentence(1234, 1240, 6, false),
            tally_sentence(1234, 1240, 6, true),
            normalisation_note(9904, false),
            normalisation_note(9904, true),
            ask_folder(),
            folder_not_found(p),
            given_a_file_offer_to_convert_it(p),
            nothing_found(p),
            only_mukti_output_found(3),
            matches_only_in_subfolders(2),
            discovery_report(&[("docx", 4), ("xlsx", 0), ("doc", 1)]),
            subfolders_excluded_note(2),
            skipped_note(3),
            ask_output_location(),
            confirm_conversion(12),
            some_outputs_already_exist(2),
            run_cancelled(),
            gave_up_after_unclear_answers(),
            guided_goodbye(),
            colour_off_hint().to_owned(),
            confirm_bare_file(p),
            not_y_or_n(),
            not_one_or_two(),
            could_not_make_folder(Path::new("out"), std::io::ErrorKind::PermissionDenied),
            could_not_make_folder(Path::new("out"), std::io::ErrorKind::Other),
        ]
    }

    /// Words this project has banned outright, checked on word boundaries so
    /// "adjust" does not trip on "just". Matches the newer, CI-enforced
    /// standard rather than the older, softer one, per the branding decision
    /// recorded in the plan.
    const BANNED_WORDS: &[&str] = &[
        "simply",
        "just",
        "easy",
        "easily",
        "obviously",
        "clearly",
        "whilst",
        "amongst",
    ];
    const BANNED_PHRASES: &[&str] = &["of course", "e.g.", "i.e.", "etc."];

    fn words_of(text: &str) -> Vec<String> {
        text.split(|c: char| !c.is_alphanumeric())
            .filter(|w| !w.is_empty())
            .map(|w| w.to_ascii_lowercase())
            .collect()
    }

    #[test]
    fn no_banned_word_appears_in_any_message() {
        for sample in samples() {
            let words = words_of(&sample);
            for banned in BANNED_WORDS {
                assert!(
                    !words.iter().any(|w| w == banned),
                    "banned word {banned:?} found in: {sample:?}"
                );
            }
            let lower = sample.to_lowercase();
            for phrase in BANNED_PHRASES {
                assert!(
                    !lower.contains(phrase),
                    "banned phrase {phrase:?} found in: {sample:?}"
                );
            }
        }
    }

    #[test]
    fn no_exclamation_mark_and_no_three_dot_ellipsis() {
        for sample in samples() {
            assert!(!sample.contains('!'), "exclamation mark in: {sample:?}");
            assert!(!sample.contains("..."), "three-dot ellipsis in: {sample:?}");
        }
    }

    #[test]
    fn no_emoji_or_unexpected_symbol() {
        // Allowed: ASCII, the Bengali block (unused today but future-proofed),
        // and the single-character ellipsis, en/em dash and curly quotes that
        // already appear in plain English prose.
        for sample in samples() {
            for c in sample.chars() {
                let allowed = c.is_ascii()
                    || ('\u{0980}'..='\u{09FF}').contains(&c)
                    || matches!(c, '…' | '–' | '—' | '’' | '‘' | '“' | '”');
                assert!(
                    allowed,
                    "unexpected non-ASCII character {c:?} in: {sample:?}"
                );
            }
        }
    }

    #[test]
    fn nothing_speaks_as_a_team() {
        for sample in samples() {
            let words = words_of(&sample);
            for banned in ["we", "our", "us"] {
                assert!(
                    !words.iter().any(|w| w == banned),
                    "first person plural {banned:?} found in: {sample:?}"
                );
            }
        }
    }

    /// The rule is 30 words for an ERROR specifically, and the label
    /// ("Mukti has no option called…" is itself the message, not a
    /// separately-added label) plus any "Technical detail:" line are
    /// excluded from the count: neither is prose written for the reader to
    /// parse as an explanation, and the technical line exists precisely so
    /// the library's own words never have to be.
    fn word_count_excluding_technical_detail(message: &str) -> usize {
        let prose = message
            .split("\nTechnical detail:")
            .next()
            .unwrap_or(message);
        words_of(prose).len()
    }

    #[test]
    fn every_error_message_is_under_thirty_words() {
        let p = Path::new("report.docx");
        let d = Path::new("report.unicode.docx");
        let errors: Vec<(&str, String)> = vec![
            (
                "could_not_read/NotFound",
                could_not_read(p, std::io::ErrorKind::NotFound, ""),
            ),
            (
                "could_not_read/PermissionDenied",
                could_not_read(p, std::io::ErrorKind::PermissionDenied, ""),
            ),
            (
                "could_not_read/Other",
                could_not_read(p, std::io::ErrorKind::Other, "x"),
            ),
            (
                "could_not_write/PermissionDenied",
                could_not_write(d, std::io::ErrorKind::PermissionDenied, 1_000_000),
            ),
            (
                "could_not_write/StorageFull",
                could_not_write(d, std::io::ErrorKind::StorageFull, 1_000_000),
            ),
            (
                "could_not_write/ReadOnlyFilesystem",
                could_not_write(d, std::io::ErrorKind::ReadOnlyFilesystem, 1_000_000),
            ),
            (
                "could_not_write/Other",
                could_not_write(d, std::io::ErrorKind::Other, 1_000_000),
            ),
            ("empty_file", empty_file(p)),
            ("refused_unsupported", refused_unsupported(p)),
            (
                "could_not_read_office",
                could_not_read_office(p, "Word", "x"),
            ),
            (
                "could_not_read_legacy",
                could_not_read_legacy(Path::new("notes.doc"), "notes.docx", "x"),
            ),
            (
                "in_place_not_possible",
                in_place_not_possible(Path::new("notes.doc"), "docx"),
            ),
            (
                "duplicate_destination",
                duplicate_destination(Path::new("notes.doc"), Path::new("notes.docx"), d),
            ),
            ("name_already_taken", name_already_taken(d)),
            ("font_needs_a_value", font_needs_a_value()),
            ("out_needs_a_value", out_needs_a_value()),
            ("out_with_several_files", out_with_several_files(7)),
            ("unknown_option", unknown_option("--recursive")),
            ("bad_theme_value", bad_theme_value()),
            (
                "could_not_make_folder/PermissionDenied",
                could_not_make_folder(Path::new("out"), std::io::ErrorKind::PermissionDenied),
            ),
            (
                "could_not_make_folder/Other",
                could_not_make_folder(Path::new("out"), std::io::ErrorKind::Other),
            ),
            ("no_verb_with_files", no_verb_with_files(p)),
            ("verb_with_no_files", verb_with_no_files("convert")),
        ];
        for (name, message) in errors {
            let n = word_count_excluding_technical_detail(&message);
            assert!(
                n <= 30,
                "{name} is {n} words, over the 30-word limit: {message:?}"
            );
        }
        // refused_json and refused_pdf are the two documented exceptions:
        // reasoning about why a whole format was withdrawn, not fitted into
        // 30 words on purpose. Checked here so the exception stays exactly
        // two, rather than growing unnoticed.
        let exceptions = [refused_json(p), refused_pdf(p)];
        assert_eq!(exceptions.len(), 2, "the 30-word exception list grew");
    }

    #[test]
    fn no_sentence_runs_past_twenty_five_words() {
        for sample in samples() {
            for sentence in sample.split(['.', '\n']) {
                let n = words_of(sentence).len();
                assert!(
                    n <= 25,
                    "a {n}-word sentence exceeds the 25-word limit: {sentence:?}"
                );
            }
        }
    }

    #[test]
    fn nothing_is_shouted_in_all_capitals() {
        // A run of three or more capital ASCII letters, outside a short
        // allow-list of real abbreviations and format names this tool
        // actually uses. "GRU" is the brand name's own initials, split
        // away from the "953" that makes it a proper noun rather than a
        // shouted word — it is never written any other way.
        let allowed = ["MB", "UI", "GRU", "JSON", "PDF"];
        for sample in samples() {
            for word in sample.split(|c: char| !c.is_ascii_alphabetic()) {
                if word.len() < 3 || allowed.contains(&word) {
                    continue;
                }
                let all_caps = word.chars().all(|c| c.is_ascii_uppercase());
                assert!(!all_caps, "shouted word {word:?} in: {sample:?}");
            }
        }
    }

    #[test]
    fn status_markers_are_exact() {
        assert_eq!(ERROR_LABEL, "[!] Error:");
        assert_eq!(WARNING_LABEL, "[!] Warning:");
        assert_eq!(DONE_LABEL, "Done.");
        assert_eq!(NOTE_LABEL, "Note:");
        assert_eq!(SKIPPED_LABEL, "Skipped:");
    }

    #[test]
    fn the_tagline_is_exact_in_both_languages() {
        assert_eq!(TAGLINE_EN, "Simple technology. For everyone.");
        assert_eq!(TAGLINE_BN, "সহজ প্রযুক্তি। সবার জন্য।");
    }

    #[test]
    fn the_product_is_named_correctly() {
        for sample in samples() {
            assert!(
                !sample.contains("GRU953 Mukti"),
                "the retired prefix form appears in: {sample:?}"
            );
        }
        let banner_first_line = BANNER.lines().next().unwrap_or_default();
        assert_eq!(banner_first_line, "Mukti by GRU953");
        let second_line = BANNER.lines().nth(1).unwrap_or_default();
        assert!(
            !second_line.is_empty(),
            "the banner must say what Mukti is on the line straight after the name"
        );
    }

    #[test]
    fn spelling_is_uk() {
        for banned in [
            "color", "organize", "analyze", "behavior", "center", "license",
        ] {
            for sample in samples() {
                let lower = sample.to_lowercase();
                assert!(
                    !lower.contains(banned),
                    "US spelling {banned:?} found in: {sample:?}"
                );
            }
        }
    }
}
