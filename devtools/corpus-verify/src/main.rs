//! Run every document in an archive through Mukti and check the result.
//!
//! The original release verified 300 documents by hand. That produced a number
//! nobody could reproduce and a check nobody could repeat, which is the same
//! mistake the accuracy claims made before they were rebuilt. This is that
//! check, written as a command.
//!
//! # What it actually checks
//!
//! Not "did it look right" — for each format, the properties that must hold for
//! **every** document, whatever is in it:
//!
//! | Kind | Checked |
//! |---|---|
//! | Office | opens; word count preserved; every non-text entry byte-identical; entry list unchanged; no legacy font left; converting twice changes nothing more |
//! | PDF | never panics; reports how much text was recoverable rather than guessing |
//! | Text | nothing converted means byte-for-byte identical output |
//! | English-only | **zero** words converted — anything else is a false positive |
//!
//! The strongest of these is **idempotence**. A converter that has genuinely
//! finished leaves nothing for a second pass to do; one that mangles its own
//! output will convert something the second time. It catches a whole class of
//! fault that eyeballing a document never will.
//!
//! # Why it writes to `local/`
//!
//! The report names real files from a private archive, so it goes where nothing
//! is ever committed from. Aggregate counts are the only thing that may leave.
//!
//! # Surviving a file that kills the process
//!
//! Each file is wrapped so a panic is recorded and the run continues. A **stack
//! overflow** cannot be caught that way — it takes the process with it, which is
//! exactly what RUSTSEC-2026-0187 did with a 21 KB PDF. So every row is flushed
//! to disk as it is produced: if the process dies, the last row names the file
//! that killed it, and `--resume` carries on past it.

use std::collections::BTreeMap;
use std::fs;
use std::io::{Read, Seek, Write};
use std::path::{Path, PathBuf};

use mukti_formats::office::{is_exactly_legacy_font, is_legacy_font, names_a_font};
use mukti_formats::{convert_office, convert_pdf_to_text, runs, Summary};

/// What we decided about one file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Status {
    /// Converted, and every invariant for its kind held.
    Ok,
    /// Read fine, but there was nothing legacy in it. Not a fault.
    Untouched,
    /// The format is not supported yet. Not a fault either — a gap.
    Unsupported,
    /// Read, but produced nothing usable. For PDFs this is often the file's
    /// fault, not ours: a font storing only glyph shapes carries no text.
    NoText,
    /// An invariant was violated. **This is a defect.**
    Failed,
    /// The code panicked. **Also a defect**, and a worse one.
    Panicked,
}

impl Status {
    fn as_str(self) -> &'static str {
        match self {
            Status::Ok => "ok",
            Status::Untouched => "untouched",
            Status::Unsupported => "unsupported",
            Status::NoText => "no-text",
            Status::Failed => "FAILED",
            Status::Panicked => "PANICKED",
        }
    }
    /// Does this row mean something is wrong with Mukti?
    fn is_defect(self) -> bool {
        matches!(self, Status::Failed | Status::Panicked)
    }
}

struct Outcome {
    status: Status,
    /// Plain-English detail. Never contains document text.
    detail: String,
    converted: usize,
    untouched: usize,
    fonts: usize,
}

impl Outcome {
    fn ok(summary: Summary) -> Self {
        let status = if summary.words_converted == 0 {
            Status::Untouched
        } else {
            Status::Ok
        };
        Self {
            status,
            detail: String::new(),
            converted: summary.words_converted,
            untouched: summary.words_untouched,
            fonts: summary.fonts_changed,
        }
    }
    fn bad(status: Status, detail: impl Into<String>) -> Self {
        Self {
            status,
            detail: detail.into(),
            converted: 0,
            untouched: 0,
            fonts: 0,
        }
    }
}

