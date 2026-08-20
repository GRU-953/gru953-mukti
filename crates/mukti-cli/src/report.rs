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
    /// Words that were already Unicode Bengali and had a two-part vowel sign
    /// joined into the single character Unicode says it is.
    ///
    /// Counted apart from `converted` because it is not a conversion — no
    /// legacy text was involved. It has been computed since 0.7.0 and printed
    /// nowhere, which meant the one edit Mukti makes to text it otherwise
    /// promises to leave alone happened silently. It is reported now.
    pub normalised: usize,
}

impl Tally {
    pub fn add(&mut self, other: Tally) {
        self.converted += other.converted;
        self.untouched += other.untouched;
        self.normalised += other.normalised;
    }

    pub fn describe(&self, mode: Mode) -> String {
        crate::words::tally_sentence(
            self.converted,
            self.converted + self.untouched,
            self.untouched,
            mode == Mode::Check,
        )
    }

    /// The extra line about joined vowel signs, or `None` when there were
    /// none — which is the common case, and printing "0 words" every time
    /// would bury the line that matters.
    pub fn normalisation_note(&self, mode: Mode) -> Option<String> {
        if self.normalised == 0 {
            return None;
        }
        Some(crate::words::normalisation_note(
            self.normalised,
            mode == Mode::Check,
        ))
    }
}

/// Today's date as `YYYY-MM-DD`, from a `SystemTime`.
///
/// Hand-rolled, and that is the interesting part: 0.9.0 removed three separate
/// date/time crates (`chrono`, `jiff`, `time`) along with the PDF reader that
/// dragged them in, and putting one back to name a folder would undo a good
/// chunk of that. `SystemTime` is std, and the civil-date arithmetic below is
/// Howard Hinnant's `civil_from_days`, which is exact for every date in the
/// proleptic Gregorian calendar and needs no table.
///
/// UTC, deliberately, with no attempt at a local timezone: std cannot read one
/// without a dependency, and a folder named a few hours off is a smaller
/// problem than a wrong dependency or a guess presented as local time.
pub fn date_stamp(now: std::time::SystemTime) -> String {
    let secs = now
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let days = secs.div_euclid(86_400);

    // Shift the epoch to 1 March 0000 so a leap day lands at the END of the
    // year being counted, which is what removes every special case.
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097); // day of era, 0..=146096
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365; // 0..=399
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // 0..=365, from 1 March
    let mp = (5 * doy + 2) / 153; // 0..=11, March-based month
    let d = doy - (153 * mp + 2) / 5 + 1; // 1..=31
    let m = if mp < 10 { mp + 3 } else { mp - 9 }; // 1..=12
    let y = if m <= 2 { y + 1 } else { y };

    format!("{y:04}-{m:02}-{d:02}")
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
    fn date_stamp_matches_known_dates() {
        use std::time::{Duration, UNIX_EPOCH};
        let at = |secs: u64| date_stamp(UNIX_EPOCH + Duration::from_secs(secs));
        assert_eq!(at(0), "1970-01-01");
        assert_eq!(at(86_399), "1970-01-01", "the last second of the day");
        assert_eq!(at(86_400), "1970-01-02");
        // A leap day, and the day after it, in a year divisible by 4.
        assert_eq!(at(1_709_164_800), "2024-02-29");
        assert_eq!(at(1_709_251_200), "2024-03-01");
        // 2000 was a leap year (divisible by 400); 1900 was not. Both are the
        // cases a naive rule gets wrong.
        assert_eq!(at(951_782_400), "2000-02-29");
        // The date this was written, as a plain sanity anchor. Every value in
        // this test was cross-checked against an independent implementation
        // rather than against the function under test -- this anchor was a day
        // out on the first attempt, and only that cross-check caught it.
        assert_eq!(at(1_787_184_000), "2026-08-20");
        assert_eq!(at(1_787_270_400), "2026-08-21");
    }

    #[test]
    fn date_stamp_is_sortable_and_fixed_width() {
        use std::time::{Duration, UNIX_EPOCH};
        let mut previous = String::new();
        // One sample a week for twenty years: the string form must sort in the
        // same order as the instants, which is the only property the folder
        // name actually relies on.
        for week in 0..(52 * 20) {
            let s = date_stamp(UNIX_EPOCH + Duration::from_secs(week * 7 * 86_400));
            assert_eq!(s.len(), 10, "{s} is not YYYY-MM-DD");
            assert!(s > previous, "{s} does not sort after {previous}");
            previous = s;
        }
    }

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
            normalised: 0,
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
            normalised: 2,
        });
        total.add(Tally {
            converted: 2,
            untouched: 1,
            normalised: 5,
        });
        assert_eq!(
            total,
            Tally {
                converted: 5,
                untouched: 8,
                normalised: 7
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
