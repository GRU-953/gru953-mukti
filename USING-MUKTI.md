# Using Mukti by GRU953

A short guide for anyone who has old Bangla documents that come out as
gibberish. No technical knowledge assumed.

> ## Warning: there is no longer a window — Mukti is a typed command
>
> Versions 0.3.0 to 0.5.0 also came as an app with a window. **From 0.6.0 it does
> not.** Everything the window did, the typed command does, and it can convert a
> whole folder in one go, which the window never could. This guide is written for
> someone who has never used a command before.
>
> **If you are on 0.4.0, please upgrade whichever you used.** In that version the
> window opened but nothing in it responded, and a damaged or deliberately
> malicious PDF or Office file could stop the program or use up your computer's
> memory. Both are fixed. Anything you converted with the 0.4.0 command is fine —
> the conversion itself has not changed.

## The easiest way: type `mukti` and answer its questions

From 0.9.0, typing `mukti` on its own — no file name, no other word after it
— starts a conversation instead of printing a list of options:

```bash
mukti
```

It asks which folder holds the files to convert, says what it found there,
asks where the results should go, checks before writing anything, and shows
a line of progress while it works. Every step is a plain question with a
plain answer; typing `q` at any point stops without changing anything.

Everything else in this guide — typing the file name directly, `--in-place`,
`--out`, and so on — still works exactly as written, for anyone who prefers
to type the whole command in one line, or who is running Mukti from a script.

## What problem this solves

Bangla typed years ago in Bijoy or SutonnyMJ is not really Bangla to a
computer. It is English letters that *look* Bangla because a special font
draws Bangla shapes on top of them. So the text cannot be searched, cannot be
spell-checked, and turns into nonsense on any machine without that font.

Mukti turns it into proper Unicode Bangla, which every modern computer and
phone understands.

**It changes only the legacy Bangla.** English, numbers, and Bangla that is
already correct are left exactly as they were.

## Getting it onto your computer

**Mukti is built for Apple Silicon Macs (M1 or later) only, as of 0.9.0.**
If you are on an older Intel Mac, Windows or Linux, this download will not
run — see the README for why, and for how to build it from source instead.

1. Go to the project's **Releases** page and download `mukti-macos`.
2. Put it somewhere you can find again — your home folder is fine.
3. Open **Terminal**: press ⌘ + Space, type `Terminal`, press Return.
4. Allow the file to run by typing this once, then Return:

```bash
chmod +x ./mukti-macos
```

5. Check it works. Type this, then Return — it should print a version number:

```bash
./mukti-macos --version
```

Everywhere below, `mukti` means the name of the file you downloaded.

## Using it

Each step is one line you type, then Return.

**See what would change, without changing anything:**

```bash
mukti check report.docx
```

**Convert a file.** The original is never touched — you get a new file beside
it, called `report.unicode.docx`:

```bash
mukti convert report.docx
```

**Convert many files at once:**

```bash
mukti convert *.docx
```

**Convert many files faster, using more than one at a time:**

```bash
mukti convert *.docx --jobs 4
```

**Replace the original instead** (only if you are sure, and have a backup):

```bash
mukti convert report.docx --in-place
```

**Turn colour on or off by hand**, if Mukti's own guess about your terminal
is wrong:

```bash
mukti convert report.docx --theme dark
```

## What Mukti can open

Six formats, and nothing else, as of 0.9.0:

| Kind of file | What you get |
|---|---|
| Word (`.docx`) | Converted inside the document; formatting kept |
| Excel (`.xlsx`) | Converted inside the workbook; formatting kept |
| PowerPoint (`.pptx`) | Converted inside the slides; formatting kept |
| Older Word (`.doc`) | Converted into a new `.docx` beside it — **text only** |
| Older Excel (`.xls`) | Converted into a new `.xlsx` — **text only** |
| Older PowerPoint (`.ppt`) | Converted into a new `.pptx` — **text only** |

**About the older three.** They store no formatting we can carry and no font
information, so only the words come across — no colours, tables or pictures —
and Mukti has to decide what is legacy Bangla from the words alone. That gives
a lower accuracy than for a `.docx`. Your original file is never changed.
Because the converted copy is a different kind of file, *replace the original*
is not offered for these.

**Anything else is refused, with a plain explanation.** Text files (`.txt`,
`.csv`, `.md`), PDF and JSON were all converted in earlier versions and no
longer are. PDF only ever produced plain text with the layout lost; JSON could
come out broken, because a Bijoy curly quote becomes a plain `"`, which ends a
JSON value early. If the Bangla inside one of these needs converting, copy it
into a Word document and convert that instead.

## Things worth knowing

**Nothing leaves your computer.** Mukti works entirely offline. There is no
account, no upload and no internet connection needed.

**It errs on the side of leaving text alone.** If Mukti cannot tell whether
something is legacy Bangla, it does nothing. Missing a word is annoying but
obvious; changing a word that should not have changed is much harder to spot.

**The first time you run it** your Mac may say it cannot check who made it.
That is because the file is not signed with a paid certificate, not because
anything is wrong with it. Open **System Settings → Privacy & Security**,
find the message about `mukti`, and click *Open Anyway*.

## It will not write over a file it named itself

Converting `report.docx` writes `report.unicode.docx`. If that name is already
taken — usually by an earlier conversion — Mukti **stops and says so** rather
than replacing it. You chose the original's name; Mukti chose the new one, so
replacing it is not something it will do quietly.

Three ways forward:

1. Move or delete the file that is in the way.
2. `--out mine.docx` to pick your own name. Naming a file counts as asking,
   so this one is replaced without complaint.
3. `--force` to let Mukti replace the name it chose.

`--in-place` is separate: it overwrites **the original**, and has to be typed.

## If something goes wrong

**"… is empty"** — the file has nothing in it.

**"Could not read … as an older Word, Excel or PowerPoint file"** — the file may
be damaged, or it may be a newer file that has simply been given an old name.
Try renaming it with an `x` on the end: `.docx`, `.xlsx` or `.pptx`.

**Bangla shows as boxes or question marks** — the conversion is fine; whatever
you are viewing it in has no Bangla font. Install *Noto Sans Bengali*, which is
free. Some command windows cannot draw Bangla at all, so check a converted file
in Word or a browser rather than on screen.

**A word did not convert** — Mukti was not confident enough. This happens
most with single words, names, and text that is mostly numbers. Converting the
whole document rather than a fragment usually helps, because Mukti reads the
surrounding words to make up its mind.

**A word converted that should not have** — measured at about 146 in every
100,000 English words, most of them traced to documents using an old-Bangla
font this tool does not yet recognise (see the README for the detail). If you
find one, it is worth reporting.

## How accurate is it?

Measured, not estimated:

- **99.989%** of words convert correctly (from 473,244 words tested).
- **99.927%** of legacy Bangla words are found and converted (from 286,412).
- **0.146%** of English words are wrongly converted — about 146 in 100,000.
- **0.000%** of Bangla that was already correct is converted.

**One small exception, added in 0.7.0.** Bangla that is already Unicode is passed
through untouched, with a single change: where a vowel sign is stored in two pieces,
the two are joined into the one character Unicode says they are. `ে` plus `া` becomes
`ো`. It looks identical either way — but a computer does not think they are the same,
so a search for `ো` will not find the two-piece spelling. Joining them makes those
words findable, and it cannot change what the text says, because the two spellings are
defined as the same character.

The full method, including what these figures *cannot* tell you, is in the
[README](./README.md).
