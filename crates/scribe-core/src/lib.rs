//! GRU953-Scribe — legacy Bangla encoding to Unicode.
//!
//! Deterministic, table-driven, and **not** a machine-learning model. Two
//! parts: a detector that decides whether a piece of text is legacy-encoded,
//! and a converter that turns it into proper Unicode Bangla.
//!
//! # Why this is more than a character swap
//!
//! Bijoy-family encodings are a *font hack*: ASCII bytes are drawn with Bengali
//! shapes, and the bytes are stored in the order the glyphs **appear**, not the
//! order the letters are **spoken**. Unicode stores the logical order. So three
//! things must happen, in order:
//!
//! 1. map each glyph to its Unicode letter (longest conjuncts first);
//! 2. move vowel signs and reph to where Unicode expects them;
//! 3. tidy up the details.
//!
//! The clearest case is the i-kar. Bijoy stores `ি` *before* its consonant,
//! because that is where it is drawn. Unicode stores it *after*. Skip step 2
//! and every such word comes out silently wrong.
//!
//! # Provenance
//!
//! The mapping tables and the reordering rules are ported from
//! `almehady/Bijoy-to-Unicode-File-Converter` (MIT). Its full licence text is
//! reproduced in `THIRD-PARTY-LICENSES` at the root of this repository and must
//! be shipped with any distribution of this crate. A second widely-cited
//! implementation was **rejected**: it carries no licence at all, so copying
//! from it would have been a licence breach.
//!
//! ## Deliberate deviations from the reference
//!
//! The reference indexes with Python semantics, where `text[i - 1]` at `i == 0`
//! silently returns the *last* character, and `text[i + 2]` past the end raises
//! an error. Both are latent faults. This port treats out-of-range positions as
//! "no character", which is the intended meaning. Output therefore differs from
//! the reference only where the reference was wrong.

pub mod dictionary;
pub mod lexicon;
pub mod roundtrip;
pub mod tables;

/// A legacy Bangla encoding GRU953-Scribe can recognise.
///
/// "Bijoy" is not one standard. Each variant is named explicitly rather than
/// hidden behind a vague "custom encodings" claim. The list below was confirmed
/// against real documents, not assumed: scanning DOCX font tables found
/// `SutonnyMJ` throughout, with `SutonnyOMJ` alongside it, and sampled
/// documents contained **zero** Unicode Bengali characters.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LegacyEncoding {
    /// SutonnyMJ and its close relatives — the dominant Bijoy font mapping.
    SutonnyMj,
    /// Already valid Unicode Bangla. No conversion needed, and converting
    /// anyway would corrupt it.
    AlreadyUnicode,
    /// No Bengali content worth converting — plain English, code, numbers.
    NotBangla,
}

/// The detector's finding, with enough confidence information for a caller to
/// route doubtful text to review rather than accept it silently.
#[derive(Debug, Clone, Copy)]
pub struct Detection {
    pub encoding: LegacyEncoding,
    /// 0.0 to 1.0.
    pub confidence: f32,
    /// Share of characters already in the Unicode Bengali block.
    pub unicode_bengali_ratio: f32,
    /// Share of characters in the byte ranges Bijoy uses to carry Bengali.
    pub legacy_range_ratio: f32,
}

const fn is_halant(c: char) -> bool {
    c == '\u{09CD}'
}

const fn is_nukta(c: char) -> bool {
    c == 'ঁ'
}

const fn is_pre_kar(c: char) -> bool {
    matches!(c, 'ি' | 'ৈ' | 'ে')
}

const fn is_post_kar(c: char) -> bool {
    matches!(c, 'া' | 'ো' | 'ৌ' | 'ৗ' | 'ু' | 'ূ' | 'ী' | 'ৃ')
}

const fn is_kar(c: char) -> bool {
    is_pre_kar(c) || is_post_kar(c)
}

/// An independent vowel — a vowel written as a letter in its own right.
///
/// These already **are** the vowel, so they never take a vowel sign as well:
/// `আ` and `আ` + `া` are not two spellings of one thing, the second is simply
/// impossible. It is a common typing slip all the same, because on several
/// Bengali keyboards the two are adjacent keys.
const fn is_independent_vowel(c: char) -> bool {
    matches!(
        c,
        'অ' | 'আ' | 'ই' | 'ঈ' | 'উ' | 'ঊ' | 'ঋ' | 'ঌ' | 'এ' | 'ঐ' | 'ও' | 'ঔ'
    )
}

/// Is this a Bengali consonant?
///
/// Note the last three explicit codepoints. `ড়`, `ঢ়` and `য়` exist in Unicode
/// **two** ways: as single precomposed characters (U+09DC, U+09DD, U+09DF), or
/// as a base consonant followed by a combining nukta (U+09BC). Written as
/// literals in source they are easily the decomposed pair, which is not a
/// `char` at all and will not compile. They are spelled out here so the
/// distinction cannot be lost in an edit.
///
/// The decomposed form still works: its base letter (`ড`, `ঢ`, `য`) is already
/// in the list above, and the nukta is handled separately in [`rearrange`].
///
/// Laid out as the alphabet's own rows and exempt from rustfmt, which would
/// otherwise spread it over 40 lines of one letter each — unreadable, and
/// impossible to check against a Bengali chart.
#[rustfmt::skip]
const fn is_consonant(c: char) -> bool {
    matches!(c,
        'ক' | 'খ' | 'গ' | 'ঘ' | 'ঙ' | 'চ' | 'ছ' | 'জ' | 'ঝ' | 'ঞ'
        | 'ট' | 'ঠ' | 'ড' | 'ঢ' | 'ণ' | 'ত' | 'থ' | 'দ' | 'ধ' | 'ন'
        | 'প' | 'ফ' | 'ব' | 'ভ' | 'ম' | 'য' | 'র' | 'ল' | 'শ' | 'ষ'
        | 'স' | 'হ' | 'ৎ' | 'ং' | 'ঃ' | 'ঁ'
        | '\u{09DC}' | '\u{09DD}' | '\u{09DF}')
}

/// Character at `idx`, or `'\0'` when out of range.
///
/// Every predicate above returns false for `'\0'`, so an out-of-range lookup
/// naturally means "there is nothing here" — which is what the reference
/// implementation meant to say before Python's negative indexing got in the way.
fn at(s: &[char], idx: usize) -> char {
    s.get(idx).copied().unwrap_or('\0')
}

/// Glyphs the reference table omits, found by running real documents through it.
///
/// Kept separate from `tables.rs` on purpose: that file is generated from the
/// upstream reference and must stay a faithful copy, so anything this crate
/// adds of its own is visible here rather than hidden in generated output.
static CORRECTIONS: &[(&str, &str)] = &[
    // `ÿ` (U+00FF) is a SutonnyMJ form of ক্ষ. The reference maps only the `¶`
    // form, so `পদক্ষেপ` came out as `পদÿেপ` and `স্বাক্ষর` as `স্বাÿর`.
    ("ÿ", "ক্ষ"),
];

/// Apply an ordered table of literal replacements, top to bottom.
///
/// Order is load-bearing: `CONVERSION_MAP` lists longer conjuncts before their
/// own prefixes, so a hash map here would corrupt output.
fn apply_map(input: &str, map: &[(&str, &str)]) -> String {
    let mut out = input.to_owned();
    for (from, to) in map {
        if out.contains(from) {
            out = out.replace(from, to);
        }
    }
    out
}

/// The whitespace rules the reference expressed as regular expressions.
///
/// Reimplemented directly so GRU953-Scribe needs no regex dependency.
fn normalise_whitespace(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut last_was_space = false;
    for c in input.chars() {
        if c == ' ' {
            if !last_was_space {
                out.push(c);
            }
            last_was_space = true;
        } else {
            last_was_space = false;
            out.push(c);
        }
    }
    // Drop spaces that ended up hugging a line break.
    out.replace(" \n", "\n").replace("\n ", "\n")
}

