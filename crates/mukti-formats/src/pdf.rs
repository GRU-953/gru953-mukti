//! Pull the legacy Bangla out of a PDF and convert it, as plain text.
//!
//! # Read-only, and why
//!
//! Everything else this crate does rewrites a document in place. A PDF cannot
//! be treated that way: Bijoy text and Unicode Bengali are different lengths
//! and different shapes, every glyph is positioned individually, and there is
//! no embedded Unicode Bengali font to draw the result with. Rewriting one
//! would mean re-laying-out the page, which is a typesetting job, not a
//! conversion. So a PDF goes in and text comes out.
//!
//! # Why this works at all, which was not obvious
//!
//! This was expected to be the hardest thing in the project and possibly
//! impossible: the usual worry is that a PDF stores glyph indexes into an
//! embedded font subset, from which no text can be recovered.
//!
//! That is not what these files do. Checked before any code was written: the
//! SutonnyMJ fonts in them declare `/Encoding /WinAnsiEncoding` and carry **no
//! `/ToUnicode` map at all**. The text operators therefore hold the original
//! Bijoy bytes, unchanged — `(Av)`, `(cwi)`, `(jU\xaav)`. And WinAnsi *is*
//! Windows-1252, the encoding [`crate::encoding`] already exists to read.
//!
//! So the bytes arrive exactly as a Bijoy `.txt` file would deliver them.
//!
//! # What is genuinely lost
//!
//! Layout. A PDF has no paragraphs, no words and no spaces — only glyphs at
//! coordinates. Spacing is inferred from the positioning operators, and that
//! inference is good rather than perfect: a wide gap becomes a space, a change
//! of line becomes a newline. Tables and columns come out as running text.
//! This is stated plainly here and in the tool's own output, because a user
//! who expects their layout back will otherwise think the conversion failed.

use lopdf::content::Content;
use lopdf::{Document, Object};

use crate::office::Summary;
use gru953_mukti::encoding::from_windows_1252;

/// How much of a gap counts as a space.
///
/// PDF measures these in thousandths of an em, negative meaning "move right".
/// Anything past this is a word gap rather than kerning. The value is the one
/// most PDF tools settle on; it is a threshold on a continuum, so it will
/// occasionally split a word or join two.
const SPACE_GAP: f64 = 180.0;

/// The fonts a PDF may be decoded as Bijoy on the strength of its name alone.
///
/// **Deliberately shorter than [`crate::office::LEGACY_FONTS`], and the
/// difference is the point.** The two lists answer different questions.
///
/// In a Word file, the font decides only whether a word is *offered* to the
/// classifier, and the classifier is the real gate — it refuses outright to
/// touch anything already containing Unicode Bengali. A font wrongly listed
/// there costs a font rename and nothing else.
///
/// In a PDF there is no such gate. The font name is the whole authority: match
/// it, and the bytes are decoded as Windows-1252 and converted with no second
/// opinion. A font wrongly listed here turns readable text into Bengali-shaped
/// nonsense, which is this project's worst failure mode.
///
/// So this list holds only fonts whose text is *known* to be Bijoy-encoded.
/// `NikoshBAN` and `Ekushey` sit in the Office list and are kept out of this one:
/// Nikosh is Bangladesh's standard **Unicode** font, and the one PDF in the
/// measurement archive that embeds `NikoshBAN` yields no Bijoy-looking text at
/// all. Ekushey is unresolved either way. Neither may be added here without
/// evidence from the font itself — LESSONS §3 is about exactly that mistake.
///
/// A test below keeps this a subset of the Office list, so the two can only ever
/// drift in the safe direction.
const CERTAIN_LEGACY_FONTS: &[&str] = &[
    // The whole Sutonny family, by the shared prefix.
    //
    // Narrowing this to `sutonnymj`/`sutonnyomj`/`sutonnyemj` was tried and
    // **measured**: it lost four real variants in the archive —
    // `SutonnyBanglaMJ`, `SutonnyBanglaMJBold`, `SutonnyUniBanglaOMJ` and
    // `SutonnySushreeMJ` — across 26 font references, and two PDFs stopped
    // converting. All four carry the `MJ`/`OMJ` suffix that marks a Bijoy
    // layout, so they belong here. The prefix is what catches the family.
    "sutonny",
    "boishakhi",
    "sulekha",
    "bornosoft",
    "chandrabati",
    // `adorsholipi` was here until 14 August 2026 and had to go. AdorshoLipi is
    // a **Unicode** Bangla family, and this list is the one place a font name
    // alone causes bytes to be converted with no second opinion — so a Unicode
    // font named here means already-correct Bengali is put through the Bijoy
    // tables. Nothing in the vendor's legacy collection of 127 families is
    // called AdorshoLipi.
    "modhumatimj",
];

