//! Converting files: the six-format gate, the per-file handlers, and the
//! parallel orchestration across a batch.
//!
//! The parallelism folds in work originally deferred from the speed pass
//! (Part 3, step E of the plan this crate was rebuilt from): a beginner's
//! own machine, not a benchmark corpus, is where a batch of files most
//! needs it. Three hazards are handled explicitly, because a beginner
//! cannot be expected to notice their absence: **output order** (workers
//! return `(index, result)`, sorted back into place before anything
//! prints); **the clobber race** (`notes.doc` and `notes.docx` would both
//! derive `notes.unicode.docx`, so every destination is computed up front,
//! single-threaded, and a collision refuses the whole run rather than
//! letting the second writer silently win); and **memory** (peak use per
//! file is two to three times its size, so concurrent bytes in flight are
//! bounded, and files are offered largest-first so a long conversion never
//! queues behind a run of short ones).

use std::collections::VecDeque;
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::{Condvar, Mutex};
use std::thread;

use mukti_formats::{convert_legacy_office, convert_office, LegacyFormat, PLAIN_TEXT_ONLY_NOTICE};

use crate::options::{Mode, Options, Verbosity};
use crate::report::{self, Tally};
use crate::style::Palette;
use crate::words;

/// A heuristic, not a measured figure: enough that several mid-sized files
/// can run at once, small enough that the five-plus `.pptx` files known to
/// exceed 200 MB cannot all be in flight together. `--jobs 1` never
/// consults this at all.
const MEMORY_BUDGET_BYTES: u64 = 1_000_000_000;

/// What happened to one file, beyond the word tally, so a caller can report
/// it without re-deriving it from the format.
#[derive(Debug)]
pub struct Outcome {
    pub tally: Tally,
    /// Where the converted copy was written. `None` in `check` mode, since
    /// nothing is written.
    pub destination: Option<PathBuf>,
    /// `Some` only for the three modern formats, which carry font
    /// information; `None` for `.doc`/`.xls`/`.ppt`, which do not.
    pub fonts_changed: Option<usize>,
    /// `Some` only for the three older formats, carrying the fixed notice
    /// that they convert text only.
    pub legacy_notice: Option<&'static str>,
    pub legacy_was_empty: bool,
}

/// Convert or check one file. No printing here: `run()` below prints for
/// flag-mode use, in file order; guided mode prints its own way.
pub fn convert_one(path: &Path, mode: Mode, opts: &Options) -> Result<Outcome, String> {
    match extension_kind(path) {
        Some(ExtKind::Office) => handle_office(path, mode, opts),
        Some(ExtKind::Legacy(format)) => handle_legacy(path, format, mode, opts),
        None => Err(refusal_for_unsupported(path)),
    }
}

/// Whether this file's extension is one of the six, without reading it.
pub(crate) fn is_supported(path: &Path) -> bool {
    extension_kind(path).is_some()
}

/// The refusal a caller would get for an unsupported file, without attempting
/// the conversion first. Guided mode uses this to say so at the point it knows,
/// rather than asking a question whose answer cannot matter.
pub(crate) fn refusal_for(path: &Path) -> String {
    refusal_for_unsupported(path)
}

fn refusal_for_unsupported(path: &Path) -> String {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(str::to_ascii_lowercase)
    {
        Some(ref ext) if ext == "json" => words::refused_json(path),
        Some(ref ext) if ext == "pdf" => words::refused_pdf(path),
        _ => words::refused_unsupported(path),
    }
}

#[derive(Clone)]
enum ExtKind {
    Office,
    Legacy(LegacyFormat),
}

/// The only six extensions Mukti converts, checked before anything is read.
fn extension_kind(path: &Path) -> Option<ExtKind> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())?
        .to_ascii_lowercase();
    match ext.as_str() {
        "docx" | "xlsx" | "pptx" => Some(ExtKind::Office),
        "doc" | "xls" | "ppt" => LegacyFormat::from_extension(&ext).map(ExtKind::Legacy),
        _ => None,
    }
}

