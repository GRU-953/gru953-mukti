//! Build a labelled token set from real Office documents.
//!
//! # Why this can be trusted
//!
//! Scribe has never had test data with a known correct answer at the scale
//! needed to measure detection. Round-trip testing manufactures an answer key
//! for *conversion*, but it cannot say anything about *detection* — it never
//! sees a word that should have been left alone.
//!
//! The documents solve this themselves. A `.docx` records the font of every
//! run of text. A run set in SutonnyMJ **is** legacy Bijoy; a run of Unicode
//! Bengali **is not**; an English run **is not**. So the labels are read off
//! the file format rather than judged by a person or, worse, by the very code
//! being measured. Tens of thousands of them, at no cost in hand-labelling.
//!
//! # Privacy
//!
//! The output holds real words from private programme documents. It is written
//! to `local/`, which is git-ignored, and nothing from it is ever committed.
//! Only the counts printed to the terminal leave this machine.
//!
//! # Splitting
//!
//! By **document**, never by token. Words from one document are heavily
//! correlated — the same names, the same headings, the same vocabulary — so
//! splitting at token level would put near-duplicates on both sides and every
//! reported figure would be flattering and wrong.

mod office;

use std::collections::BTreeMap;
use std::fs;
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};

/// Legacy Bangla fonts, lower-cased for comparison.
///
/// The two Sutonny variants are what Scribe converts. The rest are recorded
/// separately so the harness can report honestly on text Scribe is **not**
/// claiming to handle, rather than quietly scoring itself on it.
const SUTONNY: &[&str] = &["sutonnymj", "sutonnyomj", "sutonnyemj"];
const OTHER_LEGACY: &[&str] = &[
    "boishakhi",
    "bornosoft",
    "sulekha",
    "chandrabati",
    "modhumatimj",
    "adorsholipi",
    "nikoshban",
    "ekushey",
];

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
enum Label {
    /// A Sutonny run holding at least one byte from the range Bijoy uses for
    /// its conjuncts and vowel signs. Unambiguously legacy: **must** convert.
    Legacy,
    /// A Sutonny run that is pure ASCII. Ambiguous *by construction* — Bijoy
    /// is ASCII underneath, so `bvg` is the word নাম and `2026` is ২০২৬, but
    /// neither can be told apart from ordinary English or a plain number
    /// without the font that this label is derived from. Reported as its own
    /// class rather than folded into either side.
    LegacyAscii,
    /// A run in some other legacy font. Scribe does not claim to convert
    /// these; kept so that can be stated with a number beside it.
    OtherLegacy,
    /// Contains Unicode Bengali. **Must never be converted** — doing so is the
    /// corruption the whole detector exists to prevent.
    Unicode,
    /// Ordinary English in a non-legacy run. **Must never be converted.**
    English,
    /// Digits, punctuation and symbols only. Carries no evidence of any
    /// encoding and must pass through untouched.
    Inert,
    /// The declared font and the actual bytes contradict each other: a
    /// non-legacy font, but Bijoy-range characters in the text. Excluded from
    /// every figure, because a label nobody can trust is worse than no label.
    FontDisputed,
}

impl Label {
    fn as_str(self) -> &'static str {
        match self {
            Label::Legacy => "legacy",
            Label::LegacyAscii => "legacy_ascii",
            Label::OtherLegacy => "other_legacy",
            Label::Unicode => "unicode",
            Label::English => "english",
            Label::Inert => "inert",
            Label::FontDisputed => "font_disputed",
        }
    }
}

fn has_unicode_bengali(s: &str) -> bool {
    s.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c))
}

/// Characters Bijoy uses to carry conjuncts, vowel signs and reph.
///
/// The same ranges the converter's own detector uses, so the labels and the
/// thing being measured at least agree about what a Bijoy-range byte is.
fn has_bijoy_range(s: &str) -> bool {
    s.chars().any(|c| {
        let o = c as u32;
        (0x00A0..=0x024F).contains(&o) || (0x2010..=0x20FF).contains(&o)
    })
}

