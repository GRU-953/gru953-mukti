//! Turning what a person types, or drags in, into a usable path.
//!
//! Every rule here answers one real habit rather than a hypothetical one:
//! macOS's Terminal appends a trailing space after a dragged-in path; a
//! path with a space in it is often typed inside quotes; a dragged-in path
//! on some terminals arrives as a `file://` URL; `~` means home. `home` and
//! `platform` are parameters rather than reads of the real environment, so
//! both platforms' behaviour can be tested on one runner with no
//! `env::set_var`.

use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Platform {
    Unix,
    Windows,
}

/// Normalises one line of guided-mode input into a path, in the order the
/// habits above actually compose: trim the trailing space first (it is
/// outside any quoting), then a `file://` URL, then one matching pair of
/// quotes, then backslash-escaping — Unix only, since on Windows a
/// backslash is the path separator itself and must never be touched — and
/// finally `~` expansion, which has to run last so it sees the unquoted,
/// unescaped text.
pub fn normalise(raw: &str, home: &Path, platform: Platform) -> PathBuf {
    let mut s = raw.trim().to_owned();

    if let Some(rest) = s.strip_prefix("file://") {
        s = percent_decode(rest);
    }

    s = strip_one_matching_quote_pair(&s);

    if platform == Platform::Unix {
        s = unescape_unix_metacharacters(&s);
    }

    if s == "~" {
        return home.to_path_buf();
    }
    if let Some(rest) = s.strip_prefix("~/") {
        return home.join(rest);
    }

    PathBuf::from(s)
}

fn strip_one_matching_quote_pair(s: &str) -> String {
    let bytes = s.as_bytes();
    if bytes.len() >= 2 {
        let first = bytes[0];
        let last = bytes[bytes.len() - 1];
        if (first == b'"' && last == b'"') || (first == b'\'' && last == b'\'') {
            return s[1..s.len() - 1].to_owned();
        }
    }
    s.to_owned()
}

/// A drag-and-drop path from a Unix shell arrives with spaces and other
/// shell metacharacters backslash-escaped, e.g. `My\ Documents`. Swallowing
/// the backslash and keeping the character it protected undoes exactly
/// that, and nothing else.
fn unescape_unix_metacharacters(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\\' {
            if let Some(&next) = chars.peek() {
                out.push(next);
                chars.next();
                continue;
            }
        }
        out.push(c);
    }
    out
}

/// The one escape a `file://` URL is likely to carry from a dragged folder
/// name: an ordinary space. Not a general percent-decoder — this tool never
/// receives one from anywhere but a terminal's own drag-and-drop feature,
/// and that is the one sequence they are observed to produce.
fn percent_decode(s: &str) -> String {
    s.replace("%20", " ")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn home() -> PathBuf {
        PathBuf::from("/Users/example")
    }

    #[test]
    fn trailing_drag_and_drop_space_is_trimmed() {
        assert_eq!(
            normalise("/Users/example/Documents ", &home(), Platform::Unix),
            PathBuf::from("/Users/example/Documents")
        );
    }

    #[test]
    fn a_quoted_path_with_a_space_is_unwrapped() {
        assert_eq!(
            normalise("\"/Users/example/My Documents\"", &home(), Platform::Unix),
            PathBuf::from("/Users/example/My Documents")
        );
        assert_eq!(
            normalise("'/Users/example/My Documents'", &home(), Platform::Unix),
            PathBuf::from("/Users/example/My Documents")
        );
    }

    #[test]
    fn unix_backslash_escaped_spaces_are_unescaped() {
        assert_eq!(
            normalise("/Users/example/My\\ Documents", &home(), Platform::Unix),
            PathBuf::from("/Users/example/My Documents")
        );
    }

    #[test]
    fn windows_backslashes_are_never_touched() {
        assert_eq!(
            normalise("C:\\Users\\example\\Documents", &home(), Platform::Windows),
            PathBuf::from("C:\\Users\\example\\Documents")
        );
    }

    #[test]
    fn tilde_alone_means_home() {
        assert_eq!(normalise("~", &home(), Platform::Unix), home());
    }

    #[test]
    fn tilde_slash_joins_onto_home() {
        assert_eq!(
            normalise("~/Documents", &home(), Platform::Unix),
            home().join("Documents")
        );
    }

    #[test]
    fn a_file_url_is_turned_into_a_plain_path() {
        assert_eq!(
            normalise(
                "file:///Users/example/My%20Documents",
                &home(),
                Platform::Unix
            ),
            PathBuf::from("/Users/example/My Documents")
        );
    }

    #[test]
    fn an_ordinary_unquoted_path_passes_through() {
        assert_eq!(
            normalise("/Users/example/reports", &home(), Platform::Unix),
            PathBuf::from("/Users/example/reports")
        );
    }
}