/// What we can do with text drawn in a particular font.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum FontKind {
    /// A legacy Bangla font with no `/ToUnicode` map: the bytes ARE Bijoy, and
    /// the font itself is proof of it. Decode as Windows-1252 and convert.
    LegacyBijoy,
    /// An ordinary text font whose bytes are Latin. Decode and leave alone.
    PlainLatin,
    /// Anything whose bytes are glyph indexes rather than characters —
    /// a subsetted or symbolic font, or one using a `/Differences` array.
    ///
    /// **Skipped, deliberately.** Decoding these as Windows-1252 produces
    /// nonsense, and the nonsense then converts into plausible-looking Bengali
    /// that a reader cannot tell from real text. Dropping the text is bad;
    /// silently inventing Bengali is far worse.
    Unreadable,
}

fn classify_font(dictionary: &lopdf::Dictionary) -> FontKind {
    let base = dictionary
        .get(b"BaseFont")
        .ok()
        .and_then(|o| o.as_name().ok())
        .map(|n| String::from_utf8_lossy(n).to_string())
        .unwrap_or_default();
    let lower = base.to_lowercase();
    let legacy = CERTAIN_LEGACY_FONTS.iter().any(|f| lower.contains(f));

    // A simple font with a named base encoding is byte-addressed, and that is
    // the only case we can read. Anything else — a Differences array, an
    // Identity CID encoding, a symbolic subset — is glyph indexes.
    let encoding_is_simple = match dictionary.get(b"Encoding") {
        Ok(lopdf::Object::Name(name)) => {
            matches!(
                name.as_slice(),
                b"WinAnsiEncoding" | b"MacRomanEncoding" | b"StandardEncoding"
            )
        }
        // No /Encoding at all: the font's built-in encoding. For the legacy
        // Bangla fonts that is the Bijoy layout, which is what we want.
        Err(_) => true,
        _ => false,
    };

    if !encoding_is_simple {
        return FontKind::Unreadable;
    }
    if legacy {
        FontKind::LegacyBijoy
    } else {
        FontKind::PlainLatin
    }
}

/// Convert the legacy Bangla in a PDF, returning plain text.
pub fn convert_pdf_to_text(bytes: &[u8]) -> Result<(String, Summary), Box<dyn std::error::Error>> {
    let document = Document::load_mem(bytes)?;
    let mut out = String::new();
    let mut summary = Summary::default();
    let mut skipped = 0usize;

    for (number, id) in document.get_pages() {
        if !out.is_empty() {
            out.push_str("\n\n");
        }
        let _ = number;
        let mut raw = String::new();
        let mut marks: Vec<FontKind> = Vec::new();
        page_text(&document, id, &mut raw, &mut marks, &mut skipped)?;
        out.push_str(&convert_marked(&raw, &marks, &mut summary));
    }
    summary.fonts_changed = skipped;
    Ok((out, summary))
}

/// A 2-D affine transform in PDF's own order: `[a b c d e f]`.
///
/// PDF puts text where the text matrix and the current transformation matrix
/// together say, so neither alone tells you anything. A page whose transform
/// scales it to fifteen units tall — there are real ones in the measurement
/// archive — makes raw text-matrix numbers meaningless.
#[derive(Clone, Copy)]
struct Transform([f64; 6]);

impl Transform {
    const IDENTITY: Transform = Transform([1.0, 0.0, 0.0, 1.0, 0.0, 0.0]);

    /// This transform, then the other one.
    fn then(self, other: Transform) -> Transform {
        let (a, b) = (self.0, other.0);
        Transform([
            a[0] * b[0] + a[1] * b[2],
            a[0] * b[1] + a[1] * b[3],
            a[2] * b[0] + a[3] * b[2],
            a[2] * b[1] + a[3] * b[3],
            a[4] * b[0] + a[5] * b[2] + b[4],
            a[4] * b[1] + a[5] * b[3] + b[5],
        ])
    }