fn classify(token: &str, font: Option<&str>) -> Label {
    // Unicode Bengali settles it whatever the font claims. A document can
    // perfectly well carry Unicode text in a run still tagged SutonnyMJ,
    // because the typist changed the font and not the keyboard. Converting
    // that would corrupt it, so the text wins over the label.
    if has_unicode_bengali(token) {
        return Label::Unicode;
    }

    let family = font.map(str::to_lowercase).unwrap_or_default();
    if SUTONNY.iter().any(|f| family.contains(f)) {
        return if has_bijoy_range(token) {
            Label::Legacy
        } else {
            Label::LegacyAscii
        };
    }
    if OTHER_LEGACY.iter().any(|f| family.contains(f)) {
        return Label::OtherLegacy;
    }

    // The font says this is not legacy. The bytes disagree.
    //
    // A token carrying Bijoy-range characters — `©`, `~`, `‡`, `¨` — is not
    // English, whatever the run claims: English does not put a copyright sign
    // in the middle of a word. When the two sources of evidence contradict
    // each other, the honest label is neither, so the token is set aside and
    // counted rather than guessed at.
    //
    // This is not hypothetical. Converting the archive's `.doc` files to
    // `.docx` with LibreOffice dropped the font attribution on many runs, and
    // without this guard those runs were labelled English — so genuine Bijoy
    // such as `Kg©m~wP` counted as a false positive when Scribe correctly
    // turned it into কর্মসূচি. Left in, it would have made a correct
    // classifier look broken, which is the most expensive kind of wrong
    // measurement there is.
    //
    // Note the deliberate asymmetry: this only ever *removes* a token from the
    // English class. It never promotes anything into the legacy class, so it
    // cannot manufacture recall.
    //
    // The character has to sit INSIDE the word. English typography uses this
    // same byte range at the edges of words — curly quotes, an em-dash, a
    // trailing copyright sign — and none of that is evidence of anything. It
    // is `©` in the *middle* of `Kg©m~wP` that English never does.
    //
    // Two earlier versions of this guard were caught by unit tests: the first
    // swallowed a bare `—`, the second swallowed `“quoted”`. Both are here.
    //
    // Known limitation, and deliberately left: a Bijoy word whose only
    // out-of-range character is at its edge — `(`vwe)`, দাবি — is trimmed back
    // to plain ASCII and stays in the English class. That inflates the
    // measured false-positive rate rather than deflating it, so the error runs
    // in the safe direction: it understates the classifier.
    let core = token.trim_matches(|c: char| !c.is_ascii_alphanumeric());
    if has_bijoy_range(core) {
        return Label::FontDisputed;
    }

    if token.chars().any(|c| c.is_ascii_alphabetic()) {
        Label::English
    } else {
        Label::Inert
    }
}

/// Which half of the split a document belongs to.
///
/// FNV-1a over the path: deterministic, so re-running produces exactly the
/// same split, and no state file has to be kept in step with the corpus.
fn split_of(path: &Path) -> &'static str {
    let mut hash = 0xcbf2_9ce4_8422_2325u64;
    for b in path.to_string_lossy().as_bytes() {
        hash ^= u64::from(*b);
        hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
    }
    if hash % 2 == 0 {
        "tune"
    } else {
        "test"
    }
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut out = PathBuf::from("local/labelled-tokens.tsv");
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--out" => out = it.next().map(PathBuf::from).unwrap_or(out),
            other => roots.push(PathBuf::from(other)),
        }
    }
    if roots.is_empty() {
        return Err("usage: corpus-label <dir>... [--out <file>]".into());
    }

    let mut files = Vec::new();
    for root in &roots {
        find_office_files(root, &mut files)?;
    }
    files.sort();
    println!("{} Office files found.", files.len());

    // Refuse to touch the output when there is nothing to write into it.
    //
    // This is not defensive tidiness; it is a defect being closed. The output
    // file used to be created before the inputs were examined, so pointing
    // this tool at a directory that had been moved truncated a 152 MB
    // labelled set to a bare header — silently, and with an exit code of 0.
    // Destroying the previous answer key is the worst thing this tool can do,
    // so it now cannot do it by accident.
    if files.is_empty() {
        return Err(format!(
            "no .docx, .xlsx or .pptx files under {}.\n\
             Refusing to write {}, which would destroy whatever is already there.\n\
             Check the paths: they may have been moved or renamed.",
            roots
                .iter()
                .map(|r| r.display().to_string())
                .collect::<Vec<_>>()
                .join(", "),
            out.display()
        )
        .into());
    }

    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut writer = BufWriter::new(fs::File::create(&out)?);
    // A document index, not a path: the context pass needs to know which
    // words sit together, and a bare integer says that without carrying a
    // private file name out of the archive.
    writeln!(writer, "split\tdoc\tlabel\ttoken")?;

    // Counts only. Never a token, never a file name — this goes to the terminal.
    let mut counts: BTreeMap<(&str, Label), usize> = BTreeMap::new();
    let mut documents_with_legacy = 0usize;
    let mut unreadable = 0usize;

    for (doc, path) in files.iter().enumerate() {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let runs = match office::runs(file) {
            Ok(r) => r,
            Err(_) => {
                unreadable += 1;
                continue;
            }
        };
        let split = split_of(path);
        let mut legacy_here = 0usize;

        for run in runs {
            for token in run.text.split_whitespace() {
                let label = classify(token, run.font.as_deref());
                if matches!(label, Label::Legacy | Label::LegacyAscii) {
                    legacy_here += 1;
                }
                *counts.entry((split, label)).or_default() += 1;
                writeln!(writer, "{split}\t{doc}\t{}\t{token}", label.as_str())?;
            }
        }
        if legacy_here > 0 {
            documents_with_legacy += 1;
        }
    }
    writer.flush()?;

    report(
        &counts,
        files.len(),
        documents_with_legacy,
        unreadable,
        &out,
    );
    Ok(())
}

