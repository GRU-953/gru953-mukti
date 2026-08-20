# Changelog

## 0.9.0 — 20 August 2026

**Six formats, and nothing else.** `.doc`, `.xls`, `.ppt`, `.docx`, `.xlsx` and
`.pptx` are what Mukti converts now. PDF, `.txt`, `.csv`, `.md` and reading from
a pipe (`mukti convert -`) are all removed. This is the format restriction half
of a larger 0.9.0 release; the rest of the entry grows as the remaining work
lands.

### Removed

- **PDF support**, and the `lopdf` dependency with it. PDF only ever produced
  plain text with the layout lost, and recovered nothing at all from 22 of 253
  real files. Removing it also removes a parser reading untrusted input from
  the dependency tree — `lopdf` was the source of RUSTSEC-2026-0187, one of
  v0.4.0's three CVEs. `mukti convert report.pdf` now explains this and writes
  nothing.
- **Plain-text conversion** — `.txt`, `.csv` and `.md` — and the encoding
  detection it needed. Any of these files now gets the same six-format refusal
  as an unrecognised extension.
- **Reading from a pipe** (`mukti convert -`). It was the last remaining route
  into the plain-text converter, so removing it let that whole code path go.
- **A real defect this closes, not just a promise.** There was no allow-list of
  supported formats at all: anything not recognised fell through to the
  plain-text path, which decoded it as Windows-1252-or-UTF-8 and wrote a
  converted sibling regardless of what the file actually held. `mukti convert
  photo.jpg` produced a `photo.unicode.jpg` full of decoded image bytes. The
  six-format gate at the top of `mukti-cli`'s dispatch is the fix: an
  unsupported extension is refused before a single byte is read.

### Changed

- The dependency tree compiled into the shipped binary falls from roughly 88
  packages to **38** — the full workspace lock from 153 to **78**. Gone: a
  crypto stack (`aes`, `cbc`, `chacha20`, `sha2`, `md-5`, `digest`), three
  date/time stacks (`chrono`, `jiff`, `time`), and a work-stealing scheduler
  (`rayon`) that arrived only through `lopdf`. A Bangla text converter has no
  business linking AES.
- `THIRD-PARTY-LICENSES` now states both the full lock-file count and the
  smaller figure actually compiled into `mukti`, because one number was always
  ambiguous. Direct dependencies of the shipped crates: four, not five.

### Added (development tools only — not shipped, not published)

- **`devtools/bench`**, a new crate that times the converter against real
  documents instead of guessing. No benchmark harness existed before this:
  the only timing code in the workspace asserted a ratio to catch a return to
  quadratic behaviour, and would not have noticed a 30% change in either
  direction. It measures three things separately, never summed — `convert()`
  alone, the classifier on already-extracted text, and end-to-end conversion
  from disk — tiered by document size, reporting the median of several runs
  with the min and max, and a `noise-floor` mode that measures the same
  binary against itself so a later change smaller than that spread can be
  called what it is: unmeasured, not improved.
- **`corpus-verify --compare <old.tsv>`**, joining two runs of the tool on a
  SHA-256 of each input file's own bytes, never its path — the same change
  makes `--resume` proof against a renamed corpus directory, something that
  once produced 650 phantom failures. Reports four disjoint counts: identical,
  differing, vanished (checked before, not this time) and new. A companion
  `--compare-entries` mode joins on a fingerprint of each archive entry's own
  content instead of the whole output's bytes, for a change known to touch
  only how the ZIP container is written — exactly the shape of the zip 2→8
  bump, which changed three archives' central-directory bytes while leaving
  every entry's content untouched.

### Fixed

- **A false-positive defect report in `corpus-verify` itself**, found while
  building `--compare`: its "nothing converted, so require byte-identical"
  gate checked `words_converted` and `fonts_changed` but not
  `words_normalised`, so any document holding a decomposed two-part vowel
  (`ে` + `া`, correctly composed into `ো` since 0.7.0) was reported `FAILED`
  for a change the converter made correctly and on purpose. A random 15-file
  sample hit this at 20%. This is a defect in the verification tool, not in
  Mukti's conversion — nothing shipped is affected.

### Performance