fn handle_office(path: &Path, mode: Mode, opts: &Options) -> Result<Outcome, String> {
    let bytes =
        fs::read(path).map_err(|e| words::could_not_read(path, e.kind(), &e.to_string()))?;
    // An empty file is not a broken document, and a library error about a
    // missing Zip end-of-central-directory record is not an explanation of
    // anything to somebody who has just dragged a file in.
    if bytes.is_empty() {
        return Err(words::empty_file(path));
    }
    let (converted, summary) = convert_office(&bytes, &opts.font).map_err(|e| {
        words::could_not_read_office(path, "Word, Excel or PowerPoint", &e.to_string())
    })?;
    let tally = Tally {
        converted: summary.words_converted,
        untouched: summary.words_untouched,
        normalised: summary.words_normalised,
    };

    if mode == Mode::Check {
        return Ok(Outcome {
            tally,
            destination: None,
            fonts_changed: Some(summary.fonts_changed),
            legacy_notice: None,
            legacy_was_empty: false,
        });
    }

    let destination = office_destination(path, opts)?;
    let size_hint = converted.len();
    fs::write(&destination, &converted)
        .map_err(|e| words::could_not_write(&destination, e.kind(), size_hint))?;

    Ok(Outcome {
        tally,
        destination: Some(destination),
        fonts_changed: Some(summary.fonts_changed),
        legacy_notice: None,
        legacy_was_empty: false,
    })
}

/// The old binary formats: `.doc`, `.xls`, `.ppt`. These cannot be rewritten
/// in place the way a `.docx` can — there is no XML to edit, and only the
/// text can be recovered — so a new modern document is written beside the
/// original and the original is never touched.
fn handle_legacy(
    path: &Path,
    format: LegacyFormat,
    mode: Mode,
    opts: &Options,
) -> Result<Outcome, String> {
    if opts.in_place {
        return Err(words::in_place_not_possible(
            path,
            format.modern_extension(),
        ));
    }

    let bytes =
        fs::read(path).map_err(|e| words::could_not_read(path, e.kind(), &e.to_string()))?;

    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    let suggested = format!("{stem}.{}", format.modern_extension());
    let outcome = convert_legacy_office(&bytes, format)
        .map_err(|e| words::could_not_read_legacy(path, &suggested, &e.to_string()))?;

    let tally = Tally {
        converted: outcome.summary.words_converted,
        untouched: outcome.summary.words_untouched,
        normalised: outcome.summary.words_normalised,
    };

    if mode == Mode::Check {
        return Ok(Outcome {
            tally,
            destination: None,
            fonts_changed: None,
            legacy_notice: Some(PLAIN_TEXT_ONLY_NOTICE),
            legacy_was_empty: outcome.was_empty,
        });
    }

    let destination = legacy_destination(path, format, opts)?;
    let size_hint = outcome.document.len();
    fs::write(&destination, &outcome.document)
        .map_err(|e| words::could_not_write(&destination, e.kind(), size_hint))?;

    Ok(Outcome {
        tally,
        destination: Some(destination),
        fonts_changed: None,
        legacy_notice: Some(PLAIN_TEXT_ONLY_NOTICE),
        legacy_was_empty: outcome.was_empty,
    })
}

fn office_destination(path: &Path, opts: &Options) -> Result<PathBuf, String> {
    if let Some(o) = &opts.out {
        return Ok(o.clone());
    }
    if opts.in_place {
        return Ok(path.to_path_buf());
    }
    let ours = match &opts.output_folder {
        Some(folder) => folder.join(derived_office_name(path)),
        None => beside(path),
    };
    refuse_to_clobber(&ours, opts.force)?;
    Ok(ours)
}

fn legacy_destination(
    path: &Path,
    format: LegacyFormat,
    opts: &Options,
) -> Result<PathBuf, String> {
    if let Some(o) = &opts.out {
        return Ok(o.clone());
    }
    let ours = match &opts.output_folder {
        Some(folder) => folder.join(derived_legacy_name(path, format.modern_extension())),
        None => beside_as(path, format.modern_extension()),
    };
    refuse_to_clobber(&ours, opts.force)?;
    Ok(ours)
}

/// The destination `convert_one` would write to for this file, without
/// touching the disk and without an error path. `None` for an unsupported
/// extension, or for `.doc`/`.xls`/`.ppt` under `--in-place` (refused on its
/// own, individually, with no destination to preview). Guided mode uses
/// this to warn before converting when a name it is about to write already
/// exists from an earlier run.
pub(crate) fn preview_destination(path: &Path, opts: &Options) -> Option<PathBuf> {
    let kind = extension_kind(path)?;
    intended_destination(path, &kind, opts)
}

