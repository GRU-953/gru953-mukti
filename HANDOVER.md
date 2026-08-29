# Handover

Everything a new developer needs to pick Mukti by GRU953 up and carry it on.

Written on 13 August 2026, at version 0.4.0. If you are reading this much
later, check the figures against a fresh `eval` run before quoting them.

---

## 1. What this is, in one paragraph

Mukti converts legacy Bangla text — the Bijoy/SutonnyMJ encoding — into
Unicode. Bijoy is not really an encoding at all: it is a font trick. The bytes
are ordinary Latin ones, and a special font draws Bengali shapes for them. So a
Bijoy file is stored in *visual* order, the order the glyphs are drawn, while
Unicode stores *logical* order, the order the letters are spoken. Converting
means both substituting characters and reordering them.

The hard part is not the conversion. It is deciding **which words to convert**.
A page usually mixes legacy Bangla, ordinary English and Unicode Bangla, and
legacy Bangla looks exactly like nonsense English. Converting the wrong word
destroys readable text. That judgement lives in
[classify.rs](crates/mukti-core/src/classify.rs) and is where most of the
project's effort went.

## 2. Layout

    crates/
      mukti-core/     conversion, detection, embedded dictionaries  (library)
      mukti-formats/  .docx .xlsx .pptx readers/writers, and the pre-2007 reader (library)
      mukti-cli/      the `mukti` command                           (binary)
        src/main.rs      dispatch and exit codes only
        src/words.rs     every string a person can see, plus the brand tests
        src/style.rs     the colour ladder and the fixed palette
        src/options.rs   argument parsing
        src/report.rs    number formatting, the file-name defence, progress
        src/convert.rs   the six-format gate, per-file conversion, the batch
        src/pathinput.rs turning a typed or dragged-in path into a real one
        src/guided.rs    the conversation `mukti` alone has on a real terminal
    devtools/         NOT shipped, NOT published
      lexicon-build/  word lists      ->  compressed dictionary
      corpus-label/   real documents  ->  labelled token dataset
      corpus-verify/  runs every document through Mukti and checks invariants
      eval/           the accuracy measurement harness
      bench/          the speed measurement harness
    .github/workflows/  CI; release builds

Over 15,000 lines of Rust and nothing else — no HTML, no CSS, no JavaScript, no
npm, no bundler, no framework. Until 15 August 2026 there was also a desktop
window with a small web front end; it was removed, and `assets/brand/` went with
it. `mukti-cli` was one 641-line file until 0.9.0, when it was split into the
eight modules above so a beginner-facing guided mode, a colour system, and
parallelism could each have a place of their own rather than growing inside a
single dispatch function.

**`devtools/` measures the project; it is not part of the product.** It stays
in the repository because a claim you cannot reproduce is not a claim.

## 3. Getting it running

**Rust 1.97.1**, pinned by `rust-toolchain.toml` since 13 August 2026 — rustup
reads that file and switches to it automatically, fetching it if needed. You do
not choose a version.

(Two different numbers are easy to confuse here. The pin above is the one
compiler this project is built and measured with. `rust-version` in `Cargo.toml`
says **1.88**, which is the oldest compiler the *library* claims to work with —
a claim nothing currently tests. It was 1.82 until 14 August 2026, when reading
the pre-2007 Office formats brought in a dependency needing 1.88; the owner
agreed to the change rather than have the declared minimum be untrue.)

There is also a project-local build environment, which is the easier route:

```bash
source .sandbox/activate    # its own compiler and package cache, inside the project
```

Failing that, the compiler was installed here with Homebrew's rustup, which does
not put itself on the PATH automatically:

```bash
export PATH="/opt/homebrew/opt/rustup/bin:$PATH"
```

Then:

```bash
cargo test --workspace
```

246 tests, all passing. Then the command-line tool:

```bash
cargo run -p mukti-cli -- check <file>
```

