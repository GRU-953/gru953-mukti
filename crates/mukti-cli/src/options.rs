//! Turning the raw argument list into something the rest of the program can
//! act on.
//!
//! No arguments-parsing crate: the options are few enough that hand-rolled
//! parsing keeps this crate's own dependency list at one entry, the
//! converter itself — a position defended in this crate's own module doc
//! since before this file existed, and unchanged by splitting it into
//! eight.
//!
//! This module makes no decision about *whether* to run guided mode — it
//! only records that no verb and no file were given. `main.rs` decides what
//! that means once it knows whether the terminal can hold a conversation.

use std::path::PathBuf;

use crate::style::ThemeFlag;
use crate::words;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode {
    Convert,
    Check,
}

/// Three states, not a bool. "Say nothing but errors" and "say the usual
/// amount" stopped being the only two things worth recording once guided
/// mode's own conversational reporting became a third.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Verbosity {
    Quiet,
    #[default]
    Normal,
}

#[derive(Clone, Debug)]
pub struct Options {
    pub mode: Option<Mode>,
    pub files: Vec<PathBuf>,
    pub in_place: bool,
    pub force: bool,
    pub out: Option<PathBuf>,
    pub font: String,
    pub verbosity: Verbosity,
    pub jobs: usize,
    pub theme_flag: Option<ThemeFlag>,
    /// Set only by guided mode, when the reader chose "a new folder" over
    /// "beside each original". Flag mode has no switch for this and never
    /// sets it — `--out` already covers the one-file case, and nothing
    /// else in flag mode needed it.
    pub output_folder: Option<PathBuf>,
}

impl Default for Options {
    fn default() -> Self {
        Options {
            mode: None,
            files: Vec::new(),
            in_place: false,
            force: false,
            out: None,
            // Nirmala UI ships with Windows and covers Bengali; it is the
            // safest default for a document most likely to be opened in
            // Word. Anyone who prefers SolaimanLipi or Kalpurush can say so
            // with --font.
            font: String::from("Nirmala UI"),
            verbosity: Verbosity::Normal,
            jobs: 1,
            theme_flag: None,
            output_folder: None,
        }
    }
}

#[derive(Debug)]
pub enum ParseOutcome {
    Help,
    Version,
    Options(Options),
}