fn main() -> std::process::ExitCode {
    match run() {
        Ok(defects) => {
            if defects == 0 {
                std::process::ExitCode::SUCCESS
            } else {
                eprintln!("\n{defects} file(s) violated an invariant. See the report.");
                std::process::ExitCode::FAILURE
            }
        }
        Err(e) => {
            eprintln!("corpus-verify: {e}");
            std::process::ExitCode::FAILURE
        }
    }
}

struct Config {
    roots: Vec<PathBuf>,
    out: PathBuf,
    resume: bool,
    only: Option<Vec<String>>,
    limit: Option<usize>,
    /// Treat everything found as English-only: any conversion is a false positive.
    negative: bool,
}

fn parse() -> Result<Config, String> {
    let mut cfg = Config {
        roots: Vec::new(),
        out: PathBuf::from("local/verify-report.tsv"),
        resume: false,
        only: None,
        limit: None,
        negative: false,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--out" => cfg.out = PathBuf::from(it.next().ok_or("--out needs a file")?),
            "--resume" => cfg.resume = true,
            "--negative" => cfg.negative = true,
            "--only" => {
                let list = it.next().ok_or("--only needs a comma-separated list")?;
                cfg.only = Some(list.split(',').map(|s| s.trim().to_lowercase()).collect());
            }
            "--limit" => {
                let n = it.next().ok_or("--limit needs a number")?;
                cfg.limit = Some(n.parse().map_err(|_| format!("not a number: {n}"))?);
            }
            other if other.starts_with("--") => return Err(format!("unknown option {other}")),
            path => cfg.roots.push(PathBuf::from(path)),
        }
    }
    if cfg.roots.is_empty() {
        return Err(
            "usage: corpus-verify <dir>... [--out <file>] [--only docx,pdf] [--limit N] \
             [--resume] [--negative]"
                .into(),
        );
    }
    Ok(cfg)
}