No system packages are needed — a Rust toolchain is the whole requirement.
That became true on 15 August 2026 when the desktop window went; until then
the Linux build needed the WebKitGTK development headers. CI and the release
binary have covered macOS on Apple Silicon only since 0.9.0 (see §9,
Removed), but nothing about the workspace itself is platform-specific, and
`cargo test --workspace` above works the same wherever Rust does.

## 4. Read the code in this order

1. **[lib.rs](crates/mukti-core/src/lib.rs)** — `convert()` and the reordering
   functions. Every comment naming a defect is there because that defect was
   real; don't tidy them away.
2. **[classify.rs](crates/mukti-core/src/classify.rs)** — the detector. Three
   verdicts: `Legacy`, `NotLegacy`, `Uncertain`. Uncertain words are settled by
   their neighbours, and uncertain words never vouch for each other.
3. **[tokenise.rs](crates/mukti-core/src/tokenise.rs)** — splits on whitespace
   only and keeps every byte, so untouched text reassembles exactly.
4. **[encoding.rs](crates/mukti-core/src/encoding.rs)** — a Bijoy `.txt` is
   almost never UTF-8. It is Windows-1252. Not on the shipped `mukti` path any
   more since 0.9.0 removed plain-text conversion, but `corpus-verify` still
   calls it to check the English-only negative corpus, which is real safety
   cover and the reason this module stays.
5. **[office.rs](crates/mukti-formats/src/office.rs)** — Office files are ZIPs
   of XML. Replacement is position-based, never count-based; see §7.

## 5. The numbers, and what they mean

Reproduce them with:

```bash
cargo run --release -p eval -- --corpus <corpus> --labels <labels> --split test
```

| Measure | Result | Sample |
|---|---|---|
| Round-trip word accuracy | **99.989%** | 473,244 words |
| Character grid | **100%** | 3,096 combinations |
| Detection recall on legacy words | **99.927%** | 286,412 tokens |
| False positives on English | **0.146%** | 186,894 tokens |
| False positives on Unicode Bangla | **0.000%** | 1,189,851 tokens |
| Misspellings preserved unchanged | 99.979% | 14,214 pairs |

**These replace the 13 August set, and the reason is a fix, not drift.**
`corpus-label` was labelling any run declaring the font `SutonnyOMJ` as legacy
Bijoy, on a hand-maintained list that contradicted the converter's own
`office::NEVER_LEGACY` -- that font has 97 Bengali Unicode codepoints in the
vendor's own copy. Every `SutonnyOMJ` token was scored as if it were genuine
Bijoy, which quietly excluded real false positives from ever being measured.
Fixed 20 August 2026 by having the label ask `office::is_legacy_font`
directly rather than keeping a second copy of the list by hand -- see
`Dev-Memory/LESSONS.md` §42 and §44. The English false-positive figure moving
from 0.014% to 0.146%, above its own 0.10% target, is the honest result:
traced by hand, most of the residue is genuine Bijoy sitting under a font this
project has not catalogued as legacy (`Siyam Rupali ANSI` is the leading
candidate), not a new weakness in the classifier. README.md carries the same
table and the same explanation, and is the authority if these two ever
disagree again.

A font-aware use of this same font evidence inside the classifier was
designed and measured against the real corpus: it would safely rescue only
365 words, well under the bar set in advance for coupling the classifier's
decisions to font metadata, so it was not built. See `Dev-Memory/LESSONS.md`
§44 for the full measurement.

Detection figures come from a **held-out** half of the data that was never
inspected while tuning. The tuning half gives 99.936% recall — close
agreement with the test half's 99.927%, which is the evidence there is no
overfitting on that figure. The two halves do NOT agree as closely on the
English false-positive rate — 0.079% on tuning against 0.146% on test — and
that disagreement is itself informative: it is consistent with the residue
being concentrated in a small number of documents under a specific
uncatalogued font rather than spread evenly across the corpus (see above).

Two honest caveats, both of which should stay attached to these numbers:

- **Round-trip has a blind spot by construction.** If the encoder and decoder
  share a mistake, the text still comes back identical. That is why the real
  document measurement exists.
- **DONE, and re-run 19 August 2026.** Estimated accuracy on real documents is
  **99.939%** [99.846, 99.976] — honestly, "about 99.9%", with a floor of 99.879%
  if any single rater's judgement of a fault is accepted. 400 words, seed
  `20260819`, three independent blind raters, full write-up in
  `local/residue-study-2026-08-19.md` and summarised in `README.md`.

  **The re-run confirmed the 13 August figure rather than improving on it** — the
  intervals overlap heavily and the difference is noise. What it did buy is half the
  uncertainty and a measured level of rater agreement.

  **Four confirmed faults, and they are not all one class.** Three are the family
  the first study found: the reph (`র্`) or a vowel sign landing on the wrong
  consonant, or a character dropped — `LESSONS.md` §3 records that this is exactly
  where guards go wrong. **The fourth is different and worth chasing separately: an
  English word in quotation marks was put through the Bijoy tables instead of being
  left alone.** That is a detection false positive, not a conversion fault, and it
  turned up here only because the residue study looks at output that is not a
  Bengali word. It belongs with the 0.146% English false-positive rate in D2, and it
  suggests quoted Latin text is a case the detector handles less well than bare
  Latin text.

  Reproduce the sample with:

  ```bash
  cargo run --release -p eval -- --corpus "<word list>" --residue-sample 200
  ```

  The seed is recorded in the output file, so the same 200 words come back.

- **The withdrawn figure, and why it was withdrawn** (kept for the record). This
  document previously said 78.4% of converted words are found in the dictionary,
  over real documents. Two things were wrong with that.

  It came from a **superseded answer key**. The final run, on the larger key
  actually used for every other figure here, gave **93.8%** — and that number was
  never copied into the documentation, so the published one was a full 15 points
  out and traceable to a file nobody re-checked.

  And neither number is interpretable anyway. It is a **lower bound**: names,
  places, acronyms and rare words are in no word list, so a perfectly converted
  word can be missing from it. **Do not quote either figure as an accuracy, and
  do not quote its complement as an error rate.**

  So it is withdrawn rather than corrected, pending two things: measuring it
  again from scratch, and hand-classifying a sample of the words *not* found into
  names, rare words and genuine errors. **That sampling was done on 13 August 2026
  and re-run on 19 August** — the re-run drew 400 words with seed `20260819` and used
  three independent blind raters against the same pre-registered scheme. It produced
  **99.939%** [99.846, 99.976], confirming the first run's 99.908% [99.737, 99.969]
  rather than moving it. Both are recorded in README.md and the full write-up is in
  `local/residue-study-2026-08-19.md`. This paragraph described the sampling as the
  top open task, which it was when written and has not been since.

  The lesson generalises, and it is why the withdrawal is written out at length:
  a number copied from one report into a document outlives the report. Every
  figure needs to name the run it came from.

## 6. Where the ground truth came from, and why you cannot have it

The publicly available word lists contain no legacy text at all, so on their
own they can only measure whether Mukti agrees with itself.

The real measurement rests on one observation: **documents label themselves.** A
`.docx` records the font of every run of text. A run set in SutonnyMJ *is*
legacy Bijoy. A run of Unicode Bengali in a Unicode font *is not*. An English
run *is not*. So a folder of real documents yields millions of labelled tokens
automatically, with no hand-labelling and without asking the code under test
what it thinks. `corpus-label` does exactly this.

The archive used here was **private material belonging to the project owner**.
It was read locally only. **Nothing derived from it is in this repository** —
`local/` is git-ignored, and only aggregate statistics were ever allowed out.
You will need your own document set. Any collection of real `.docx` files
containing a mix of legacy and Unicode Bangla will work; run `corpus-label`
over it and you have your labels.