/// Move a reph that sits after its consonant cluster to the front of it.
fn move_reph(s: &mut Vec<char>, i: usize) -> bool {
    // How far back does the cluster this reph belongs to reach?
    let mut j = 1usize;
    loop {
        if j > i {
            return false;
        }
        let c = at(s, i - j);
        if is_consonant(c) && j < i && is_halant(at(s, i - j - 1)) {
            j += 2;
        } else if j == 1 && is_kar(c) {
            j += 1;
        } else {
            break;
        }
    }
    if j > i {
        return false;
    }
    let mut t = Vec::with_capacity(s.len());
    t.extend_from_slice(&s[..i - j]);
    t.push(s[i]);
    t.push(s[i + 1]);
    t.extend_from_slice(&s[i - j..i]);
    t.extend_from_slice(&s[i + 2..]);
    *s = t;
    true
}

/// Put vowel signs, reph and nukta where Unicode expects them.
///
/// This is the step that makes the difference between real Bangla and a string
/// that merely contains Bengali characters.
fn rearrange(input: &str) -> String {
    let joined: String = input.chars().collect();
    // A doubled halant is always a mistake.
    let mut s: Vec<char> = joined
        .replace("\u{09CD}\u{09CD}", "\u{09CD}")
        .chars()
        .collect();

    let mut i = 0usize;
    while i < s.len() {
        // Reph — and only reph.
        //
        // Two sequences look alike and mean opposite things:
        //   `র` + halant  = reph, which belongs BEFORE the consonant it rides
        //   halant + `র`  = ra-phala, which belongs AFTER its consonant
        //
        // Bijoy draws reph after the cluster, so it must be moved back. A
        // ra-phala is already correct and must be left alone. The test that
        // separates them is whether a halant *precedes* the `র`.
        //
        // The reference implementation moved both, which is why a word such
        // as `ব্র্যান্ড` came out as `বর্যান্ড`.
        if i + 1 < s.len()
            && s[i] == 'র'
            && is_halant(at(&s, i + 1))
            && i > 0
            && !is_halant(at(&s, i - 1))
            && (is_consonant(at(&s, i - 1)) || is_kar(at(&s, i - 1)))
            && move_reph(&mut s, i)
        {
            // Skip the two characters the reph now occupies. It advanced by one,
            // but `move_reph` shifts the cluster *forward* by two — so the loop
            // landed back on a vowel sign it had already placed and moved it a
            // second time. `আর্থিক` came out `আর্থকি`, `সর্বোচ্চ` as `সর্বােচ্চ`.
            //
            // Well-formed Bengali every time, just the wrong word, which is why
            // no structural check ever saw it. Found by round-trip testing,
            // where it turned out to affect several hundred distinct words.
            i += 2;
            continue;
        }

        // Vowel sign + halant + consonant  ->  halant + consonant + vowel sign.
        if i > 0
            && i + 1 < s.len()
            && is_halant(s[i])
            && (is_kar(at(&s, i - 1)) || is_nukta(at(&s, i - 1)))
        {
            let mut t = Vec::with_capacity(s.len());
            t.extend_from_slice(&s[..i - 1]);
            t.push(s[i]);
            t.push(s[i + 1]);
            t.push(s[i - 1]);
            t.extend_from_slice(&s[i + 2..]);
            s = t;
        }

        // RA + halant + vowel sign  ->  vowel sign + RA + halant.
        if i > 0
            && i + 1 < s.len()
            && is_halant(s[i])
            && at(&s, i - 1) == 'র'
            && !(i >= 2 && is_halant(at(&s, i - 2)))
            && is_kar(at(&s, i + 1))
        {
            let mut t = Vec::with_capacity(s.len());
            t.extend_from_slice(&s[..i - 1]);
            t.push(s[i + 1]);
            t.push(s[i - 1]);
            t.push(s[i]);
            t.extend_from_slice(&s[i + 2..]);
            s = t;
        }

        // The important one: a pre-kar drawn before its consonant moves after
        // the whole cluster. `ি` + `ক` becomes `কি`.
        if i + 1 < s.len() && is_pre_kar(s[i]) && !at(&s, i + 1).is_whitespace() {
            let mut t: Vec<char> = s[..i].to_vec();

            // Walk forward over the consonant cluster this vowel belongs to.
            let mut j = 1usize;
            while i + j < s.len().saturating_sub(1) && is_consonant(at(&s, i + j)) {
                if is_halant(at(&s, i + j + 1)) {
                    j += 2;
                } else {
                    break;
                }
            }

            t.extend_from_slice(&s[i + 1..(i + j + 1).min(s.len())]);

            // Two-part vowels are written as two glyphs in Bijoy and must be
            // joined into the single Unicode sign.
            let mut consumed_extra = 0usize;
            let following = at(&s, i + j + 1);
            if s[i] == 'ে' && following == 'া' {
                t.push('ো');
                consumed_extra = 1;
            } else if s[i] == 'ে' && following == 'ৗ' {
                t.push('ৌ');
                consumed_extra = 1;
            } else {
                t.push(s[i]);
            }

            let tail = i + j + consumed_extra + 1;
            if tail < s.len() {
                t.extend_from_slice(&s[tail..]);
            }
            s = t;
            i += j;
        }

        // Nukta belongs after a following vowel sign, not before it.
        if i + 1 < s.len() && is_nukta(s[i]) && is_post_kar(at(&s, i + 1)) {
            let mut t = Vec::with_capacity(s.len());
            t.extend_from_slice(&s[..i]);
            t.push(s[i + 1]);
            t.push(s[i]);
            t.extend_from_slice(&s[i + 2..]);
            s = t;
        }

        i += 1;
    }

    s.into_iter().collect()
}