fn run() -> Result<usize, String> {
    let cfg = parse()?;

    let mut files = Vec::new();
    for root in &cfg.roots {
        if !root.is_dir() {
            return Err(format!("not a directory: {}", root.display()));
        }
        collect(root, &mut files).map_err(|e| format!("walking {}: {e}", root.display()))?;
    }
    files.sort();

    if let Some(only) = &cfg.only {
        files.retain(|p| {
            extension(p).is_some_and(|e| only.contains(&e))
        });
    }

    // Refuse to write an empty report over a real one. corpus-label learned this
    // the expensive way: pointed at a moved directory it truncated a 152 MB
    // labelled set to a bare header, silently, and exited successfully.
    if files.is_empty() {
        return Err("found no files to check — is the path right? Refusing to write an \
                    empty report over an existing one."
            .into());
    }

    let already: std::collections::HashSet<String> = if cfg.resume && cfg.out.exists() {
        fs::read_to_string(&cfg.out)
            .map_err(|e| e.to_string())?
            .lines()
            .skip(1)
            .filter_map(|l| l.split('\t').next().map(str::to_owned))
            .collect()
    } else {
        std::collections::HashSet::new()
    };

    if let Some(parent) = cfg.out.parent() {
        fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let fresh = already.is_empty();
    let mut report = fs::OpenOptions::new()
        .create(true)
        .append(!fresh)
        .write(true)
        .truncate(fresh)
        .open(&cfg.out)
        .map_err(|e| format!("opening {}: {e}", cfg.out.display()))?;
    if fresh {
        writeln!(report, "path\tkind\tstatus\tconverted\tuntouched\tfonts\tdetail")
            .map_err(|e| e.to_string())?;
    }

    let total = files.len();
    println!("Checking {total} file(s).");
    if cfg.negative {
        println!("Negative mode: any converted word counts as a defect.");
    }

    let mut tally: BTreeMap<(String, &'static str), usize> = BTreeMap::new();
    let mut defects = 0usize;
    let mut done = 0usize;

    for path in files {
        if let Some(limit) = cfg.limit {
            if done >= limit {
                break;
            }
        }
        let key = path.to_string_lossy().to_string();
        if already.contains(&key) {
            continue;
        }
        let kind = extension(&path).unwrap_or_else(|| "none".into());

        // A panic here is a defect to record, not a reason to stop.
        let outcome = std::panic::catch_unwind(|| check(&path, &kind, cfg.negative))
            .unwrap_or_else(|_| {
                Outcome::bad(
                    Status::Panicked,
                    "the code panicked while reading this file",
                )
            });

        *tally.entry((kind.clone(), outcome.status.as_str())).or_default() += 1;
        if outcome.status.is_defect() {
            defects += 1;
            println!("  {} {}  {}", outcome.status.as_str(), key, outcome.detail);
        }

        writeln!(
            report,
            "{key}\t{kind}\t{}\t{}\t{}\t{}\t{}",
            outcome.status.as_str(),
            outcome.converted,
            outcome.untouched,
            outcome.fonts,
            outcome.detail
        )
        .map_err(|e| e.to_string())?;
        // Flush every row. A stack overflow cannot be caught, so the last row
        // written is the only evidence of which file caused it.
        report.flush().map_err(|e| e.to_string())?;

        done += 1;
        if done % 100 == 0 {
            println!("  {done}/{total}...");
        }
    }

    println!("\n{:-<62}", "");
    println!("{:<8} {:<12} {:>8}", "kind", "status", "files");
    println!("{:-<62}", "");
    for ((kind, status), n) in &tally {
        println!("{kind:<8} {status:<12} {n:>8}");
    }
    println!("{:-<62}", "");
    println!("Report: {}", cfg.out.display());
    println!(
        "\n{} file(s) checked, {} defect(s).",
        done, defects
    );
    Ok(defects)
}

fn extension(p: &Path) -> Option<String> {
    p.extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_lowercase())
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    for entry in fs::read_dir(dir)? {
        let entry = entry?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().to_string();
        // Word and Excel lock files are not documents.
        if name.starts_with("~$") || name == ".DS_Store" {
            continue;
        }
        // Never walk into a git checkout: its objects are not documents and
        // there are thousands of them.
        if path.is_dir() {
            if name == ".git" {
                continue;
            }
            collect(&path, out)?;
        } else {
            out.push(path);
        }
    }
    Ok(())
}

fn check(path: &Path, kind: &str, negative: bool) -> Outcome {
    let bytes = match fs::read(path) {
        Ok(b) => b,
        Err(e) => return Outcome::bad(Status::Failed, format!("could not read it: {e}")),
    };
    if bytes.is_empty() {
        // A zero-byte file in the archive. Not a defect in Mukti — there is
        // nothing to convert and nothing it could have done differently. The CLI
        // already catches this case and says so before the zip reader sees it.
        return Outcome::bad(Status::Unsupported, "the file is empty (0 bytes)");
    }

    match kind {
        "docx" | "xlsx" | "pptx" => check_office(&bytes, negative),
        "pdf" => check_pdf(&bytes),
        "doc" | "xls" | "ppt" => Outcome::bad(
            Status::Unsupported,
            "the pre-2007 binary format is not supported yet",
        ),
        "txt" | "csv" | "tsv" | "md" | "json" | "html" | "htm" | "py" | "yaml" | "yml"
        | "ipynb" | "sample" | "rev" | "idx" | "pack" => check_text(&bytes, negative),
        _ => Outcome::bad(Status::Unsupported, "not a kind this tool converts"),
    }
}

/// Every property an Office conversion must satisfy, whatever the document.
fn check_office(bytes: &[u8], negative: bool) -> Outcome {
    let before = match runs(std::io::Cursor::new(bytes)) {
        Ok(r) => r,
        Err(e) => return Outcome::bad(Status::Failed, format!("could not be read: {e}")),
    };

    let (out, summary) = match convert_office(bytes, "Nirmala UI") {
        Ok(v) => v,
        Err(e) => return Outcome::bad(Status::Failed, format!("conversion failed: {e}")),
    };

    if negative && summary.words_converted > 0 {
        return Outcome::bad(
            Status::Failed,
            format!(
                "{} word(s) converted in a document that should be untouched",
                summary.words_converted
            ),
        );
    }

    let after = match runs(std::io::Cursor::new(&out)) {
        Ok(r) => r,
        Err(e) => {
            return Outcome::bad(
                Status::Failed,
                format!("the converted file could not be re-read: {e}"),
            )
        }
    };

    // 1. If nothing was converted, every entry must come back byte-identical.
    //
    //    The strongest check that is actually achievable, and the product's
    //    central promise. It caught the run-relocation bug: a converted word has
    //    to be consolidated into one run because its length changes, but that was
    //    being done to unconverted words too, so a document with no legacy Bangla
    //    at all came back rearranged — one run holding `t` came back holding
    //    `trainng`. Nothing weaker sees it, because the visible text was always
    //    correct.
    //
    //    Entry content, not the whole archive: measured 13 Aug 2026, a rebuilt
    //    archive differs from Word's by around 1,800 bytes of container framing
    //    even when every entry is copied through verbatim, because the zip crate
    //    does not reproduce Word's exact extra fields and alignment. That is not
    //    something this project promises or a user can observe. What is promised
    //    is that the content does not change, and that is what is checked.
    if summary.words_converted == 0 && summary.fonts_changed == 0 {
        if let Err(why) = every_entry_identical(bytes, &out) {
            return Outcome::bad(
                Status::Failed,
                format!("nothing was converted, yet {why}"),
            );
        }
    }

    // 2. The joined text must contain the same number of words.
    //
    //    Joined, not per-run, deliberately. A converted word that spanned several
    //    runs is written into the first of them, so per-run counts legitimately
    //    fall; the joined count must not. This is what sees words being glued
    //    together or newlines going missing.
    let joined_before: String = before.iter().map(|r| r.text.as_str()).collect();
    let joined_after: String = after.iter().map(|r| r.text.as_str()).collect();
    let words_before = joined_before.split_whitespace().count();
    let words_after = joined_after.split_whitespace().count();
    if words_before != words_after {
        return Outcome::bad(
            Status::Failed,
            format!("joined word count changed: {words_before} became {words_after}"),
        );
    }

    // 3. Every character that was not part of a converted word must survive.
    //    Whitespace is the sharpest probe: it is never converted, so any change
    //    in its shape means text moved.
    let ws_before = joined_before.chars().filter(|c| c.is_whitespace()).count();
    let ws_after = joined_after.chars().filter(|c| c.is_whitespace()).count();
    if ws_before != ws_after {
        return Outcome::bad(
            Status::Failed,
            format!("whitespace changed: {ws_before} characters became {ws_after}"),
        );
    }

    // 2. Nothing outside the text and font parts may change, and no entry may
    //    appear or disappear.
    if let Err(why) = entries_match(bytes, &out) {
        return Outcome::bad(Status::Failed, why);
    }

    // 3. No legacy font may survive, or the reader is asked for a font that
    //    contains no Bengali at all.
    if let Err(why) = no_legacy_font_left(&out) {
        return Outcome::bad(Status::Failed, why);
    }

    // 4. Idempotence. Converting the output again must find nothing to do.
    //    A converter that mangles its own output fails here and nowhere else.
    match convert_office(&out, "Nirmala UI") {
        Ok((_, second)) => {
            if second.words_converted > 0 {
                return Outcome::bad(
                    Status::Failed,
                    format!(
                        "not finished after one pass: a second pass converted {} more word(s)",
                        second.words_converted
                    ),
                );
            }
        }
        Err(e) => {
            return Outcome::bad(
                Status::Failed,
                format!("its own output could not be converted again: {e}"),
            )
        }
    }

    Outcome::ok(summary)
}

/// Compare the two archives entry by entry.
fn entries_match(before: &[u8], after: &[u8]) -> Result<(), String> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(before))
        .map_err(|e| format!("the original is not a readable archive: {e}"))?;
    let mut b = zip::ZipArchive::new(std::io::Cursor::new(after))
        .map_err(|e| format!("the result is not a readable archive: {e}"))?;

    let names_a: Vec<String> = a.file_names().map(str::to_owned).collect();
    let names_b: Vec<String> = b.file_names().map(str::to_owned).collect();
    if names_a.len() != names_b.len() {
        return Err(format!(
            "the archive gained or lost entries: {} became {}",
            names_a.len(),
            names_b.len()
        ));
    }
    let mut sorted_a = names_a.clone();
    let mut sorted_b = names_b.clone();
    sorted_a.sort();
    sorted_b.sort();
    if sorted_a != sorted_b {
        return Err("the set of entries in the archive changed".to_owned());
    }

    for name in &names_a {
        // Text, font and font-metadata parts are meant to change. Everything
        // else must not.
        if mukti_formats::office::is_text_part(name)
            || mukti_formats::office::is_font_part(name)
            || mukti_formats::office::is_metadata_font_part(name)
        {
            continue;
        }
        let one = read_entry(&mut a, name)?;
        let two = read_entry(&mut b, name)?;
        if one != two {
            return Err(format!(
                "an entry that should have been copied through was altered ({} bytes became {})",
                one.len(),
                two.len()
            ));
        }
    }
    Ok(())
}

