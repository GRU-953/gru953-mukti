//! Colour: the fixed palette, and the decision of whether to use it at all.
//!
//! Two things kept deliberately apart. `decide()` is pure — it takes the
//! signals a real terminal would give and returns a decision, with no I/O of
//! its own — so the ten-step ladder can be tested against invented signals
//! without a real terminal. `main.rs`/`options.rs` gather the real signals
//! (`std::io::IsTerminal`, environment variables) and hand them in.
//!
//! No sixth colour, no 256-colour approximation of a truecolour hue, and no
//! background-colour query over OSC 11 — each is a decision recorded in the
//! plan, not an oversight.

use std::fmt;

/// The nine roles the brand kit defines. One job each: `brand` marks the
/// wordmark line only, `accent` the question or prompt only — never a file
/// path, which stays plain and copyable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Palette {
    pub ink: &'static str,
    pub ink_muted: &'static str,
    pub ink_subtle: &'static str,
    pub brand: &'static str,
    pub accent: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub danger: &'static str,
    pub info: &'static str,
}

/// Proved against `#FFFFFF` only. Used when the terminal's background is
/// light, or assumed light for lack of any signal saying otherwise.
pub const LIGHT: Palette = Palette {
    ink: "#0B0E14",
    ink_muted: "#4D5157",
    ink_subtle: "#6A6D74",
    brand: "#1A1753",
    accent: "#B45A39",
    success: "#007131",
    warning: "#805100",
    danger: "#CE393A",
    info: "#4C4EAD",
};

/// Proved against `#0B0E14` only. A Solarized terminal, for one example,
/// gets these values because its polarity is dark — the measured ratio
/// there is unverified, which is why colour is never the only signal a
/// state is what it is.
pub const DARK: Palette = Palette {
    ink: "#F4F5F9",
    ink_muted: "#A8ACB4",
    ink_subtle: "#858990",
    brand: "#7E86F6",
    accent: "#FFAB8E",
    success: "#32AE62",
    warning: "#C88400",
    danger: "#F25855",
    info: "#7E86F6",
};

/// What `--theme` was given on the command line, if anything.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ThemeFlag {
    Light,
    Dark,
    Off,
}

impl ThemeFlag {
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "light" => Some(Self::Light),
            "dark" => Some(Self::Dark),
            "off" => Some(Self::Off),
            _ => None,
        }
    }
}

/// Every signal the ten-step ladder reads, gathered once so `decide()` stays
/// pure. A field left at its "no signal" value (`None`, or `false` for the
/// booleans) means that source said nothing, not that it said no.
#[derive(Clone, Debug, Default)]
pub struct Signals {
    pub theme_flag: Option<ThemeFlag>,
    pub no_color_set: bool,
    pub is_terminal: bool,
    pub term: Option<String>,
    pub windows_vt_capable: bool,
    pub colorterm: Option<String>,
    pub mukti_theme: Option<String>,
    pub colorfgbg: Option<String>,
}

/// The outcome of the ladder: either no colour (with a one-line hint on how
/// to turn it on), or a specific, chosen palette.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    Off { hint: Option<&'static str> },
    On(Palette),
}

impl Decision {
    pub fn palette(&self) -> Option<&Palette> {
        match self {
            Decision::On(p) => Some(p),
            Decision::Off { .. } => None,
        }
    }
}