/// Decide whether a piece of text is legacy-encoded Bangla.
///
/// The test that matters is simple and hard to fool: legacy Bangla contains
/// Bengali *words* but **no** Unicode Bengali *characters*, because the bytes
/// are Latin ones drawn with Bengali shapes.
pub fn detect(input: &str) -> Detection {
    let mut bengali = 0usize;
    let mut legacy_range = 0usize;
    let mut considered = 0usize;

    for c in input.chars() {
        if c.is_whitespace() || c.is_ascii_digit() {
            continue;
        }
        considered += 1;
        let o = c as u32;
        if (0x0980..=0x09FF).contains(&o) {
            bengali += 1;
        } else if c.is_ascii_graphic()
            || (0x00A0..=0x024F).contains(&o)
            || (0x2010..=0x20FF).contains(&o)
        {
            legacy_range += 1;
        }
    }

    if considered == 0 {
        return Detection {
            encoding: LegacyEncoding::NotBangla,
            confidence: 1.0,
            unicode_bengali_ratio: 0.0,
            legacy_range_ratio: 0.0,
        };
    }

    let bengali_ratio = bengali as f32 / considered as f32;
    let legacy_ratio = legacy_range as f32 / considered as f32;

    // Already Unicode: real Bengali characters are present in quantity.
    if bengali_ratio > 0.15 {
        return Detection {
            encoding: LegacyEncoding::AlreadyUnicode,
            confidence: bengali_ratio.min(1.0),
            unicode_bengali_ratio: bengali_ratio,
            legacy_range_ratio: legacy_ratio,
        };
    }

    // Legacy Bijoy leans on accented Latin-1 and punctuation glyphs that plain
    // English almost never uses. That signature, plus the absence of Unicode
    // Bengali, is what identifies it.
    // Whitespace is never evidence of an encoding, whatever its code point.
    // A non-breaking space (U+00A0) sits inside the Latin-1 range Bijoy uses,
    // and counting it made `January 30, 2026` — one nbsp among seven letters —
    // score 0.125 and be rewritten as Bengali. Word and PowerPoint put
    // non-breaking spaces in dates and headings constantly.
    let exotic = input
        .chars()
        .filter(|c| !c.is_whitespace())
        .filter(|c| {
            let o = *c as u32;
            (0x00A0..=0x024F).contains(&o) || (0x2010..=0x20FF).contains(&o)
        })
        .count();
    let exotic_ratio = exotic as f32 / considered as f32;

    // Two tests, because either alone gives a wrong answer.
    //
    // Density is necessary but not sufficient. Measured against real lines,
    // the least exotic Bijoy line and the most exotic English line both score
    // 0.111, so no threshold separates them cleanly.
    //
    // Trial conversion does not help either, tempting as it sounds: the Bijoy
    // tables map the whole ASCII range, so *any* English sentence converts to
    // Bengali-looking output. "Consultant: Riverbank Advisory Group" comes out
    // over 40% Bengali. That test was tried and discarded.
    //
    // What actually separates them is that English is English. Bijoy text is
    // Bengali wearing ASCII, so it does not contain English function words.
    // Density says "this might be Bijoy"; the word check says "this is plainly
    // not". Both must agree before a line is rewritten — because corrupting
    // readable English is worse than missing a conversion.
    // Two ways in. Most Bijoy is dense in accented Latin-1, which the density
    // test catches. A run of plain ASCII glyphs — common in table headings —
    // carries no unusual bytes at all, so it is decided by whether it converts
    // into real Bengali words.
    // Real Bijoy text draws on many different glyphs — vowel signs, conjuncts,
    // reph. A long run of one repeated character is decoration, not an encoding.
    // A contents page reading `ABSTRACT........03` is stored as leader dots that
    // land in the same byte range, and its sheer length made it look like the
    // densest Bijoy anywhere in the test data. It converted to
    // `অইঝঞজঅঈঞৃৃৃৃৃৃ…` — the single largest source of broken output.
    //
    // Two, not three: a short genuine Bijoy word can carry only two distinct
    // glyphs. Two still rejects a run of one repeated character, which is the
    // whole failure.
    let distinct_exotic = {
        let mut seen: Vec<char> = Vec::new();
        for c in input.chars().filter(|c| !c.is_whitespace()).filter(|c| {
            let o = *c as u32;
            (0x00A0..=0x024F).contains(&o) || (0x2010..=0x20FF).contains(&o)
        }) {
            if !seen.contains(&c) {
                seen.push(c);
            }
            if seen.len() >= 2 {
                break;
            }
        }
        seen.len()
    };
    let dense = exotic_ratio >= 0.10 && distinct_exotic >= 2 && !reads_as_english(input);
    // Both routes to "this is Bijoy" must clear the English test. `dense` did;
    // the trial-conversion route did not, so a line of HTML —
    // `<p className="digest-feature-try">watch something while you keep working`
    // — converted into Bengali-shaped nonsense that hit enough word stems by
    // chance to pass. English is English whichever route asks.
    let looks_legacy =
        dense || (converts_to_real_bengali_strict(input) && !reads_as_english(input));
    if looks_legacy && bengali_ratio < 0.02 {
        Detection {
            encoding: LegacyEncoding::SutonnyMj,
            confidence: (exotic_ratio * 5.0).min(1.0),
            unicode_bengali_ratio: bengali_ratio,
            legacy_range_ratio: legacy_ratio,
        }
    } else {
        Detection {
            encoding: LegacyEncoding::NotBangla,
            confidence: 1.0 - exotic_ratio,
            unicode_bengali_ratio: bengali_ratio,
            legacy_range_ratio: legacy_ratio,
        }
    }
}

/// Convert legacy Bangla to Unicode, unconditionally.
///
/// Prefer [`convert_if_legacy`] in the ingestion pipeline: running this over
/// text that is already Unicode will damage it.
pub fn convert(input: &str) -> String {
    if input.is_empty() {
        return String::new();
    }
    let s = normalise_whitespace(input);
    let s = apply_map(&s, tables::PRE_MAP);
    let s = apply_map(&s, tables::CONVERSION_MAP);
    let s = apply_map(&s, CORRECTIONS);
    let s = rearrange(&s);
    let s = compose_two_part_vowels(&s);
    apply_map(&s, tables::POST_MAP)
}

/// Join the two halves of `ো` and `ৌ` wherever they ended up side by side.
///
/// Bijoy draws these as two glyphs, one each side of the consonant. The
/// reordering pass joins them when it is the pass that brings them together —
/// but when a *reph* move is what leaves them adjacent, nothing did. So
/// `সর্বোচ্চ` came out as `সর্বােচ্চ`: the right letters, in the right order,
/// with the vowel left in two pieces.
///
/// Doing it here instead means it holds however the two halves met.
/// Repair Bengali that is already Unicode but was badly converted by something
/// else — no glyph mapping, only the fixes that are safe on any Bengali text.
///
/// Each step undoes something structurally impossible in the language, so it
/// cannot damage correct text: a two-part vowel left in halves, a repeated mark,
/// or two vowel signs on one syllable. Text with no Bengali in it is returned
/// untouched, so English and code cost nothing.
pub fn repair_unicode(s: &str) -> String {
    if !s.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c)) {
        return s.to_owned();
    }
    let s = reunite_split_vowels(s);
    compose_two_part_vowels(&s)
}

/// Move a space that landed between a letter and its vowel sign.
///
/// **A vowel sign cannot begin a word.** So when one does, the space in front of
/// it is in the wrong place, and the sign belongs to the letter before it:
/// `কিছ ুমনে` is `কিছু মনে`. Word extraction from tables and PDFs scatters these
/// through real documents.
///
/// Only a plain space is moved. A tab is a column boundary — crossing one would
/// merge two table cells.
fn reunite_split_vowels(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    let mut out = String::with_capacity(s.len());
    let mut i = 0usize;
    while i < chars.len() {
        let is_split = chars[i] == ' '
            && i > 0
            && i + 1 < chars.len()
            && (is_kar(chars[i + 1]) || is_halant(chars[i + 1]))
            // **Only a bare consonant can take a vowel sign.** The first version
            // accepted any Bengali character, so a letter that already carried
            // one received a second — `কথা ুমনে` became `কথাু মনে`. That turned
            // tens of thousands
            // of orphaned signs into thousands of doubled ones: a different
            // fault, not fewer. An audit caught it; no test would have.
            //
            // `ং`, `ঃ` and `ঁ` are excluded: they close a syllable and cannot
            // carry a vowel either, though `is_consonant` groups them with the
            // letters for the reordering pass.
            && is_consonant(chars[i - 1])
            && !matches!(chars[i - 1], 'ং' | 'ঃ' | 'ঁ');
        if is_split {
            out.push(chars[i + 1]); // the sign rejoins its letter
            out.push(' '); // and the space moves after it
            i += 2;
        } else {
            out.push(chars[i]);
            i += 1;
        }
    }
    out
}

fn compose_two_part_vowels(s: &str) -> String {
    let s = s
        .replace("\u{09C7}\u{09BE}", "\u{09CB}") // ে + া -> ো
        .replace("\u{09C7}\u{09D7}", "\u{09CC}"); // ে + ৗ -> ৌ
    let s = collapse_impossible_doubles(&s);
    let s = repair_mistyped_vowels(&s);
    reunite_split_vowels(&s)
}

/// Drop a repeated joiner or vowel sign.
///
/// A syllable carries one vowel sign, and two joiners in a row spell nothing —
/// both are impossible in Bengali, so collapsing them cannot damage valid text.
/// They come from doubled keystrokes in the original typing: `প্রস্তুতি` was
/// stored with the joiner and the u-kar each struck twice, and came out as
/// `প্রস্্তুুতি`.
/// Repair a mistyped extra vowel sign, but only when the result is a real word.
///
/// A syllable carries one vowel sign. Two in a row is a slip of the fingers,
/// and old documents are full of them — typed years ago and preserved exactly.
/// Left alone, they are invisible to search: nobody looking for `অনুযায়ী` will
/// ever find `অনিুযায়ী`.
///
/// **Which one to drop cannot be fixed in advance.** `অনিুযায়ী` needs the first
/// removed, `নারীাদের` the second. So both are tried and the word list decides.
/// If exactly one candidate is a word a Bengali reader would recognise, that is
/// the repair; if neither or both are, the text is left untouched.
///
/// That asymmetry is the whole safeguard. A rule that always dropped one side
/// would fix half these words and break the other half.
fn repair_mistyped_vowels(s: &str) -> String {
    if !s.chars().any(is_kar) {
        return s.to_owned();
    }
    let mut out = String::with_capacity(s.len());
    for (i, word) in s.split_inclusive(char::is_whitespace).enumerate() {
        let _ = i;
        out.push_str(&repair_word(word));
    }
    out
}

