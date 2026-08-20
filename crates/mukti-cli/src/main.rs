//! Mukti by GRU953 on the command line.
//!
//! Converts legacy Bijoy/SutonnyMJ Bangla into Unicode, **word by word**, so
//! English, numbers and Bengali that is already Unicode come through
//! exactly as they went in.
//!
//! # Two rules this tool will not break
//!
//! **It never writes over a file unless asked to.** The default is a new
//! file beside the original, and if that new name is already taken the run
//! stops rather than destroying whatever was there. `--in-place` overwrites
//! the original and has to be typed; `--out` names a file, and naming it
//! counts as asking; `--force` allows the derived name to be replaced.
//!
//! **It never claims to have done more than it did.** Every run says how
//! many words changed, and `check` shows that without writing anything.
//!
//! # Eight modules, one job each
//!
//! `words` holds every string a person can see, with the brand-compliance
//! tests that check all of them at once. `style` decides whether colour is
//! shown and holds the fixed palette. `options` turns argv into a typed
//! `Options`. `report` turns tallies and file names into what appears on
//! screen. `convert` does the six-format gate, the per-file conversion, and
//! the parallel run across a batch. `pathinput` turns what a person types
//! or drags into a usable path. `guided` is the conversation that runs when
//! `mukti` is typed alone. This file only dispatches between them.
//!
//! No arguments-parsing crate: the options are few enough that hand-rolled
//! parsing keeps this crate's own dependency list at one entry, the
//! converter itself.

mod convert;
mod guided;
mod options;
mod pathinput;
mod report;
mod style;
mod words;

use std::io::{self, IsTerminal, Write};
use std::process::ExitCode;

use options::{Mode, Options, ParseOutcome};
use style::{Decision, Palette, Signals, ThemeFlag};

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    dispatch(args)
}

fn dispatch(args: Vec<String>) -> ExitCode {
    let opts = match options::parse_args(args) {
        Ok(ParseOutcome::Help) => {
            print!("{}", words::HELP);
            return ExitCode::SUCCESS;
        }
        Ok(ParseOutcome::Version) => {
            println!("Mukti by GRU953 {}", env!("CARGO_PKG_VERSION"));
            return ExitCode::SUCCESS;
        }
        Ok(ParseOutcome::Options(o)) => o,
        Err(message) => {
            eprintln!("{message}");
            return ExitCode::FAILURE;
        }
    };

    let stdin_is_terminal = io::stdin().is_terminal();
    let stdout_is_terminal = io::stdout().is_terminal();
    let stderr_is_terminal = io::stderr().is_terminal();
    let can_converse = stdin_is_terminal && stdout_is_terminal;

    let stdout_decision = style::decide(&signals_for(stdout_is_terminal, opts.theme_flag));
    let stderr_decision = style::decide(&signals_for(stderr_is_terminal, opts.theme_flag));

    match (opts.mode, opts.files.is_empty()) {
        (None, true) if can_converse => run_guided(opts, stdout_decision),
        (None, true) => {
            // Not a real conversation: a script, a pipe, or CI. Printing
            // help and exiting cleanly leaves both untouched, per the plan
            // this crate was rebuilt from.
            print!("{}", words::HELP);
            ExitCode::SUCCESS
        }
        (None, false) if can_converse && opts.files.len() == 1 => {
            confirm_and_convert_one_file(opts, stdout_decision, stderr_decision)
        }
        (None, false) => {
            eprintln!("{}", words::no_verb_with_files(&opts.files[0]));
            ExitCode::FAILURE
        }
        (Some(mode), true) => {
            let verb = match mode {
                Mode::Convert => "convert",
                Mode::Check => "check",
            };
            eprintln!("{}", words::verb_with_no_files(verb));
            ExitCode::FAILURE
        }
        (Some(mode), false) => run_flag_mode(mode, &opts, stderr_decision.palette()),
    }
}

fn run_flag_mode(mode: Mode, opts: &Options, stderr_palette: Option<&Palette>) -> ExitCode {
    let result = convert::run(&opts.files, mode, opts, stderr_palette);
    if result.any_failed {
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

fn run_guided(opts: Options, stdout_decision: Decision) -> ExitCode {
    // Shown once, before the conversation starts, and nowhere else: the
    // ladder's own last step, only ever reached on a real terminal that
    // never said which background it uses.
    if let Decision::Off { hint: Some(text) } = &stdout_decision {
        println!("{} {text}", style::warning(None, words::WARNING_LABEL));
    }
    let mut stdin = io::stdin().lock();
    let mut stdout = io::stdout().lock();
    let world = guided::RealWorld;
    let outcome = guided::converse(
        &mut stdin,
        &mut stdout,
        &world,
        &opts,
        stdout_decision.palette(),
    );
    match outcome {
        guided::GuidedOutcome::Ran(result) if result.any_failed => ExitCode::FAILURE,
        _ => ExitCode::SUCCESS,
    }
}

/// What a beginner actually types: `mukti report.docx`, no verb at all.
/// Asked about directly, on a real terminal, rather than refused with a
/// message about a word ("convert") the reader never had reason to know.
fn confirm_and_convert_one_file(
    opts: Options,
    stdout_decision: Decision,
    stderr_decision: Decision,
) -> ExitCode {
    let path = opts.files[0].clone();
    let palette = stdout_decision.palette();
    let question = words::confirm_bare_file(&path);
    print!("{} ", style::accent(palette, &format!("{question} [y/n]")));
    let _ = io::stdout().flush();

    let mut line = String::new();
    if io::stdin().read_line(&mut line).is_err() {
        return ExitCode::SUCCESS;
    }
    match line.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" => run_flag_mode(Mode::Convert, &opts, stderr_decision.palette()),
        _ => {
            println!("{}", words::run_cancelled());
            ExitCode::SUCCESS
        }
    }
}

fn signals_for(is_terminal: bool, theme_flag: Option<ThemeFlag>) -> Signals {
    Signals {
        theme_flag,
        no_color_set: std::env::var_os("NO_COLOR").is_some(),
        is_terminal,
        term: std::env::var("TERM").ok(),
        windows_vt_capable: windows_vt_capable(),
        colorterm: std::env::var("COLORTERM").ok(),
        mukti_theme: std::env::var("MUKTI_THEME").ok(),
        colorfgbg: std::env::var("COLORFGBG").ok(),
    }
}

/// Only consulted by `style::decide` under `cfg!(windows)`; a best-effort
/// signal since there is no VT-capability query without a dependency this
/// crate does not carry. `WT_SESSION` covers Windows Terminal; `ConEmuANSI`
/// covers ConEmu/Cmder. Neither present means "assume not capable", which
/// only costs colour, never correctness.
fn windows_vt_capable() -> bool {
    if cfg!(windows) {
        std::env::var_os("WT_SESSION").is_some()
            || std::env::var("ConEmuANSI")
                .map(|v| v == "ON")
                .unwrap_or(false)
    } else {
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn signals_for_reads_the_flag_through_regardless_of_environment() {
        let s = signals_for(true, Some(ThemeFlag::Dark));
        assert_eq!(s.theme_flag, Some(ThemeFlag::Dark));
        assert!(s.is_terminal);
    }

    #[test]
    fn helper_paths_compile_and_agree_with_options() {
        // A light smoke test that the pieces this file wires together still
        // fit — the real behavioural coverage lives in each module's own
        // tests (`options`, `convert`, `guided`, `style`, `words`).
        let path = Path::new("report.docx");
        assert!(words::confirm_bare_file(path).contains("report.docx"));
    }
}
