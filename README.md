# gru953-scribe

Convert legacy **Bijoy / SutonnyMJ** Bangla text into proper **Unicode** Bengali.

Zero dependencies. Pure Rust. Deterministic.

```toml
[dependencies]
gru953-scribe = "0.1"
```

```rust
use gru953_scribe::{convert, convert_document, detect, LegacyEncoding};

// Convert one string that you already know is legacy-encoded.
assert_eq!(convert("Kg©m~wP"), "কর্মসূচি");

// Or let it decide for itself, line by line, across a whole document.
let result = convert_document(text);
println!("{} of {} lines converted", result.lines_converted, result.lines_total);

// Or just ask what a piece of text is.
match detect(text).encoding {
    LegacyEncoding::SutonnyMj    => println!("legacy Bijoy"),
    LegacyEncoding::AlreadyUnicode => println!("already Unicode Bengali"),
    LegacyEncoding::NotBangla    => println!("not Bangla"),
}
```

## What it does

Bijoy-family encodings are a **font hack**. The bytes stored in the file are
ordinary ASCII and Latin-1; they only look Bengali because a font draws Bengali
shapes on top of them. Worse, the bytes are stored in the order the glyphs are
**drawn**, not the order the letters are **spoken**. Unicode stores the spoken
order.

So converting is not a character swap. Three things must happen, in order:

1. map each glyph to its Unicode letter, longest conjuncts first;
2. move vowel signs, reph and nukta to where Unicode expects them;
3. tidy up the two-part vowels and other details.

The clearest example is the i-kar. Bijoy stores `ি` *before* its consonant,
because that is where it is drawn; Unicode stores it *after*. Skip step 2 and
every such word comes out silently wrong — still well-formed Bengali, just not
the word that was written.

## Public API

| Item | What it is |
|---|---|
| `convert(&str) -> String` | Convert legacy text, unconditionally. |
| `convert_if_legacy(&str) -> (String, Detection)` | Convert only if the whole string looks legacy. |
| `convert_document(&str) -> DocumentConversion` | Convert **line by line**. Use this for real files. |
| `detect(&str) -> Detection` | What is this text? Confidence included. |
| `LegacyEncoding`, `Detection`, `DocumentConversion` | The result types. |
| `repair_unicode(&str) -> String` | Fix Bengali that is *already* Unicode but was badly converted by something else. |
| `bengali_is_plausible(&str) -> bool` | Is this text arranged the way Bengali actually works? |
| `roundtrip::to_bijoy(&str) -> String` | The reverse: Unicode Bengali **into** Bijoy. |
| `roundtrip::round_trip_report(&str)` | Which words fail to survive a round trip. |
| `lexicon::STEMS`, `lexicon::reads_as_bengali` | The small Bengali word list used to tell a real conversion from a fake one. |

Use `convert_document` rather than `convert` on anything file-sized. Real
documents mix encodings — Unicode headings, legacy body text and plain English
in one file, because they were edited over years by different people. Judging a
whole file at once lets the majority silently decide for the minority.

Running `convert` over text that is **already** Unicode will corrupt it. That is
why `detect` exists.

## Accuracy, and its caveat

**99.771% character accuracy, 95% CI [99.767, 99.775], measured over 47.3
million characters** from a large real-world Bengali document archive.

Read that figure with its caveat, which matters:

- It comes from **round-trip testing** — take real Unicode Bengali, encode it
  into Bijoy with `to_bijoy`, convert it back, and compare. The source text is
  the answer key.
- Round-trip testing **cannot detect an error where the encoder and the decoder
  share the same mistake.** If `to_bijoy` and `convert` are wrong in exactly
  matching ways, the text still comes back intact and the harness sees nothing.
- So 99.771% is an **upper bound**, not a guarantee.
- It was measured on **one** archive. Your documents may differ.

Nothing here is battle-tested beyond that. It is one measurement, on one body of
text, by one method with a known blind spot.

## Bijoy variants

"Bijoy" is not a single standard. The tables here are tuned for **SutonnyMJ**
(and its close relative SutonnyOMJ), which is by far the most common in
practice. Other Bijoy-family fonts differ in places, and text produced by them
may convert imperfectly or not at all.

Where a document's real glyphs were found to be missing from the upstream
reference table, the additions are kept in `CORRECTIONS` in `src/lib.rs` rather
than merged into the generated tables, so what is ported and what is added
locally stay visible.

## This is a port, not a model

There is no machine learning here, no training data, no inference. It is a
deterministic table lookup plus a set of hand-written reordering rules. The same
input always gives the same output, on any machine, offline.

The mapping tables and the core reordering rules are **ported from
[`almehady/Bijoy-to-Unicode-File-Converter`](https://github.com/almehady/Bijoy-to-Unicode-File-Converter)
(MIT)**. Its full licence text is in
[`THIRD-PARTY-LICENSES`](./THIRD-PARTY-LICENSES) and must ship with any copy of
this crate.

Two deliberate deviations from that reference:

- It indexes with Python semantics, where `text[i - 1]` at `i == 0` silently
  returns the *last* character and `text[i + 2]` past the end raises. Both are
  latent faults. This port treats an out-of-range position as "no character",
  which is the intended meaning.
- It moves a `র` + hasant sequence in both directions. A **reph** (`র` before
  hasant) belongs before its cluster; a **ra-phala** (hasant before `র`) is
  already in the right place and must not move. Moving both corrupts every word
  containing a ra-phala.

A second widely-cited Bijoy implementation was examined and **rejected**: it
carries no licence at all, so copying from it would have been a licence breach.

## Building

```sh
cargo test
```

No dependencies, no build script, no network. Bengali text in the tests is
either ordinary dictionary vocabulary or constructed examples.

## Licence

[PolyForm Noncommercial License 1.0.0](./LICENSE) — free for noncommercial use.
For a commercial licence, contact the copyright holder.

Third-party obligations: see [`THIRD-PARTY-LICENSES`](./THIRD-PARTY-LICENSES).