    /// Where the text cursor sits once transformed.
    fn position(self) -> (f64, f64) {
        (self.0[4], self.0[5])
    }

    /// How much this transform stretches vertically, and horizontally.
    fn scales(self) -> (f64, f64) {
        let a = self.0;
        (
            (a[0] * a[0] + a[1] * a[1]).sqrt(),
            (a[2] * a[2] + a[3] * a[3]).sqrt(),
        )
    }

    fn shift(x: f64, y: f64) -> Transform {
        Transform([1.0, 0.0, 0.0, 1.0, x, y])
    }
}

/// A vertical move of at least this many font sizes starts a new line.
///
/// **Measured, not chosen.** Across 385,372 consecutive text runs in 200 real
/// documents, the vertical step between one run and the next is sharply
/// bimodal: 73.4% of steps are under a tenth of a font size — 72.9% are exactly
/// zero, the same baseline — and 26.4% land between 1.0 and 1.6, which is
/// ordinary single and one-and-a-half line spacing. **Only 0.25% of steps fall
/// anywhere between 0.1 and 1.0.**
///
/// So this number sits in the middle of a valley containing a quarter of one
/// percent of the data, and moving it anywhere from 0.2 to 0.9 reclassifies at
/// most a few hundred steps out of 385,000. That insensitivity is the point:
/// the threshold is not a tuned parameter, it is a gap in the data.
const NEW_LINE_AT_FONT_SIZES: f64 = 0.5;

/// A horizontal gap of at least this many font sizes, beyond where the previous
/// run is estimated to have ended, means a space belongs between them.
///
/// Runs that continue a word — `Ultra`, `-`, `Poor` — butt up against each
/// other and must not gain a space. Two cells of a table row do not, and must.
const SPACE_AT_FONT_SIZES: f64 = 0.2;

/// A rough width per character, as a fraction of the font size.
///
/// The exact width needs every glyph's metrics from the font programme, which is
/// a great deal of parsing for a decision this coarse. Half the font size is the
/// usual approximation for mixed text, and it only has to be good enough to tell
/// "these runs touch" from "there is a gap here".
const WIDTH_PER_CHARACTER: f64 = 0.5;