If you receive a copy of the archive from the owner, keep the same rule: it
does not go into git, and it does not go to a third party.

## 7. Traps that cost real time here

Fuller versions of these are in the private development notes; these are the
ones that will bite a newcomer.

1. **When a measurement looks wrong, suspect the measurement first.** Five
   separate times the harness or the test fixture was at fault, not the code.
   Two of those flaws made the code look *worse* than it was — which is exactly
   why they had to be fixed. A harness you cannot trust downward cannot be
   trusted upward either.
2. **Never replace text by counting.** An early Office rewriter matched runs by
   ordinal position in a count, and drift turned 781 words into 421. Replace by
   byte span, always.
3. **If two passes over a document must agree, they must share one predicate.**
   A paragraph-boundary rule written twice, slightly differently, produced an
   off-by-one that only showed up on real files.
4. **Guards written from intuition are too broad.** A Roman-numeral guard also
   matched `cv`, `ci` and `mi`, which are ordinary Bangla words in Bijoy. A
   superscript guard excluded `¹ ² ³`, which *are* Bijoy glyphs. Each cost 2.2%
   of recall. Measure every guard against the corpus before keeping it.
5. **Validate inputs before creating outputs.** `corpus-label` opened its output
   file before checking its inputs existed. Pointed at a moved directory, it
   silently truncated a 152 MB dataset and exited 0. Fixed, but the pattern is
   worth remembering everywhere.
6. **Test on real files early.** Three Office bugs — including SmartArt text
   never being converted — were invisible to unit tests and obvious on the
   first real document.
7. **A licence at a repository's root may not cover its contents.** The Noto
   fonts' repository root carries Apache-2.0, which covers the build tooling.
   The fonts themselves are OFL 1.1, stated in each font's own embedded licence
   field. Check the artefact, not the folder.

## 7b. Measured against the whole corpus, 19 August 2026

The published binaries were run over the full 1,173-file test corpus twice — v0.7.0 and
then v0.7.1 — with the harness rebuilt first, because three of its earlier answers were
wrong. Everything below comes from those runs, not from the unit tests.

| | Result |
|---|---|
| Files converted | **1,173 of 1,173**, every one exiting 0, across ten formats |
| Round-trip word accuracy | **99.9764%** of 2,686,285 words |
| Round-trip character accuracy | **99.9933%** of 13,558,475 characters |
| Office files aligned for detection | **750 of 750**, none discarded |
| Tokens aligned | 7,801,733 |
| English words wrongly converted | **0** |
| 422 KB English negative control | **byte-for-byte identical** |

**The most useful figure is the one that looks worst.** Ten Bijoy/Unicode pairs published
by another project — independent ground truth, not our own encoder — give **82.40%** word
accuracy. Every one of the 69 mismatches was classified: **65 were the detector declining
to convert, 4 were quote style, and 0 were conversion errors.** The tables are flawless
against outside data. The entire shortfall is the documented refusal to touch short
pure-ASCII Bijoy in a plain text file, where no font proves the encoding.

That distinction matters for anyone quoting these numbers: the round trip measures the
CONVERSION, because it feeds the tables text our own encoder produced. The independent
pairs measure the WHOLE PRODUCT including the detector's decision — and the detector is
where essentially all the remaining loss is. See LESSONS §39.

## 8. What is open

In rough order of value:

1. ~~Open the app and use it.~~ **Closed, 15 August 2026 — there is no app.** The
   window was inert in 0.4.0, was fixed and hand-verified on 13 August, and was
   then removed entirely on 15 August in favour of doing one thing well. Mukti is
   a command-line tool and a library.