A real CPU profile of converting a 43 MB real workbook (`samply`, see
`Dev-Memory/LESSONS.md` §41) found over a quarter of all time going to pure
memory allocation and copying, and a single substring-search call
(`apply_map`'s `contains`/`replace`) at 23% on its own — confirming what
static analysis had already suggested and ranking the fixes by actual weight.

- **The classifier no longer converts a word twice.** `Features::of` used to
  skip its trial conversion only when text was already Unicode Bengali or
  inert, so every English word still paid the full 223-entry `convert()`
  table-scan pipeline for a result the classifier's hard stops discard
  unread. It now also skips the trial for a common English word, a
  roman-numeral list marker, or a sub/superscript — and the trial
  conversion, once computed, is reused instead of being run a second time
  once a word is confirmed `Legacy`. Both changes are provably
  output-identical by construction (see the comments in `classify.rs`), not
  merely measured as unchanged.
- **`opt-level` changed from `"s"` to `3`.** The previous setting optimised
  for binary size on the premise that "conversion is already far faster than
  any file we can read from disk" — the profile above shows the opposite by
  a factor of 60. Binary size grew by only 4.1% (6.38 MB → 6.65 MB).
- Two small allocation removals: `rearrange` no longer makes a redundant
  full copy of its input before using it, and `normalise_whitespace`'s two
  trailing full-string scans are folded into the single pass that already
  existed, checked against the original two-pass behaviour on all 1,365
  possible short strings over the relevant alphabet.
- **`RUSTFLAGS` consolidated into a new `.cargo/config.toml`.** It was set as
  an environment variable in two places (CI and the local sandbox) but not
  in the release workflow — since an environment `RUSTFLAGS` silently
  overrides `.cargo/config.toml` rather than merging with it, this meant the
  one build that ships was the one build that did not enforce
  warnings-as-errors. `target-cpu` was measured and deliberately not set:
  `apple-m4` adds only three instruction-set features, two of them
  matrix-multiply instructions this codebase never uses, for an expected
  0–3% at the cost of the binary no longer running on an M1, M2 or M3 Mac.

**Measured against the real corpus** (`bench noise-floor`, 200 real files,
converted end to end, same binary run twice so the noise floor is known):

| | Before | After | Change |
|---|---|---|---|
| 200-file batch (twice) | 9.05s / 9.03s | 4.39s / 4.39s | **-51%** |
| Noise floor | 0.3% | 0.1% | — |

Every change above was measured individually and gated on
`corpus-verify --compare` against the full real corpus before the next one
landed. The combined result, compared against the pre-Part-3 baseline:
**1,614 identical, 0 differing, 0 vanished, 0 new, across 1,775 files** — the
whole of this section changes nothing about what any document converts to.

- **`apply_map` reads each string once per rule instead of twice.** It was
  `if out.contains(from) { out = out.replace(from, to) }` — a full scan to
  answer "is it in there?", then a second scan from the beginning to do the
  work. One `find` answers both at once and gives the index to start copying
  from. Profiling had put this single function at 23% of a real conversion,
  the largest cost in the pipeline. The algorithm is untouched: same rules,
  same order, one at a time. A differential test holds it to that, checking
  the new form against the old one across **all 226 rules** in the four real
  tables, each rule's key alone, doubled, and padded on both sides, plus the
  self-overlap case a naive rewrite would get wrong.
- **Three allocations per word removed from the classifier.** Every word
  lower-cased itself before the ASCII gate that would have short-circuited
  the whole expression, and then `contains_english` lower-cased the same word
  a second time; both are now inside the branch that reads them, and the
  duplicate is gone. The distinct-character count kept a heap `Vec<char>`
  with a linear `contains` per character, where every rule that consults it
  asks only "at least one?" or "at least two?" — it is now two booleans and
  allocates nothing. **The plan's own suggestion here was wrong and was not
  followed:** it called for a stack `[bool; 256]` as the exact replacement,
  but the character range in question spans U+2010..=U+20FF as well as
  Latin-1, so 256 entries would not have covered it and distinct characters
  could have collided in the array.

Two further items from the original plan were assessed and **not** pursued
this round: the `apply_map` **automaton** — the Aho–Corasick rewrite that
folds all 191 rules into a single pass, which is a genuine change of
algorithm with a genuine risk of changing output, unlike the one-`find`
change above; and threading two reusable `String` buffers through
`convert()`'s seven stages, which touches every stage at once and wants the
full corpus gate rather than a differential test. Parallelism across files
was deferred to the CLI rebuild below, and landed there.

### Fixed

- **A measurement bug in `corpus-label`, the tool that builds the answer key
  every accuracy figure is measured against.** It labelled any run declaring
  the font `SutonnyOMJ` as legacy Bijoy, on a hand-maintained list that
  contradicted the converter's own `office::NEVER_LEGACY` — the vendor's own
  copy of that font has 97 codepoints in the Bengali Unicode block and files
  it under "Unicode Fonts", not the legacy collection. Every token in a
  `SutonnyOMJ` run was being measured as if it were genuine Bijoy, which
  quietly excluded real false positives from ever being counted. Fixed by
  having the label ask the converter's own `office::is_legacy_font` directly
  rather than keeping a second copy of that list by hand — the same fix also
  removed three Unicode font names (`adorsholipi`, `nikoshban`, `ekushey`)
  that the hand-written list still carried as legacy, years after the
  converter's own list had removed them on evidence. Nothing shipped is
  affected — this is a defect in the measurement tool, not in Mukti.
- The labelled corpus was rebuilt against this fix, from 1,048 real documents
  (7,245,028 labelled tokens, up from 3,782,953). Every published accuracy
  figure tied to it has been updated in README.md, USING-MUKTI.md and
  HANDOVER.md to match — including the English false-positive rate, which
  moved from 0.014% to 0.146% and now exceeds its own 0.10% target. That is
  the honest result of the fix, not a new problem: traced by hand, the
  residue is overwhelmingly genuine Bijoy sitting under a font this project
  has not yet catalogued (`Siyam Rupali ANSI` is the leading candidate), not
  a new weakness in the classifier. See `Dev-Memory/LESSONS.md` §42 and §44.

### Assessed and not built

- **Using a document's own font information to settle detection refusals.**
  A `.docx`/`.xlsx`/`.pptx` records the font of every run, and the classifier
  has never used it — a real, measurable gap. A rule was designed
  (`font == Legacy && has_ascii_letter && alphanumeric >= 2 &&
  converted_plausible`) with a proof, checked by unit tests, that it can only
  ever ADD a conversion and can never touch a word already protected as
  English, Unicode Bengali, or otherwise excluded. Measured against the real
  corpus before being built: it would safely rescue only **365 words**,
  against a 2,840-word bar set in advance for whether the change was worth
  making the classifier's decisions depend on font metadata at all. Not
  built, on that measurement. A larger, riskier version — letting a declared
  legacy font override the English dictionary, reaching up to 12,175 more
  words — was designed but not pursued: it has no proven safety net (no
  English-only Office document in the negative corpus carries a legacy font
  today), and the smaller version's own measurement suggests the honest
  reachable population is smaller than hoped throughout.
- A new diagnostic, kept because it is useful independent of this decision:
  `eval`'s D2 report now breaks down exactly *why* every unrecovered
  `legacy_ascii` token was refused, by rule, rather than requiring anyone to
  guess which rules are in principle capable of firing.

### The command line, rebuilt for someone who has never used one

**`mukti` alone now has a conversation instead of printing a usage block.**
On a real terminal, with no verb and no file named, it asks which folder to
convert, reports what it found there (by type, noting sub-folders it did not
look inside and any of its own earlier output it skipped), asks whether the
result should go in a new folder or beside each original, warns before
replacing anything that already exists from an earlier run, confirms before
writing a single byte, shows one progress line while converting, and reports
— leading with any failure, so a run of 390 successes never reads as
cheerful about the 10 that were not. Piped, scripted, or run from CI, it
prints the same help text as before and exits 0: nothing about flag-mode use
changes. Typing a single file with no verb — `mukti report.docx`, the thing
a beginner actually types — is asked about directly rather than refused
over a word ("convert") there was never a reason to know.

**Two new flags.** `--jobs <n>` converts up to that many files at once
(default 1, which reproduces the exact single-threaded order of every
earlier release); `--theme <light|dark|off>` sets or disables colour by
hand. `--out`, `--in-place`, `--force`, `--font` and `--quiet` are
unchanged.

**Renamed, in every string Mukti prints, from "GRU953 Mukti" to "Mukti by
GRU953".** The prefix form is the brand kit's own fallback for a name too
generic to stand alone on its own — "Mukti" is not that, so this follows
the naming rule as written rather than bending it.

**Colour, decided by a ten-step ladder, never hard-coded.** First match
wins: `--theme off`; `NO_COLOR` set; stdout not a terminal; `TERM` unset or
`dumb`; Windows without a VT-capable host; `COLORTERM` not truecolor;
`--theme light|dark`; `MUKTI_THEME`; `COLORFGBG`'s background field;
otherwise **no colour**, with a one-line hint (`mukti --theme dark`) shown
once, before the guided conversation starts. Stdout and stderr are judged
separately, so `mukti convert *.docx > log.txt` still shows coloured errors
on screen while writing a clean log. Every coloured state also carries a
plain-ASCII marker (`[!] Error:`, `[!] Warning:`, `Done.`, `Note:`,
`Skipped:`), so meaning survives with colour off, in a log file, or in a
terminal with no glyph coverage for anything fancier.

**Parallelism, folded in from the speed work this release deferred.**
`--jobs` runs files through `std::thread::scope` (no new dependency —
`rayon` left the tree in the PDF removal above and stayed out), largest
file first so a long conversion never queues behind a run of short ones,
with concurrent bytes in flight bounded so the five-plus `.pptx` files
known to exceed 200 MB cannot all be in memory at once. Every destination
is computed up front, single-threaded, before any file is read: two inputs
that would derive the same output name (`notes.doc` and `notes.docx` both
become `notes.unicode.docx`) refuse the whole run, naming both, rather than
letting the second writer silently win a race that never existed before
threads did.

**A file name is treated as data a stranger chose, not as safe text.** Every
path reaching a printed message is passed through a defence that turns a
raw control byte or terminal escape sequence into its visible Unicode
"control picture" glyph — so a file cleverly named to repaint the terminal
or impersonate Mukti's own output prints as something safely inert instead.

**Eight modules where one 641-line file stood before**: `words` (every
string a person can see, plus the brand-compliance test suite that sweeps
all of them for a banned word, an exclamation mark, an ungrouped number, a
sentence over 25 words, an error over 30, and the locked tagline byte-exact
even though it is not currently printed as its own line); `style` (the
fixed palette and the colour ladder); `options` (argument parsing);
`report` (number formatting, the file-name defence, word wrap, the progress
line); `convert` (the six-format gate, per-file conversion, the parallel
batch); `pathinput` (turning a typed or dragged-in folder path into
something usable — trailing drag-and-drop spaces, quoted paths, Unix
backslash-escaping, `file://` URLs, `~`); `guided` (the conversation, tested
by scripting it over an in-memory stand-in for the file system, with no
real terminal or document involved); `main` (dispatch only).

**Two simplifications from the original design, recorded rather than
hidden.** Guided mode converts one file at a time, through the same
per-file logic flag mode uses, rather than through the parallel batch path
— the trade for a progress line that can report after every file rather
than after a whole, possibly reordered, batch; `--jobs` from guided mode is
not offered, so this costs nothing today. And "some outputs already exist"
is one yes/no for the whole batch rather than a per-file choice — simpler,
at the cost of an all-or-nothing answer when only some files collide.

### Release: macOS arm64 only

**`release.yml` builds one binary now, not three.** Windows, Linux and the
Intel half of the macOS universal binary are gone; `mukti-macos` is an
Apple Silicon binary only, asserted so by `lipo -archs` rather than merely
claimed in the file name. Two decisions this deletes were both real,
expensive lessons, preserved here rather than lost with the code that
embodied them: the two-job Intel/ARM macOS split this project tried before
the universal binary was abandoned because `macos-13` runners queued for
half an hour without starting on a private repository's quota; and the
Linux build was pinned to `ubuntu-22.04` rather than `ubuntu-latest`
specifically to hold the binary's glibc requirement at 2.35, so it would
keep running on Debian 12 and RHEL 9 — a constraint that stops mattering
the day there is no Linux build to hold it for.

**`ci.yml` tests on macOS only**, for the same reason. The formatting and
lint job stays on Ubuntu regardless — cheaper, and its verdict does not
depend on the operating system running it, so this is a stated difference
rather than an inconsistency. The real cost, worth naming rather than
burying: a portability break in `gru953-mukti` or `mukti-formats`, both
ordinary Rust with nothing macOS-specific in them, would now go unnoticed
here rather than failing on the platform it broke.

### Fixed on a verification pass over this whole release

Everything above was audited against the plan it came from once it was
written, which found ten things worth correcting. They are listed because
several are the kind that would otherwise have quietly stayed wrong.

- **A comment that lied about the code, in the place it mattered most.**
  `compose_two_part_vowels` carried "Nothing in the shipped path calls this;
  it is reached only through `repair_unicode`, which is itself unused." The
  first half was false: `convert` calls it directly, on every conversion, so
  the two **deleting** repair passes inside it run on every word this tool
  converts. The code is defensible and each pass is documented where it is
  defined — but a deletion nobody knows about is exactly the mechanism that
  could hide the reordering faults this project has recorded three times, and
  the comment invited a reader to treat it as dead code.
- **`words_normalised` is finally printed.** It has been computed since 0.7.0
  and shown nowhere, which meant the one edit Mukti makes to text it
  otherwise promises to leave untouched — joining a two-piece vowel sign into
  the single character Unicode defines it as — happened silently. Both flag
  mode and guided mode now say so, and only when it actually happened.
- **The output folder guided mode offers is dated**, `mukti-converted-2026-08-20`,
  so converting the same folder next week does not collide with this week. The
  date arithmetic is hand-rolled from `SystemTime` rather than reached for
  from a crate: this release deleted three date/time libraries along with the
  PDF reader that pulled them in, and putting one back to name a folder would
  have undone much of that.
- **`file(s)` and `folder(s)` are gone**, replaced by real agreement — "1 file",
  "2 files". Found by writing the golden-screen test the plan asked for and
  actually reading the transcript it captured, which is the entire argument
  for having one.
- **`RUSTFLAGS` was still set in a third place.** The consolidation into
  `.cargo/config.toml` removed it from `ci.yml` and `.sandbox/activate` and
  missed `.claude/settings.json`, which then went on silently overriding the
  file that was supposed to be the single source of truth. A neat
  demonstration of the hazard the consolidation existed to fix.
- `encoding.rs`'s module doc still argued entirely about reading `.txt` files
  and still claimed a user could be shown an error that is now unreachable.
  Rewritten to say what is true: nothing the `mukti` command does reaches it,
  and it is kept for `corpus-verify`'s English-only negative check.
- `LEGACY_FONTS`' doc comment still named `pdf.rs`, deleted in this release,
  as one of two other font lists in the workspace. It is now the only one, and
  says how both of the others came to be retired.
- `.github/dependabot.yml` still grouped `lopdf`, which no longer exists.
  Replaced with `office_oxide` — the crate that parses the oldest and hairiest
  binary formats, and which should have been in the file-parsers group from
  the day it was adopted.
- **Two tests the plan required and did not get**: the golden screen above,
  and one integration test that runs the real binary via
  `env!("CARGO_BIN_EXE_mukti")` — no dev-dependency — covering the exit codes,
  which stream each line goes to, and that a bare `mukti` in a script still
  prints help and exits 0. That last one is the guard that keeps guided mode
  from breaking every script that ever called this tool.
- The plan's three-state `Verbosity` was built with two states, because the
  third had no reader: guided mode prints in its own voice and has no
  flag-mode output to suppress. Recorded rather than quietly dropped.

### Closing the items 0.9.0 had left open

Four things were recorded as deferred earlier in this entry. Each was
investigated properly before being either built or refused, and two of the
refusals are more useful than the builds.

**Two false positives in real English documents are fixed, both measured
before they landed.** The English-only negative corpus — 311 Markdown files,
66 notebooks and 8 Python files that must come through a conversion
completely untouched — had four failures, open since 13 August. It is now
**clean, 0 of 517**.

- A lone `¹`, `²` or `³` is no longer converted. A footnote marker `¹` was
  becoming জ্ঞ, because in Bijoy that character IS জ্ঞ and that bare
  consonant cluster happens to be one of the 451,348 words in the shipped
  list. The new rule says something narrow and structural: all three of those
  Bijoy readings are **conjuncts**, and a conjunct is part of a word, never a
  whole one. So a one-character token reading as one is impossible by
  construction. **Measured cost: exactly zero** — identical recall on both
  splits and both answer keys, zero legacy words newly missed. Nothing about
  a `¹` inside a word changes: `j²x` is still লক্ষ্মী. This is deliberately
  NOT the wider sub/superscript rule, which was measured at -2.2 points of
  recall and reversed in an earlier release.
- A conversion that would open on a character Bengali never opens a word with
  — `ঞ` `ং` `ঃ` `ঁ` `ৎ` `ড়` `ঢ়` `য়` — is no longer treated as plausible.
  `Tomáš`, a Czech name, was becoming `ঞড়সপ্সন্`: two accented Latin letters
  are genuine Bijoy table entries, so the density rule fired. The premise was
  checked against the shipped word list rather than assumed — **zero** of
  451,348 words begin with five of those characters, and the fourteen that
  begin with the other three are word-list noise. **Measured cost: -0.0023
  percentage points of recall**, four tokens, against 0.93 points of headroom
  on a 99% gate. Every one of those four was already producing a
  non-dictionary output, and four of the ten across both splits were tokens
  where the old behaviour mangled embedded English — `Thickness/†Lvqvi` came
  out as `ঞযরপশহবংং/খোয়ার`. By this project's own asymmetry rule, refusing
  those is a gain.

Both changes are **monotone**: they can only turn a conversion off, never on,
so neither can create a new false positive anywhere. Verified on the real
corpus rather than argued: of 1,614 documents, 23 differ, and in every one of
the 23 the converted count fell, the untouched count rose by exactly the same
amount, the total was conserved, and no file broke. 47 words stopped
converting in total.

**`convert()` is 38.7% faster, and none of it came from the change the plan
proposed.** Measured on the repository's own `bench inline`: 215,943 →
299,590 words per second, against a noise floor of 0.03%.

- One-character keys are searched for as a `char`, not a `&str`. 187 of
  `CONVERSION_MAP`'s 191 keys are a single character, and handing `find` and
  `replace` a `&str` made them build a Two-Way substring searcher — setup
  measured at roughly a third of `apply_map`'s entire cost, for needles that
  need none of it. Same algorithm, same rule order, and the existing
  differential test covers it unchanged.
- The stem list is nukta-folded once, not once per call. `word_hits` was
  folding all of its stems on every call, three allocations each: **462
  allocations per call**, against 107 for converting a whole ordinary line.
  It is now 3.
- The vowel composition uses the single-pass composer that already existed
  instead of two chained `.replace()` calls. Those two allocated a
  full-length copy of the text on every conversion **whether or not either
  pattern was present** — `str::replace` allocates its output even when it
  matches nothing.
- `repair_word` asks whether there is anything to repair before allocating
  rather than after, and `rearrange` reserves its output properly on the way
  out. `String: FromIterator<char>` reserves the character count as though it
  were a byte count, so Bengali at three bytes per character under-reserved
  threefold and regrew twice.

**The Aho–Corasick rewrite of `apply_map` was measured and refused.** Folding
the tables into one pass was *proved* sound for `CONVERSION_MAP` and
`POST_MAP` — zero cascading rule pairs across 18,145 and 136 ordered pairs,
confirmed twice independently, and zero output differences over tens of
millions of differential cases. It was still refused, for four reasons that
only appeared once someone looked.

`PRE_MAP` cannot be folded at all: its three descending newline rules are a
deliberate hand-rolled iterated collapse, 15 cascading pairs, each
demonstrated with an input where a single pass gives a different answer. The
no-cascade property turns out to be **necessary but not sufficient** — the
table `[("xa","Y"),("aa","Z")]` on input `"xaaa"` satisfies it and still
diverges under one of the two obvious ways to write the resolution loop. The
arithmetic does not favour it either: `convert()` is called one word at a
time, mean 6.47 bytes, so on a million real tokens a single pass that builds
its automaton per call is 1.10× while the `char` fast path that shipped
instead is 1.85×, and only a cached automaton reaches 3.55×. And merging the
two "clean" tables into one pass — the obvious next step — was measured to
corrupt 223 words in 2,043,887: `"24th"` became `২৪ঃয` instead of `২৪:য`.
Recorded so nobody re-derives it.

**Threading two reusable buffers through `convert()` was also refused, on
measurement.** A counting allocator put one conversion of an ordinary line at
107 allocations; the seven stage boundaries the plan wanted to eliminate are
**7 of those 107**, and on a longer input the share falls to 0.13%. An
optimisation whose win shrinks as inputs grow is the wrong one, and the four
changes listed above were found by looking for where the allocations
actually were.

**The one silent deletion in the converter is now counted and printed.**
`repair_word` removes a character when the word list recognises neither
candidate repair — no evidence either way, so it guesses from structure.
That is defensible, and it is exactly the kind of mechanism that could hide
a reordering fault, so it is tallied in `mukti-core` and `corpus-verify`
prints the total on every corpus run. A tally nobody reads is not
instrumentation.

### Fixed: the release gate could not pass

`./check-figures.sh` has been unusable since the answer-key rebuild earlier
in this release, and nobody noticed because the way it failed looked like the
thing it was built to do.

The rebuild moved the English false-positive figure through its own 0.10%
target, for a reason diagnosed at the time: the answer key labels genuine
Bijoy as English (`Avq` → আয় 27 times, `UvKv` → টাকা 22 times). `eval`
correctly exits non-zero on any missed target, so the script printed the
report and stopped — **before reaching the section that compares every
published figure against what was measured**. A gate that can never pass
cannot report anything new either, and two of its own hard-coded figures had
gone stale behind that failure.

Now: exactly one named exception, with a **ceiling**. Any other missed target
is still fatal, and the known one getting worse is fatal too. The stale
figures are corrected, and the script passes again — so the five published
accuracy figures are once more checked against the code on every run.

### Fixed: three counts and one unreachable rule

Every stated table count in `mukti-core` was wrong at once. `tables.rs` said
190 entries for 191 — the Greek mu entry added in 0.7.0 was never counted —
and a test comment said 226 rules for 224. All three are corrected and a new
test counts the tables rather than trusting the comment, because in this
project a number in a comment is a claim.

While counting, one rule turned out to be unreachable: `("¤œ", "ম্ন")` can
never fire, because `("œ", "্ন")` sits earlier in the same table and always
eats the `œ` first. What actually happens is that `¤œ` becomes `ম` + halant +
halant + `ন`, and the doubled-halant collapse downstream removes the extra —
so the output is right after all, by a longer road. The table is **not**
reordered: this project's rule is that a generated table is not reordered
without evidence from the font. Instead the behaviour is pinned by a test, so
if that downstream cleanup ever changes the failure is loud rather than
silent in real documents.

### Verified

**246 tests**, up from 153 at 0.8.0. Full workspace build, test, `cargo fmt
--check`, `cargo clippy -D warnings` and `cargo deny check` all clean, and
the clippy and test runs were repeated with `RUSTFLAGS` unset so the run
matched what CI sees rather than what a local shell happened to export.

**`corpus-verify --compare` against the pre-change baseline: 0 differing,
0 failed, 0 panicked.** That is the gate the plan set for every optimisation,
and it is what licenses the two performance changes above: the differential
test proves `apply_map` is output-identical rule by rule, and the corpus run
proves it on real documents.

**A note for anyone parsing this tool's output:** the per-file text on stdout
has changed wording throughout this release. It was never a stable interface
and is not becoming one — `--quiet` and the **exit codes** are the contract
to depend on. Anything reading the prose will need adapting.

## 0.8.0 — 19 August 2026

**One removal, and it is a removal rather than a fix on purpose.** `.json` is no longer
a supported format. The minor version rises because a documented format has been
withdrawn, which is a breaking change for anyone who scripted it.

### Removed

- **`.json` support.** It was listed as supported from 0.3.0 and **never once tested.**
  The first time JSON was put through Mukti, on 19 August 2026, **5 of 13 real files
  came out invalid.** The conversion tables map Bijoy's curly double quotes (`Ò` and
  `Ó`) to a plain ASCII `"`, which ends a JSON string value early and makes the file
  unloadable.

  Reproduced in 100 bytes:

      {"b": "ÓKg©m~wPÒ Gi Rb¨"}   ->   {"b": ""কর্মসূচি" Gi জন্য"}

  Converting JSON properly means parsing it, converting only the string contents and
  re-serialising with correct escaping. That needs a JSON parser as a dependency, and
  the choice was to drop a format nobody had asked for rather than add one.

  **Refusing is the honest implementation of that choice.** There was no list of
  supported text extensions to remove JSON from — anything unrecognised falls through
  to the plain-text path — so editing the documentation alone would have left the
  corruption in place while claiming otherwise. `mukti convert x.json` now explains
  what would go wrong and writes nothing. `.txt`, `.csv` and `.md` are unaffected: none
  of them has escaping rules to break.

  A pipe cannot be checked this way, because a pipe has no name. `cat x.json | mukti
  convert -` is still treated as plain text, and the refusal message says so.