fn repair_word(word: &str) -> String {
    let chars: Vec<char> = word.chars().collect();
    let Some(at) = chars.windows(2).position(|w| is_kar(w[0]) && is_kar(w[1])) else {
        return word.to_owned();
    };

    let drop = |idx: usize| -> String {
        chars
            .iter()
            .enumerate()
            .filter(|(i, _)| *i != idx)
            .map(|(_, c)| *c)
            .collect()
    };
    let first = drop(at);
    let second = drop(at + 1);
    match (
        lexicon::reads_as_bengali(&first),
        lexicon::reads_as_bengali(&second),
    ) {
        (true, false) => first,
        (false, true) => second,
        // The word list is small and knows neither candidate for most words, so
        // it decides only the cases it actually recognises. Otherwise fall back
        // to which sign is the likely intruder.
        //
        // By this point `ে`+`া` and `ে`+`ৗ` have already been joined into `ো`
        // and `ৌ`, so any remaining pair is genuinely an error — only the choice
        // of which to drop is open.
        //
        // A **pre-kar** is the sign the reordering moves, so a stranded one is
        // the likely mistake: `অনিুযায়ী` -> `অনুযায়ী`. When the first sign is a
        // correctly-placed post-kar, the second is the intruder instead:
        // `নারীাদের` -> `নারীদের`. Both real, and in opposite directions.
        _ if is_pre_kar(chars[at]) => first,
        _ => second,
    }
}

fn collapse_impossible_doubles(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut prev = '\0';
    for c in s.chars() {
        let repeated_mark = c == prev && (is_halant(c) || is_kar(c));
        if !repeated_mark {
            out.push(c);
        }
        prev = c;
    }
    out
}

/// Convert only when the text is actually legacy-encoded.
///
/// Judges the whole string at once. Correct for a single paragraph; **wrong
/// for a whole document** — see [`convert_document`], which is what the
/// ingestion pipeline actually calls.
pub fn convert_if_legacy(input: &str) -> (String, Detection) {
    let d = detect(input);
    match d.encoding {
        LegacyEncoding::SutonnyMj => (convert(input), d),
        _ => (input.to_owned(), d),
    }
}

/// What converting a whole document did.
#[derive(Debug, Clone)]
pub struct DocumentConversion {
    pub text: String,
    pub lines_total: usize,
    pub lines_converted: usize,
    /// What the document is *mostly*, for the record kept against the file.
    pub dominant: LegacyEncoding,
}

/// Below this many judgeable characters, a line carries too little signal for
/// the detector to be trusted on its own.
const MIN_CHARS_TO_JUDGE: usize = 12;

/// Convert a document **line by line**.
///
/// Real documents mix encodings. A report can hold Unicode Bangla headings,
/// legacy Bijoy body text and plain English in the same file, because it was
/// edited over years by different people. Judging the whole file at once means
/// the majority silently decides for the minority: a mostly-Unicode document
/// keeps its legacy paragraphs as gibberish, and nobody notices because the
/// file *looks* fine.
///
/// That is exactly what happened to several test files, and it is why this
/// works per line instead.
///
/// Short lines — headings, table cells, single words — are the awkward case:
/// too little text to judge confidently, but converting them wrongly would
/// corrupt readable English. They are therefore only converted when the
/// document has *already proved* it contains legacy text elsewhere, and the
/// line itself carries at least one byte from the range Bijoy uses.
pub fn convert_document(input: &str) -> DocumentConversion {
    if input.is_empty() {
        return DocumentConversion {
            text: String::new(),
            lines_total: 0,
            lines_converted: 0,
            dominant: LegacyEncoding::NotBangla,
        };
    }

    // Split on tabs as well as newlines. Word and Excel emit a table row as
    // one line of tab-separated cells, and a single heading cell is far too
    // short to judge on its own — so a whole row of legacy headings used to be
    // waved through as unjudgeable. Separators are kept so the
    // document's shape is rebuilt exactly.
    let lines: Vec<&str> = split_keeping_separators(input);

    // First pass: judge every line that has enough text to judge.
    let verdicts: Vec<Option<LegacyEncoding>> = lines
        .iter()
        .map(|line| {
            // A segment with no letters or digits carries no evidence of any
            // encoding, so it must never inherit the document's verdict. A
            // bullet on its own is the case that bit: `•` maps to `ঙ্`, and a
            // tab-separated English list in a mostly-Bijoy workbook had every
            // bullet turned into a Bengali letter.
            if !line.chars().any(|c| c.is_alphanumeric()) {
                return Some(LegacyEncoding::AlreadyUnicode);
            }
            let judgeable = line.chars().filter(|c| !c.is_whitespace()).count();
            if judgeable < MIN_CHARS_TO_JUDGE {
                None
            } else {
                Some(detect(line).encoding)
            }
        })
        .collect();

    let legacy_lines = verdicts
        .iter()
        .filter(|v| **v == Some(LegacyEncoding::SutonnyMj))
        .count();
    let unicode_lines = verdicts
        .iter()
        .filter(|v| **v == Some(LegacyEncoding::AlreadyUnicode))
        .count();
    let document_has_legacy = legacy_lines > 0;

    // Second pass: convert.
    let mut converted = 0usize;
    let out: Vec<String> = lines
        .iter()
        .zip(&verdicts)
        .map(|(line, verdict)| match verdict {
            // Separators pass through untouched.
            _ if line.chars().all(|c| c == '\n' || c == '\t') => (*line).to_owned(),
            Some(LegacyEncoding::SutonnyMj) => {
                converted += 1;
                convert(line)
            }
            // Already Unicode, but not necessarily *good* Unicode. Much of the
            // archive was converted years ago by other tools that left doubled
            // marks and mistyped vowels behind. Repair those, without touching
            // the glyph mapping — converting text that is already Unicode is
            // precisely the corruption this branch exists to prevent.
            //
            // Widened deliberately, so that words some other tool broke years
            // ago are findable too.
            Some(_) => repair_unicode(line),
            // Too short to judge alone. Only convert it if this document has
            // legacy text elsewhere AND the line looks like it belongs to it.
            None if document_has_legacy
                && looks_like_bijoy(line)
                // For a fragment too short to judge, ask what it would *become*.
                // A high-range glyph is strong evidence on its own — that is
                // what the font hack needs for conjuncts and vowel signs. Pure
                // ASCII is not, so it must earn conversion by producing real
                // Bengali words.
                //
                // Both cases are real. `Awd†mi bvgt` is a genuine Bijoy table
                // heading that is pure ASCII and must convert. But
                // `E1: "Q1-Q18"` in the same workbook became `ঊ১: "ছ১-ছ১৮"` —
                // a question reference turned into nonsense. Only the
                // first produces words a Bengali reader would recognise.
                && (!line.is_ascii() || lexicon::reads_as_bengali(&convert(line))) =>
            {
                converted += 1;
                convert(line)
            }
            None => (*line).to_owned(),
        })
        .collect();

    let dominant = if legacy_lines > 0 && legacy_lines >= unicode_lines {
        LegacyEncoding::SutonnyMj
    } else if unicode_lines > 0 {
        LegacyEncoding::AlreadyUnicode
    } else if legacy_lines > 0 {
        LegacyEncoding::SutonnyMj
    } else {
        LegacyEncoding::NotBangla
    };

    DocumentConversion {
        text: out.concat(),
        lines_total: lines.len(),
        lines_converted: converted,
        dominant,
    }
}

/// Split on newlines and tabs, keeping the separators as their own pieces.
///
/// Keeping them means the reassembled document has exactly the original
/// layout — no row collapses into a paragraph, no column boundary is lost.
fn split_keeping_separators(input: &str) -> Vec<&str> {
    let mut out = Vec::new();
    let mut start = 0usize;
    for (i, c) in input.char_indices() {
        if c == '\n' || c == '\t' {
            if i > start {
                out.push(&input[start..i]);
            }
            out.push(&input[i..i + c.len_utf8()]);
            start = i + c.len_utf8();
        }
    }
    if start < input.len() {
        out.push(&input[start..]);
    }
    out
}

