//! Turning tallies and file names into what appears on screen: number
//! formatting, a defence against a file name that tries to abuse the
//! terminal, word wrap, and the single progress line guided mode redraws in
//! place.

use std::path::Path;
use std::time::{Duration, Instant};

use crate::options::Mode;

/// How many words changed, and how many did not, across one file or a
/// whole run.
#[derive(Default, Clone, Copy, Debug, PartialEq, Eq)]
pub struct Tally {
    pub converted: usize,
    pub untouched: usize,
}

impl Tally {
    pub fn add(&mut self, other: Tally) {
        self.converted += other.converted;
        self.untouched += other.untouched;
    }

    pub fn describe(&self, mode: Mode) -> String {
        let verb = match mode {
            Mode::Convert => "converted",
            Mode::Check => "would be converted",
        };
        format!(
            "{} of {} words {verb}; {} left exactly as they were.",
            group_thousands(self.converted),
            group_thousands(self.converted + self.untouched),
            group_thousands(self.untouched)
        )
    }
}

/// `12345` becomes `"12,345"`. Latin numerals throughout, per the brand's
/// own rule, so this never has to weigh a locale.
pub fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}

/// A file name is data a stranger chose, not something Mukti wrote — and a
/// name holding a raw escape or control byte could otherwise repaint the
/// terminal, hide part of itself, or impersonate Mukti's own output. Every
/// non-printable character becomes its Unicode "control picture" glyph
/// (`U+2400` onward), so the name still prints as something, safely,
/// rather than as a live control sequence.
pub fn safe_for_screen(text: &str) -> String {
    text.chars()
        .map(|c| {
            if c == '\u{7f}' {
                '\u{2421}'
            } else if (c as u32) < 0x20 {
                char::from_u32(0x2400 + c as u32).unwrap_or('\u{fffd}')
            } else {
                c
            }
        })
        .collect()
}

/// [`safe_for_screen`], specialised to the one type that needs it at every
/// call site: a path. Sanitising has to happen here, at the point an
/// untrusted path first becomes displayable text, and never at the far end
/// where a finished line is printed — by then it may already carry this
/// crate's own, entirely intentional, colour codes, and sanitising a
/// second time would corrupt those rather than any file name.
pub fn show_path(path: &Path) -> String {
    safe_for_screen(&path.display().to_string())
}

/// Wraps prose to `width` columns on word boundaries, never splitting a
/// word. Used for the handful of longer sentences guided mode prints, in
/// case a beginner's terminal window is narrower than the text.
pub fn wrap(text: &str, width: usize) -> String {
    let mut lines: Vec<String> = Vec::new();
    for paragraph in text.split('\n') {
        if paragraph.is_empty() {
            lines.push(String::new());
            continue;
        }
        let mut line = String::new();
        for word in paragraph.split(' ') {
            if line.is_empty() {
                line.push_str(word);
            } else if line.chars().count() + 1 + word.chars().count() <= width {
                line.push(' ');
                line.push_str(word);
            } else {
                lines.push(std::mem::take(&mut line));
                line.push_str(word);
            }
        }
        lines.push(line);
    }
    lines.join("\n")
}

/// The single line guided mode redraws in place while converting a batch:
/// capped at 60 characters, and redrawn no more often than every 80 ms so a
/// fast run over small files does not spend more time drawing than
/// converting.
pub struct Progress {
    last_drawn: Option<Instant>,
    min_interval: Duration,
}

impl Progress {
    pub fn new() -> Self {
        Progress {
            last_drawn: None,
            min_interval: Duration::from_millis(80),
        }
    }

    /// Whether enough time has passed, as of `now`, that a redraw should
    /// happen. Takes `now` explicitly, rather than reading the clock
    /// itself, so tests can advance time without sleeping.
    pub fn should_draw(&mut self, now: Instant) -> bool {
        let due = match self.last_drawn {
            None => true,
            Some(last) => now.duration_since(last) >= self.min_interval,
        };
        if due {
            self.last_drawn = Some(now);
        }
        due
    }