fn report(
    counts: &BTreeMap<(&str, Label), usize>,
    files: usize,
    with_legacy: usize,
    unreadable: usize,
    out: &Path,
) {
    let labels = [
        Label::Legacy,
        Label::LegacyAscii,
        Label::OtherLegacy,
        Label::Unicode,
        Label::English,
        Label::Inert,
        Label::FontDisputed,
    ];
    println!("\n{files} files, {with_legacy} carrying legacy text, {unreadable} unreadable.\n");
    println!(
        "{:<14} {:>12} {:>12} {:>12}",
        "label", "tune", "test", "total"
    );
    println!("{}", "-".repeat(54));
    let mut grand = 0usize;
    for label in labels {
        let tune = counts.get(&("tune", label)).copied().unwrap_or(0);
        let test = counts.get(&("test", label)).copied().unwrap_or(0);
        grand += tune + test;
        println!(
            "{:<14} {tune:>12} {test:>12} {:>12}",
            label.as_str(),
            tune + test
        );
    }
    println!("{}", "-".repeat(54));
    println!("{:<14} {:>38}", "total", grand);
    println!(
        "\nWritten to {} — git-ignored, never committed.",
        out.display()
    );
}

fn find_office_files(dir: &Path, out: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !dir.is_dir() {
        return Ok(());
    }
    for entry in fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            find_office_files(&path, out)?;
        } else if matches!(
            path.extension().and_then(|e| e.to_str()),
            Some("docx" | "xlsx" | "pptx")
        ) {
            // Word writes a lock file beside an open document. It is not a
            // document and does not parse as one.
            if !path
                .file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with("~$"))
            {
                out.push(path);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unicode_bengali_outranks_whatever_font_the_run_claims() {
        // A typist changed the font and not the keyboard. The text is Unicode
        // and converting it would corrupt it, so the text must win.
        assert_eq!(classify("প্রতিবেদন", Some("SutonnyMJ")), Label::Unicode);
        assert_eq!(classify("প্রতিবেদন", None), Label::Unicode);
    }

    #[test]
    fn a_sutonny_run_splits_on_whether_it_is_pure_ascii() {
        // Carries Bijoy-range bytes: unambiguously legacy.
        assert_eq!(classify("Kg\u{a9}m~wP", Some("SutonnyMJ")), Label::Legacy);
        // Pure ASCII: genuinely legacy, but indistinguishable from English
        // without the font, so it is kept as its own class.
        assert_eq!(classify("bvg", Some("SutonnyMJ")), Label::LegacyAscii);
        assert_eq!(classify("2026", Some("SutonnyOMJ")), Label::LegacyAscii);
    }

    #[test]
    fn other_legacy_fonts_are_recorded_but_not_claimed() {
        assert_eq!(classify("Kg\u{a9}", Some("Boishakhi")), Label::OtherLegacy);
        assert_eq!(classify("abc", Some("Sulekha")), Label::OtherLegacy);
    }

    #[test]
    fn ordinary_text_is_labelled_by_what_it_is() {
        assert_eq!(classify("Programme", Some("Calibri")), Label::English);
        assert_eq!(classify("Programme", None), Label::English);
        assert_eq!(classify("2026", Some("Calibri")), Label::Inert);
        assert_eq!(classify("—", None), Label::Inert);
        assert_eq!(classify("(12.5%)", None), Label::Inert);
    }

    /// The font says English; the bytes say Bijoy. Neither label is trustworthy.
    #[test]
    fn a_run_whose_font_and_bytes_disagree_is_set_aside() {
        // Real case: converting .doc to .docx dropped the font attribution, so
        // genuine Bijoy arrived claiming to be Calibri.
        for bijoy in ["Kg\u{a9}m~wP", "Ki\u{2021}Z", "\u{af}\u{2019}vbvš\u{cd}i"] {
            assert_eq!(
                classify(bijoy, Some("Calibri")),
                Label::FontDisputed,
                "Bijoy bytes were labelled English: {bijoy}"
            );
        }
        // But ordinary English punctuation shares that byte range and must not
        // be dragged in with it.
        // English typography lives in the same byte range at the EDGES of
        // words, and must not be dragged in with it.
        assert_eq!(classify("\u{2014}", None), Label::Inert);
        assert_eq!(classify("\u{201c}quoted\u{201d}", None), Label::English);
        assert_eq!(classify("Ltd.\u{a9}", None), Label::English);
    }

    #[test]
    fn the_split_is_stable_and_divides_documents_not_tokens() {
        let a = Path::new("/corpus/one.docx");
        let b = Path::new("/corpus/two.docx");
        assert_eq!(split_of(a), split_of(a), "the split must be deterministic");
        // Both halves must actually be reachable, or the split is not a split.
        let halves: std::collections::BTreeSet<&str> = (0..200)
            .map(|i| split_of(&PathBuf::from(format!("/corpus/{i}.docx"))))
            .collect();
        assert_eq!(halves.len(), 2, "every document landed in the same half");
        let _ = b;
    }
}