### Verified

152 tests (one new, asserting both that the refusal happens and that no file is
written — refusing while still writing something would be worse than either), clippy
clean, formatting clean, `cargo deny` clean.

Everything else is unchanged from 0.7.1, which was tested against all 1,173 corpus
files on 19 August: 1,173 of 1,173 converted with every one exiting 0, round-trip
accuracy 99.9764% of words and 99.9933% of characters, zero English words wrongly
converted across 7,801,733 aligned tokens, and a 422 KB English negative control that
came out byte-for-byte identical.


## 0.7.1 — 19 August 2026

**One fix: the Unicode normalisation 0.7.0 announced did not work on Word, Excel or
PowerPoint.** It worked only on plain text files. Found by testing the published 0.7.0
binary against 1,059 real documents — the release notes claimed something that was not
true of the format most people use.

### Fixed

- **The two-part vowel composition now reaches Office documents.** Measured across the
  806 converted Office files in that run: **9,904 decomposed vowel pairs survived**
  inside real text elements, against 404,972 composed. Two independent causes, both
  invisible to the tests that existed:

  1. `mukti-formats/src/office.rs` does not call `classify::convert_pieces` — it
     **duplicates** that loop, calling `classify_words` and `convert` directly, so the
     normalisation added to `convert_pieces` in 0.7.0 never ran for an Office document.
     This project's own audit had already flagged the duplicated loop as a drift risk.
     This is what the drift cost.

  2. Even once that was fixed it still did nothing, for a second reason: a rewritten
     part is kept only if the rewrite *did* something, and that test counted converted
     words and renamed fonts. A document needing nothing but composition had its
     rewrite computed and then thrown away with the part. `Summary` gains
     `words_normalised`, which is counted where the composition happens and joins that
     test. The existing comment there is right that comparing strings would be the
     wrong test — re-serialising XML changes bytes without changing text — so this is
     a counter, not a comparison.

  The regression test uses a document containing **no legacy text at all**, which is
  what makes it catch the second cause. A test with legacy text in it passes on the
  first fix alone and hides the discard.

