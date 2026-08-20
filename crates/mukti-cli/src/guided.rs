//! The guided conversation: what runs when `mukti` is typed alone at a real
//! keyboard, with no verb and no file named.
//!
//! `converse()` is pure over `&mut dyn BufRead`, `&mut dyn Write` and a
//! [`World`] trait standing in for the file system — so the whole
//! conversation can be scripted and checked without a real terminal, a
//! real folder, or a real document. `main.rs` supplies [`RealWorld`], the
//! only implementation that touches an actual disk.
//!
//! Guided mode converts one file at a time rather than reaching for
//! `convert::run`'s parallel batch: the trade is a live, redrawn progress
//! line a reader can watch, which needs a result after every single file
//! rather than after the whole, possibly reordered, batch. Flag mode's own
//! `--jobs` still uses the parallel path in full.

use std::io::{BufRead, Write};
use std::path::{Path, PathBuf};
use std::time::Instant;

use crate::convert::{self, Outcome, RunResult};
use crate::options::{Mode, Options};
use crate::pathinput::{self, Platform};
use crate::report::{self, Progress, Tally};
use crate::style::{self, Palette};
use crate::words;

/// Prose is wrapped to this many columns before printing — comfortably
/// inside even a narrow terminal window, and narrower than the 80 a
/// beginner's window is least likely to be resized below.
const WRAP_WIDTH: usize = 76;

/// Everything guided mode needs from the outside world, so the
/// conversation itself never calls `std::fs` or `std::env` directly.
pub trait World {
    fn current_dir(&self) -> PathBuf;
    fn home_dir(&self) -> PathBuf;
    fn platform(&self) -> Platform;
    fn is_file(&self, path: &Path) -> bool;
    fn is_dir(&self, path: &Path) -> bool;
    /// One level only, deliberately: sub-folders are reported as excluded,
    /// never walked into.
    fn list_dir(&self, dir: &Path) -> Vec<PathBuf>;
    fn create_dir_all(&self, dir: &Path) -> std::io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
    /// The wall clock, behind the trait like everything else, so a scripted
    /// conversation gets a fixed date rather than today's.
    fn now(&self) -> std::time::SystemTime;
    fn convert_one(&self, path: &Path, mode: Mode, opts: &Options) -> Result<Outcome, String>;
}

/// The real file system and the real converter. The only `World` this
/// crate ships that actually changes anything on disk.
pub struct RealWorld;

impl World for RealWorld {
    fn current_dir(&self) -> PathBuf {
        std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
    }

    fn home_dir(&self) -> PathBuf {
        std::env::var_os("HOME")
            .or_else(|| std::env::var_os("USERPROFILE"))
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from("/"))
    }

    fn platform(&self) -> Platform {
        if cfg!(windows) {
            Platform::Windows
        } else {
            Platform::Unix
        }
    }

    fn is_file(&self, path: &Path) -> bool {
        path.is_file()
    }

    fn is_dir(&self, path: &Path) -> bool {
        path.is_dir()
    }

    fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
        std::fs::read_dir(dir)
            .map(|entries| entries.filter_map(|e| e.ok().map(|e| e.path())).collect())
            .unwrap_or_default()
    }

    fn create_dir_all(&self, dir: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)
    }

    fn exists(&self, path: &Path) -> bool {
        path.exists()
    }

    fn now(&self) -> std::time::SystemTime {
        std::time::SystemTime::now()
    }

    fn convert_one(&self, path: &Path, mode: Mode, opts: &Options) -> Result<Outcome, String> {
        convert::convert_one(path, mode, opts)
    }
}

/// How the conversation ended, so `main.rs` can choose an exit code without
/// re-deriving it from printed text.
#[derive(Debug, PartialEq, Eq)]
pub enum GuidedOutcome {
    /// A batch ran; `any_failed` inside says whether to exit non-zero.
    Ran(RunResult),
    /// The reader chose to stop, or there was nothing to do.
    Stopped,
    /// The same question went unanswered three times running.
    GaveUp,
}