/// Every entry, in order, with identical content. Used when the conversion
/// reported that it changed nothing at all.
fn every_entry_identical(before: &[u8], after: &[u8]) -> Result<(), String> {
    let mut a = zip::ZipArchive::new(std::io::Cursor::new(before))
        .map_err(|e| format!("the original is not a readable archive: {e}"))?;
    let mut b = zip::ZipArchive::new(std::io::Cursor::new(after))
        .map_err(|e| format!("the result is not a readable archive: {e}"))?;
    if a.len() != b.len() {
        return Err(format!(
            "the entry count changed: {} became {}",
            a.len(),
            b.len()
        ));
    }
    for i in 0..a.len() {
        let (name_a, one) = {
            let mut f = a
                .by_index(i)
                .map_err(|e| format!("entry {i} could not be opened: {e}"))?;
            let name = f.name().to_owned();
            let mut v = Vec::new();
            f.read_to_end(&mut v)
                .map_err(|e| format!("entry {i} could not be read: {e}"))?;
            (name, v)
        };
        let (name_b, two) = {
            let mut f = b
                .by_index(i)
                .map_err(|e| format!("entry {i} could not be opened: {e}"))?;
            let name = f.name().to_owned();
            let mut v = Vec::new();
            f.read_to_end(&mut v)
                .map_err(|e| format!("entry {i} could not be read: {e}"))?;
            (name, v)
        };
        if name_a != name_b {
            return Err(format!(
                "the entry order changed at position {i}: {name_a} became {name_b}"
            ));
        }
        if one != two {
            return Err(format!(
                "{name_a} changed ({} bytes became {})",
                one.len(),
                two.len()
            ));
        }
    }
    Ok(())
}