/// Common English function words.
///
/// Short and deliberately dull. These carry no meaning on their own, which is
/// exactly why they are reliable: English prose is full of them and Bengali
/// text — however it is encoded — is not.
const ENGLISH_MARKERS: &[&str] = &[
    "the", "and", "of", "to", "in", "for", "is", "was", "are", "with", "that", "this", "from",
    "by", "on", "at", "as", "be", "or", "an", "it", "has", "have", "will", "not", "all", "any",
    "may", "shall", "our", "their", "still", "more", "most", "other", "such", "when", "which",
    "than", "then", "been", "were", "also", "into", "over", "under", "after", "before", "each",
    "both", "only", "same", "some", "they", "them", "there", "here", "what", "who", "how", "why",
    "can", "must", "should", "would", "could", "about", "between", "during", "within", "per",
    "via", "these", "those", "but", "if", "its", "his", "her", "we", "you", "was", "does", "did",
    "done", "made",
];

/// Does this line read as English?
///
/// Two or more function words is the bar. One can appear by chance in a Bijoy
/// line, since Bijoy is ASCII underneath; two together effectively never do.
fn reads_as_english(input: &str) -> bool {
    let mut hits = 0usize;
    for token in input.split(|c: char| !c.is_ascii_alphabetic()) {
        if token.len() >= 2 {
            let lower = token.to_ascii_lowercase();
            if ENGLISH_MARKERS.contains(&lower.as_str()) {
                hits += 1;
                if hits >= 2 {
                    return true;
                }
            }
        }
    }
    false
}

/// Is this Bengali text well formed?
///
/// Not "are these Bengali characters" — that is easy and useless. This asks
/// whether the characters are arranged the way Bengali actually works, and it
/// catches two different faults that no character-level test can:
///
/// * a PDF font map that produces genuine Bengali codepoints in the **wrong
///   order**, which looks perfectly fine to any decoder;
/// * a short ASCII fragment wrongly converted from something that was never
///   Bijoy, which produces Bengali-shaped nonsense.
///
/// The rules are the ones Bengali orthography does not permit:
/// a vowel sign cannot open a word, two vowel signs cannot sit together, and a
/// hasant cannot be followed by a vowel sign or open a word.
pub fn bengali_is_plausible(text: &str) -> bool {
    let chars: Vec<char> = text.chars().collect();
    let mut bengali = 0usize;
    let mut faults = 0usize;
    let mut at_word_start = true;

    for (i, &c) in chars.iter().enumerate() {
        if c.is_whitespace() {
            at_word_start = true;
            continue;
        }
        let is_bengali = ('\u{0980}'..='\u{09FF}').contains(&c);
        if !is_bengali {
            at_word_start = false;
            continue;
        }
        bengali += 1;
        let prev = if i > 0 { chars[i - 1] } else { ' ' };

        if (is_kar(c) || is_halant(c)) && at_word_start {
            faults += 1; // a vowel sign or hasant cannot open a word
        } else if is_kar(c) && is_kar(prev) {
            faults += 1; // two vowel signs cannot sit together
        } else if is_kar(c) && is_halant(prev) {
            faults += 1; // a hasant is never followed by a vowel sign
        } else if is_halant(c) && is_kar(prev) {
            faults += 1; // nor does a hasant ever follow one — see below
        }
        at_word_start = false;
    }

    if bengali < 8 {
        return true; // too little Bengali to judge; do not accuse it
    }
    (faults as f32 / bengali as f32) < 0.04
}

/// Is this single word well formed Bengali?
///
/// The same orthographic rules as [`bengali_is_plausible`], but applied to one
/// word with **no length escape hatch** — that function forgives anything under
/// eight Bengali characters, because a short fragment carries too little signal
/// to accuse. A single word is different: the rules either hold across it or
/// they do not, and its length is not evidence either way.
///
/// The rules are the ones Bengali orthography does not permit:
///
/// * a vowel sign or a hasant cannot open a word;
/// * two vowel signs cannot sit together;
/// * a hasant is never followed by a vowel sign;
/// * **a vowel sign is never followed by a hasant.** A vowel sign closes its
///   syllable, so a hasant after one has no consonant left to join. `দূ্র্য` is
///   impossible for the same reason `দ্র্যূ` is ordinary;
/// * **an independent vowel never takes a vowel sign.** `আ` already is the
///   vowel; `অ` + `া` is a typing slip for it, not a second spelling of it.
///
/// That last rule earned its place immediately. Four entries in the reference
/// character grid — `দূ্র্য`, `ন্ধূ্র`, `মূ্ন`, `ষ্টূ্র` — are typed this way,
/// each one the sixth cell of a row whose other seven cells are spelled
/// correctly. Scribe converts them to the well-formed spelling, and the harness
/// was scoring it wrong for doing so. Stating the rule here means the harness
/// can excuse them on an independent orthographic test rather than on a claim
/// that Scribe happens to be right.
pub fn word_is_well_formed(word: &str) -> bool {
    let chars: Vec<char> = word.chars().collect();
    let mut seen_bengali = false;
    for (i, &c) in chars.iter().enumerate() {
        if !('\u{0980}'..='\u{09FF}').contains(&c) {
            continue;
        }
        let prev = if i > 0 { chars[i - 1] } else { ' ' };
        let opens = !seen_bengali;
        seen_bengali = true;

        let fault = ((is_kar(c) || is_halant(c)) && opens)
            || (is_kar(c) && is_kar(prev))
            || (is_kar(c) && is_halant(prev))
            || (is_halant(c) && is_kar(prev))
            || (is_kar(c) && is_independent_vowel(prev));
        if fault {
            return false;
        }
    }
    true
}

/// Does this text turn into real Bengali when run through the tables?
///
/// The decisive test, and the only one that has held. Bijoy maps the whole
/// ASCII range, so every English sentence converts to something Bengali-shaped;
/// but only genuine Bijoy converts into recognisable **words**. Character
/// density and capitalisation rules were each tried and reverted before this.
fn converts_to_real_bengali_strict(line: &str) -> bool {
    converts_with(line, lexicon::reads_as_bengali_strict)
}

/// As above, but for fragments inside a document already known to hold Bijoy.
fn converts_to_real_bengali(line: &str) -> bool {
    converts_with(line, lexicon::reads_as_bengali)
}

fn converts_with(line: &str, accept: fn(&str) -> bool) -> bool {
    if line.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c)) {
        return false; // already Unicode Bengali — never touch it
    }
    if !line.chars().any(|c| c.is_alphabetic()) {
        return false; // digits and punctuation carry no encoding
    }
    let trial = convert(line);
    accept(&trial) && bengali_is_plausible(&trial)
}

/// Could this short fragment be Bijoy?
///
/// Short table cells are the awkward case: "Awd†mi bvgt" is pure ASCII, so
/// looking for unusual bytes finds nothing. Instead the fragment is
/// trial-converted and the result judged: real Bijoy turns into well-formed
/// Bengali, and an English word does not.
fn looks_like_bijoy(line: &str) -> bool {
    if reads_as_english(line) {
        return false;
    }
    if converts_to_real_bengali(line) {
        return true;
    }
    // A short fragment may hold a perfectly good Bengali word the lexicon has
    // never heard of — `দায়িত্ববোধ` was exactly that, an 11-character table
    // cell one character under the bar to judge on its own.
    //
    // Density rescues it. Accented Latin-1 at 10% or more is a strong signal in
    // itself, and this path only runs for fragments inside a document that has
    // **already proved** it contains Bijoy. Context plus density is enough
    // where either alone would not be.
    let considered = line.chars().filter(|c| !c.is_whitespace()).count();
    if considered == 0 {
        return false;
    }
    let exotic = line
        .chars()
        .filter(|c| !c.is_whitespace())
        .filter(|c| {
            let o = *c as u32;
            (0x00A0..=0x024F).contains(&o) || (0x2010..=0x20FF).contains(&o)
        })
        .count();
    exotic as f32 / considered as f32 >= 0.10
}