/// Parses argv (without the program name). `--help`/`--version` win over
/// everything else and are checked first, matching the behaviour a
/// beginner expects from typing either by mistake alongside other flags.
pub fn parse_args(args: Vec<String>) -> Result<ParseOutcome, String> {
    if args.iter().any(|a| a == "--help" || a == "-h") {
        return Ok(ParseOutcome::Help);
    }
    if args.iter().any(|a| a == "--version" || a == "-V") {
        return Ok(ParseOutcome::Version);
    }

    let mut options = Options::default();
    let mut it = args.into_iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "convert" if options.mode.is_none() => options.mode = Some(Mode::Convert),
            "check" if options.mode.is_none() => options.mode = Some(Mode::Check),
            "--font" => {
                options.font = it.next().ok_or_else(words::font_needs_a_value)?;
            }
            "--in-place" => options.in_place = true,
            "--force" => options.force = true,
            "--quiet" | "-q" => options.verbosity = Verbosity::Quiet,
            "--out" => {
                options.out = Some(
                    it.next()
                        .map(PathBuf::from)
                        .ok_or_else(words::out_needs_a_value)?,
                );
            }
            "--jobs" => {
                let raw = it.next().ok_or_else(words::jobs_needs_a_number)?;
                let n: usize = raw.parse().map_err(|_| words::jobs_needs_a_number())?;
                if n < 1 {
                    return Err(words::jobs_must_be_at_least_one());
                }
                options.jobs = n;
            }
            "--theme" => {
                let raw = it.next().ok_or_else(words::bad_theme_value)?;
                options.theme_flag =
                    Some(ThemeFlag::parse(&raw).ok_or_else(words::bad_theme_value)?);
            }
            other if other.starts_with("--") => return Err(words::unknown_option(other)),
            other => options.files.push(PathBuf::from(other)),
        }
    }

    if options.out.is_some() && options.files.len() > 1 {
        return Err(words::out_with_several_files(options.files.len()));
    }

    Ok(ParseOutcome::Options(options))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strs(args: &[&str]) -> Vec<String> {
        args.iter().map(|s| s.to_string()).collect()
    }

    fn expect_options(args: &[&str]) -> Options {
        match parse_args(strs(args)).unwrap() {
            ParseOutcome::Options(o) => o,
            _ => panic!("expected Options for {args:?}"),
        }
    }

    #[test]
    fn help_flag_wins_regardless_of_position() {
        assert!(matches!(
            parse_args(strs(&["convert", "a.docx", "--help"])).unwrap(),
            ParseOutcome::Help
        ));
        assert!(matches!(
            parse_args(strs(&["--help"])).unwrap(),
            ParseOutcome::Help
        ));
        assert!(matches!(
            parse_args(strs(&["-h"])).unwrap(),
            ParseOutcome::Help
        ));
    }

    #[test]
    fn version_flag_is_recognised() {
        assert!(matches!(
            parse_args(strs(&["--version"])).unwrap(),
            ParseOutcome::Version
        ));
        assert!(matches!(
            parse_args(strs(&["-V"])).unwrap(),
            ParseOutcome::Version
        ));
    }

    #[test]
    fn bare_invocation_has_no_mode_and_no_files() {
        let o = expect_options(&[]);
        assert_eq!(o.mode, None);
        assert!(o.files.is_empty());
    }

    #[test]
    fn convert_and_check_are_recognised_with_their_files() {
        let o = expect_options(&["convert", "a.docx", "b.docx"]);
        assert_eq!(o.mode, Some(Mode::Convert));
        assert_eq!(
            o.files,
            vec![PathBuf::from("a.docx"), PathBuf::from("b.docx")]
        );

        let o = expect_options(&["check", "a.docx"]);
        assert_eq!(o.mode, Some(Mode::Check));
    }

    #[test]
    fn a_second_verb_is_treated_as_a_file_name() {
        // "convert" is only special the first time; a document could really
        // be named "check.docx", but a bare second verb word is unlikely —
        // still, the parser must not silently drop it.
        let o = expect_options(&["convert", "check"]);
        assert_eq!(o.mode, Some(Mode::Convert));
        assert_eq!(o.files, vec![PathBuf::from("check")]);
    }

    #[test]
    fn flags_are_all_recognised() {
        let o = expect_options(&[
            "convert",
            "a.docx",
            "--font",
            "Kalpurush",
            "--in-place",
            "--force",
            "--quiet",
            "--jobs",
            "4",
            "--theme",
            "dark",
        ]);
        assert_eq!(o.font, "Kalpurush");
        assert!(o.in_place);
        assert!(o.force);
        assert_eq!(o.verbosity, Verbosity::Quiet);
        assert_eq!(o.jobs, 4);
        assert_eq!(o.theme_flag, Some(ThemeFlag::Dark));
    }

    #[test]
    fn default_font_and_jobs_and_verbosity() {
        let o = expect_options(&["convert", "a.docx"]);
        assert_eq!(o.font, "Nirmala UI");
        assert_eq!(o.jobs, 1);
        assert_eq!(o.verbosity, Verbosity::Normal);
        assert_eq!(o.theme_flag, None);
    }

    #[test]
    fn out_names_the_single_output_file() {
        let o = expect_options(&["convert", "a.docx", "--out", "b.docx"]);
        assert_eq!(o.out, Some(PathBuf::from("b.docx")));
    }

    #[test]
    fn out_with_several_files_is_an_error() {
        let err =
            parse_args(strs(&["convert", "a.docx", "b.docx", "--out", "c.docx"])).unwrap_err();
        assert!(err.contains('2'), "{err}");
    }

    #[test]
    fn missing_values_are_reported_plainly() {
        assert!(parse_args(strs(&["convert", "a.docx", "--font"])).is_err());
        assert!(parse_args(strs(&["convert", "a.docx", "--out"])).is_err());
        assert!(parse_args(strs(&["convert", "a.docx", "--jobs"])).is_err());
        assert!(parse_args(strs(&["convert", "a.docx", "--theme"])).is_err());
    }

    #[test]
    fn jobs_must_be_a_positive_number() {
        assert!(parse_args(strs(&["convert", "a.docx", "--jobs", "0"])).is_err());
        assert!(parse_args(strs(&["convert", "a.docx", "--jobs", "abc"])).is_err());
        assert!(parse_args(strs(&["convert", "a.docx", "--jobs", "-3"])).is_err());
    }

    #[test]
    fn bad_theme_value_is_rejected() {
        assert!(parse_args(strs(&["convert", "a.docx", "--theme", "purple"])).is_err());
    }

    #[test]
    fn unknown_option_is_reported_by_name() {
        let err = parse_args(strs(&["convert", "a.docx", "--recursive"])).unwrap_err();
        assert!(err.contains("--recursive"), "{err}");
    }
}