### Known, and deliberately not fixed

A further **141** decomposed pairs in that run sit inside `<v>` elements — Excel's
cache of a formula's last computed value. Those are never rewritten, because `<v>`
normally holds numbers and editing it would corrupt data. The cache is regenerated from
the formula, so the practical consequence is only this: **a cell whose text arrives via
a formula keeps its old spelling on screen until Excel recalculates.**

### From the same test run, for the record

The published 0.7.0 was otherwise sound on all 1,059 files: every one converted, all
exiting 0; **zero** English words of four letters or more wrongly converted across
7,680,430 aligned tokens; and all four of the garbage strings 0.6.1 produced (`ঐধৎস্থ`,
`উবপরফব্থ`, `লঁফমসবহঃ)্থ্থ`, `ঙহিবৎং্থ`) absent from all 806 converted documents, where
0.6.1 produced 11 of them.

Round-trip accuracy on that run: **99.9763%** of words and **99.9934%** of characters,
over 2,640,521 words the encoder can represent.

**The measuring harness had to be rebuilt before any of that could be trusted**, and it
still got one answer wrong: it first reported 2,006 English words converted, whose pairs
turned out to be unrelated tokens sitting at the same offset inside a replace block of
unequal length. Restricted to one-for-one replacements the count is zero. Recorded
because a harness that is wrong in the flattering direction is the more dangerous kind.