/// One page's text, decoded according to the font each string is drawn in.
fn page_text(
    document: &Document,
    page: lopdf::ObjectId,
    out: &mut String,
    marks: &mut Vec<FontKind>,
    skipped: &mut usize,
) -> Result<(), Box<dyn std::error::Error>> {
    let fonts = document.get_page_fonts(page)?;
    let kinds: std::collections::BTreeMap<Vec<u8>, FontKind> = fonts
        .iter()
        .map(|(name, dict)| (name.clone(), classify_font(dict)))
        .collect();

    // A cap on how much one page may expand to when decompressed.
    //
    // A PDF content stream is compressed drawing instructions, and the
    // compression ratio has no ceiling — so a small file can ask for an
    // unbounded amount of memory. lopdf 0.44 provides this limited variant for
    // exactly that reason; the unlimited `get_page_content` accumulates whatever
    // it is handed. A page of text is normally a few kilobytes, so 32 MiB is
    // roughly a thousand times generous and still bounded.
    //
    // Over the limit, the page is skipped and counted rather than aborting the
    // whole document: one hostile or broken page should not lose the other
    // ninety-nine. This matches how a page of unreadable fonts is treated.
    const MAX_PAGE_CONTENT: usize = 32 * 1024 * 1024;

    let Ok(data) = document.get_page_content_with_limit(page, MAX_PAGE_CONTENT) else {
        *skipped += 1;
        return Ok(());
    };
    let content = Content::decode(&data)?;
    // Until a font is selected nothing can be read safely.
    let mut current = FontKind::Unreadable;

    // Where the text is, and how big.
    //
    // Before 14 August 2026 none of this was tracked: every positioning
    // operator simply emitted a line break, so a word split across three runs
    // for kerning came out on three lines and real prose arrived as confetti —
    // `Md. Al`, `-`, `Hasan`. A break now happens only where the text actually
    // moves down the page.
    let mut ctm = Transform::IDENTITY;
    let mut saved: Vec<Transform> = Vec::new();
    let mut text = Transform::IDENTITY;
    let mut line = Transform::IDENTITY;
    let mut font_size = 0.0f64;
    let mut leading = 0.0f64;
    // The previous run's baseline, and where it is estimated to have ended.
    let mut previous: Option<(f64, f64)> = None;

    for operation in content.operations {
        let number = |i: usize| -> f64 {
            match operation.operands.get(i) {
                Some(Object::Real(r)) => f64::from(*r),
                Some(Object::Integer(v)) => *v as f64,
                _ => 0.0,
            }
        };
        match operation.operator.as_str() {
            // The graphics state stack. Bounded, because a hostile file can nest
            // `q` for as long as it likes and this must not grow without limit.
            "q" => {
                if saved.len() < 64 {
                    saved.push(ctm);
                }
            }
            "Q" => {
                if let Some(m) = saved.pop() {
                    ctm = m;
                }
            }
            "cm" => {
                ctm = Transform([
                    number(0),
                    number(1),
                    number(2),
                    number(3),
                    number(4),
                    number(5),
                ])
                .then(ctm)
            }
            "BT" => {
                // The text matrices reset, but **not** the memory of where the
                // last run sat. A page is continuous however many text objects
                // it is chopped into, and real files chop it finely: several in
                // the archive wrap every single run in its own `BT`/`ET`.
                // Forgetting here made every run look like the first one, so
                // nothing was ever separated and whole pages came out as one
                // unbroken line.
                text = Transform::IDENTITY;
                line = Transform::IDENTITY;
            }
            "Tf" => {
                if let Some(lopdf::Object::Name(name)) = operation.operands.first() {
                    current = kinds
                        .get(name.as_slice())
                        .copied()
                        .unwrap_or(FontKind::Unreadable);
                }
                font_size = number(1);
            }
            "TL" => leading = number(0),
            "Tm" => {
                text = Transform([
                    number(0),
                    number(1),
                    number(2),
                    number(3),
                    number(4),
                    number(5),
                ]);
                line = text;
            }
            "Td" => {
                text = Transform::shift(number(0), number(1)).then(line);
                line = text;
            }
            "TD" => {
                leading = -number(1);
                text = Transform::shift(number(0), number(1)).then(line);
                line = text;
            }
            "T*" => {
                text = Transform::shift(0.0, -leading).then(line);
                line = text;
            }
            "Tj" | "'" | "\"" => {
                // The apostrophe and quote operators move to the next line first.
                if matches!(operation.operator.as_str(), "'" | "\"") {
                    text = Transform::shift(0.0, -leading).then(line);
                    line = text;
                }
                let placed = position(text, ctm, font_size);
                separate(out, marks, &mut previous, placed);
                let mut written = 0usize;
                for object in &operation.operands {
                    written += push_string(out, marks, object, current, skipped);
                }
                advance(&mut previous, placed, written);
            }
            "TJ" => {
                let placed = position(text, ctm, font_size);
                separate(out, marks, &mut previous, placed);
                let mut written = 0usize;
                if let Some(Object::Array(items)) = operation.operands.first() {
                    for item in items {
                        match item {
                            Object::String(..) => {
                                written += push_string(out, marks, item, current, skipped)
                            }
                            Object::Real(..) | Object::Integer(..) => {
                                let gap = match item {
                                    Object::Real(r) => f64::from(*r),
                                    Object::Integer(i) => *i as f64,
                                    _ => 0.0,
                                };
                                if -gap > SPACE_GAP && !out.ends_with(' ') && !out.ends_with('\n') {
                                    out.push(' ');
                                    marks.push(FontKind::PlainLatin);
                                    written += 1;
                                }
                            }
                            _ => {}
                        }
                    }
                }
                advance(&mut previous, placed, written);
            }
            _ => {}
        }
    }
    Ok(())
}

/// Where a run sits on the page, and how big its text is, in device units.
///
/// Returns `None` for a size that cannot be reasoned about — zero, negative, or
/// not a number. A file is free to contain those and must not make us divide by
/// them.
fn position(text: Transform, ctm: Transform, font_size: f64) -> Option<Placed> {
    let combined = text.then(ctm);
    let (x, y) = combined.position();
    let (across, down) = combined.scales();
    let height = font_size * down;
    let width = font_size * across;
    if !x.is_finite() || !y.is_finite() || !height.is_finite() || height <= 0.0 {
        return None;
    }
    Some(Placed {
        x,
        y,
        height,
        // A font may be stretched horizontally; when it is not, or the number
        // is unusable, the height is the better guess of the two.
        width: if width.is_finite() && width > 0.0 {
            width
        } else {
            height
        },
    })
}