const SIX_EXTENSIONS: [&str; 6] = ["docx", "xlsx", "pptx", "doc", "xls", "ppt"];

struct Discovery {
    convertible: Vec<PathBuf>,
    by_extension: Vec<(&'static str, usize)>,
    subfolder_count: usize,
    skipped_count: usize,
}

fn discover(dir: &Path, world: &dyn World) -> Discovery {
    let mut convertible = Vec::new();
    let mut counts = [0usize; 6];
    let mut subfolder_count = 0;
    let mut skipped_count = 0;

    for entry in world.list_dir(dir) {
        if world.is_dir(&entry) {
            subfolder_count += 1;
            continue;
        }
        let name = entry
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        if name.starts_with("~$") || name.contains(".unicode.") {
            skipped_count += 1;
            continue;
        }
        let Some(ext) = entry
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
        else {
            continue;
        };
        if let Some(i) = SIX_EXTENSIONS.iter().position(|e| *e == ext) {
            counts[i] += 1;
            convertible.push(entry);
        }
    }

    let by_extension = SIX_EXTENSIONS.iter().copied().zip(counts).collect();
    Discovery {
        convertible,
        by_extension,
        subfolder_count,
        skipped_count,
    }
}

// ---------------------------------------------------------------------
// Printing: one line at a time, each through the right defence and,
// where the state calls for it, the right marker and colour.
// ---------------------------------------------------------------------

/// A wrapped, uncoloured sentence. Every string reaching this has already
/// been through `words::show()` for any path it names, so nothing here
/// sanitises a second time — doing so would corrupt, rather than protect,
/// any colour code a caller had already added (see `report::show_path`'s
/// own doc comment for why sanitising happens once, at the source).
fn say(output: &mut dyn Write, text: &str) {
    let _ = writeln!(output, "{}", report::wrap(text, WRAP_WIDTH));
}

/// A line that already carries its own colour codes, printed exactly as
/// built — `wrap`'s word-splitting has no notion of an escape sequence, so
/// a coloured line is written whole rather than risked through it.
fn say_raw(output: &mut dyn Write, text: &str) {
    let _ = writeln!(output, "{text}");
}

fn note(output: &mut dyn Write, palette: Option<&Palette>, text: &str) {
    say_raw(
        output,
        &format!("{} {text}", style::info(palette, words::NOTE_LABEL)),
    );
}

fn skipped(output: &mut dyn Write, palette: Option<&Palette>, text: &str) {
    say_raw(
        output,
        &format!("{} {text}", style::warning(palette, words::SKIPPED_LABEL)),
    );
}

fn done(output: &mut dyn Write, palette: Option<&Palette>, text: &str) {
    say_raw(
        output,
        &format!("{} {text}", style::success(palette, words::DONE_LABEL)),
    );
}

fn error_line(output: &mut dyn Write, palette: Option<&Palette>, text: &str) {
    say_raw(
        output,
        &format!("{} {text}", style::danger(palette, words::ERROR_LABEL)),
    );
}

/// The banner: the wordmark line in brand colour, the rest plain — brand
/// marks the wordmark only, never a sentence, per the brand kit this crate
/// follows.
fn say_banner(output: &mut dyn Write, palette: Option<&Palette>) {
    let mut lines = words::BANNER.lines();
    if let Some(first) = lines.next() {
        say_raw(output, &style::brand(palette, first));
    }
    for line in lines {
        say(output, line);
    }
    say(output, "");
}

fn ask(output: &mut dyn Write, palette: Option<&Palette>, text: &str) {
    let _ = write!(output, "{} ", style::accent(palette, text));
    let _ = output.flush();
}

fn read_line(input: &mut dyn BufRead) -> Option<String> {
    let mut buf = String::new();
    match input.read_line(&mut buf) {
        Ok(0) => None,
        Err(_) => None,
        Ok(_) => Some(buf.trim_end_matches(['\n', '\r']).to_owned()),
    }
}

// ---------------------------------------------------------------------
// Questions
// ---------------------------------------------------------------------

enum Answer {
    Yes,
    No,
    Stop,
    GaveUp,
}

fn ask_yes_no(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    palette: Option<&Palette>,
    question: &str,
) -> Answer {
    for attempt in 0..3 {
        ask(output, palette, &format!("{question} [y/n]"));
        let Some(line) = read_line(input) else {
            return Answer::Stop;
        };
        match line.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Answer::Yes,
            "n" | "no" => return Answer::No,
            "q" | "quit" | "stop" => return Answer::Stop,
            _ if attempt < 2 => say(output, &words::not_y_or_n()),
            _ => {}
        }
    }
    Answer::GaveUp
}