### Verified

151 tests (one new), clippy clean, formatting clean, `cargo deny` clean with zero
ignored advisories and zero licence exceptions. Every figure in the accuracy harness
unchanged from 0.7.0.


## 0.7.0 — 19 August 2026

Six defects fixed, one of them serious enough to lose data silently, plus a larger
English dictionary and an optimisation that pays for the biggest fix. Every change was
measured against v0.6.1 over 400 real documents, and the accuracy harness was re-run
after each one.

### Fixed

- **BLOCKER: an Excel workbook that stores its text inline was never converted, and
  the tool said "0 of 0 words converted".** Excel may keep a cell's string in
  `xl/sharedStrings.xml` or inline in the worksheet; both are valid, and only the
  first was read. The failure was silent — "0 of 0" is indistinguishable from a file
  with no legacy Bangla — so nobody would notice. Found by converting a real archive
  in which 2 of 140 spreadsheets were written that way, one of them 911,834 cells.
  Neither lost data, because both happened to be already Unicode. That was luck.

- **An English word ending in a curly apostrophe was transliterated into Bengali.**
  `Harm’` became `ঐধৎস্থ`. The English test was gated on the token being plain ASCII,
  and a trailing `’`, `”` or `—` from a word processor defeats that, so neither the
  English dictionary nor the short-word guard was ever consulted. 11 tokens in 7 of
  1,059 documents, **three of them inside live spreadsheet formulas**, where a
  transliterated identifier breaks the formula rather than merely looking wrong.

  The obvious version of this fix was tried in August and rejected on measurement:
  trimming both ends of any punctuation exposed short Bijoy cores and cost a full
  point of detection recall. This one trims only a trailing run of six typographic
  characters and requires at least four letters to remain — a floor added after the
  first attempt was measured and *did* cost recall, turning `Mi“` (গরু) into English.

