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

## Install

Download from the [latest release](https://github.com/GRU-953/gru953-mukti/releases/latest).

| Your computer | Download |
|---|---|
| **macOS** (Intel or Apple silicon) | `GRU953.Mukti_0.4.0_universal.dmg` |
| **Windows** | `GRU953.Mukti_0.4.0_x64-setup.exe` |
| **Ubuntu / Debian** | `GRU953.Mukti_0.4.0_amd64.deb` |
| **Other Linux** | `GRU953.Mukti_0.4.0_amd64.AppImage` |

For the command line, download the `mukti-*` file for your platform and put it
on your `PATH`.

> **The installers are not signed.** macOS will say "unidentified developer" —
> right-click the app, choose *Open*, then *Open* again. Windows SmartScreen
> will warn — click *More info*, then *Run anyway*. Signing needs paid
> certificates from Apple and a certificate authority; neither is set up.

## Use it

**In the app:** paste text on the left, or drop a file on the window. The result
appears on the right. **Show what changed** marks every word Mukti touched, so
you can check its judgement rather than trust it.

**On the command line:**

```sh
mukti check report.docx     # say what would change, write nothing
mukti convert report.docx   # writes report.unicode.docx, formatting intact
mukti convert *.txt         # many files at once
```

Your original is never overwritten unless you type `--in-place`.

## What it handles

| Format | What happens |
|---|---|
| `.txt` `.csv` `.md` `.json` | Converted. Windows-1252 detected automatically — which is what legacy Bangla files usually are |
| `.docx` `.xlsx` `.pptx` | Converted **inside the document**. Formatting, tables and images untouched. Includes SmartArt, charts, speaker notes and comments |
| `.pdf` | **Read-only, best effort.** Text extracted and converted; layout is lost |
| Older `.doc` `.xls` `.ppt` | Not supported — save as the newer format first |

Verified across 300 randomly chosen documents from a real archive: word count
preserved on all 300, whitespace identical on all 300, every archive entry
intact, no legacy font left behind.

## Accuracy

Measured, each with its sample size. Reproduce with `cargo run --release -p eval`.

| | Result | Sample |
|---|---|---|
| Conversion, word accuracy | **99.989%** | 473,244 words |
| Conversion, character accuracy | 99.997% | 3,879,440 characters |
| Every consonant × every vowel and conjunct | **100%** | 3,096 combinations |
| Detection, legacy words found | **99.951%** | 154,928 words |
| Detection, English wrongly converted | **0.006%** | 462,074 words |
| Detection, Unicode Bangla wrongly converted | **0.000%** | 343,077 words |
| Misspellings preserved, not "corrected" | 99.979% | 14,214 words |

The detection figures come from a **held-out** half of the data, never looked at
while anything was tuned. The tuning half gave 99.962% and 0.014% on the same
code, so these are not a lucky draw.

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

**One figure is deliberately not a headline.** Converted words found in the
dictionary, over real documents, sits at **78.4%**. That is a *lower* bound, not
an error rate — names, places, acronyms and rare words are in no word list. It
should not be read as "21.6% wrong" until the residue is sampled and classified.

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
cargo test --workspace     # 105 tests, no network needed
cargo run -p mukti-app    # the desktop app
cargo run -p mukti-cli    # the command-line tool
```

The dictionaries are compiled and checked in, so building needs no corpus and no
network. Linux also needs `libwebkit2gtk-4.1-dev` and friends — the exact list
is in `.github/workflows/release.yml`.

| Crate | What it is |
|---|---|
| `crates/mukti-core` | The converter, the detector, the dictionaries |
| `crates/mukti-formats` | Word, Excel, PowerPoint and PDF handling |
| `crates/mukti-cli` | The `mukti` command |
| `crates/mukti-app` | The desktop app (Tauri) |
| `devtools/` | Dictionary builder, corpus labeller, accuracy harness. Not shipped |

## Known limitations

- Installers are **not signed**; the first launch warns.
- Older `.doc`, `.xls`, `.ppt` are not supported.
- **PDF layout is lost**, and quality varies widely.
- The interface is **English only** for now. Every string sits in one table, so
  Bangla is a translation rather than a rebuild.
- Tuned for **SutonnyMJ and SutonnyOMJ**. Other legacy fonts — Boishakhi,
  Sulekha and the rest — appeared in too few documents to verify, so they are
  not claimed.

## Contributing

Issues and pull requests welcome. If you find a word converted that should not
have been — measured at 6 in every 100,000 English words — please report it with
the text. Those cases are the most valuable.

Work happens on `development`; `main` is what was last released.

## Licence

[MIT](./LICENSE). Third-party obligations, including the fonts and the word
lists: [`THIRD-PARTY-LICENSES`](./THIRD-PARTY-LICENSES).

More: [how to use it](./USING-MUKTI.md) · [what changed](./CHANGELOG.md)