enum OutputChoice {
    NewFolder,
    Beside,
    Stop,
    GaveUp,
}

fn ask_output_location(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    palette: Option<&Palette>,
) -> OutputChoice {
    for attempt in 0..3 {
        ask(output, palette, &words::ask_output_location());
        let Some(line) = read_line(input) else {
            return OutputChoice::Stop;
        };
        match line.trim().to_ascii_lowercase().as_str() {
            "1" | "new" => return OutputChoice::NewFolder,
            "2" | "beside" => return OutputChoice::Beside,
            "q" | "quit" | "stop" => return OutputChoice::Stop,
            _ if attempt < 2 => say(output, &words::not_one_or_two()),
            _ => {}
        }
    }
    OutputChoice::GaveUp
}

fn ask_folder(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    world: &dyn World,
    palette: Option<&Palette>,
) -> Option<PathBuf> {
    ask(output, palette, &words::ask_folder());
    let line = read_line(input)?;
    let trimmed = line.trim();
    if trimmed.eq_ignore_ascii_case("q") {
        return None;
    }
    if trimmed.is_empty() {
        return Some(world.current_dir());
    }
    Some(pathinput::normalise(
        trimmed,
        &world.home_dir(),
        world.platform(),
    ))
}

/// Converts every file one at a time, redrawing a single progress line no
/// more often than every 80 ms, then reports — leading with failure when
/// there was any, so a run of 390 successes never reads as cheerful about
/// the 10 that were not.
fn run_batch(
    output: &mut dyn Write,
    world: &dyn World,
    opts: &Options,
    palette: Option<&Palette>,
    files: Vec<PathBuf>,
) -> GuidedOutcome {
    let total = files.len();
    let mut tally = Tally::default();
    let mut any_failed = false;
    let mut progress = Progress::new();

    for (i, path) in files.iter().enumerate() {
        let done_so_far = i + 1;
        if total > 1 && (done_so_far == total || progress.should_draw(Instant::now())) {
            let _ = write!(output, "{}", Progress::line(path, done_so_far, total));
            let _ = output.flush();
        }
        match world.convert_one(path, Mode::Convert, opts) {
            Ok(outcome) => tally.add(outcome.tally),
            Err(message) => {
                if total > 1 {
                    let _ = writeln!(output);
                }
                error_line(output, palette, &message);
                any_failed = true;
            }
        }
    }
    if total > 1 {
        let _ = writeln!(output);
    }

    if any_failed {
        say(
            output,
            "Some files could not be converted; the messages above say which and why.",
        );
    } else {
        done(output, palette, &tally.describe(Mode::Convert));
    }
    // The one edit Mukti makes to text it otherwise leaves alone, reported
    // whether or not anything failed -- it happened either way.
    if let Some(text) = tally.normalisation_note(Mode::Convert) {
        note(output, palette, &text);
    }
    GuidedOutcome::Ran(RunResult {
        total: tally,
        any_failed,
    })
}