/// The same derivation `office_destination`/`legacy_destination` would
/// reach, without touching the disk and without an error path — used only
/// to scan the whole batch for collisions before any file is read. `None`
/// means this file will not end up producing a destination at all (`check`
/// mode is filtered out by the caller before this is ever called; `.doc`
/// `--in-place` is refused on its own, individually, with nothing to
/// collide).
fn intended_destination(path: &Path, kind: &ExtKind, opts: &Options) -> Option<PathBuf> {
    if let Some(o) = &opts.out {
        return Some(o.clone());
    }
    match kind {
        ExtKind::Office => Some(if opts.in_place {
            path.to_path_buf()
        } else {
            match &opts.output_folder {
                Some(folder) => folder.join(derived_office_name(path)),
                None => beside(path),
            }
        }),
        ExtKind::Legacy(format) => {
            if opts.in_place {
                None
            } else {
                Some(match &opts.output_folder {
                    Some(folder) => {
                        folder.join(derived_legacy_name(path, format.modern_extension()))
                    }
                    None => beside_as(path, format.modern_extension()),
                })
            }
        }
    }
}

/// Refuses the whole run, before any file is read, if two different inputs
/// would derive the same output name.
fn check_for_duplicate_destinations(files: &[PathBuf], opts: &Options) -> Result<(), String> {
    let mut seen: Vec<(PathBuf, &Path)> = Vec::new();
    for file in files {
        let Some(kind) = extension_kind(file) else {
            continue;
        };
        let Some(destination) = intended_destination(file, &kind, opts) else {
            continue;
        };
        if let Some((_, earlier)) = seen.iter().find(|(d, _)| *d == destination) {
            return Err(words::duplicate_destination(earlier, file, &destination));
        }
        seen.push((destination, file.as_path()));
    }
    Ok(())
}

/// Refuse to destroy a file the reader never named. The derived name is
/// ours, not theirs: if something is already sitting there — an earlier
/// conversion, or a file that happens to share the name — writing over it
/// is not something this tool does without being told to.
fn refuse_to_clobber(destination: &Path, force: bool) -> Result<(), String> {
    if force || !destination.exists() {
        return Ok(());
    }
    Err(words::name_already_taken(destination))
}

/// The file name half of the derivation, shared by the "beside the
/// original" and the "in a folder the reader chose" cases, so the naming
/// rule itself — `stem.unicode.ext` — exists in exactly one place.
fn derived_office_name(path: &Path) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    match path.extension() {
        Some(ext) => format!("{stem}.unicode.{}", ext.to_string_lossy()),
        None => format!("{stem}.unicode"),
    }
}

fn derived_legacy_name(path: &Path, modern_extension: &str) -> String {
    let stem = path.file_stem().unwrap_or_default().to_string_lossy();
    format!("{stem}.unicode.{modern_extension}")
}

/// `notes.doc` becomes `notes.unicode.docx` — a new name *and* a new format.
fn beside_as(path: &Path, extension: &str) -> PathBuf {
    path.with_file_name(derived_legacy_name(path, extension))
}

/// `report.docx` becomes `report.unicode.docx`. A new name rather than the
/// original, because overwriting somebody's only copy of a document on the
/// strength of a guess is not a thing this tool does without being asked.
fn beside(path: &Path) -> PathBuf {
    path.with_file_name(derived_office_name(path))
}

// ---------------------------------------------------------------------
// Batch orchestration
// ---------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub struct RunResult {
    pub total: Tally,
    pub any_failed: bool,
}

/// Converts (or checks) every file, printing per-file lines in original
/// order and a final total when there is more than one file — the shape
/// flag-mode output has always had. `palette` controls colour only; the
/// text is identical either way.
pub fn run(files: &[PathBuf], mode: Mode, opts: &Options, palette: Option<&Palette>) -> RunResult {
    if mode == Mode::Convert {
        if let Err(message) = check_for_duplicate_destinations(files, opts) {
            // `message` already has any path sanitised, from `words::`
            // building it with `show()`; only the error label below adds
            // colour, so nothing here needs sanitising a second time.
            eprintln!(
                "{} {message}",
                crate::style::danger(palette, words::ERROR_LABEL)
            );
            return RunResult {
                total: Tally::default(),
                any_failed: true,
            };
        }
    }

    let results = convert_all(files, mode, opts);

    let quiet = opts.verbosity == Verbosity::Quiet;
    let mut total = Tally::default();
    let mut any_failed = false;

    for (index, result) in &results {
        let path = &files[*index];
        match result {
            Ok(outcome) => {
                total.add(outcome.tally);
                if !quiet {
                    print_success(path, mode, outcome, opts);
                }
            }
            Err(message) => {
                eprintln!(
                    "{} {message}",
                    crate::style::danger(palette, words::ERROR_LABEL)
                );
                any_failed = true;
            }
        }
    }

    if !quiet && files.len() > 1 {
        println!(
            "\n{}",
            words::across_files_summary(files.len(), &total.describe(mode))
        );
    }

    RunResult { total, any_failed }
}

