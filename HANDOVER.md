# Handover

Everything a new developer needs to pick GRU953 Mukti up and carry it on.

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
      mukti-formats/  .docx .xlsx .pptx readers/writers, PDF reader (library)
      mukti-cli/      the `mukti` command                           (binary)
      mukti-app/      the desktop app, Tauri v2                     (binary)
    devtools/         NOT shipped, NOT published
      lexicon-build/  word lists      ->  compressed dictionary
      corpus-label/   real documents  ->  labelled token dataset
      eval/           the measurement harness
    assets/brand/     the brand's CSS tokens
    .github/workflows/  CI on three platforms; release builds

About 7,200 lines of Rust plus a small HTML/CSS/JS front end. No npm, no
bundler, no framework.

**`devtools/` measures the project; it is not part of the product.** It stays
in the repository because a claim you cannot reproduce is not a claim.

## 3. Getting it running

**Rust 1.97.1**, pinned by `rust-toolchain.toml` since 13 August 2026 — rustup
reads that file and switches to it automatically, fetching it if needed. You do
not choose a version.

(Two different numbers are easy to confuse here. The pin above is the one
compiler this project is built and measured with. `rust-version` in `Cargo.toml`
says **1.82**, which is the oldest compiler the *library* claims to work with —
a claim nothing currently tests. This document used to say "1.97.1 or newer" as
though it were a requirement, which contradicted the declared 1.82.)

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

105 tests, all passing. Then the command-line tool:

```bash
cargo run -p mukti-cli -- check <file>
```

And the app:

```bash
cargo run -p mukti-app
```

Linux needs the WebKitGTK development packages first — the exact list is in
[.github/workflows/release.yml](.github/workflows/release.yml), which installs
them for the Ubuntu build.

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
   almost never UTF-8. It is Windows-1252. Without this the file will not open
   at all.
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
| Detection recall on legacy words | **99.951%** | 154,928 tokens |
| False positives on English | **0.006%** | 462,074 tokens |
| False positives on Unicode Bangla | **0.000%** | 343,077 tokens |
| Misspellings preserved unchanged | 99.979% | 14,214 pairs |

Detection figures come from a **held-out** half of the data that was never
inspected while tuning. The tuning half gave 99.962% and 0.014% — the agreement
between the two is the evidence there is no overfitting.

Two honest caveats, both of which should stay attached to these numbers:

- **Round-trip has a blind spot by construction.** If the encoder and decoder
  share a mistake, the text still comes back identical. That is why the real
  document measurement exists.
- **DONE, 13 August 2026 — the residue was sampled, and this is no longer the
  most valuable open task.** Estimated accuracy on real documents is **99.9%**
  (conservative floor 99.76%). The method, the classification counts, the
  independent second rating and the confidence intervals are all in `README.md`
  under Accuracy. The three confirmed faults are all the same kind: the reph
  (`র্`) or a vowel sign landing on the wrong consonant, plus one dropped
  character. That is a specific defect class to attack, and `LESSONS.md` §3
  already records that this is exactly where guards go wrong.

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
  names, rare words and genuine errors. That sampling remains **the most valuable
  open task in the project** — it is the only thing that turns this into a figure
  anyone may quote.

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

## 8. What is open

In rough order of value:

1. **Open the app and use it.** It builds on all three platforms and the front
   end was verified in a browser, but **no human has ever opened the window**.
   Native rendering, the file dialogs and drag-and-drop are unexercised.
2. **Sample the residue described in §5.** Take 200 converted words that are
   absent from the dictionary and classify them by hand: name, rare word, or
   genuine error. Until that is done the true accuracy on real documents is
   unknown.
3. **Bangla interface.** All user-facing strings sit in one table in
   [app.js](crates/mukti-app/ui/app.js). This is a translation job, not a
   rebuild.
4. **crates.io.** The crates are prepared and metadata is complete, but
   publishing needs a token belonging to the account owner. Publish in
   dependency order: `gru953-mukti`, then `mukti-formats`, then `mukti-cli`.
5. **Code signing.** Installers are unsigned, so both macOS and Windows warn on
   first launch. Needs a paid Apple developer account and a Windows certificate.
6. **PDF layout.** Output is running text. Column and table detection would
   help; 6 of 60 test PDFs produced nothing usable.
7. **Old binary Office formats** (`.doc`, `.xls`, `.ppt`) are not supported.

## 9. Ground rules worth keeping

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