/// Where one run of text sits, and how big it is, in device units.
#[derive(Clone, Copy)]
struct Placed {
    x: f64,
    y: f64,
    /// Drives the line-break decision.
    height: f64,
    /// Drives the space decision, and the estimate of where the run ends.
    width: f64,
}

/// Decide what belongs between the previous run and this one: a line break, a
/// space, or nothing at all.
fn separate(
    out: &mut String,
    marks: &mut Vec<FontKind>,
    previous: &mut Option<(f64, f64)>,
    placed: Option<Placed>,
) {
    let Some(run) = placed else {
        // Position unknown, so fall back to the old behaviour: break the line.
        // Losing a line break is worse than gaining one.
        push_break(out, marks);
        *previous = None;
        return;
    };
    let Some((last_y, last_end)) = *previous else {
        return;
    };
    if (run.y - last_y).abs() >= run.height * NEW_LINE_AT_FONT_SIZES {
        push_break(out, marks);
    } else if run.x - last_end >= run.width * SPACE_AT_FONT_SIZES
        && !out.ends_with(' ')
        && !out.ends_with('\n')
    {
        out.push(' ');
        marks.push(FontKind::PlainLatin);
    }
}

/// Record where this run ended, so the next one can be placed relative to it.
fn advance(previous: &mut Option<(f64, f64)>, placed: Option<Placed>, written: usize) {
    if let Some(run) = placed {
        let end = run.x + written as f64 * run.width * WIDTH_PER_CHARACTER;
        *previous = Some((run.y, if end.is_finite() { end } else { run.x }));
    }
}

fn push_break(out: &mut String, marks: &mut Vec<FontKind>) {
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
        marks.push(FontKind::PlainLatin);
    }
}

/// Emit one PDF string, recording which font each character came from.
///
/// The characters are NOT converted here. A PDF splits a word across as many
/// string operators as its kerning needs, and converting each fragment on its
/// own breaks the reordering that makes Bijoy readable: `mswk¬ó` arriving in
/// three pieces came out as `সংিশ্লষ্ট` instead of `সংশ্লিষ্ট`, with the vowel
/// sign stranded. The whole page is assembled first and converted after, which
/// is the same lesson the Office rewriter learned about runs.
fn push_string(
    out: &mut String,
    marks: &mut Vec<FontKind>,
    object: &Object,
    kind: FontKind,
    skipped: &mut usize,
) -> usize {
    let Object::String(bytes, _) = object else {
        return 0;
    };
    if kind == FontKind::Unreadable {
        // Nothing is emitted. See FontKind::Unreadable.
        if !bytes.iter().all(u8::is_ascii_whitespace) {
            *skipped += 1;
        }
        // Still report the width, so the run after it is not pulled leftwards
        // onto text it never sat beside.
        return from_windows_1252(bytes).chars().count();
    }
    let text = from_windows_1252(bytes);
    let mut written = 0usize;
    for c in text.chars() {
        out.push(c);
        marks.push(kind);
        written += 1;
    }
    written
}

