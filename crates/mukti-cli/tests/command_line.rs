//! The command as a person actually meets it: the real binary, run.
//!
//! Every other test in this crate exercises a function. This one spawns
//! `mukti` itself, so it covers the things only a real process has — the exit
//! code, which stream each line goes to, and the fact that the pieces are
//! wired together at all. A crate can be perfectly unit-tested and still ship
//! a binary that does nothing, and this project has shipped exactly that
//! before: v0.4.0's window opened and responded to nothing, because no test
//! ever started the thing a user starts.
//!
//! `env!("CARGO_BIN_EXE_mukti")` is Cargo's own path to the binary it just
//! built, so this needs no dev-dependency and cannot test a stale copy.

use std::path::PathBuf;
use std::process::{Command, Output};

fn mukti(args: &[&str]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_mukti"))
        .args(args)
        // stdin is not a terminal here, so guided mode never starts and the
        // no-argument case prints help instead of waiting for an answer --
        // which is the behaviour scripts and CI depend on.
        .output()
        .expect("the mukti binary should be runnable")
}

fn stdout_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stdout).into_owned()
}

fn stderr_of(out: &Output) -> String {
    String::from_utf8_lossy(&out.stderr).into_owned()
}

#[test]
fn version_prints_the_name_and_number_and_exits_zero() {
    let out = mukti(&["--version"]);
    assert!(out.status.success(), "--version must exit 0");
    let text = stdout_of(&out);
    assert!(
        text.starts_with("Mukti by GRU953 "),
        "the wordmark is wrong: {text:?}"
    );
    assert!(
        text.contains(env!("CARGO_PKG_VERSION")),
        "the version printed is not this build's: {text:?}"
    );
    assert!(
        !text.contains("GRU953 Mukti"),
        "the retired prefix form is back: {text:?}"
    );
}

#[test]
fn help_lists_the_six_formats_and_exits_zero() {
    let out = mukti(&["--help"]);
    assert!(out.status.success(), "--help must exit 0");
    let text = stdout_of(&out);
    for ext in [".docx", ".xlsx", ".pptx", ".doc", ".xls", ".ppt"] {
        assert!(text.contains(ext), "{ext} missing from --help");
    }
}

/// Piped or scripted, with no arguments, Mukti prints help and exits 0 rather
/// than starting a conversation nothing can answer. This is the guard that
/// keeps guided mode from breaking every script that ever ran `mukti`.
#[test]
fn no_arguments_without_a_terminal_prints_help_and_exits_zero() {
    let out = mukti(&[]);
    assert!(
        out.status.success(),
        "a bare `mukti` in a script must exit 0, not hang or fail"
    );
    assert!(
        stdout_of(&out).contains("Run mukti on its own"),
        "help was not printed"
    );
}

#[test]
fn an_unsupported_file_is_refused_on_stderr_with_a_failing_exit_code() {
    let out = mukti(&["convert", "no-such-thing.jpg"]);
    assert!(
        !out.status.success(),
        "a refusal must not report success to a script"
    );
    // The refusal names the file and goes to stderr, so `mukti convert *.docx
    // > log.txt` keeps the log clean and still shows the problem on screen.
    assert!(
        stderr_of(&out).contains("no-such-thing.jpg"),
        "the refusal does not name the file: {:?}",
        stderr_of(&out)
    );
    assert!(
        stdout_of(&out).is_empty(),
        "a refusal wrote to stdout: {:?}",
        stdout_of(&out)
    );
}

#[test]
fn a_missing_file_with_a_supported_extension_fails_without_panicking() {
    let out = mukti(&["convert", "definitely-not-here.docx"]);
    assert!(!out.status.success());
    let text = stderr_of(&out);
    assert!(
        !text.contains("panicked"),
        "the binary panicked instead of explaining itself: {text:?}"
    );
    assert!(
        text.contains("definitely-not-here.docx"),
        "the error does not name the file: {text:?}"
    );
}

#[test]
fn an_unknown_option_is_named_and_points_at_help() {
    let out = mukti(&["convert", "a.docx", "--recursive"]);
    assert!(!out.status.success());
    let text = stderr_of(&out);
    assert!(text.contains("--recursive"), "{text:?}");
    assert!(
        text.contains("--help"),
        "the way to find the real options is not offered: {text:?}"
    );
    // The whole usage block used to be dumped under every error, which buried
    // a one-line problem under thirty lines of options.
    assert!(
        !text.contains("Files Mukti can open"),
        "the full usage block is being dumped under the error again"
    );
}

/// `check` must never write. Asserted against a real directory listing before
/// and after, not against the absence of a message.
#[test]
fn check_writes_nothing_at_all() {
    let dir = std::env::temp_dir().join("mukti-integration-check-writes-nothing");
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("make the scratch directory");
    let input = dir.join("not-a-real-document.docx");
    std::fs::write(&input, b"this is not a Word file").expect("write the fixture");

    let before = listing(&dir);
    let out = Command::new(env!("CARGO_BIN_EXE_mukti"))
        .arg("check")
        .arg(&input)
        .output()
        .expect("run mukti");
    let after = listing(&dir);

    assert_eq!(
        before, after,
        "`check` changed the directory; it must only ever read"
    );
    // Garbage bytes cannot be checked successfully, so this also proves the
    // failure path writes nothing either.
    assert!(!out.status.success());

    let _ = std::fs::remove_dir_all(&dir);
}

fn listing(dir: &PathBuf) -> Vec<String> {
    let mut names: Vec<String> = std::fs::read_dir(dir)
        .expect("read the scratch directory")
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    names.sort();
    names
}