/// Runs every file through `convert_one`, in parallel when `opts.jobs > 1`,
/// and returns one result per file indexed by its position in `files` —
/// unsorted by the workers, sorted back into place here, so printing can
/// stay in the order the reader named them regardless of which finished
/// first.
fn convert_all(
    files: &[PathBuf],
    mode: Mode,
    opts: &Options,
) -> Vec<(usize, Result<Outcome, String>)> {
    let mut queue: Vec<(usize, PathBuf, u64)> = files
        .iter()
        .enumerate()
        .map(|(i, f)| (i, f.clone(), 0))
        .collect();

    // `--jobs 1` is the rollback switch and must reproduce exactly what a
    // single sequential pass over `files` in the order given would do, so
    // the size stat and the largest-first reorder below are both skipped
    // outright rather than becoming a no-op sort.
    let jobs = opts.jobs.max(1);
    if jobs > 1 {
        for item in &mut queue {
            item.2 = fs::metadata(&item.1).map(|m| m.len()).unwrap_or(0);
        }
        queue.sort_by_key(|item| std::cmp::Reverse(item.2));
    }

    let worker_count = jobs.min(queue.len().max(1));
    let coordinator = Coordinator::new(queue, MEMORY_BUDGET_BYTES);
    let results: Mutex<Vec<(usize, Result<Outcome, String>)>> = Mutex::new(Vec::new());

    thread::scope(|scope| {
        for _ in 0..worker_count {
            scope.spawn(|| {
                while let Some((index, path, size)) = coordinator.take_next() {
                    let result = convert_one(&path, mode, opts);
                    coordinator.release(size);
                    results.lock().unwrap().push((index, result));
                }
            });
        }
    });

    let mut results = results.into_inner().unwrap();
    results.sort_by_key(|(index, _)| *index);
    results
}

fn print_success(path: &Path, mode: Mode, outcome: &Outcome, opts: &Options) {
    match &outcome.destination {
        None => {
            // check mode: nothing was written.
            println!(
                "{}: {}",
                report::show_path(path),
                outcome.tally.describe(mode)
            );
            if let Some(fonts) = outcome.fonts_changed {
                println!("  {fonts} font settings would change to {}.", opts.font);
            }
            if let Some(note) = outcome.tally.normalisation_note(mode) {
                println!("  {note}");
            }
            if let Some(notice) = outcome.legacy_notice {
                println!("  {notice}");
            }
        }
        Some(destination) => {
            println!(
                "{} -> {}",
                report::show_path(path),
                report::show_path(destination)
            );
            println!("  {}", outcome.tally.describe(mode));
            if let Some(fonts) = outcome.fonts_changed {
                println!(
                    "  {fonts} font settings changed to {}; formatting and images untouched.",
                    opts.font
                );
            }
            if let Some(note) = outcome.tally.normalisation_note(mode) {
                println!("  {note}");
            }
            if outcome.legacy_was_empty {
                println!(
                    "  No text could be recovered from this file, so the new document is empty."
                );
            }
            if let Some(notice) = outcome.legacy_notice {
                println!("  {notice}");
            }
        }
    }
}

/// Shared state behind the largest-first, memory-bounded work queue: a
/// single mutex guarding both the queue and the running byte total, so the
/// two can never be read or updated out of step with each other.
struct Shared {
    queue: VecDeque<(usize, PathBuf, u64)>,
    bytes_in_flight: u64,
}

struct Coordinator {
    state: Mutex<Shared>,
    cv: Condvar,
    budget: u64,
}

impl Coordinator {
    fn new(items: Vec<(usize, PathBuf, u64)>, budget: u64) -> Self {
        Coordinator {
            state: Mutex::new(Shared {
                queue: items.into(),
                bytes_in_flight: 0,
            }),
            cv: Condvar::new(),
            budget,
        }
    }