fn read_entry<R: Read + Seek>(zip: &mut zip::ZipArchive<R>, name: &str) -> Result<Vec<u8>, String> {
    let mut f = zip
        .by_name(name)
        .map_err(|e| format!("entry could not be opened: {e}"))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)
        .map_err(|e| format!("entry could not be read: {e}"))?;
    Ok(buf)
}

/// No legacy font name may survive anywhere a font is actually named.
///
/// **Where** matters as much as what. An earlier version of this function scanned
/// every word of every XML part, and reported a spreadsheet as retaining a legacy
/// font because a participant list contained the name **SULEKHA** — which is also
/// a font. That is not a new mistake: the release check for v0.4.0 made the
/// identical one on the identical corpus, and `LESSONS.md` §1 records it. Writing
/// a verifier is no protection against repeating the bug it was written to catch.
///
/// So this parses the XML and looks in exactly two places:
///
/// * **attribute values on elements that name a font** — `w:rFonts`, `a:latin`,
///   `rFont` and the rest, via the converter's own `names_a_font`;
/// * **text nodes in `docProps/app.xml`**, the one part that records font names as
///   text, and there only when the text is *exactly* a font name.
///
/// Cell values, slide titles and body text are not font names and are not looked
/// at.
fn no_legacy_font_left(after: &[u8]) -> Result<(), String> {
    use quick_xml::events::Event;

    let mut zip = zip::ZipArchive::new(std::io::Cursor::new(after))
        .map_err(|e| format!("the result is not a readable archive: {e}"))?;
    let names: Vec<String> = zip.file_names().map(str::to_owned).collect();

    for name in names {
        if !name.ends_with(".xml") {
            continue;
        }
        let bytes = read_entry(&mut zip, &name)?;
        let xml = String::from_utf8_lossy(&bytes).into_owned();
        let metadata = mukti_formats::office::is_metadata_font_part(&name);

        let mut reader = quick_xml::Reader::from_str(&xml);
        reader.config_mut().trim_text(false);
        loop {
            let event = match reader.read_event() {
                Ok(e) => e,
                // A part we cannot parse is not evidence of a surviving font.
                // Report it as its own problem rather than silently passing.
                Err(e) => return Err(format!("{name} could not be parsed: {e}")),
            };
            match event {
                Event::Eof => break,
                Event::Start(e) | Event::Empty(e) => {
                    if !names_a_font(local_name(e.name().as_ref())) {
                        continue;
                    }
                    for attr in e.attributes().flatten() {
                        let value = attr
                            .normalized_value(quick_xml::XmlVersion::Implicit1_0)
                            .unwrap_or_default();
                        if is_legacy_font(&value) {
                            return Err(format!(
                                "a legacy font is still named by an attribute in {name}"
                            ));
                        }
                    }
                }
                Event::Text(e) if metadata => {
                    let text = e.decode().unwrap_or_default();
                    if is_exactly_legacy_font(&text) {
                        return Err(format!(
                            "a legacy font name survives as text in {name}"
                        ));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

/// An element's name with any namespace prefix removed.
fn local_name(name: &[u8]) -> &[u8] {
    match name.iter().position(|b| *b == b':') {
        Some(i) => &name[i + 1..],
        None => name,
    }
}

fn check_pdf(bytes: &[u8]) -> Outcome {
    match convert_pdf_to_text(bytes) {
        Ok((text, summary)) => {
            if text.trim().is_empty() {
                return Outcome::bad(
                    Status::NoText,
                    format!(
                        "no readable text: {} string(s) were in fonts that store shapes \
                         rather than characters, so they were skipped rather than guessed at",
                        summary.fonts_changed
                    ),
                );
            }
            Outcome::ok(summary)
        }
        Err(e) => Outcome::bad(Status::Failed, format!("could not be read: {e}")),
    }
}

fn check_text(bytes: &[u8], negative: bool) -> Outcome {
    let (text, _encoding) = gru953_mukti::encoding::decode(bytes);
    let (out, summary) = mukti_formats::convert_text_with_summary(&text);

    if negative && summary.words_converted > 0 {
        return Outcome::bad(
            Status::Failed,
            format!(
                "{} word(s) converted in text that should be untouched",
                summary.words_converted
            ),
        );
    }

    // Nothing converted must mean nothing changed at all. This is the promise
    // the whole product rests on, so it is checked on every single file.
    if summary.words_converted == 0 && out != text {
        return Outcome::bad(
            Status::Failed,
            "no word was converted, yet the text came back different",
        );
    }
    Outcome::ok(summary)
}
