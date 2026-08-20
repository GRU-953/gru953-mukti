# Mukti by GRU953

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

> ## Warning: the desktop window is gone as of 0.6.0
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

**macOS on Apple Silicon (M1 or later) only, as of 0.9.0.** Windows, Linux and
Intel Macs are not built or tested any more — see CHANGELOG.md for why.
Building from source (below) still works on any platform Rust itself
supports; only the pre-built binary on the releases page is restricted.

Download `mukti-macos` from the
[latest release](https://github.com/GRU-953/gru953-mukti/releases/latest),
put it on your `PATH`, and make it executable: `chmod +x mukti-macos`.

> **The binary is not signed.** macOS will say "unidentified developer" the
> first time it runs — allow it once in *System Settings → Privacy &
> Security → Open Anyway*. Signing needs a paid Apple Developer certificate,
> and none is set up.

**Mukti is a command-line tool.** There was a desktop window until version 0.5.0;
it was removed in 0.6.0 to keep one thing working well rather than two adequately.
Everything it did, the command line does — and more, because it converts whole
folders in one go.

## Use it

Type `mukti` on its own, on a real terminal, and it asks which folder to
convert, reports what it found, and confirms before writing anything — no
flags required. Everything below still works exactly as written, for
scripts, CI, and anyone who prefers to type the whole command:

```sh
mukti check report.docx     # say what would change, write nothing
mukti convert report.docx   # writes report.unicode.docx, formatting intact
mukti convert *.docx        # many files at once
mukti convert *.docx --jobs 4   # convert up to four files at once
```

Your original is never overwritten unless you type `--in-place`. A file Mukti
named itself is never replaced unless you add `--force`. Colour is decided
automatically from the terminal; `mukti --theme dark` (or `light`, or `off`)
overrides that by hand.

## What it handles

| Format | What happens |
|---|---|
| `.docx` `.xlsx` `.pptx` | Converted **inside the document**. Formatting, tables and images untouched. Includes SmartArt, charts, speaker notes and comments |
| Older `.doc` `.xls` `.ppt` | Converted into a **new** `.docx`, `.xlsx` or `.pptx` beside the original. **Text only** — these formats hold no formatting we can carry, and no font information, so accuracy matches plain text rather than the higher figure above |

**Only these six formats are converted, as of 0.9.0.** Everything else — PDF,
`.txt`, `.csv`, `.md`, `.json`, anything else — is refused with a plain
explanation, before a single byte is read. Until this version there was no such
list: an unrecognised file fell through to a plain-text path that decoded and
rewrote it regardless of what it actually held, so `mukti convert photo.jpg`
produced a `photo.unicode.jpg` full of decoded bytes. PDF support is gone
entirely — it only ever produced plain text with the layout lost, and it took
a PDF-parsing dependency with it that had already carried one real
vulnerability ([RUSTSEC-2026-0187](https://rustsec.org/advisories/RUSTSEC-2026-0187)).
JSON was refused from 0.8.0 for the same reason narrowed formats generally are:
converting one could produce a file that no longer loads, because a Bijoy
curly quote becomes a plain `"`, which ends a JSON string early — it happened
to 5 of 13 real files the first time JSON was tested, on 19 August 2026.

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

Re-measured from scratch on 20 August 2026, on a freshly rebuilt answer key of
7,245,028 labelled tokens from 1,048 real documents.

| | Result | Sample |
|---|---|---|
| Conversion, word accuracy | **99.989%** | 473,244 words |
| Conversion, character accuracy | 99.997% | 3,879,440 characters |
| Every consonant × every vowel and conjunct | **100%** | 3,096 combinations |
| Detection, legacy words found | **99.927%** | 286,412 words |
| Detection, English wrongly converted | **0.146%** | 186,894 words |
| Detection, Unicode Bangla wrongly converted | **0.000%** | 1,189,851 words |
| Misspellings preserved, not "corrected" | 99.979% | 14,214 words |

**The English figure now fails its own 0.10% target, and that is the honest
result of a fix, not a new problem.** The answer key used to label a run
declaring the font `SutonnyOMJ` as legacy Bijoy, on the strength of an
unverified, hand-maintained list that contradicted the converter's own
evidence: the vendor's own copy of that font has 97 codepoints in the
Bengali Unicode block and files it under "Unicode Fonts". Every token in a
`SutonnyOMJ` run was being measured as if it were genuine Bijoy, which
quietly excluded some genuine false positives from ever being counted at
all. Fixing the label (`corpus-label` now asks the converter's own
`is_legacy_font`, rather than keeping a second list by hand) surfaced them:
tokens such as `UvKv` (would be টাকা, "Taka") are correctly recognised as
legacy Bijoy **97% of the time** across the corpus, and the residual few
percent sit under a font this project has not catalogued as legacy —
`Siyam Rupali ANSI` is the leading candidate, an ANSI-suffixed variant of a
family this project's own font list already verifies as Unicode in its
plain form. Recognising a new legacy font family needs the same measurement
discipline the existing 127 went through, not a guess added on the strength
of one document, so it is recorded as open work rather than fixed here.
A font-aware use of this same evidence inside the classifier was designed,
measured against the real corpus, and found to safely rescue only 365
words — well under the bar set in advance for whether it was worth making
the classifier's decisions depend on font metadata — so it was not built.

**One deliberate exception to "already-correct Bangla is untouched", from 0.7.0.**
Already-Unicode Bengali passes through byte-for-byte, with a single change: a vowel
sign stored in two pieces is composed into the one character Unicode defines it to be
(`ে`+`া` → `ো`, `ে`+`ৗ` → `ৌ`). This is Unicode canonical composition, so it cannot
alter meaning — but it does alter *findability*, which is the point: a search for `ো`
does not match the two-piece spelling in Word, in a browser or in a database. A
1,059-document run found **1,688 words** written that way, none of them Mukti's doing.
Nothing else about a non-legacy word is changed, and a composition is not counted as
a conversion in the figures a user is shown.

The detection figures come from a **held-out** half of the data, never looked at
while anything was tuned.

**The old figures cannot be reproduced exactly, and that is not a caveat being
buried.** Which documents land in the held-out half is decided by a hash of each
file's path, and the document archive has moved. So the halves are a different
split of the same material, now also drawn from a larger and more accurately
labelled set (see above). Re-measuring was the honest option; quoting figures
whose answer key was known to be wrong was not.

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

The direct measurement is that **93.481%** of converted words, over real
documents, are found in the dictionary (286,404 words). It moved twice upward
because a fault was found and fixed — 94.015% → 94.023% when a lost halant was
corrected, and → 94.053% on 14 August 2026 when byte 0xD0 was found to be the
conjunct ণ্ড rather than a dash — and then down to **93.481%** on 20 August
2026, for a different reason: a larger, more accurately labelled document set
(see above), not a new converter fault. That is a **floor**, not an accuracy:
names, places, acronyms and rare words are in no word list, so a perfectly
converted word can be missing from one. Earlier versions of this page reported
78.4%, which was simply wrong — it came from a superseded answer key.

So the remaining residue was sampled and read. **Twice** — the study was first run
on 13 August 2026 against the residue as it then stood, and re-run on 19 August
after two accuracy fixes had changed what the residue contained. The figures here
are the re-run's; the first run's are kept below for comparison.

**Not re-run against the 20 August answer key.** The residue study is a hand-rated
exercise — three blind raters reading real word pairs against a pre-registered
scheme — not something `eval` reproduces on its own, so the figures below still
describe the residue from the 19 August labelling. The corpus-label fix described
above changed which documents are sampled and how some tokens are labelled, so a
third run would very likely draw a different residue. Recorded as open work,
not silently left looking current.

**400 words** were drawn at random from the 10,530-word residue with a recorded
seed (`20260819`), so the same sample can be drawn again. Each was classified with
the original Bijoy shown beside the output — because the Bengali alone cannot
separate "rare word" from "wrong word" — by **three independent raters who saw only
the word pairs and the classification rules**, could not see each other's answers,
and were told nothing about what result was wanted.

| What the residue actually contains | of 400 |
|---|---|
| A genuine word, correctly converted, that no dictionary lists | **384** |
| The source was not a word at all — a fragment or a stray symbol | 8 |
| A name | 4 |
| **A genuine mis-conversion** | **4** |
| Could not be judged | 0 |

So **97% of the residue is correctly converted text**: compounds, case-marked and
inflected forms, pairs joined by a slash or hyphen, transliterated English,
abbreviations, and — most of all — **source misspellings that Mukti faithfully
preserved**, which is exactly what it is supposed to do.

Correcting the floor by the measured mis-conversion rate gives **99.939%**
[99.846, 99.976]. But a single voting rule would hide how much that depends on how
disagreement between raters is resolved, so all three are given:

| Rule for calling a word a mis-conversion | Faults | Estimate |
|---|---|---|
| **Any** one of the three raters said so | 8 | 99.879% [99.764, 99.938] |
| A **majority** said so — the headline | 4 | **99.939%** [99.846, 99.976] |
| **All three** said so | 2 | 99.970% [99.891, 99.992] |

**The honest summary is "about 99.9%".** Anything finer over-reads the data.

**Three checks on that.** 377 of the 400 rows were unanimous, and of the 23 that
were not, only 4 touched the rare-word/mis-conversion boundary the estimate
actually depends on — most disagreement was about whether a fragment of source
counted as a word at all, which changes what is *excluded*, not the fault count.
Fleiss' κ was 0.596, depressed as before by how lopsided the categories are rather
than by real disagreement. And the round-trip figure of 99.989% is an *upper* bound
by construction: every estimate here sits below it, which is what a consistent pair
of measurements must do.

**The first run, for comparison.** 13 August 2026, 200 words, two raters, one of
them a human with no knowledge of the project: 3 mis-conversions, giving **99.908%**
[99.737, 99.969], or 99.756% counting all five unjudgeable cases as errors. Cohen's
κ 0.607. **The re-run confirms that figure and roughly halves the uncertainty; it
does not show an improvement.** The intervals overlap heavily, so the gap between
99.908% and 99.939% is noise, not progress.

**What this still cannot tell you.** One thing got *worse* between the runs: not a
single one of the 1,200 judgements in the re-run used "cannot judge", where the
human rater in the first run produced five. Raters who never abstain probably forced
some genuinely ambiguous words into the "correct" column, and the direction of that
bias is unknown. It also means the first run's honest floor — count every
unjudgeable case as an error, 99.756% — has no counterpart here; the
any-rater-said-so row of the table above, 99.879%, is the floor to quote instead.
Neither run covers the 118 legacy font families added on 15 August, because the
labelled corpus predates them. All 3 confirmed faults are the same
kind — the **reph (`র্`) or a vowel sign landing on the wrong consonant**, plus one
dropped character — which is a specific thing to fix rather than a vague error
rate. And one honest note on method: the classification rules had to be clarified
mid-study, because the corpus is full of source misspellings and reproducing one
faithfully is correct behaviour, not an error. That clarification moved the result
*upwards*, so it is recorded here rather than left implicit.

</details>

## Why only the right words change

Bijoy **is** ASCII wearing Bangla shapes. `bvg` is the word নাম, and it is also
three ordinary Latin letters, and nothing inside the word can tell you which.

So Mukti reaches **three** verdicts, not two — legacy, not legacy, and
genuinely uncertain — and lets the surrounding words settle the last of them. In
the measured data, **71% of legacy words carrying no evidence at all** of being
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
465,971 English ones. Same input, same output, on any machine, offline.

## Build it yourself

```sh
cargo test --workspace     # 246 tests, no network needed
cargo run -p mukti-cli    # the command-line tool
```

The dictionaries are compiled and checked in, so building needs no corpus, no
network and no system libraries — a Rust toolchain is the only requirement.
Building from source works on any platform Rust itself supports; **CI and the
pre-built binary on the releases page cover macOS on Apple Silicon only**, as
of 0.9.0 (see Install, above).

| Crate | What it is |
|---|---|
| `crates/mukti-core` | The converter, the detector, the dictionaries |
| `crates/mukti-formats` | Word, Excel and PowerPoint handling |
| `crates/mukti-cli` | The `mukti` command |
| `devtools/` | Dictionary builder, corpus labeller, accuracy harness. Not shipped |

## Known limitations

- The binaries are **not signed**; the first run warns that the maker cannot be checked.
- Older `.doc`, `.xls`, `.ppt` are read, but **only their text**. Formatting,
  tables and images are not carried across, and because these formats record no
  font, the conversion is decided from the words alone — the same accuracy as a
  plain text file, not the font-gated accuracy quoted for `.docx`.
- The command's own messages are **English only** for now. They sit in the
  command-line source rather than in a string table, so Bangla would be a small
  refactor plus a translation. Lower value than it looks: somebody typing commands
  is already reading English.
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
have been — measured at 14 in every 100,000 English words — please report it with
the text. Those cases are the most valuable.

Work happens on `development`; `main` is what was last released.

If you are picking this project up rather than dipping into it, start with
[`HANDOVER.md`](./HANDOVER.md) — how it is built, how the accuracy was measured,
what is still open, and the traps that cost time here.

## Licence

[MIT](./LICENSE). Third-party obligations, including the word lists and the four
dependency licences that MIT cannot be elected in place of:
[`THIRD-PARTY-LICENSES`](./THIRD-PARTY-LICENSES). No fonts ship as of 0.6.0.

More: [how to use it](./USING-MUKTI.md) · [what changed](./CHANGELOG.md)