- **The Greek letter μ was not mapped where the micro sign µ was.** They are visual
  twins and the legacy font draws both as `ক্র`, so `বিক্রেতা` came out as `বিμেতা`.
  105 words in the same run; 60 runs of text in the 400-document comparison now
  convert that did not before.

- **A zero-width joiner stranded a vowel sign in front of its consonant.** The walk
  that moves a pre-kar after its consonant advances only over consonants, and a joiner
  is not one, so the vowel stayed in visual order. 37 tokens across 16 documents.

- **`mukti check` on a PDF hid both things that make a PDF different** — that layout
  will be lost, and that some text cannot be read at all. `convert` reported both;
  `check`, the command whose entire job is to say what would happen, reported neither.
  Over 253 real PDFs it was silent about 1,196,732 unreadable pieces, including 22
  files from which nothing at all would be recovered.

- **The PDF failure path was the last one echoing a library's own error wording** at
  the user — cross-reference tables and invalid file headers. It now says, in one
  sentence, that the file is not a readable PDF.

### Changed

- **Already-Unicode Bengali is now composed to Unicode NFC.** A vowel sign stored in
  two pieces (`ে`+`া`) becomes the single character Unicode defines it to be (`ো`).
  This is the one exception to "already-correct Bangla is left untouched", and it is
  confined to canonical equivalence, so it cannot change meaning — only findability,
  which is the entire purpose of converting. 1,688 words in a 1,059-document run
  arrived written that way; none of it was Mukti's doing. It is not counted as a
  conversion. The wider repair passes remain deliberately unused: they *delete*
  characters, which is a judgement about intent rather than a normalisation.