/// Convert every word that came from a legacy font, and only those.
///
/// A word counts as legacy if ANY of its characters were drawn in a legacy
/// font, because a word split across operators may change font mid-way — a
/// bold first letter is enough to do it. The font is authority here, so no
/// guessing is needed and the word-level classifier is not consulted at all.
fn convert_marked(text: &str, marks: &[FontKind], summary: &mut Summary) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut out = String::with_capacity(text.len());
    let mut i = 0usize;
    while i < chars.len() {
        if chars[i].is_whitespace() {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        let start = i;
        while i < chars.len() && !chars[i].is_whitespace() {
            i += 1;
        }
        let word: String = chars[start..i].iter().collect();
        let legacy = marks[start..i].contains(&FontKind::LegacyBijoy);
        if legacy {
            out.push_str(&gru953_mukti::convert(&word));
            summary.words_converted += 1;
        } else {
            out.push_str(&word);
            summary.words_untouched += 1;
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn font(base: &str, encoding: Option<&str>) -> lopdf::Dictionary {
        let mut d = lopdf::Dictionary::new();
        d.set("BaseFont", lopdf::Object::Name(base.as_bytes().to_vec()));
        if let Some(e) = encoding {
            d.set("Encoding", lopdf::Object::Name(e.as_bytes().to_vec()));
        }
        d
    }

    /// Build a page whose content stream is exactly these operators, run the
    /// real extractor over it, and return the text.
    fn extract(operators: &str) -> String {
        use lopdf::{dictionary, Document, Object, Stream};
        let mut doc = Document::with_version("1.5");
        let font = doc.add_object(dictionary! {
            "Type" => "Font", "Subtype" => "Type1",
            "BaseFont" => "Helvetica", "Encoding" => "WinAnsiEncoding",
        });
        let resources = doc.add_object(dictionary! { "Font" => dictionary! { "F1" => font } });
        let content = doc.add_object(Stream::new(dictionary! {}, operators.as_bytes().to_vec()));
        let pages_id = doc.new_object_id();
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id,
            "Contents" => content, "Resources" => resources,
            "MediaBox" => vec![0.into(), 0.into(), 612.into(), 792.into()],
        });
        doc.objects.insert(
            pages_id,
            Object::Dictionary(dictionary! {
                "Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1,
            }),
        );
        let catalogue = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
        doc.trailer.set("Root", catalogue);
        let mut bytes = Vec::new();
        doc.save_to(&mut bytes)
            .expect("the test document should save");
        convert_pdf_to_text(&bytes)
            .expect("the test document should read")
            .0
    }

    #[test]
    fn the_pdf_font_list_is_a_subset_of_the_office_one() {
        // The PDF path trusts a font absolutely, so it may be stricter than the
        // Office path but must never be looser: a font it converts on sight has
        // to be one the rest of the project already agrees is legacy. Without
        // this test the two lists drift apart silently, which is how they came
        // to disagree in the first place.
        // Every name this list matches must also match the Office list. Since
        // both match by substring, that holds exactly when each entry here
        // already contains an entry there.
        for font in CERTAIN_LEGACY_FONTS {
            assert!(
                crate::office::is_legacy_font(font),
                "a PDF font matching {font:?} would be converted on sight, \
                 but the Office reader does not consider it legacy"
            );
        }
    }

    #[test]
    fn runs_on_one_baseline_join_even_across_separate_text_objects() {
        // The bug this pins, found on 14 August 2026 and caught by reading real
        // output: several documents in the archive wrap **every single run** in
        // its own `BT`/`ET`. Resetting the remembered position at `BT` made every
        // run look like the first one on the page, so nothing was ever
        // separated — a whole contact card came out as
        // one unbroken line: a name, job title, employer and address with no
        // separator anywhere between them.
        //
        // Three runs, same baseline, each in its own text object, butted up
        // against each other: they must become one word, with no break.
        let text = extract(
            "BT /F1 12 Tf 1 0 0 1 72 700 Tm (Ultra) Tj ET\n\
             BT /F1 12 Tf 1 0 0 1 102 700 Tm (-) Tj ET\n\
             BT /F1 12 Tf 1 0 0 1 108 700 Tm (Poor) Tj ET",
        );
        assert_eq!(text.trim(), "Ultra-Poor", "runs on one baseline were split");
    }

    #[test]
    fn a_move_down_the_page_starts_a_new_line() {
        // 12-point text moved down 14 points: 14 is well past half a font size,
        // so this is a new line. Both runs are in one text object here, so this
        // fails for a different reason than the test above if it fails at all.
        let text =
            extract("BT /F1 12 Tf 1 0 0 1 72 700 Tm (first) Tj 1 0 0 1 72 686 Tm (second) Tj ET");
        assert_eq!(text.trim(), "first\nsecond", "a new line was not started");
    }

    #[test]
    fn a_wide_horizontal_gap_becomes_a_space() {
        // Two table cells on one baseline. `left` is five characters of 12-point
        // text, so it is estimated to end around x=102; the next cell starts at
        // 300, which is a gap of many font sizes and plainly not a joined word.
        let text = extract(
            "BT /F1 12 Tf 1 0 0 1 72 700 Tm (left) Tj ET\n\
             BT /F1 12 Tf 1 0 0 1 300 700 Tm (right) Tj ET",
        );
        assert_eq!(text.trim(), "left right", "two cells ran together");
    }

    #[test]
    fn a_page_scaled_by_the_transformation_matrix_still_breaks_lines() {
        // The whole page scaled to a twentieth. The steps are now 0.7 units, not
        // 14, so any threshold in absolute units would fail here — real files in
        // the archive are laid out this way, one of them about fifteen units
        // tall. Only the ratio to the font size is stable.
        let text = extract(
            "q 0.05 0 0 0.05 0 0 cm\n\
             BT /F1 12 Tf 1 0 0 1 1440 14000 Tm (first) Tj ET\n\
             BT /F1 12 Tf 1 0 0 1 1440 13720 Tm (second) Tj ET\n\
             Q",
        );
        assert_eq!(text.trim(), "first\nsecond", "a scaled page lost its lines");
    }

    #[test]
    fn a_legacy_font_with_a_simple_encoding_is_readable() {
        for name in ["BCDEEE+SutonnyMJ-Bold", "SutonnyMJ", "Boishakhi"] {
            assert_eq!(
                classify_font(&font(name, Some("WinAnsiEncoding"))),
                FontKind::LegacyBijoy,
                "{name} was not recognised as legacy"
            );
        }
        // No /Encoding at all means the font's built-in one, which for these
        // fonts is the Bijoy layout.
        assert_eq!(
            classify_font(&font("SutonnyMJ", None)),
            FontKind::LegacyBijoy
        );
    }

    #[test]
    fn an_ordinary_text_font_is_read_but_not_converted() {
        assert_eq!(
            classify_font(&font("ArialMT", Some("WinAnsiEncoding"))),
            FontKind::PlainLatin
        );
    }

    /// The case that matters most: refusing to guess.
    #[test]
    fn a_font_whose_bytes_are_glyph_indexes_is_skipped_not_guessed_at() {
        // A Differences array, a symbolic subset, an Identity CID encoding —
        // none of these hold characters. Decoding them as Windows-1252 and
        // converting produced plausible-looking Bengali nonsense on a real
        // circular, which is worse than dropping the text.
        let mut differences = lopdf::Dictionary::new();
        differences.set("BaseFont", lopdf::Object::Name(b"ABCDEF+SymbolMT".to_vec()));
        differences.set(
            "Encoding",
            lopdf::Object::Dictionary(lopdf::Dictionary::new()),
        );
        assert_eq!(classify_font(&differences), FontKind::Unreadable);

        assert_eq!(
            classify_font(&font("ABCDEF+SutonnyMJ", Some("Identity-H"))),
            FontKind::Unreadable,
            "a CID-encoded legacy font must not be trusted either"
        );
    }

    #[test]
    fn a_legacy_string_is_read_as_windows_1252_and_converted() {
        let mut out = String::new();
        let mut marks = Vec::new();
        let mut summary = Summary::default();
        let mut skipped = 0;
        // 0x86 is `†` in Windows-1252 and the vowel sign ে in Bijoy. Note the
        // word arrives in TWO pieces, as a real PDF delivers it.
        for piece in [b"Awd\x86".as_slice(), b"mi".as_slice()] {
            push_string(
                &mut out,
                &mut marks,
                &Object::String(piece.to_vec(), lopdf::StringFormat::Literal),
                FontKind::LegacyBijoy,
                &mut skipped,
            );
        }
        assert_eq!(skipped, 0);
        // Converted only after the whole word is assembled, or the vowel sign
        // ends up in the wrong place.
        assert_eq!(convert_marked(&out, &marks, &mut summary), "অফিসের");
        assert_eq!(summary.words_converted, 1);
    }

    #[test]
    fn an_unreadable_string_emits_nothing_and_is_counted() {
        let mut out = String::new();
        let mut marks = Vec::new();
        let mut skipped = 0;
        push_string(
            &mut out,
            &mut marks,
            &Object::String(b"\x03\x11\x42".to_vec(), lopdf::StringFormat::Literal),
            FontKind::Unreadable,
            &mut skipped,
        );
        assert_eq!(out, "", "glyph indexes were turned into text");
        assert_eq!(skipped, 1, "the skip was not reported");
    }

    #[test]
    fn line_breaks_are_not_doubled_up() {
        let mut out = String::from("one");
        let mut marks = vec![FontKind::PlainLatin; 3];
        push_break(&mut out, &mut marks);
        push_break(&mut out, &mut marks);
        assert_eq!(out, "one\n");
        assert_eq!(marks.len(), out.chars().count(), "marks drifted from text");
    }
}
