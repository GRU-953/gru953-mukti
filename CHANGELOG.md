# Changelog

## Unreleased — becomes 0.5.0

### New

- **The older Office formats are read.** `.doc`, `.xls` and `.ppt` are converted
  into new `.docx`, `.xlsx` and `.pptx` files beside the original, which is never
  modified. All 141 in the test archive convert, and every generated document
  passes the same structural checks Office applies before it will open a file.
  PowerPoint keeps its real slide breaks and titles.
  **Text only.** These formats carry no formatting we can keep and no font
  information, so the conversion is decided from the words alone — the same
  accuracy as a plain text file, not the higher figure quoted for `.docx`. The
  app and the command-line tool both say so.
- **`--force`**, so the tool can be told it may replace a file it named itself.

### Fixed

- **PDF text no longer arrives in fragments.** A line break was emitted at every
  positioning instruction, so a word split for kerning came out on three lines:
  `Md. Al`, `-`, `Hasan`. Breaks now happen only where the text moves down the
  page. Measured across 40 documents: 62,389 lines became 27,726, and none got
  worse. The threshold comes from the data — across 385,372 consecutive runs the
  vertical step is sharply bimodal, with only 0.25% falling between the two modes.
- **The promise about not overwriting files now holds.** The tool said it never
  writes over your file unless asked. That was true of the original and false of
  everything else: the derived `.unicode.txt` sibling and any `--out` target were
  truncated silently. A name the tool chose is now refused if something is
  already there.
- **Word-processor curly quotes could turn English into Bengali.** In the
  document-level converter, `as “Village planting”` became Bengali-shaped
  nonsense while the straight-quoted form was correctly left alone. **No shipped
  path called that function**, so no released version was affected — this is
  recorded because the fault was real, not because anyone met it.
- **Four legacy font variants were being missed** — `SutonnyBanglaMJ`,
  `SutonnyBanglaMJBold`, `SutonnyUniBanglaOMJ` and `SutonnySushreeMJ` — because
  the font lists named three exact spellings and matched by substring. Both
  lists now match on the family.
- **A self-closing `<w:t/>` confused the Office rewriter's first pass** into
  believing a text element was open that would never close. It could not corrupt
  anything, for a reason now written down, and is fixed rather than relied upon.
- **Converting an old `.doc` twice could change it twice.** The document written
  from one is now settled through the ordinary Office converter before it is
  returned, so it cannot be improved by converting it again.

### Changed

- **The minimum Rust version is now 1.88**, up from 1.82, which is what reading
  the older Office formats requires.
- **The archive check now covers the old formats** instead of skipping them, by
  handing each converted document to the full Office check. 2,315 files, 0
  defects. That change immediately found the double-conversion fault above.
- **One place now decides what a legacy word is**, instead of four copies of the
  same loop in the core, the formats crate, the tool and the app.
- **An unsoundness advisory can no longer pass the release gate unnamed.**


### Known faults in 0.4.0, being fixed

- **The desktop app's window does not respond to anything.** It opens, renders
  correctly, and every control is dead. One configuration setting
  (`app.withGlobalTauri`) was never enabled, so the window's script fails on its
  first line and no button is ever connected. Separately, no permissions file
  existed, which would independently have blocked *Open*, *Save as…* and
  drag-and-drop. Nothing is wrong with the conversion itself, and the
  command-line tool is unaffected.

  It shipped because nothing in the automated checks had ever opened the window.
  An automated test that really opens it, types legacy Bangla and checks the
  result is being added, so this class of fault cannot return.

- **The app never actually supported Office or PDF files**, despite the
  documentation saying so. It does not depend on the code that reads them, and
  its file picker only offered `.txt`, `.csv`, `.md`, `.json` and `.tsv`. Those
  formats have always worked from the command line.

### Fixed

- **Three security flaws in the file readers** (`lopdf`, `quick-xml`), all
  published after 0.4.0 was built. A crafted PDF could abort the program via
  unbounded recursion ([RUSTSEC-2026-0187]); a crafted Office file could cause
  quadratic slowdown ([RUSTSEC-2026-0194]) or exhaust memory
  ([RUSTSEC-2026-0195]). Fixed by moving to `lopdf` 0.44 and `quick-xml` 0.41,
  and each PDF page's decompressed size is now capped.

  Found by turning the one-off release audit into a check that runs every time.

- **A silent data-loss fault introduced by that upgrade, caught before release.**
  From `quick-xml` 0.41 an XML entity reference arrives as its own event rather
  than inside the text. Code written for the older version still compiles, and
  quietly drops every `&`, `<` and `>` — while shifting all following character
  positions, which would have corrupted documents well beyond the character
  itself. Caught by an existing test; two more were added, including one that
  places entities *before* Bangla text so any drift fails visibly.

- **A reference that cannot be resolved now stops the conversion** instead of
  being dropped. Office files are not permitted to declare their own entities, so
  one appearing means the file is damaged or probing — and either guessing or
  dropping would corrupt the text silently. The original is left untouched.

### Corrected

- **The "78.4% of converted words found in the dictionary" figure is withdrawn.**
  It came from a superseded answer key; the final run gave 93.8%. More
  importantly, neither number is an accuracy — it is a lower bound, because names
  and rare words are in no word list. It returns only after being re-measured and
  after a hand-classified sample of the words not found.
