# GRU953 Scribe

Convert legacy **Bijoy / SutonnyMJ** Bangla into proper **Unicode** Bengali —
word by word, so English, numbers and Bengali that is *already* Unicode come
through exactly as they went in.

Works offline. Nothing you convert leaves your machine. The Bangla fonts are
bundled, so it looks the same on every platform.

```sh
scribe convert report.docx      # writes report.unicode.docx, formatting intact
scribe convert notes.txt        # writes notes.unicode.txt
scribe check *.docx             # says what would change, writes nothing
```

Or open the app and paste text into it.

## What it does

Bijoy-family encodings are a **font hack**. The bytes in the file are ordinary
ASCII and Latin-1; they only look Bengali because a font draws Bengali shapes
on top of them. Worse, the bytes are stored in the order the glyphs are
**drawn**, not the order the letters are **spoken**. Unicode stores the spoken
order.

So converting is not a character swap. The clearest case is the i-kar: Bijoy
stores `ি` *before* its consonant, because that is where it is drawn; Unicode
stores it *after*. Skip the reordering and every such word comes out silently
wrong — still well-formed Bengali, just not the word that was written.

## Accuracy

Every figure below is measured, with its sample size. Re-run them yourself with
`cargo run --release -p eval`.

| What | Result | Sample |
|---|---|---|
| **Conversion**, word accuracy | **99.989%** | 473,244 words |
| Conversion, character accuracy | 99.997% | 3,879,440 characters |
| Character grid — every consonant × every vowel and conjunct | **100%** | 3,096 combinations |
| **Detection**, recall on legacy words | **99.951%** | 154,928 words |
| **Detection**, false positives on English | **0.006%** | 462,074 words |
| Detection, false positives on Unicode Bengali | **0.000%** | 343,077 words |
| Misspellings preserved rather than "corrected" | 99.979% | 14,214 words |

The detection figures come from a **held-out** half of the data that was never
looked at while anything was being tuned. The tuning half gave 99.962% and
0.014% on the same code, so these are not a lucky draw.

### How that was measured, including what it cannot tell you

**Conversion** is measured by round trip: take real Unicode Bengali, encode it
into Bijoy, convert it back, compare. The source text is the answer key. This
**cannot detect an error the encoder and the decoder share** — if both are
wrong in matching ways the word returns intact and the harness sees nothing.
It is an upper bound.

**Detection** is measured against real documents, which label themselves: a
`.docx` records the font of every run of text, so a run set in SutonnyMJ *is*
legacy and an English run *is not*. No hand-labelling, and no asking the code
under test what it thinks. Runs that declare no font are excluded rather than
guessed at, as are runs whose declared font contradicts their bytes.

**One figure is deliberately not quoted as a headline.** Converted words found
in the dictionary, run over real legacy documents, sits at 78.4%. That is a
*lower* bound, not an error rate: names, places, acronyms and rare words are
absent from any word list. It is reported by the harness but it should not be
read as "21.6% wrong" until the residue has been sampled and classified.

## What it converts

| Format | What happens |
|---|---|
| `.txt` `.csv` `.md` `.json` | Converted. Windows-1252 is detected automatically, which is what most legacy Bangla files actually are |
| `.docx` `.xlsx` `.pptx` | Converted **in place**: formatting, tables, images and layout untouched. Includes SmartArt, charts, speaker notes and comments |
| `.pdf` | **Read-only, best effort.** Text is extracted and converted; layout is not preserved |
| Anything else | Left alone |

Verified across 300 randomly chosen Office documents from a real archive: word
count preserved on all 300, whitespace identical on all 300, every archive
entry intact, no legacy font left behind.

### The PDF caveat, in full

A PDF has no words and no spaces, only glyphs at coordinates, so spacing is
inferred and tables come out as running text. Text drawn in a subsetted or
symbolic font is **skipped and counted**, never guessed at — guessing produces
convincing Bengali nonsense, which is worse than a gap.

Measured on 60 legacy-font PDFs: 28 came out good (70%+ real words), 19 fair,
7 poor, 6 produced no Bengali at all. Median 71.3%. Treat it as a useful
best effort, not a guarantee.

## Why only the right words change

Bijoy **is** ASCII wearing Bengali shapes, so `bvg` is the word নাম and it is
also three ordinary Latin letters, and nothing inside the word can tell you
which. Scribe therefore reaches three verdicts, not two — legacy, not legacy,
and genuinely uncertain — and lets the surrounding words settle the last of
those. In the measured data, 72% of legacy words that carry *no evidence
whatsoever* of being legacy are recovered from their neighbours alone.

The thresholds are deliberately asymmetric. Missing a legacy word leaves it
unreadable, which is visible and fixable. Converting a word that was *not*
legacy destroys readable text and the reader may never notice. **When the
evidence runs out, the answer is "leave it alone".**

## Building

```sh
cargo test --workspace     # 100+ tests, no network needed
cargo run -p scribe-app    # the desktop app
```

The dictionaries are compiled and checked in, so building needs no corpus and
no network.

## This is not a model

No machine learning, no training, no inference. A deterministic table lookup, a
set of hand-written reordering rules, and two dictionaries. The same input
always gives the same output, on any machine, offline.

## Licence

[MIT](./LICENSE). Third-party obligations, including the fonts and the word
lists: [`THIRD-PARTY-LICENSES`](./THIRD-PARTY-LICENSES).