- **Dictionary lookups now compose the two-part vowels too.** The dictionary is built
  with the composed spelling but was queried with whatever the caller held, so an
  ordinary word spelled the other way was reported not to exist — weakening detection,
  since "is the converted word a real word?" is one of the signals used. The same
  ambiguity in the nukta has already cost this codebase four defects.

- **The English dictionary grows from 234,428 to 465,971 words.** Webster's Second
  International lists headwords only: `owner` is present and `owners` is not, as are
  `member`/`members` and `meeting`/`meetings`, so every English plural had no
  dictionary protection at all. Regular plurals are now derived from the existing
  public-domain list rather than importing a new one, which avoids adding a licence
  obligation and keeps the derivation auditable — the rules are fifteen lines. Only
  bases of four letters or more are pluralised, again because a measurement showed
  three-letter bases cost recall.

  Measured effect: English false positives **0.014% → 0.013%**, precision
  99.953% → 99.956%, and detection recall unchanged at **99.962%**.

### Performance

- **Adding worksheets to the rewritten parts cost about 85% on large spreadsheets**,
  and nearly all of it was waste: a worksheet whose strings live in `sharedStrings`
  has no text elements at all. The expensive parse is now gated behind a substring
  search of the raw bytes, which cannot produce a false negative. Measured on three
  real workbooks of 10, 16 and 41 MB:

  | Workbook | v0.6.1 | with the fix | with the gate |
  |---|---|---|---|
  | 10.1 MB | 0.92s | 1.69s | **1.00s** |
  | 16.2 MB | 1.71s | 3.31s | **1.90s** |
  | 41.4 MB | 3.78s | 6.78s | **4.25s** |

### Verified

150 tests (six new regression tests), clippy clean, formatting clean, `cargo deny`
clean with zero ignored advisories and zero licence exceptions.

Against v0.6.1 over 400 real documents drawn with a recorded seed: **12 of 400 files
differ, 60 runs of text now convert that did not, and every one of the six remaining
places where Bengali disappeared is an English word correctly restored.** No
regression survived. Two did not survive the first attempt and were found by this
comparison rather than by the harness — the accuracy figures did not move when recall
was genuinely being lost, which is worth knowing about the harness.


## 0.6.1 — 19 August 2026

A maintenance release. Nothing a user does changes; both items are about what the
project depends on, and both were measured rather than assumed.

### Changed

- **`zip` upgraded from 2 to 8** — six major versions — in the code that opens
  every `.docx`, `.xlsx` and `.pptx`. That is untrusted input, so this is the one
  dependency where being current matters most.

  The published 0.6.0 binary carried **two** archive parsers: ours on 2.4.2 and
  `office_oxide`'s on 8.6.0. There is now one to audit instead of two, and the
  dependency tree drops from 157 packages to 153.

  **Verified against 3,088 real documents** (1,877 distinct), converted before and
  after and compared on a hash of the whole output archive:

  | | |
  |---|---|
  | Identical output | **1,874 of 1,877** |
  | Differing | 3 |
  | Word and font counts changed | **none** |
  | Documents that stopped converting | **none** |

  The three differ by 36 bytes each — two per archive entry, in the central
  directory's external-attributes field, where the old version stamped octal
  `100644` and the new one stamps `100000`. Every entry's content hash, CRC,
  compressed size and compression method is identical, and the outputs still open
  in an OOXML reader. Those three documents take the `raw_copy_file` path, so this
  is how the library restamps a copied entry, not anything Mukti writes. Office
  formats do not read Unix permission bits.

  **The binary got 64 bytes smaller**, which is to say it did not change. A
  duplicate parser was expected to cost more than that; the linker had already been
  discarding the unused copy. The reason to do this is the audit surface, not the
  size — stated because the opposite would have been the easier claim to make.

  144 tests pass, clippy clean, formatting clean, `cargo deny` clean.

- **Four GitHub Actions moved off the deprecated Node 20 runtime.**
  `actions/checkout` v4 → v7.0.1, `actions/upload-artifact` v4 → v7.0.1,
  `actions/download-artifact` v4 → v8.0.1 and `softprops/action-gh-release`
  v2 → v3.0.2. Every 0.6.0 build logged a deprecation warning saying these were
  being forced onto Node 24; this makes that explicit instead of accidental.

  All four remain pinned by commit SHA, and each SHA was checked against the tag it
  claims to be before it was trusted — `action-gh-release` v3.0.2 is an *annotated*
  tag, so its ref points at a tag object rather than a commit, and it has to be
  dereferenced before the two can be compared at all. The other three are
  lightweight tags where the ref is the commit.

  `action-gh-release` v3.0.0 is a runtime change only, with no altered inputs, so
  the two this project passes (`files` and `draft`) are unaffected. It is
  nonetheless the step that creates the release itself, so the whole workflow was
  proved on a throwaway tag before this one was cut.


