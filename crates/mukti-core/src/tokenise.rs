//! Split text into words and the gaps between them, losing nothing.
//!
//! The guarantee this module exists to provide: **concatenating the pieces
//! reproduces the input byte for byte.** Everything downstream rewrites some
//! words and leaves others alone, and "leaves alone" has to mean *exactly*
//! that — the same spaces, the same tabs, the same line endings, the same
//! trailing whitespace nobody ever looks at.
//!
//! Splitting is on whitespace only. That may look crude next to splitting off
//! punctuation, but it is deliberate twice over. Bijoy **is** ASCII wearing
//! Bengali shapes, so several characters that look like punctuation are
//! letters in it — `©` is a reph, `~` and `†` are vowel signs — and a
//! tokeniser that stripped punctuation would tear those out of the middle of
//! words. And the labelled ground truth is whitespace-delimited, so this keeps
//! what is measured identical to what is judged.

/// One piece of the input: a run of non-whitespace, or a run of whitespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Segment<'a> {
    pub text: &'a str,
    pub kind: Kind,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// A run of non-whitespace. Candidate for conversion.
    Word,
    /// A run of whitespace. Never converted, never altered.
    Gap,
}

/// Split `input` into alternating words and gaps.
///
/// The segments are borrowed from `input`, so this allocates only the vector.
pub fn tokenise(input: &str) -> Vec<Segment<'_>> {
    let mut out = Vec::new();
    let mut start = 0usize;
    let mut current: Option<Kind> = None;

    for (i, c) in input.char_indices() {
        let kind = if c.is_whitespace() {
            Kind::Gap
        } else {
            Kind::Word
        };
        match current {
            Some(k) if k == kind => {}
            Some(k) => {
                out.push(Segment {
                    text: &input[start..i],
                    kind: k,
                });
                start = i;
                current = Some(kind);
            }
            None => current = Some(kind),
        }
    }
    if let Some(k) = current {
        out.push(Segment {
            text: &input[start..],
            kind: k,
        });
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The one property everything else rests on.
    #[test]
    fn the_pieces_always_reassemble_into_the_original() {
        for input in [
            "",
            " ",
            "one",
            "one two",
            "  leading and trailing  ",
            "tabs\tand\nnewlines\r\n",
            "\n\n\n",
            "Kg©m~wP এবং English mixed",
            "কর্মসূচি\tপ্রতিবেদন\nআরও",
            "punctuation, stays. attached! (yes)",
        ] {
            let rebuilt: String = tokenise(input).iter().map(|s| s.text).collect();
            assert_eq!(rebuilt, input, "reassembly changed the text: {input:?}");
        }
    }

    #[test]
    fn words_and_gaps_alternate_and_carry_the_right_kind() {
        let segments = tokenise("  Kg©m~wP  এবং\tok\n");
        let kinds: Vec<Kind> = segments.iter().map(|s| s.kind).collect();
        assert_eq!(
            kinds,
            vec![
                Kind::Gap,
                Kind::Word,
                Kind::Gap,
                Kind::Word,
                Kind::Gap,
                Kind::Word,
                Kind::Gap
            ]
        );
        let words: Vec<&str> = segments
            .iter()
            .filter(|s| s.kind == Kind::Word)
            .map(|s| s.text)
            .collect();
        assert_eq!(words, vec!["Kg©m~wP", "এবং", "ok"]);
    }

    /// Bijoy's letters include characters that look like punctuation. Tearing
    /// them off the word would destroy it.
    #[test]
    fn bijoy_letters_that_look_like_punctuation_stay_in_the_word() {
        // `©` is a reph, `~` a vowel sign. This is কর্মসূচি.
        let segments = tokenise("Kg\u{a9}m~wP");
        assert_eq!(segments.len(), 1);
        assert_eq!(segments[0].text, "Kg\u{a9}m~wP");
        assert_eq!(segments[0].kind, Kind::Word);
    }

    #[test]
    fn an_empty_input_produces_no_segments() {
        assert!(tokenise("").is_empty());
    }
}