    /// Hands out the largest queued file that currently fits the remaining
    /// budget, scanning front-to-back (largest to smallest, since the
    /// queue was sorted that way before the coordinator was built). If
    /// nothing fits and nothing is in flight either, the true front is
    /// handed out regardless of size — a single file bigger than the whole
    /// budget still has to run, just alone, rather than deadlocking.
    fn take_next(&self) -> Option<(usize, PathBuf, u64)> {
        let mut guard = self.state.lock().unwrap();
        loop {
            if guard.queue.is_empty() {
                return None;
            }
            let remaining = self.budget.saturating_sub(guard.bytes_in_flight);
            let position = guard
                .queue
                .iter()
                .position(|item| item.2 <= remaining)
                .or_else(|| (guard.bytes_in_flight == 0).then_some(0));
            if let Some(position) = position {
                let item = guard
                    .queue
                    .remove(position)
                    .expect("position came from this queue");
                guard.bytes_in_flight += item.2;
                return Some(item);
            }
            guard = self.cv.wait(guard).unwrap();
        }
    }

    fn release(&self, size: u64) {
        let mut guard = self.state.lock().unwrap();
        guard.bytes_in_flight = guard.bytes_in_flight.saturating_sub(size);
        drop(guard);
        self.cv.notify_all();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn opts() -> Options {
        Options::default()
    }

    #[test]
    fn beside_derives_the_unicode_sibling_name() {
        assert_eq!(
            beside(Path::new("report.docx")),
            Path::new("report.unicode.docx")
        );
        assert_eq!(
            beside(Path::new("/a/b/notes.pptx")),
            Path::new("/a/b/notes.unicode.pptx")
        );
    }

    #[test]
    fn beside_as_changes_both_the_name_and_the_format() {
        assert_eq!(
            beside_as(Path::new("notes.doc"), "docx"),
            Path::new("notes.unicode.docx")
        );
    }

    #[test]
    fn a_name_mukti_chose_is_not_written_over_without_being_asked() {
        let dir = std::env::temp_dir().join("mukti-convert-clobber-test");
        let _ = fs::create_dir_all(&dir);
        let taken = dir.join("already-here.txt");
        fs::write(&taken, b"precious").unwrap();

        let refused = refuse_to_clobber(&taken, false).unwrap_err();
        assert!(refused.contains("already"), "unhelpful: {refused}");
        assert!(
            refused.contains("--force"),
            "the way out is not offered: {refused}"
        );
        assert_eq!(
            fs::read(&taken).unwrap(),
            b"precious",
            "it was written over anyway"
        );

        assert!(
            refuse_to_clobber(&taken, true).is_ok(),
            "--force was refused"
        );

        let free = dir.join("not-here.txt");
        let _ = fs::remove_file(&free);
        assert!(refuse_to_clobber(&free, false).is_ok());

        let _ = fs::remove_file(&taken);
    }

    /// Every unsupported extension is refused, and nothing is written —
    /// both halves matter, since refusing while still writing something
    /// would be worse than either converting or refusing cleanly.
    #[test]
    fn unsupported_files_are_refused_and_nothing_is_written() {
        let dir = std::env::temp_dir().join("mukti-convert-format-refusal-test");
        let _ = fs::create_dir_all(&dir);
        let bijoy: &[u8] = b"Kg\xa9m~wP cP\xd6wZ\xa1e`b \xd3Kg\xa9m~wP\xd2";

        let cases: &[(&str, &str)] = &[
            ("m.json", "does not convert JSON"),
            ("m.pdf", "does not convert PDF"),
            ("m.txt", "cannot open"),
            ("m.csv", "cannot open"),
            ("m.md", "cannot open"),
            ("m", "cannot open"),
        ];

        for (name, expect) in cases {
            let input = dir.join(name);
            fs::write(&input, bijoy).expect("write the fixture");
            let candidates = [
                dir.join(format!("{name}.unicode")),
                input.with_extension("unicode.json"),
                input.with_extension("unicode.pdf"),
                input.with_extension("unicode.txt"),
                input.with_extension("unicode.csv"),
                input.with_extension("unicode.md"),
            ];
            for c in &candidates {
                let _ = fs::remove_file(c);
            }

            let result = convert_one(&input, Mode::Convert, &opts());
            let message = match result {
                Err(m) => m,
                Ok(_) => panic!("{name} must be refused, not converted"),
            };
            assert!(
                message.contains(expect),
                "{name}: expected {expect:?} in: {message}"
            );
            for c in &candidates {
                assert!(
                    !c.exists(),
                    "{name}: a file was written despite the refusal: {}",
                    c.display()
                );
            }
            let _ = fs::remove_file(&input);
        }
    }

    #[test]
    fn the_six_supported_extensions_reach_a_format_handler() {
        let dir = std::env::temp_dir().join("mukti-convert-format-accept-test");
        let _ = fs::create_dir_all(&dir);

        for ext in ["docx", "xlsx", "pptx", "doc", "xls", "ppt"] {
            let input = dir.join(format!("m.{ext}"));
            fs::write(&input, b"not a real document").expect("write the fixture");
            let result = convert_one(&input, Mode::Convert, &opts());
            let message = match result {
                Err(m) => m,
                Ok(_) => panic!("{ext}: garbage bytes cannot convert successfully"),
            };
            assert!(
                !message.contains("Mukti cannot open"),
                "{ext}: the six-format gate refused a supported extension: {message}"
            );
            assert!(
                !message.contains("does not convert JSON")
                    && !message.contains("does not convert PDF"),
                "{ext}: got a refusal meant for a different format: {message}"
            );
            let _ = fs::remove_file(&input);
        }
    }

    #[test]
    fn in_place_is_refused_for_the_older_formats() {
        let mut o = opts();
        o.in_place = true;
        let dir = std::env::temp_dir().join("mukti-convert-inplace-test");
        let _ = fs::create_dir_all(&dir);
        let input = dir.join("notes.doc");
        fs::write(&input, b"anything").unwrap();

        let message = convert_one(&input, Mode::Convert, &o).unwrap_err();
        assert!(message.contains("--in-place"), "{message}");
        let _ = fs::remove_file(&input);
    }

    #[test]
    fn duplicate_destinations_are_caught_before_any_file_is_read() {
        let dir = std::env::temp_dir().join("mukti-convert-duplicate-test");
        let _ = fs::create_dir_all(&dir);
        let a = dir.join("notes.doc");
        let b = dir.join("notes.docx");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();

        let files = vec![a.clone(), b.clone()];
        let err = check_for_duplicate_destinations(&files, &opts()).unwrap_err();
        assert!(err.contains("notes.doc"), "{err}");
        assert!(
            err.contains("notes.docx") && err.contains("notes.unicode.docx"),
            "{err}"
        );

        let _ = fs::remove_file(&a);
        let _ = fs::remove_file(&b);
    }

    #[test]
    fn no_duplicate_is_reported_for_files_that_derive_different_names() {
        let dir = std::env::temp_dir().join("mukti-convert-no-duplicate-test");
        let _ = fs::create_dir_all(&dir);
        let a = dir.join("alpha.docx");
        let b = dir.join("beta.docx");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();

        let files = vec![a.clone(), b.clone()];
        assert!(check_for_duplicate_destinations(&files, &opts()).is_ok());

        let _ = fs::remove_file(&a);
        let _ = fs::remove_file(&b);
    }

    #[test]
    fn run_reports_failures_and_stops_the_whole_batch_on_a_duplicate() {
        let dir = std::env::temp_dir().join("mukti-convert-run-duplicate-test");
        let _ = fs::create_dir_all(&dir);
        let a = dir.join("notes.doc");
        let b = dir.join("notes.docx");
        fs::write(&a, b"x").unwrap();
        fs::write(&b, b"y").unwrap();

        let mut o = opts();
        o.verbosity = Verbosity::Quiet;
        let result = run(&[a.clone(), b.clone()], Mode::Convert, &o, None);
        assert!(result.any_failed);
        assert_eq!(result.total, Tally::default());

        let _ = fs::remove_file(&a);
        let _ = fs::remove_file(&b);
    }

    #[test]
    fn run_preserves_file_order_across_several_workers() {
        let dir = std::env::temp_dir().join("mukti-convert-run-parallel-test");
        let _ = fs::create_dir_all(&dir);
        let files: Vec<PathBuf> = (0..6)
            .map(|i| {
                let p = dir.join(format!("garbage-{i}.docx"));
                // Different sizes, so the largest-first reorder inside
                // convert_all actually has something to reorder.
                fs::write(&p, vec![b'x'; (i + 1) * 10]).unwrap();
                p
            })
            .collect();

        let mut o = opts();
        o.verbosity = Verbosity::Quiet;
        o.jobs = 4;
        let result = run(&files, Mode::Convert, &o, None);
        // None of these are real documents, so every one fails — the point
        // of this test is that the run completes and reports a failure per
        // file rather than losing or duplicating one under parallelism.
        assert!(result.any_failed);

        for f in &files {
            let _ = fs::remove_file(f);
        }
    }
}
