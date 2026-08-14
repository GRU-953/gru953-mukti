# GRU953 Mukti

**Turn legacy Bijoy / SutonnyMJ Bangla into Unicode — word by word, leaving
everything else alone.**

Old Bangla documents are not really Bangla to a computer. They are English
letters that *look* Bangla because a font draws Bangla shapes over them. They
cannot be searched, cannot be spell-checked, and become nonsense on any machine
without that font.

Mukti converts them. English, numbers, and Bangla that is already correct come
through **byte for byte unchanged**.

```
Awd†mi bvgt Kg©m~wP     →   অফিসের নামঃ কর্মসূচি
Programme review 2026   →   Programme review 2026     (untouched)
এই অংশটি ইউনিকোডে আছে    →   এই অংশটি ইউনিকোডে আছে      (untouched)
```

Works completely offline. No account, no upload, no network.

---

> ## ⚠️ The desktop window is gone as of 0.6.0
>
> Mukti is now **only** a command-line tool. The window shipped in 0.3.0 through
> 0.5.0 and has been removed — one thing done well rather than two adequately.
>
> **If you used the window:** everything it did, the command line does. Converting
> a whole folder is one command rather than one file at a time, which the window
> could never do.
>
> **If you are on 0.4.0, upgrade regardless of which you used.** That version had
> two faults: the window did not respond to anything, and three flaws in the
> libraries that read PDF and Office files
> ([RUSTSEC-2026-0187](https://rustsec.org/advisories/RUSTSEC-2026-0187),
> [-0194](https://rustsec.org/advisories/RUSTSEC-2026-0194),
> [-0195](https://rustsec.org/advisories/RUSTSEC-2026-0195)) meant a crafted file
> could stop the program or exhaust its memory. Both are fixed.
>
> **Anything you converted with an earlier command-line tool is fine.** That part
> always worked, and the conversion itself has only improved since.

---

## Install

Download from the [latest release](https://github.com/GRU-953/gru953-mukti/releases/latest)
and put the file on your `PATH`.

| Your computer | Download |
|---|---|
| **macOS** (Intel or Apple silicon) | `mukti-universal-apple-darwin` |
| **Windows** | `mukti-x86_64-pc-windows-msvc.exe` |
| **Linux** | `mukti-x86_64-unknown-linux-gnu` |

On macOS and Linux, make it executable first: `chmod +x mukti-*`.

> **The binaries are not signed.** macOS may say "unidentified developer" — allow
> it once in *System Settings → Privacy & Security*. Windows SmartScreen may warn:
> *More info*, then *Run anyway*. Signing needs paid certificates from Apple and a
> certificate authority, and neither is set up.

**Mukti is a command-line tool.** There was a desktop window until version 0.5.0;
it was removed in 0.6.0 to keep one thing working well rather than two adequately.
Everything it did, the command line does — and more, because it converts whole
folders in one go.

## Use it

```sh
mukti check report.docx     # say what would change, write nothing
mukti convert report.docx   # writes report.unicode.docx, formatting intact
mukti convert *.txt         # many files at once
```

Your original is never overwritten unless you type `--in-place`. A file Mukti
named itself is never replaced unless you add `--force`.

## What it handles

| Format | What happens |
|---|---|
| `.txt` `.csv` `.md` `.json` | Converted. Windows-1252 detected automatically — which is what legacy Bangla files usually are |
| `.docx` `.xlsx` `.pptx` | Converted **inside the document**. Formatting, tables and images untouched. Includes SmartArt, charts, speaker notes and comments |
| `.pdf` | **Read-only, best effort.** Text extracted and converted; layout is lost |
| Older `.doc` `.xls` `.ppt` | Converted into a **new** `.docx`, `.xlsx` or `.pptx` beside the original. **Text only** — these formats hold no formatting we can carry, and no font information, so accuracy matches plain text rather than the higher figure above |

Verified across **every one of 2,315 documents** in a real archive — not a sample.

**127 legacy font families are recognised**, read from the internal name tables of the
fonts Bijoy ships rather than guessed from filenames. Font names that merely look
legacy are excluded on evidence: `SutonnyOMJ` is a Unicode font despite its name, and
`+mj-lt` is not a font name at all but an Office theme reference.
Word count and whitespace preserved, every archive entry intact, no legacy font left
behind, and converting an already-converted file changes nothing further. That last
one matters most: a converter that mangles its own output looks perfectly correct on
a single pass.

Checking all of them rather than 300 found four faults a sample had missed,
including one that moved text between runs in documents containing no legacy Bangla
at all. Re-run it yourself with `cargo run --release -p corpus-verify -- <folder>`.

## Accuracy

Measured, each with its sample size. To reproduce them you need your own document
set — the material these were measured against is private and cannot be shipped
(see `HANDOVER.md` §6 for what kind of set works). Then, in order:

```
cargo run --release -p corpus-label  -- "<your documents folder>"
cargo run --release -p lexicon-build -- --corpus "<your word list folder>" --mode extended
cargo run --release -p eval          -- --corpus "<your word list folder>"
```

The first two write to `local/`, which is where `eval` looks by default, so the
third command needs nothing but the word list. `--corpus` has no default on
purpose: it names material that is not in this repository, and a default would
only produce a confusing error.

**`eval` exits with a failure if any target is missed.** Until 13 August 2026 it
printed "NOT MET" and exited successfully, so a script asking whether the figures
still held was told yes regardless.

Re-measured from scratch on 13 August 2026, on a freshly rebuilt answer key of
3,782,953 labelled tokens from 1,377 real documents.

| | Result | Sample |
|---|---|---|
| Conversion, word accuracy | **99.989%** | 473,244 words |
| Conversion, character accuracy | 99.997% | 3,879,440 characters |
| Every consonant × every vowel and conjunct | **100%** | 3,096 combinations |
| Detection, legacy words found | **99.962%** | 177,079 words |
| Detection, English wrongly converted | **0.014%** | 494,050 words |
| Detection, Unicode Bangla wrongly converted | **0.000%** | 436,952 words |
| Misspellings preserved, not "corrected" | 99.979% | 14,214 words |

The detection figures come from a **held-out** half of the data, never looked at
while anything was tuned.

**Two of these moved, and both are worth saying out loud.** Recall improved
slightly, on a larger sample. But English wrongly converted went from 0.006% to
**0.014%** — still comfortably inside the 0.1% target, and still the wrong
direction. Two things account for it: the held-out half is now a different set of
documents (see below), and a class of false positive was found that this corpus
cannot show — an accented European name like `Tomáš` has its accented letters
inside the byte range Bijoy reuses, so it can be mistaken for legacy Bangla.

**The old figures cannot be reproduced exactly, and that is not a caveat being
buried.** Which documents land in the held-out half is decided by a hash of each
file's path, and the document archive has moved. So the halves are a different
split of the same material. Re-measuring was the honest option; quoting figures
whose answer key no longer existed was not.

<details>
<summary><b>How that was measured — including what it cannot tell you</b></summary>

**Conversion** is measured by round trip: take real Unicode Bangla, encode it
into Bijoy, convert it back, compare. The source is the answer key. This
**cannot detect an error the encoder and decoder share** — if both are wrong in
matching ways, the word returns intact and the harness sees nothing. It is an
upper bound.

**Detection** is measured on real documents, which label themselves: a `.docx`
records the font of every run of text, so a run set in SutonnyMJ *is* legacy and
an English run *is not*. No hand-labelling, and the code under test is never
asked what it thinks. Runs declaring no font are excluded rather than guessed
at, as are runs whose declared font contradicts their own bytes.

**Accuracy on real documents: about 99.9%, and here is how that is known.**

The direct measurement is that **94.053%** of converted words, over real
documents, are found in the dictionary (177,071 words). It has moved twice, both
times upward and both times because a fault was found and fixed: 94.015% →
94.023% when a lost halant was corrected, and → **94.053%** on 14 August 2026
when byte 0xD0 was found to be the conjunct ণ্ড rather than a dash — 123
occurrences across 27 words, every one of which had been coming out with a hyphen
in the middle. That is a **floor**, not an accuracy: names, places, acronyms and rare words are in no word list, so a
perfectly converted word can be missing from one. Earlier versions of this page
reported 78.4%, which was simply wrong — it came from a superseded answer key.

So the remaining residue was sampled and read. The study below was run against
the 5.985% that remained at 94.015%; the two later fixes each moved a handful of
words out of the residue and into the dictionary, which cannot make the estimate
below worse, but the study has not been re-run and the figures are the original
ones. 200 of them were
drawn at random with a recorded seed, so the same sample can be drawn again, and
each was classified by hand with the original Bijoy shown beside the output —
because the Bengali alone cannot separate "rare word" from "wrong word".

| What the residue actually contains | of 200 |
|---|---|
| The output is exactly what the source encodes | 187 |
| **A genuine mis-conversion** | **3** |
| The source was not a word at all — a fragment or a stray symbol | 4 |
| A name | 1 |
| Could not be judged honestly | 5 |

Most of the residue is compound words, pairs joined by a slash or hyphen,
transliterated English, and — most of all — **source misspellings that Mukti
faithfully preserved**, which is exactly what it is supposed to do.

Correcting the floor by the measured mis-conversion rate:

| | Estimate |
|---|---|
| Treating the five unjudgeable cases as correct | **99.908%** [99.737, 99.969] |
| Treating all five as errors | **99.756%** [99.530, 99.875] |

**Two checks on that.** A second rater, given only the word pairs and the
classification rules and no knowledge of this project, independently found
**exactly 3** mis-conversions — a partly different three, giving 99.909%. Raw
agreement was 96% (Cohen's κ 0.607, depressed by how lopsided the categories are
rather than by real disagreement). And the round-trip figure of 99.989% is an
*upper* bound by construction: every estimate here sits below it, which is what a
consistent pair of measurements must do.

**What this still cannot tell you.** At 200 words the interval is roughly ±3
points near these rates, so this settles "is the residue mostly names or mostly
errors?" and will not support finer ranking. All 3 confirmed faults are the same
kind — the **reph (`র্`) or a vowel sign landing on the wrong consonant**, plus one
dropped character — which is a specific thing to fix rather than a vague error
rate. And one honest note on method: the classification rules had to be clarified
mid-study, because the corpus is full of source misspellings and reproducing one
faithfully is correct behaviour, not an error. That clarification moved the result
*upwards*, so it is recorded here rather than left implicit.

**PDF quality varies widely.** Across 60 legacy-font PDFs: 28 good (70%+ real
words), 19 fair, 7 poor, 6 produced nothing. Median 71.3%.
</details>

## Why only the right words change

Bijoy **is** ASCII wearing Bangla shapes. `bvg` is the word নাম, and it is also
three ordinary Latin letters, and nothing inside the word can tell you which.

So Mukti reaches **three** verdicts, not two — legacy, not legacy, and
genuinely uncertain — and lets the surrounding words settle the last of them. In
the measured data, **72% of legacy words carrying no evidence at all** of being
legacy are recovered from their neighbours alone.

The thresholds are deliberately lopsided. Missing a legacy word leaves it
unreadable: visible, annoying, fixable. Converting a word that was *not* legacy
destroys readable text and the reader may never notice. **When the evidence runs
out, the answer is "leave it alone".**

## How it works

Three things must happen, in order, and the middle one is why this is not a
character swap:

1. map each glyph to its Unicode letter, longest conjuncts first;
2. **move vowel signs, reph and nukta to where Unicode expects them**;
3. tidy up the two-part vowels.

Bijoy stores `ি` *before* its consonant, because that is where it is drawn.
Unicode stores it *after*. Skip step 2 and every such word comes out silently
wrong — still well-formed Bangla, just not the word that was written.

**No machine learning.** A deterministic table lookup, hand-written reordering
rules, and two dictionaries built into the binary: 451,348 Bangla words and
234,428 English ones. Same input, same output, on any machine, offline.

## Build it yourself

```sh
cargo test --workspace     # 144 tests, no network needed
cargo run -p mukti-cli    # the command-line tool
```

The dictionaries are compiled and checked in, so building needs no corpus, no
network and no system libraries — a Rust toolchain is the only requirement, on
all three platforms.

| Crate | What it is |
|---|---|
| `crates/mukti-core` | The converter, the detector, the dictionaries |
| `crates/mukti-formats` | Word, Excel, PowerPoint and PDF handling |
| `crates/mukti-cli` | The `mukti` command |
| `devtools/` | Dictionary builder, corpus labeller, accuracy harness. Not shipped |

## Known limitations

- The binaries are **not signed**; the first run warns that the maker cannot be checked.
- Older `.doc`, `.xls`, `.ppt` are read, but **only their text**. Formatting,
  tables and images are not carried across, and because these formats record no
  font, the conversion is decided from the words alone — the same accuracy as a
  plain text file, not the font-gated accuracy quoted for `.docx`.
- **PDF layout is lost**, and quality varies widely. Text is joined into real
  lines, but headings, columns and tables are not recovered: measured on 80
  documents, 3 had tables whose rows cannot be reconstructed, so a figure may
  appear away from the row it belongs to. Check any converted table against the
  original.
- The interface is **English only** for now. Every string sits in one table, so
  Bangla is a translation rather than a rebuild.
- **Measured on SutonnyMJ.** Every accuracy figure above comes from documents in
  that font, which is what the test archive is overwhelmingly written in — half a
  million text runs, against about a thousand for the next legacy font.
  Mukti now *recognises* 127 legacy families, taken from the internal name tables
  of the fonts Bijoy itself ships, but recognising a font is not the same as
  having measured it, and only SutonnyMJ has been measured.
- **`SutonnyOMJ` is deliberately treated as NOT legacy**, which corrects a claim
  this page used to make. Despite the `MJ` in its name it is a Unicode font: the
  copy its own vendor serves has 97 codepoints in the Bengali block and no glyph
  at all where a legacy font keeps its letters. Its text is already correct, so
  converting it would damage it.

## Contributing

Issues and pull requests welcome. If you find a word converted that should not
have been — measured at 6 in every 100,000 English words — please report it with
the text. Those cases are the most valuable.

Work happens on `development`; `main` is what was last released.

If you are picking this project up rather than dipping into it, start with
[`HANDOVER.md`](./HANDOVER.md) — how it is built, how the accuracy was measured,
what is still open, and the traps that cost time here.

## Licence

[MIT](./LICENSE). Third-party obligations, including the fonts and the word
lists: [`THIRD-PARTY-LICENSES`](./THIRD-PARTY-LICENSES).

More: [how to use it](./USING-MUKTI.md) · [what changed](./CHANGELOG.md)