#[cfg(test)]
mod tests {
    // ---------------------------------------------------------------------
    // Ground truth: hand-checked words against the rendered font.
    //
    // These glyphs are SutonnyMJ *positional variants*: the font stores
    // several shapes of the same mark, because `রু`, `গু` and `শু` each draw
    // their u-kar differently. The generated table pattern-completed them into
    // the neighbouring conjunct run instead, so `উল্লেখ্য` came out as
    // `উলেস্নখ্য` — valid Bengali Unicode, entirely the wrong word.
    //
    // Nothing else caught this. The mojibake check looks for leftover Latin
    // characters and this output has none; `স্ন`, `ম্ন` and `ত্ম` are all real
    // conjuncts, so no structural check fires either. Only comparison against
    // known-correct text finds it — which is what these are.
    //
    // Left-hand sides are the raw bytes as a SutonnyMJ document stores them.
    #[test]
    fn variant_glyph_forms_convert_to_the_right_words() {
        for (legacy, expected, note) in [
            (
                "D\u{2021}j\u{f8}L\u{a8}",
                "উল্লেখ্য",
                "la-phala variant after ল",
            ),
            ("mswk\u{f8}\u{f3}", "সংশ্লিষ্ট", "the same variant after শ"),
            ("i\u{e6}wUb", "রুটিন", "u-kar variant after র"),
            // য় written as the precomposed U+09DF. The decomposed spelling looks
            // identical and is equally legal — that ambiguity has now cost four
            // separate defects, so it is spelled explicitly here.
            (
                "ev\u{af}\u{cd}evqb",
                "বাস্তবা\u{9df}ন",
                "ta-phala variant after স",
            ),
        ] {
            assert_eq!(convert(legacy), expected, "{note}");
        }
    }

    /// The bug that hid the bug: these are all well-formed Bengali, so every
    /// check we had passed them. Guards against re-introducing the mapping.
    /// Repairs extended to text that was already Unicode.
    ///
    /// Plenty of Bengali was converted years ago by other tools that left
    /// doubled marks and mistyped vowels behind; those words were unfindable.
    /// Every step is safe on any Bengali text because each undoes something the
    /// language cannot contain.
    #[test]
    fn already_unicode_text_is_repaired_not_converted() {
        // A doubled joiner and u-kar, already in Unicode — never touched before.
        let broken = "\u{09AA}\u{09CD}\u{09B0}\u{09B8}\u{09CD}\u{09CD}\u{09A4}\u{09C1}\u{09C1}\u{09A4}\u{09BF}";
        assert_eq!(repair_unicode(broken), "প্রস্তুতি");
        assert_eq!(repair_unicode("অনিুযায়ী"), "অনুযায়ী");
        // A two-part vowel left in halves.
        assert_eq!(repair_unicode("স\u{09C7}\u{09BE}"), "সো");

        // Correct Bengali is untouched, and so is everything else.
        for same in [
            "সর্বোচ্চ",
            "প্রতিবন্ধী",
            "অন্ন",
            "hello world",
            "",
            "fn main() {}",
        ] {
            assert_eq!(repair_unicode(same), same);
        }

        // And it must repair *without* converting: a document that is already
        // Unicode must not be treated as legacy.
        let doc = convert_document("অনিুযায়ী ব্যবস্থা গ্রহণ করা হবে");
        assert_eq!(doc.lines_converted, 0, "already-Unicode text was converted");
        assert!(doc.text.contains("অনুযায়ী"), "not repaired: {:?}", doc.text);
    }

    /// A space that landed between a letter and its vowel sign.
    #[test]
    fn split_vowels_are_reunited() {
        // `কিছু মনে` extracted with the space one place early.
        assert_eq!(reunite_split_vowels("কিছ ুমনে"), "কিছু মনে");
        assert_eq!(
            repair_unicode("গল্প বলা- কোন কিছ ুমনে রাখার"),
            "গল্প বলা- কোন কিছু মনে রাখার"
        );

        // A tab is a column boundary. Merging two cells would corrupt a table.
        assert_eq!(reunite_split_vowels("কিছ\tুমনে"), "কিছ\tুমনে");
        // Nothing to attach to: leave it alone rather than invent a join.
        assert_eq!(reunite_split_vowels("abc ুমনে"), "abc ুমনে");
        // A letter that already carries a vowel sign must not receive a second.
        // The first version did this and doubled the fault it was fixing.
        assert_eq!(reunite_split_vowels("কথা ুমনে"), "কথা ুমনে");
        // Nor may a syllable-closing mark take one.
        assert_eq!(reunite_split_vowels("বাং ুমনে"), "বাং ুমনে");

        // Correct text is untouched.
        for good in ["কিছু মনে", "সর্বোচ্চ কথা", "hello world"]
        {
            assert_eq!(reunite_split_vowels(good), good);
        }
    }

    /// English inside HTML must never be converted as Bijoy.
    #[test]
    fn html_and_english_are_not_bijoy() {
        let html = "<p className=\"digest-feature-try\">watch something while you keep working</p>";
        assert_ne!(detect(html).encoding, LegacyEncoding::SutonnyMj);
        assert_eq!(convert_document(html).lines_converted, 0);
        // Genuine Bijoy is unaffected.
        assert_eq!(detect(LEGACY_LINE).encoding, LegacyEncoding::SutonnyMj);
    }

    /// Mistyped extra vowel signs repaired, so search can find the word.
    ///
    /// The typos are in the source documents, keyed years ago; Scribe was
    /// reproducing them faithfully, which meant nobody searching for `অনুযায়ী`
    /// would ever find `অনিুযায়ী`.
    ///
    /// Note the two directions: the first word needs the *first* sign dropped,
    /// the second needs the *second*. Any fixed rule would repair one and break
    /// the other, which is why the word list decides.
    #[test]
    fn mistyped_vowels_are_repaired_only_when_a_word_results() {
        assert_eq!(repair_word("অনিুযায়ী"), "অনুযায়ী");
        assert_eq!(repair_word("নারীাদের"), "নারীদের");

        // Correct words must pass through untouched.
        for good in ["অনুযায়ী", "নারীদের", "সর্বোচ্চ", "প্রতিবন্ধী", "বাস্তবায়ন"]
        {
            assert_eq!(repair_word(good), good, "a correct word was altered");
        }
        // Two vowel signs are never valid Bengali, so an unknown word is still
        // repaired — the pre-kar is dropped as the likely intruder.
        assert_eq!(repair_word("ঝিুঝ"), "ঝুঝ");
    }

    /// A run of one repeated glyph is decoration, not an encoding.
    #[test]
    fn leader_dots_are_not_bijoy() {
        // A contents page: `ABSTRACT` followed by leader dots, which land in the
        // same byte range Bijoy uses. Its length made it look like the densest
        // Bijoy in the test data; it converted to `অইঝঞজঅঈঞৃৃৃৃ…`.
        let toc = format!("ABSTRACT{}.03", "\u{201E}".repeat(40));
        assert_ne!(detect(&toc).encoding, LegacyEncoding::SutonnyMj);
        assert_eq!(convert_document(&toc).lines_converted, 0);
        // Genuine Bijoy that is short and glyph-poor must still convert.
        assert_eq!(
            detect("e\u{00AA}\u{00A8}v\u{00DB}").encoding,
            LegacyEncoding::SutonnyMj
        );
    }

    /// Doubled joiners and vowel signs, from doubled keystrokes.
    #[test]
    fn impossible_doubles_are_collapsed() {
        // প্রস্তুতি typed with the joiner and the u-kar each struck twice.
        let broken = "\u{09AA}\u{09CD}\u{09B0}\u{09B8}\u{09CD}\u{09CD}\u{09A4}\u{09C1}\u{09C1}\u{09A4}\u{09BF}";
        assert_eq!(collapse_impossible_doubles(broken), "প্রস্তুতি");
        // A doubled *consonant* is a real word and must survive.
        assert_eq!(collapse_impossible_doubles("অন্ন"), "অন্ন");
        assert_eq!(collapse_impossible_doubles("সর্বোচ্চ"), "সর্বোচ্চ");
    }