## 0.6.0 — 15 August 2026

### Corrected in the documents

An independent audit of the 0.6.0 tree, run before it was published, found that
removing the window had left a trail of statements that were no longer true. They
are corrected here rather than after release.

- **The published accuracy figures disagreed with each other.** `USING-MUKTI.md`
  and `HANDOVER.md` still carried a superseded answer key — detection recall
  99.951% of 154,928 tokens and English false positives 0.006% — while `README.md`
  carried the re-measured 99.962% of 177,079 and **0.014%**. README was right.
  The false-positive rate genuinely rose when the sample grew, and quoting the
  older, flattering number in the two documents a user actually reads was the
  wrong way round. README also contradicted its own table in one sentence.

- **The licence audit's package count was wrong by a factor of three.** It stated
  484 packages; the published `Cargo.lock` resolves **157**. The 484 figure
  predated the window's removal.

- **`calamine` was listed as a dependency.** It is not one, and never has been in
  any commit. It was introduced into `THIRD-PARTY-LICENSES` during the window's
  removal, in the same edit that took out the Tauri entries.

- **"Where a package offers a choice of licences, this project takes MIT" was not
  the whole truth.** Four packages in the shipped binary carry an obligation that
  electing MIT does not remove — `encoding_rs` (BSD-3-Clause, AND-ed),
  `unicode-ident` (Unicode-3.0, AND-ed), `zlib-rs` (Zlib only) and `zopfli`
  (Apache-2.0 only). All four are permissive and all four are now named.

- **The handover's repository map** still listed `assets/brand/` and claimed a
  HTML/CSS/JS front end, with a line count out by about 4,000. There are now no
  HTML, CSS or JavaScript files at all, and 11,100 lines of Rust.

- Smaller ones: a `.gitignore` rule for the deleted app crate; an OFL-1.1 licence
  entry justified by fonts that no longer ship; an `ubuntu-22.04` pin explained by
  a webkit package name when the real reason is now glibc 2.34; four comments in
  the shipped library describing text being pasted "into the window"; a README
  claim that every interface string sits in one table; and a handover paragraph
  calling the residue study the top open task two days after it was completed.

### Fixed, and it is a licence matter

- **The binaries shipped with no licence beside them.** Anyone who downloaded a
  single file from the releases page received neither `LICENSE` nor
  `THIRD-PARTY-LICENSES`, though MIT and all four of the licences above require
  their notice to travel with the software. Both files are now attached to every
  release, and the release job fails if either is missing or empty.

- **Release assets were named after the Rust target triple**, so somebody wanting
  the Linux build had to work out that they needed
  `mukti-x86_64-unknown-linux-gnu`. They are now `mukti-macos`, `mukti-linux` and
  `mukti-windows.exe`.

- **A fabricated consultancy** — a company name with a legal form and a city —
  was published as a test fixture, where all the test needs is nonsense. Replaced
  with something that cannot be mistaken for a real organisation.

### Removed

- **The desktop window.** Mukti is now a command-line tool and a library, and
  nothing else. The window shipped in 0.3.0 and was made to work properly in
  0.5.0, so this is not a retreat from something broken — it is a decision to keep
  one thing working well rather than two adequately. Everything the window did the
  command line does, including converting a whole folder in one command, which the
  window never could.

  Going with it: the four embedded typefaces, the brand colour tokens, the two
  GRU953 marks, and the webkit and GTK dependencies that made the Linux build need
  system packages. The licence notices for the fonts, the tokens and the marks have
  gone too, because there is nothing left here for them to describe.

- **Installers.** There is one file per platform now — the binary — rather than a
  `.dmg`, `.msi`, `.exe`, `.deb`, `.AppImage` and `.rpm`. The release job checks
  that each binary *runs* rather than merely that it exists.

### Fixed

- **A large spreadsheet no longer looks like a hang.** Rewriting a document cost
  the number of runs times the number of pieces, because the function handing each
  run its share of the text scanned the whole document to find it. Found by
  converting an 8.1 GB archive: five spreadsheets never finished inside 300 seconds
  and twelve took over thirty, every one an `.xlsx`. A 62 MB workbook took 61 ms to
  read and **131 seconds** to convert — and converted nothing at all, having no
  legacy Bangla in it.

  Now linear. The same workbook takes **0.5 seconds**; a 98 MB one that never
  finished takes 1.9. Proved to change nothing else by reconverting 492 real
  documents and comparing against what the previous build wrote: all 492 identical.

- **Byte 0xD0 is the conjunct `ণ্ড`, not a dash.** It had been a hyphen since the
  tables were first ported. 123 occurrences across 27 words in the test archive
  were coming out with a hyphen in the middle — `অর্থদ-` for `অর্থদণ্ড`. Settled
  from the font itself, which draws the conjunct. Dictionary hit on real documents
  rose from 94.023% to **94.053%**.

- **118 legacy font families were not recognised.** The list matched 9 of the 127
  families Bijoy ships. Rebuilt from the internal name tables of 498 font files.

- **Two Unicode fonts were being treated as legacy** — `SutonnyOMJ` and
  `SutonnyUniBanglaOMJ`, along with `+mj-lt`, which is not a font name at all but
  an Office theme reference. 2,069 runs of already-correct Bengali were being
  offered to the converter.

- **Error messages no longer leak the parser's own wording.** A damaged file said
  `invalid Zip archive: Could not find EOCD` or `CFB error: I/O error: failed to
  fill whole buffer`. Both now say what a person can act on.

### Changed

- The Linux build needs no system packages, because nothing draws a window.


## 0.5.0 — 14 August 2026

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
  `Green`, `-`, `Belt`. Breaks now happen only where the text moves down the
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


### The faults in 0.4.0 that this release fixes

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