    /// One line, carriage-returned rather than newlined so the next call
    /// overwrites it in place, truncated to at most 60 characters with a
    /// trailing `…` if the file's own name had to be cut short.
    pub fn line(current: &Path, done: usize, total: usize) -> String {
        const CAP: usize = 60;
        let name = safe_for_screen(&current.display().to_string());
        let prefix = format!("[{done}/{total}] ");
        let budget = CAP.saturating_sub(prefix.chars().count());
        let shown = if name.chars().count() > budget && budget > 1 {
            let truncated: String = name.chars().take(budget - 1).collect();
            format!("{truncated}…")
        } else {
            name
        };
        format!("\r{prefix}{shown}")
    }
}

impl Default for Progress {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn group_thousands_places_commas_correctly() {
        assert_eq!(group_thousands(0), "0");
        assert_eq!(group_thousands(7), "7");
        assert_eq!(group_thousands(999), "999");
        assert_eq!(group_thousands(1000), "1,000");
        assert_eq!(group_thousands(12345), "12,345");
        assert_eq!(group_thousands(1234567), "1,234,567");
    }

    #[test]
    fn tally_describe_uses_grouped_numbers() {
        let t = Tally {
            converted: 1234,
            untouched: 6,
        };
        assert_eq!(
            t.describe(Mode::Convert),
            "1,234 of 1,240 words converted; 6 left exactly as they were."
        );
        assert_eq!(
            t.describe(Mode::Check),
            "1,234 of 1,240 words would be converted; 6 left exactly as they were."
        );
    }

    #[test]
    fn tally_add_accumulates_both_fields() {
        let mut total = Tally::default();
        total.add(Tally {
            converted: 3,
            untouched: 7,
        });
        total.add(Tally {
            converted: 2,
            untouched: 1,
        });
        assert_eq!(
            total,
            Tally {
                converted: 5,
                untouched: 8
            }
        );
    }

    #[test]
    fn control_characters_become_visible_pictures_not_live_sequences() {
        let hostile = "report\u{1b}[31m.docx";
        let safe = safe_for_screen(hostile);
        assert!(
            !safe.contains('\u{1b}'),
            "the raw escape byte survived: {safe:?}"
        );
        assert!(
            safe.contains('\u{241b}'),
            "no visible stand-in was substituted: {safe:?}"
        );
    }

    #[test]
    fn ordinary_text_is_untouched() {
        assert_eq!(safe_for_screen("report.docx"), "report.docx");
        assert_eq!(safe_for_screen("প্রতিবেদন.docx"), "প্রতিবেদন.docx");
    }

    #[test]
    fn wrap_breaks_only_at_word_boundaries() {
        let text = "one two three four five";
        let wrapped = wrap(text, 11);
        for line in wrapped.lines() {
            assert!(line.chars().count() <= 11, "line too long: {line:?}");
        }
        assert_eq!(wrapped.replace('\n', " "), text);
    }

    #[test]
    fn wrap_preserves_existing_newlines() {
        let text = "first line\nsecond line";
        assert_eq!(wrap(text, 80), text);
    }

    #[test]
    fn progress_line_is_capped_and_carriage_returned() {
        let long_name = Path::new("a-genuinely-very-long-report-file-name-that-does-not-fit.docx");
        let line = Progress::line(long_name, 3, 400);
        assert!(line.starts_with('\r'));
        assert!(
            line.chars().count() <= 61,
            "line: {line:?} ({}ch)",
            line.chars().count()
        );
        assert!(line.contains('…'));
    }

    #[test]
    fn progress_line_leaves_a_short_name_untruncated() {
        let line = Progress::line(Path::new("a.docx"), 1, 2);
        assert_eq!(line, "\r[1/2] a.docx");
    }

    #[test]
    fn progress_throttles_to_the_minimum_interval() {
        let mut p = Progress::new();
        let t0 = Instant::now();
        assert!(p.should_draw(t0), "the first call should always draw");
        assert!(
            !p.should_draw(t0 + Duration::from_millis(10)),
            "too soon to redraw"
        );
        assert!(
            p.should_draw(t0 + Duration::from_millis(90)),
            "enough time passed"
        );
    }
}