    /// A vowel sign moved twice when a reph moved.
    ///
    /// Every word below is ordinary, high-frequency Bengali. The output was
    /// well-formed Bengali every time — just the wrong word — so no structural
    /// check could see it. Round-trip testing is what found it.
    ///
    /// `wbe©vPb` is the control: same shape, but its vowel sign belongs to a
    /// different syllable, so it was always correct. That is what proved the
    /// reordering right in general and wrong only inside the reph's own cluster.
    #[test]
    fn vowel_signs_survive_a_reph_move() {
        for (legacy, expected) in [
            ("Avw_©K", "আর্থিক"),
            ("m\u{2021}e\u{00A9}v\u{201D}P", "সর্বোচ্চ"),
            ("mvwe©K", "সার্বিক"),
            ("wbw`©\u{00F3}", "নির্দিষ্ট"),
            ("m¤úwK©Z", "সম্পর্কিত"),
            ("gv\u{2021}K©U", "মার্কেট"),
            ("wb\u{2021}`©kbv", "নির্দেশনা"),
            ("wbe©vPb", "নির্বাচন"),
        ] {
            assert_eq!(convert(legacy), expected);
        }
    }

    /// Conjuncts settled by rendering the SutonnyMJ font itself.
    ///
    /// `²` was mapped to `ক্ষ্ণ`. Rendering `j²x` in the real font shows
    /// **লক্ষ্মী**, not লক্ষ্ণী. `ক্ষ্ণ` is a different sequence — `¶è` — which
    /// the table did not carry at all.
    ///
    /// `¤œ` is here because `¤`→`ম্` and `œ`→`্ন` composed to a *doubled*
    /// joiner, which no Bengali word contains.
    #[test]
    fn conjuncts_confirmed_against_the_font() {
        assert_eq!(convert("j²x"), "লক্ষ্মী");
        assert_eq!(convert("\u{00B6}\u{00E8}"), "ক্ষ্ণ");
        let mn = convert("\u{00A4}\u{0153}");
        assert_eq!(mn, "ম্ন");
        assert!(!mn.contains("\u{09CD}\u{09CD}"), "doubled joiner: {mn:?}");
        // Ordering: a shorter key must never eat a longer one it prefixes.
        assert_eq!(convert("\u{2022}\u{00B6}"), "ঙ্ক্ষ");
    }

    /// A bullet is not evidence of an encoding.
    ///
    /// `•` maps to `ঙ্`. Because a document is split on tabs as well as
    /// newlines, a lone bullet became a segment too short to judge, inherited
    /// the document's verdict, and turned into a Bengali letter in the middle
    /// of English. Found by auditing real documents, not by any unit test.
    #[test]
    fn punctuation_only_segments_are_never_converted() {
        let doc = "GwK\tGB\tKw\u{2022}\t% of total workforce, ";
        let out = convert_document(doc).text;
        assert!(
            !out.contains('\u{0999}'),
            "a bullet became Bengali: {out:?}"
        );
        assert!(out.contains('\u{2022}'), "the bullet vanished: {out:?}");

        // Question references must survive as written.
        let refs = convert_document(&format!("{LEGACY_LINE}\tE1: \"Q1-Q18\"\t2.45")).text;
        assert!(
            refs.contains("Q1-Q18"),
            "an English reference was converted: {refs:?}"
        );
        assert!(refs.contains("2.45"), "a number was converted: {refs:?}");
    }

    #[test]
    fn wrong_conjuncts_are_gone() {
        let out = convert("D\u{2021}j\u{f8}L\u{a8} i\u{e6}wUb ev\u{af}\u{cd}evqb");
        for wrong in ["স্ন", "ম্ন", "ত্ম"] {
            assert!(!out.contains(wrong), "{wrong} reappeared in {out:?}");
        }
    }

    use super::*;

    /// A constructed legacy line: ordinary Bengali words, encoded to SutonnyMJ.
    ///
    /// Reads "সাপ্তাহিক প্রতিবেদন এবং কাজের বিবরণ" — "weekly report and
    /// description of work".
    const LEGACY_LINE: &str = "mvßvwnK cªwZ\u{2020}e`b Ges Kv\u{2020}Ri weeiY";
    const LEGACY_LINE_UNICODE: &str = "সাপ্তাহিক প্রতিবেদন এবং কাজের বিবরণ";

    /// The orthographic rules, on single words, with no length forgiveness.
    #[test]
    fn well_formed_words_pass_and_impossible_ones_do_not() {
        for good in [
            "কর্মসূচি",
            "প্রতিবেদন",
            "দ্র্যূ", // the correct spelling of the grid's সিক্স cell
            "ন্ধ্রূ",
            "ম্নূ",
            "ষ্ট্রূ",
            "ব্র্যান্ড",  // a ra-phala, which must not be mistaken for a fault
            "সর্বোচ্চ", // a reph
            "অন্ন",
            "hello", // nothing Bengali in it to judge
            "",
        ] {
            assert!(
                word_is_well_formed(good),
                "well-formed word rejected: {good}"
            );
        }

        // The four malformed entries in the reference character grid. Each is a
        // vowel sign followed by a hasant, which is structurally impossible:
        // the vowel closes the syllable, so the hasant has nothing to join.
        for impossible in ["দূ্র্য", "ন্ধূ্র", "মূ্ন", "ষ্টূ্র"]
        {
            assert!(
                !word_is_well_formed(impossible),
                "an impossible spelling was accepted: {impossible}"
            );
        }

        // Typing slips found in the reference word list, every one of which
        // Scribe already converts to the correct spelling. Each is impossible
        // by orthography, which is what lets the harness set them aside
        // without having to claim Scribe is right about them.
        for slip in [
            "আহমেদুুল", // the u-kar struck twice
            "অতিারকা",  // two vowel signs on one consonant
            "অাগে",    // আ typed as অ followed by a matra
            "আামায়",    // and again, after a real আ
            "ক্ষেোভে", // ে and ো together
        ] {
            assert!(
                !word_is_well_formed(slip),
                "a known typing slip was accepted as well formed: {slip}"
            );
        }

        // And the rules that were already here, now without the length escape.
        for impossible in ["\u{09BF}কথা", "\u{09CD}কথা", "কাা", "ক\u{09CD}\u{09BE}"]
        {
            assert!(
                !word_is_well_formed(impossible),
                "an impossible spelling was accepted: {impossible:?}"
            );
        }
    }

    #[test]
    fn unicode_bangla_is_left_alone() {
        let text = "সহজ প্রযুক্তি। সবার জন্য।";
        let (out, d) = convert_if_legacy(text);
        assert_eq!(d.encoding, LegacyEncoding::AlreadyUnicode);
        assert_eq!(out, text, "already-Unicode text must never be rewritten");
    }

    #[test]
    fn plain_english_is_left_alone() {
        let text = "Programme operations, budgets and field reports for 2026.";
        let (out, d) = convert_if_legacy(text);
        assert_eq!(d.encoding, LegacyEncoding::NotBangla);
        assert_eq!(out, text);
    }

    #[test]
    fn empty_input_is_safe() {
        assert_eq!(convert(""), "");
    }

    #[test]
    fn conversion_produces_unicode_bengali() {
        // "Avwg" is how the Bijoy encoding stores আমি ("I").
        let out = convert("Avwg");
        assert!(
            out.chars().any(|c| ('\u{0980}'..='\u{09FF}').contains(&c)),
            "expected Bengali characters, got {out:?}"
        );
    }

    #[test]
    fn pre_kar_moves_after_its_consonant() {
        // The i-kar is stored before its consonant and must end up after it.
        let out = convert("wK");
        let chars: Vec<char> = out.chars().collect();
        if let Some(kar) = chars.iter().position(|c| *c == 'ি') {
            assert!(kar > 0, "i-kar must not remain at the start: {out:?}");
        }
    }