- The instruction to reproduce the figures with `cargo run --release -p eval` was
  wrong: that command cannot run, as the corpus argument is required and has no
  default.
- The stated Rust requirement ("1.97.1 or newer") contradicted the declared
  minimum of 1.82. The build compiler is now pinned by `rust-toolchain.toml`.

### Added

- A project-local build environment (`source .sandbox/activate`) with its own
  pinned compiler and package cache, and `deny.toml` stating the dependency
  policy — permissive licences only, with MPL-2.0 permitted for five named crates
  after confirming they ship only inside the desktop app.

[RUSTSEC-2026-0187]: https://rustsec.org/advisories/RUSTSEC-2026-0187
[RUSTSEC-2026-0194]: https://rustsec.org/advisories/RUSTSEC-2026-0194
[RUSTSEC-2026-0195]: https://rustsec.org/advisories/RUSTSEC-2026-0195

## 0.4.0 — 13 August 2026

**Renamed from GRU953 Scribe to GRU953 Mukti** (মুক্তি, "freedom"). The app, the
command, the crates and the repository all follow.

This is a breaking change for anyone who installed 0.3.0: the command is now
`mukti` rather than `scribe`, and the crate names changed with it. Nothing
about the conversion changed — all 105 tests pass unaltered, and every accuracy
figure below is the same measurement as 0.3.0.

The brand kit's naming rule was amended rather than quietly broken. It required
"one plain, lower-risk English word"; it now also allows a plain Bangla word,
written in Latin script, where a product is made specifically for Bangla
speakers. The rule exists so a name survives translation — and for an audience
that reads Bangla, a Bangla word travels further than an English one.

## 0.3.0 — 12 August 2026

The first version you can actually install and use. 0.2.0 was a Rust library
with no window and no command; this is an app, a command-line tool, and an
accuracy figure that means something.

### New

- **A desktop app** for macOS, Windows and Ubuntu. Two panes, convert as you
  type, drag a file onto the window, light and dark. Its **Show what changed**
  view marks every word that was converted, so Mukti's judgement is something
  you can check rather than something you have to trust.
- **A command-line tool**, `mukti convert` and `mukti check`, for doing many
  files at once. It never overwrites your file unless you type `--in-place`.
- **Word, Excel and PowerPoint files** converted in place, with formatting,
  tables and pictures untouched. Includes SmartArt, charts, speaker notes and
  comments — all of which hold text people write and expect to be converted.
- **The brand's two typefaces are bundled** — Noto Sans and Noto Sans Bengali,
  under the SIL Open Font License 1.1. The app therefore looks the same on all
  three platforms, and Bangla renders even on a Linux install that ships no
  Bangla font of its own.
- **PDF**, read-only and best effort. Text is extracted and converted; layout
  is not preserved. Text drawn in fonts that store shapes rather than letters
  is skipped and counted, never guessed at.
- **Windows-1252 files are read correctly.** A Bijoy document saved as plain
  text almost never is UTF-8, and the glyphs Bijoy leans on sit exactly where
  the two encodings disagree. Previously such a file would not open at all.

### Changed

- **Detection is word by word, using context.** Previously a whole line was
  judged at once, so a line mixing an English heading with Bangla was
  converted or skipped as a whole. Recall on legacy words went from 39.7% to
  99.95% as a result.
- **A 451,348-word Bengali dictionary and a 234,428-word English one are built
  in**, replacing 150 hand-written word stems. This is what makes word-level
  detection possible: the question changed from "does this look like Bangla?"
  to "is this an actual Bangla word?".
- **Licence changed from PolyForm Noncommercial to MIT.**

### Accuracy

Every figure measured, with its sample size, and the detection figures taken
from a held-out half of the data never looked at during tuning:

| Measure | Result | Sample |
|---|---|---|
| Conversion, word accuracy | 99.989% | 473,244 words |
| Character grid, all conjuncts and vowels | 100% | 3,096 combinations |
| Detection, recall on legacy words | 99.951% | 154,928 words |
| Detection, false positives on English | 0.006% | 462,074 words |
| Detection, false positives on Unicode Bangla | 0.000% | 343,077 words |

### Known limitations

- **The installers are not signed.** Your computer will warn you the first
  time. Signing needs paid certificates from Apple and a Windows certificate
  authority, and neither is set up.
- **Older `.doc`, `.xls` and `.ppt`** cannot be read. Save as the newer format
  first.
- **PDF quality varies widely.** Measured across 60 legacy-font PDFs: 28 good,
  19 fair, 7 poor, 6 produced nothing.
- **The interface is English only.** Bangla is planned; every string already
  sits in one place so it is a translation rather than a rebuild.
- **One reported figure is not yet interpretable.** Converted words found in
  the dictionary, over real documents, sits at 78.4%. **(Withdrawn — see
  Unreleased above. This came from a superseded answer key and is not an
  accuracy in any case.)** That is a lower bound —
  names and rare words are in no word list — and should not be read as an
  error rate until the residue has been sampled.

## 0.2.0 and earlier

A Rust library only: `convert`, `detect`, `convert_document`. No app, no
command-line tool, and an accuracy claim inherited from a single round-trip
measurement that could not be reproduced.