2. ~~Sample the residue.~~ **Done 13 August, re-run 19 August 2026.** The re-run
   drew 400 words (seed `20260819`) from the 10,530-word residue and had three
   independent blind raters classify every one: 4 mis-conversions, 384 correctly
   converted words no dictionary lists, 8 non-words, 4 names, 0 abstentions.
   Real-document accuracy **99.939%** [99.846, 99.976], floor 99.879%.

   **Still open, and now specified:** the labelled corpus predates the 15 August
   font-list widening, so neither run covers the 118 families added then. A study
   that does needs the corpus re-labelled, which changes the tune/test split
   (LESSONS §11) and so cannot be compared directly with either run. Worth doing as
   its own measurement, not as a third data point in this series.
3. **Bangla in the command's own messages.** Since 0.9.0 every string Mukti can
   show lives in one file, `crates/mukti-cli/src/words.rs`, with the brand-kit's
   English writing rules enforced there by test — so this is now closer to a
   translation than the refactor-plus-translation it used to be, though a
   parallel Bangla writing standard and its own test suite would still need
   deciding first. Lower value than it was: someone typing commands is already
   reading English.
4. **crates.io.** The crates are prepared and metadata is complete, but
   publishing needs a token belonging to the account owner. Publish in
   dependency order: `gru953-mukti`, then `mukti-formats`, then `mukti-cli`.
5. **Code signing.** The binaries are unsigned, so both macOS and Windows warn on
   first launch. Needs a paid Apple developer account and a Windows certificate.
6. ~~Old binary Office formats are not supported.~~ **Supported since 14 August
   2026.** `.doc`, `.xls` and `.ppt` are read and written out as new `.docx`,
   `.xlsx` and `.pptx` files beside the original, which is never modified. All
   141 in the archive convert, and every generated document passes the same
   structural checks Office itself applies. **Text only:** these formats carry no
   formatting we can keep and no font information, so the conversion is decided
   from the words alone — plain-text accuracy, not the font-declared figure the
   other three formats reach.
7. **Markdown and HTML output.** Planned, then deferred by the owner in favour of
   releasing. Measurement showed it could carry bold (declared in the file, so no
   guessing) and about one line break in eight joined into paragraphs, but not
   headings — font size does not separate them in this archive.

## 9. Removed

Kept here rather than deleted, because each is a measurement record, not a gap.

- **PDF, in 0.9.0.** Text was joined into real lines rather than one fragment
  per positioning instruction, which removed 56% of the line breaks on a sampled
  set. Headings, columns and tables were never recovered, and that was a
  measured decision rather than an omission: 80 documents were judged against a
  pre-registered scheme, and of the three that were badly scrambled, all three
  were **tables**, not columns — so the column detection once planned would have
  fixed none of them. Removed along with plain text, `.csv` and `.md`, so that
  only the six ordinary Office formats are converted. Took `lopdf` out of the
  dependency tree with it — a PDF parser reading untrusted input, and the
  source of RUSTSEC-2026-0187, one of v0.4.0's three CVEs.
- **Windows, Linux and Intel macOS, from CI and the release binary, in
  0.9.0.** Both workflows now build and test macOS on Apple Silicon only.
  The two-job Intel/ARM macOS split tried before the universal binary was
  abandoned because scarce `macos-13` runners queued half an hour on a
  private repository's quota; the Linux build was pinned to `ubuntu-22.04`
  specifically to hold the binary's glibc requirement at 2.35, a constraint
  that stopped mattering the day there was no Linux build left to hold it
  for. Nothing in `gru953-mukti` or `mukti-formats` is platform-specific, so
  building from source still works anywhere Rust does — only CI coverage
  and the pre-built binary narrowed.

## 10. Ground rules worth keeping

- **Measure before improving.** No accuracy claim ships without its method and
  its sample size beside it.
- **When the evidence runs out, leave the text alone.** A missed conversion is
  visible and fixable. A wrongly converted word destroys readable text and may
  never be noticed. The thresholds are deliberately asymmetric for this reason.
- **Never commit anything derived from private documents.** The repository is
  public.
- **Two branches only:** `main` (released) and `development`. Nothing else.
- **Say what is unverified.** Several figures in this document carry caveats
  because they earned them. Keep them attached.