/// The ten-step ladder, first match wins. Order matters: a capability check
/// (can colour be shown at all) comes before a polarity check (which colour
/// should it be), so a terminal that cannot show colour never gets asked
/// which shade it prefers.
pub fn decide(signals: &Signals) -> Decision {
    // 1. --theme off
    if signals.theme_flag == Some(ThemeFlag::Off) {
        return Decision::Off { hint: None };
    }
    // 2. NO_COLOR (https://no-color.org — any value, including empty, counts)
    if signals.no_color_set {
        return Decision::Off { hint: None };
    }
    // 3. stdout is not a terminal
    if !signals.is_terminal {
        return Decision::Off { hint: None };
    }
    // 4. TERM unset or dumb
    match signals.term.as_deref() {
        None | Some("") | Some("dumb") => return Decision::Off { hint: None },
        _ => {}
    }
    // 5. Windows without a VT-capable host
    if cfg!(windows) && !signals.windows_vt_capable {
        return Decision::Off { hint: None };
    }
    // 6. COLORTERM not truecolor
    match signals.colorterm.as_deref() {
        Some("truecolor") | Some("24bit") => {}
        _ => return Decision::Off { hint: None },
    }
    // 7. --theme light|dark
    match signals.theme_flag {
        Some(ThemeFlag::Light) => return Decision::On(LIGHT),
        Some(ThemeFlag::Dark) => return Decision::On(DARK),
        _ => {}
    }
    // 8. MUKTI_THEME
    match signals.mukti_theme.as_deref() {
        Some("light") => return Decision::On(LIGHT),
        Some("dark") => return Decision::On(DARK),
        _ => {}
    }
    // 9. COLORFGBG — read only the background field, the last one, since
    // that is what decides polarity; the foreground tells us nothing here.
    if let Some(raw) = &signals.colorfgbg {
        if let Some(polarity) = polarity_from_colorfgbg(raw) {
            return Decision::On(match polarity {
                Polarity::Light => LIGHT,
                Polarity::Dark => DARK,
            });
        }
    }
    // 10. No signal said which. Guessing wrong is worse than saying nothing,
    // so colour stays off, with a one-line way to turn it on by hand. The
    // sentence itself lives in `words.rs`, so it is swept by the brand
    // tests there rather than escaping them by sitting in this file.
    Decision::Off {
        hint: Some(crate::words::colour_off_hint()),
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Polarity {
    Light,
    Dark,
}

/// `COLORFGBG` conventionally holds `"<foreground>;<background>"`, sometimes
/// with a third legacy field ignored here. Background codes 0–6 and 8 read
/// as dark; 7 and 9–15 read as light — the same split terminals themselves
/// have used for decades to decide whether "bright" means white-on-black or
/// black-on-white.
fn polarity_from_colorfgbg(raw: &str) -> Option<Polarity> {
    let background = raw.split(';').next_back()?.trim();
    let code: u8 = background.parse().ok()?;
    match code {
        0..=6 | 8 => Some(Polarity::Dark),
        7 | 9..=15 => Some(Polarity::Light),
        _ => None,
    }
}

/// One coloured role, for a string that must stay plain when colour is off
/// (`palette` is `None`) or wrapped in true-colour ANSI when it is on. The
/// reset lives in the very same `format!` that sets the colour, so a run
/// interrupted mid-write can never leave the terminal tinted.
fn paint(palette: Option<&Palette>, hex: &'static str, text: &str) -> String {
    match palette {
        None => text.to_owned(),
        Some(_) => {
            let (r, g, b) = hex_to_rgb(hex);
            format!("\x1b[38;2;{r};{g};{b}m{text}\x1b[0m")
        }
    }
}

pub fn brand(palette: Option<&Palette>, text: &str) -> String {
    paint(palette, palette.map_or(LIGHT.brand, |p| p.brand), text)
}

pub fn accent(palette: Option<&Palette>, text: &str) -> String {
    paint(palette, palette.map_or(LIGHT.accent, |p| p.accent), text)
}

pub fn success(palette: Option<&Palette>, text: &str) -> String {
    paint(palette, palette.map_or(LIGHT.success, |p| p.success), text)
}

pub fn warning(palette: Option<&Palette>, text: &str) -> String {
    paint(palette, palette.map_or(LIGHT.warning, |p| p.warning), text)
}

pub fn danger(palette: Option<&Palette>, text: &str) -> String {
    paint(palette, palette.map_or(LIGHT.danger, |p| p.danger), text)
}

pub fn info(palette: Option<&Palette>, text: &str) -> String {
    paint(palette, palette.map_or(LIGHT.info, |p| p.info), text)
}

fn hex_to_rgb(hex: &str) -> (u8, u8, u8) {
    let hex = hex.trim_start_matches('#');
    let r = u8::from_str_radix(&hex[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&hex[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&hex[4..6], 16).unwrap_or(0);
    (r, g, b)
}

impl fmt::Display for ThemeFlag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            ThemeFlag::Light => "light",
            ThemeFlag::Dark => "dark",
            ThemeFlag::Off => "off",
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base() -> Signals {
        Signals {
            theme_flag: None,
            no_color_set: false,
            is_terminal: true,
            term: Some("xterm-256color".to_owned()),
            windows_vt_capable: true,
            colorterm: Some("truecolor".to_owned()),
            mukti_theme: None,
            colorfgbg: None,
        }
    }

    #[test]
    fn all_eighteen_hex_values_are_well_formed_and_exact() {
        // 9 roles × 2 palettes = 18, matching the plan's palette table.
        let expected_light = [
            ("ink", "#0B0E14"),
            ("ink_muted", "#4D5157"),
            ("ink_subtle", "#6A6D74"),
            ("brand", "#1A1753"),
            ("accent", "#B45A39"),
            ("success", "#007131"),
            ("warning", "#805100"),
            ("danger", "#CE393A"),
            ("info", "#4C4EAD"),
        ];
        let expected_dark = [
            ("ink", "#F4F5F9"),
            ("ink_muted", "#A8ACB4"),
            ("ink_subtle", "#858990"),
            ("brand", "#7E86F6"),
            ("accent", "#FFAB8E"),
            ("success", "#32AE62"),
            ("warning", "#C88400"),
            ("danger", "#F25855"),
            ("info", "#7E86F6"),
        ];
        let actual_light = [
            ("ink", LIGHT.ink),
            ("ink_muted", LIGHT.ink_muted),
            ("ink_subtle", LIGHT.ink_subtle),
            ("brand", LIGHT.brand),
            ("accent", LIGHT.accent),
            ("success", LIGHT.success),
            ("warning", LIGHT.warning),
            ("danger", LIGHT.danger),
            ("info", LIGHT.info),
        ];
        let actual_dark = [
            ("ink", DARK.ink),
            ("ink_muted", DARK.ink_muted),
            ("ink_subtle", DARK.ink_subtle),
            ("brand", DARK.brand),
            ("accent", DARK.accent),
            ("success", DARK.success),
            ("warning", DARK.warning),
            ("danger", DARK.danger),
            ("info", DARK.info),
        ];
        assert_eq!(actual_light, expected_light);
        assert_eq!(actual_dark, expected_dark);
        for (_, hex) in expected_light.iter().chain(expected_dark.iter()) {
            assert_eq!(hex.len(), 7, "{hex} is not #RRGGBB");
            assert!(hex.starts_with('#'));
            assert!(
                u32::from_str_radix(&hex[1..], 16).is_ok(),
                "{hex} is not valid hex"
            );
        }
    }

    #[test]
    fn theme_off_flag_wins_over_everything() {
        let mut s = base();
        s.theme_flag = Some(ThemeFlag::Off);
        s.no_color_set = false;
        assert_eq!(decide(&s), Decision::Off { hint: None });
    }

    #[test]
    fn no_color_env_turns_colour_off_even_in_a_real_terminal() {
        let mut s = base();
        s.no_color_set = true;
        assert_eq!(decide(&s), Decision::Off { hint: None });
    }

    #[test]
    fn piped_output_gets_no_colour() {
        let mut s = base();
        s.is_terminal = false;
        assert_eq!(decide(&s), Decision::Off { hint: None });
    }

    #[test]
    fn dumb_term_gets_no_colour() {
        let mut s = base();
        s.term = Some("dumb".to_owned());
        assert_eq!(decide(&s), Decision::Off { hint: None });
        s.term = None;
        assert_eq!(decide(&s), Decision::Off { hint: None });
    }

    #[test]
    fn no_truecolor_gets_no_colour_even_with_256_colours_available() {
        let mut s = base();
        s.colorterm = Some("256".to_owned());
        assert_eq!(decide(&s), Decision::Off { hint: None });
        s.colorterm = None;
        assert_eq!(decide(&s), Decision::Off { hint: None });
    }

    #[test]
    fn explicit_theme_flag_picks_the_palette() {
        let mut s = base();
        s.theme_flag = Some(ThemeFlag::Dark);
        assert_eq!(decide(&s), Decision::On(DARK));
        s.theme_flag = Some(ThemeFlag::Light);
        assert_eq!(decide(&s), Decision::On(LIGHT));
    }

    #[test]
    fn mukti_theme_env_picks_the_palette_when_no_flag_given() {
        let mut s = base();
        s.mukti_theme = Some("dark".to_owned());
        assert_eq!(decide(&s), Decision::On(DARK));
    }

    #[test]
    fn colorfgbg_dark_background_picks_dark_palette() {
        let mut s = base();
        s.colorfgbg = Some("15;0".to_owned());
        assert_eq!(decide(&s), Decision::On(DARK));
    }

    #[test]
    fn colorfgbg_light_background_picks_light_palette() {
        let mut s = base();
        s.colorfgbg = Some("0;15".to_owned());
        assert_eq!(decide(&s), Decision::On(LIGHT));
    }

    #[test]
    fn no_polarity_signal_at_all_gives_no_colour_with_a_hint() {
        let s = base();
        match decide(&s) {
            Decision::Off { hint: Some(text) } => {
                assert!(text.contains("mukti --theme dark"));
            }
            other => panic!("expected Off with a hint, got {other:?}"),
        }
    }

    #[test]
    fn paint_functions_are_plain_text_when_colour_is_off() {
        assert_eq!(brand(None, "Mukti"), "Mukti");
        assert_eq!(danger(None, "boom"), "boom");
    }

    #[test]
    fn paint_functions_reset_within_the_same_string_when_colour_is_on() {
        let out = brand(Some(&DARK), "Mukti");
        assert!(out.starts_with("\x1b[38;2;"));
        assert!(out.ends_with("\x1b[0m"));
        assert!(out.contains("Mukti"));
    }

    #[test]
    fn hex_to_rgb_reads_every_channel_correctly() {
        assert_eq!(hex_to_rgb("#1A1753"), (0x1A, 0x17, 0x53));
        assert_eq!(hex_to_rgb("#FFAB8E"), (0xFF, 0xAB, 0x8E));
    }
}