/// The whole guided conversation, start to finish.
pub fn converse(
    input: &mut dyn BufRead,
    output: &mut dyn Write,
    world: &dyn World,
    opts: &Options,
    palette: Option<&Palette>,
) -> GuidedOutcome {
    say_banner(output, palette);

    let Some(target) = ask_folder(input, output, world, palette) else {
        say(output, &words::guided_goodbye());
        return GuidedOutcome::Stopped;
    };

    if world.is_file(&target) {
        // A file Mukti cannot open is said so directly, here, rather than
        // offered and then refused a moment later. Being asked "convert this?",
        // answering yes, and only then being told it was never possible is a
        // worse conversation than being told at the point the answer is known.
        if !convert::is_supported(&target) {
            error_line(output, palette, &convert::refusal_for(&target));
            return GuidedOutcome::Stopped;
        }
        say(output, &words::given_a_file_offer_to_convert_it(&target));
        return match ask_yes_no(input, output, palette, &words::confirm_bare_file(&target)) {
            Answer::Yes => run_batch(output, world, opts, palette, vec![target]),
            Answer::No => {
                say(output, &words::run_cancelled());
                GuidedOutcome::Stopped
            }
            Answer::Stop => {
                say(output, &words::guided_goodbye());
                GuidedOutcome::Stopped
            }
            Answer::GaveUp => {
                say(output, &words::gave_up_after_unclear_answers());
                GuidedOutcome::GaveUp
            }
        };
    }

    if !world.is_dir(&target) {
        say(output, &words::folder_not_found(&target));
        return GuidedOutcome::Stopped;
    }

    let discovery = discover(&target, world);

    if discovery.convertible.is_empty() {
        if discovery.subfolder_count > 0 {
            say(
                output,
                &words::matches_only_in_subfolders(discovery.subfolder_count),
            );
        } else if discovery.skipped_count > 0 {
            say(
                output,
                &words::only_mukti_output_found(discovery.skipped_count),
            );
        } else {
            say(output, &words::nothing_found(&target));
        }
        return GuidedOutcome::Stopped;
    }

    note(
        output,
        palette,
        &words::discovery_report(&discovery.by_extension),
    );
    if discovery.subfolder_count > 0 {
        skipped(
            output,
            palette,
            &words::subfolders_excluded_note(discovery.subfolder_count),
        );
    }
    if discovery.skipped_count > 0 {
        skipped(
            output,
            palette,
            &words::skipped_note(discovery.skipped_count),
        );
    }

    let mut opts = opts.clone();
    match ask_output_location(input, output, palette) {
        OutputChoice::NewFolder => {
            // Dated, so converting the same folder again next week lands
            // somewhere new instead of colliding with the earlier run -- and
            // so a reader looking at two of these can tell which is which.
            // `world.now()` rather than the clock directly, so the test can
            // pin the date.
            let folder = target.join(format!(
                "mukti-converted-{}",
                report::date_stamp(world.now())
            ));
            match world.create_dir_all(&folder) {
                Ok(()) => opts.output_folder = Some(folder),
                Err(e) => {
                    error_line(
                        output,
                        palette,
                        &words::could_not_make_folder(&folder, e.kind()),
                    );
                    return GuidedOutcome::Stopped;
                }
            }
        }
        OutputChoice::Beside => {
            let clashing = discovery
                .convertible
                .iter()
                .filter(|f| {
                    convert::preview_destination(f.as_path(), &opts)
                        .is_some_and(|d| world.exists(&d))
                })
                .count();
            if clashing > 0 {
                match ask_yes_no(
                    input,
                    output,
                    palette,
                    &words::some_outputs_already_exist(clashing),
                ) {
                    Answer::Yes => opts.force = true,
                    Answer::No => {
                        say(output, &words::run_cancelled());
                        return GuidedOutcome::Stopped;
                    }
                    Answer::Stop => {
                        say(output, &words::guided_goodbye());
                        return GuidedOutcome::Stopped;
                    }
                    Answer::GaveUp => {
                        say(output, &words::gave_up_after_unclear_answers());
                        return GuidedOutcome::GaveUp;
                    }
                }
            }
        }
        OutputChoice::Stop => {
            say(output, &words::guided_goodbye());
            return GuidedOutcome::Stopped;
        }
        OutputChoice::GaveUp => {
            say(output, &words::gave_up_after_unclear_answers());
            return GuidedOutcome::GaveUp;
        }
    }

    match ask_yes_no(
        input,
        output,
        palette,
        &words::confirm_conversion(discovery.convertible.len()),
    ) {
        Answer::Yes => run_batch(output, world, &opts, palette, discovery.convertible),
        Answer::No => {
            say(output, &words::run_cancelled());
            GuidedOutcome::Stopped
        }
        Answer::Stop => {
            say(output, &words::guided_goodbye());
            GuidedOutcome::Stopped
        }
        Answer::GaveUp => {
            say(output, &words::gave_up_after_unclear_answers());
            GuidedOutcome::GaveUp
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use std::collections::BTreeMap;
    use std::io::Cursor;

    /// An in-memory stand-in for the file system: a fixed current
    /// directory and home, and a directory listing keyed by path, so a
    /// whole conversation can be scripted with no real disk access.
    struct FakeWorld {
        current_dir: PathBuf,
        home: PathBuf,
        dirs: BTreeMap<PathBuf, Vec<PathBuf>>,
        files: Vec<PathBuf>,
        existing_outputs: Vec<PathBuf>,
        created_dirs: RefCell<Vec<PathBuf>>,
        converted: RefCell<Vec<PathBuf>>,
        fail: bool,
    }

    impl FakeWorld {
        fn new() -> Self {
            FakeWorld {
                current_dir: PathBuf::from("/home/example/docs"),
                home: PathBuf::from("/home/example"),
                dirs: BTreeMap::new(),
                files: Vec::new(),
                existing_outputs: Vec::new(),
                created_dirs: RefCell::new(Vec::new()),
                converted: RefCell::new(Vec::new()),
                fail: false,
            }
        }

        fn with_listing(mut self, dir: &str, entries: &[&str]) -> Self {
            self.dirs.insert(
                PathBuf::from(dir),
                entries.iter().map(|e| PathBuf::from(dir).join(e)).collect(),
            );
            self
        }

        fn with_file(mut self, path: &str) -> Self {
            self.files.push(PathBuf::from(path));
            self
        }

        fn with_existing_output(mut self, path: &str) -> Self {
            self.existing_outputs.push(PathBuf::from(path));
            self
        }
    }

    impl World for FakeWorld {
        fn current_dir(&self) -> PathBuf {
            self.current_dir.clone()
        }
        fn home_dir(&self) -> PathBuf {
            self.home.clone()
        }
        fn platform(&self) -> Platform {
            Platform::Unix
        }
        fn is_file(&self, path: &Path) -> bool {
            self.files.contains(&path.to_path_buf())
        }
        fn is_dir(&self, path: &Path) -> bool {
            self.dirs.contains_key(path)
        }
        fn list_dir(&self, dir: &Path) -> Vec<PathBuf> {
            self.dirs.get(dir).cloned().unwrap_or_default()
        }
        fn create_dir_all(&self, dir: &Path) -> std::io::Result<()> {
            self.created_dirs.borrow_mut().push(dir.to_path_buf());
            Ok(())
        }
        fn exists(&self, path: &Path) -> bool {
            self.existing_outputs.contains(&path.to_path_buf())
        }
        fn now(&self) -> std::time::SystemTime {
            // Fixed, so the dated output folder has one right answer.
            // 1787184000 = 2026-08-20, cross-checked independently.
            std::time::UNIX_EPOCH + std::time::Duration::from_secs(1_787_184_000)
        }
        fn convert_one(
            &self,
            path: &Path,
            _mode: Mode,
            _opts: &Options,
        ) -> Result<Outcome, String> {
            self.converted.borrow_mut().push(path.to_path_buf());
            if self.fail {
                return Err("a fake failure, for testing".to_owned());
            }
            Ok(Outcome {
                tally: Tally {
                    converted: 1,
                    untouched: 0,
                    normalised: 0,
                },
                destination: Some(path.with_extension("unicode.docx")),
                fonts_changed: Some(1),
                legacy_notice: None,
                legacy_was_empty: false,
            })
        }
    }

    fn run_conversation(world: &FakeWorld, script: &str) -> (GuidedOutcome, String) {
        let mut input = Cursor::new(script.as_bytes().to_vec());
        let mut output: Vec<u8> = Vec::new();
        let outcome = converse(&mut input, &mut output, world, &Options::default(), None);
        (outcome, String::from_utf8(output).unwrap())
    }

    /// The whole happy path, written out, so any change to any word in it is
    /// a diff a reviewer sees rather than a surprise a reader gets.
    ///
    /// Two things about how this reads. The prompts have no line break after
    /// them: on a real terminal the reader's own Return supplies it, and this
    /// scripted input cannot echo. And the progress line is carriage-returned
    /// so it overwrites itself in place, which a captured transcript cannot
    /// show -- `\r` is rendered here as `[CR]` so the assertion covers it
    /// rather than hiding it.
    #[test]
    fn the_happy_path_reads_exactly_like_this() {
        let world =
            FakeWorld::new().with_listing("/home/example/docs", &["report.docx", "notes.doc"]);
        let (outcome, transcript) = run_conversation(&world, "\n2\ny\n");

        assert!(matches!(outcome, GuidedOutcome::Ran(_)));
        let shown = transcript.replace('\r', "[CR]");
        let expected = "\
Mukti by GRU953
Converts old Bangla writing in Office files to Unicode.
Simple technology. For everyone.

Which folder has the files to convert? Press Return to use the current folder, \
or type q to stop. Note: Found 1 .docx, 1 .doc.
Where should the converted files go? Type 1 for a new folder next to this one, \
or 2 to save each file beside its original. About to convert 2 files. Nothing \
changes until this is confirmed. Continue? [y/n] \
[CR][1/2] /home/example/docs/report.docx[CR][2/2] /home/example/docs/notes.doc
Done. 2 of 2 words converted; 0 left exactly as they were.
";
        assert_eq!(
            shown, expected,
            "the guided conversation's wording changed -- read the diff and \
             confirm it is an improvement before updating this test"
        );
    }

    #[test]
    fn typing_q_at_the_folder_prompt_stops_cleanly() {
        let world = FakeWorld::new();
        let (outcome, transcript) = run_conversation(&world, "q\n");
        assert_eq!(outcome, GuidedOutcome::Stopped);
        assert!(transcript.contains("Stopped"));
    }

    #[test]
    fn a_blank_answer_uses_the_current_folder() {
        let world = FakeWorld::new().with_listing("/home/example/docs", &["report.docx"]);
        let (outcome, transcript) = run_conversation(&world, "\n2\ny\n");
        assert!(matches!(outcome, GuidedOutcome::Ran(_)));
        assert!(transcript.contains("Found 1 .docx"));
        assert_eq!(world.converted.borrow().len(), 1);
        assert_eq!(
            world.converted.borrow()[0],
            PathBuf::from("/home/example/docs/report.docx")
        );
    }

    #[test]
    fn an_empty_folder_says_so_and_stops() {
        let world = FakeWorld::new().with_listing("/home/example/docs", &[]);
        let (outcome, transcript) = run_conversation(&world, "\n");
        assert_eq!(outcome, GuidedOutcome::Stopped);
        assert!(transcript.contains("no Word, Excel or PowerPoint files"));
    }

    #[test]
    fn a_folder_holding_only_earlier_mukti_output_says_so() {
        let world = FakeWorld::new().with_listing("/home/example/docs", &["report.unicode.docx"]);
        let (outcome, _) = run_conversation(&world, "\n");
        assert_eq!(outcome, GuidedOutcome::Stopped);
    }

    #[test]
    fn matches_only_in_subfolders_points_the_reader_there() {
        let world = FakeWorld::new()
            .with_listing("/home/example/docs", &["Archive"])
            .with_listing("/home/example/docs/Archive", &["old.docx"]);
        let (outcome, transcript) = run_conversation(&world, "\n");
        assert_eq!(outcome, GuidedOutcome::Stopped);
        // Word-wrapped at 76 columns, so the phrase may carry a line break
        // where a space would otherwise be — flattened before the check.
        assert!(transcript
            .replace('\n', " ")
            .contains("did not look inside"));
    }

    #[test]
    fn a_file_instead_of_a_folder_is_offered_directly() {
        let world = FakeWorld::new().with_file("/home/example/docs/report.docx");
        let (outcome, transcript) = run_conversation(&world, "/home/example/docs/report.docx\ny\n");
        assert!(matches!(outcome, GuidedOutcome::Ran(_)));
        assert!(transcript.contains("is a file, not a folder"));
        assert_eq!(
            world.converted.borrow()[0],
            PathBuf::from("/home/example/docs/report.docx")
        );
    }

    #[test]
    fn a_folder_that_does_not_exist_is_reported_plainly() {
        let world = FakeWorld::new();
        let (outcome, transcript) = run_conversation(&world, "/nowhere\n");
        assert_eq!(outcome, GuidedOutcome::Stopped);
        assert!(transcript.contains("no folder called"));
    }

    #[test]
    fn declining_the_confirmation_changes_nothing() {
        let world = FakeWorld::new().with_listing("/home/example/docs", &["report.docx"]);
        let (outcome, transcript) = run_conversation(&world, "\n2\nn\n");
        assert_eq!(outcome, GuidedOutcome::Stopped);
        assert!(transcript.contains("Nothing was changed"));
        assert_eq!(world.converted.borrow().len(), 0);
    }

    #[test]
    fn three_unclear_answers_give_up_rather_than_loop_forever() {
        let world = FakeWorld::new().with_listing("/home/example/docs", &["report.docx"]);
        let (outcome, _) = run_conversation(&world, "\nbanana\nbanana\nbanana\n");
        assert_eq!(outcome, GuidedOutcome::GaveUp);
    }

    #[test]
    fn choosing_a_new_folder_sets_output_folder_and_creates_it() {
        let world = FakeWorld::new().with_listing("/home/example/docs", &["report.docx"]);
        let (outcome, _) = run_conversation(&world, "\n1\ny\n");
        assert!(matches!(outcome, GuidedOutcome::Ran(_)));
        assert_eq!(
            world.created_dirs.borrow().as_slice(),
            [PathBuf::from(
                "/home/example/docs/mukti-converted-2026-08-20"
            )]
        );
    }

    #[test]
    fn the_final_line_leads_with_failure_when_any_file_failed() {
        let mut world = FakeWorld::new().with_listing("/home/example/docs", &["report.docx"]);
        world.fail = true;
        let (_, transcript) = run_conversation(&world, "\n2\ny\n");
        assert!(transcript.contains("could not be converted"));
    }

    #[test]
    fn an_existing_output_is_flagged_before_writing_and_can_be_declined() {
        let world = FakeWorld::new()
            .with_listing("/home/example/docs", &["report.docx"])
            .with_existing_output("/home/example/docs/report.unicode.docx");
        let (outcome, transcript) = run_conversation(&world, "\n2\nn\ny\n");
        // "2" (beside) -> existing-output warning -> "n" declines that,
        // ending the run before the final confirmation is ever reached.
        assert_eq!(outcome, GuidedOutcome::Stopped);
        assert!(transcript.contains("already"));
        assert_eq!(world.converted.borrow().len(), 0);
    }

    #[test]
    fn agreeing_to_replace_an_existing_output_proceeds() {
        let world = FakeWorld::new()
            .with_listing("/home/example/docs", &["report.docx"])
            .with_existing_output("/home/example/docs/report.unicode.docx");
        let (outcome, _) = run_conversation(&world, "\n2\ny\ny\n");
        assert!(matches!(outcome, GuidedOutcome::Ran(_)));
        assert_eq!(world.converted.borrow().len(), 1);
    }
}