    /// Ordinary Bengali words with their correct spellings. Every one of them
    /// was produced wrongly by the reference implementation before the reph and
    /// `ÿ` fixes.
    #[test]
    fn known_words_convert_correctly() {
        let cases = [
            ("eª¨vÛ", "ব্র্যান্ড", "ra-phala must NOT be treated as reph"),
            ("Kg©m~wP", "কর্মসূচি", "reph moves back over its consonant"),
            (
                "wi‡cvU©",
                "রিপোর্ট",
                "reph at the end of a word; crashes the reference",
            ),
            (
                "c`‡ÿc",
                "পদক্ষেপ",
                "the ÿ form of ক্ষ, missing from the reference table",
            ),
            ("¯^vÿi", "স্বাক্ষর", "ÿ again, inside a conjunct"),
            ("c~e©eZ©x", "পূর্ববর্তী", "two rephs in one word"),
        ];
        let mut failures = Vec::new();
        for (input, expected, why) in cases {
            let got = convert(input);
            if got != expected {
                failures.push(format!(
                    "  {input:?} -> {got:?}, expected {expected:?}  ({why})"
                ));
            }
        }
        assert!(failures.is_empty(), "\n{}", failures.join("\n"));
    }

    #[test]
    fn ra_phala_is_never_moved() {
        // `্র` attaches to the consonant before it and is already in the right
        // place. Moving it is the single most damaging thing Scribe could do.
        let out = convert("eª¨vÛ");
        assert!(
            !out.starts_with("বর"),
            "ra-phala was wrongly relocated: {out:?}"
        );
    }

    // ---- documents that mix encodings -----------------------------------

    #[test]
    fn a_mixed_document_converts_only_its_legacy_lines() {
        // The shape that defeated the old whole-document detector: mostly
        // Unicode Bangla, with legacy paragraphs buried inside.
        let doc = [
            "কার্যক্রমের সাপ্তাহিক প্রতিবেদন এবং পর্যালোচনা",
            LEGACY_LINE,
            "এই অংশটি ইতিমধ্যেই ইউনিকোডে লেখা আছে এবং বদলানো উচিত নয়",
        ]
        .join("\n");

        let r = convert_document(&doc);
        let out: Vec<&str> = r.text.split('\n').collect();

        assert_eq!(
            r.lines_converted, 1,
            "expected exactly one legacy line converted"
        );
        assert_eq!(
            out[0], "কার্যক্রমের সাপ্তাহিক প্রতিবেদন এবং পর্যালোচনা",
            "an already-Unicode line was altered"
        );
        assert_eq!(
            out[2], "এই অংশটি ইতিমধ্যেই ইউনিকোডে লেখা আছে এবং বদলানো উচিত নয়",
            "an already-Unicode line was altered"
        );
        assert_eq!(
            out[1], LEGACY_LINE_UNICODE,
            "the legacy line was not converted"
        );
    }

    #[test]
    fn english_lines_are_never_converted_even_beside_legacy_text() {
        let doc = [
            "Programme operations and budget review for the 2026 cycle.",
            "Kg©m~wP",
            "Prepared by the review team. All figures are provisional.",
        ]
        .join("\n");
        let r = convert_document(&doc);
        let out: Vec<&str> = r.text.split('\n').collect();
        assert_eq!(
            out[0],
            "Programme operations and budget review for the 2026 cycle."
        );
        assert_eq!(
            out[2],
            "Prepared by the review team. All figures are provisional."
        );
    }

    #[test]
    fn a_wholly_legacy_document_still_converts_completely() {
        let doc = ["eª¨vÛ", LEGACY_LINE, "c~e©eZ©x wfwR‡U"].join("\n");
        let r = convert_document(&doc);
        assert!(
            r.lines_converted >= 3,
            "expected every text line converted, got {}",
            r.lines_converted
        );
        assert_eq!(r.dominant, LegacyEncoding::SutonnyMj);
        assert!(r.text.contains("ব্র্যান্ড"));
    }

    #[test]
    fn a_short_heading_beside_legacy_text_is_carried_with_it() {
        // "eª¨vÛ" alone is too short to judge, but the document has proved it
        // holds legacy text, and the line carries Bijoy-range bytes.
        let doc = ["eª¨vÛ", LEGACY_LINE].join("\n");
        let r = convert_document(&doc);
        assert!(
            r.text.starts_with("ব্র্যান্ড"),
            "short heading left unconverted: {:?}",
            r.text
        );
    }

    #[test]
    fn an_unfamiliar_bengali_word_in_a_table_cell_still_converts() {
        // An 11-character cell, one character under the bar to judge alone,
        // holding a word the lexicon does not know.
        let doc = [LEGACY_LINE, "\n`vwqZ¡‡eva\t\u{2610}"].concat();
        let r = convert_document(&doc);
        // Asserted as a property, not an exact string. `য়` has two legal
        // spellings and this comparison has already broken on that twice; what
        // matters is that no Bijoy survives and Bengali came out.
        let bengali = r
            .text
            .chars()
            .filter(|c| ('\u{0980}'..='\u{09FF}').contains(c))
            .count();
        assert!(bengali > 8, "little or no Bengali produced: {:?}", r.text);
        assert!(
            !r.text.contains("vwqZ"),
            "the cell was left as Bijoy: {:?}",
            r.text
        );
    }

    #[test]
    fn a_short_english_heading_is_left_alone() {
        let doc = ["Annex 4", LEGACY_LINE].join("\n");
        let r = convert_document(&doc);
        assert!(
            r.text.starts_with("Annex 4"),
            "an English heading was corrupted: {:?}",
            r.text
        );
    }

    #[test]
    fn symbol_heavy_english_is_not_mistaken_for_bijoy() {
        // A real false positive: an English document whose rules and dashes
        // made it look non-ASCII enough to convert.
        for line in [
            "═══════════════════════════════════════════════",
            "─────────────────────────────────────────────",
            "Consultant: Riverbank Advisory Group LLC — New York",
            "•  Doc #1 — Board Charter / Terms of Reference",
            "Café résumé naïve — accented English is still English",
        ] {
            assert_ne!(
                detect(line).encoding,
                LegacyEncoding::SutonnyMj,
                "symbol- or accent-heavy English was flagged as Bijoy: {line:?}"
            );
        }
    }

    #[test]
    fn a_document_with_no_legacy_text_is_returned_untouched() {
        let doc = "Budget 2026\nProgramme operations\nসম্পূর্ণ ইউনিকোড বাংলা লেখা";
        let r = convert_document(doc);
        assert_eq!(r.text, doc);
        assert_eq!(r.lines_converted, 0);
    }

    #[test]
    fn legacy_headings_in_a_table_row_are_converted() {
        // Word emits a table row as one line of tab-separated cells. Each
        // heading alone is too short to judge, so the whole row used to survive
        // as gibberish. The cells read "অফিসের নামঃ", "তারিখঃ", "বিভাগঃ".
        let row = "Awd†mi bvgt\tZvwiLt\twefvMt";
        let doc = format!("{LEGACY_LINE}\n{row}");
        let r = convert_document(&doc);
        assert!(
            !r.text.contains("Awd†mi") && !r.text.contains("ZvwiLt"),
            "table-cell Bijoy survived: {:?}",
            r.text
        );
        assert!(
            r.text.contains("\t"),
            "the row's column boundaries were lost"
        );
    }

    #[test]
    fn english_table_cells_are_still_left_alone() {
        let doc = "Region\tTotal\tBalance\nDhaka\t1200\t340";
        assert_eq!(convert_document(doc).text, doc);
    }

    #[test]
    fn line_structure_is_preserved_exactly() {
        let doc = "a\n\nb\n\n\nc";
        assert_eq!(
            convert_document(doc).text.matches('\n').count(),
            doc.matches('\n').count()
        );
    }

    #[test]
    fn tables_are_ordered_longest_first() {
        // A shorter key appearing before a longer key it prefixes would mean
        // the longer conjunct could never match.
        for (idx, (key, _)) in tables::CONVERSION_MAP.iter().enumerate() {
            for (later, _) in tables::CONVERSION_MAP.iter().skip(idx + 1) {
                assert!(
                    !later.starts_with(key) || later == key,
                    "{later:?} can never match: {key:?} is applied first"
                );
            }
        }
    }
}
