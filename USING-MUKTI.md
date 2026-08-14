# Using GRU953 Mukti

A short guide for anyone who has old Bangla documents that come out as
gibberish. No technical knowledge assumed.

> ## ⚠️ If you have version 0.4.0, please install 0.5.0
>
> In **version 0.4.0** the window opened and looked right, but **nothing in it
> responded**: not typing, not the buttons, not dragging a file in. That was our
> fault, not yours. **Version 0.5.0 fixes it.**
>
> The same version also fixes a fault where a damaged or deliberately malicious
> PDF or Office file could make the program stop unexpectedly or use up your
> computer's memory. Your own ordinary documents would not have done this.
>
> **Anything you converted with the 0.4.0 command-line tool is fine.** That part
> always worked, and the conversion itself has not changed.

## What problem this solves

Bangla typed years ago in Bijoy or SutonnyMJ is not really Bangla to a
computer. It is English letters that *look* Bangla because a special font
draws Bangla shapes on top of them. So the text cannot be searched, cannot be
spell-checked, and turns into nonsense on any machine without that font.

Mukti turns it into proper Unicode Bangla, which every modern computer and
phone understands.

**It changes only the legacy Bangla.** English, numbers, and Bangla that is
already correct are left exactly as they were.

## Using the app

1. Open **GRU953 Mukti**.
2. Paste your text into the box on the left, or drag a file onto the window.
3. The result appears on the right, straight away.
4. Click **Show what changed** to see exactly which words Mukti touched.
   Converted words are tinted and underlined; everything else it left alone.
5. Click **Copy** to take the result, or **Save as…** to write a file.

The line at the bottom always tells you how many words changed. If it says
"3 of 16 words converted", the other 13 came through untouched.

### Converting a whole document

Drag a Word, Excel or PowerPoint file onto the window, or use **Open a file**.
Mukti converts the Bangla inside it and leaves the formatting, tables and
pictures exactly as they were.

## Using it from the command line

If you have many files, this is much faster.

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

**Replace the original instead** (only if you are sure, and have a backup):

```bash
mukti convert report.docx --in-place
```

## What Mukti can open

| Kind of file | What you get |
|---|---|
| Text (`.txt`, `.csv`, `.md`) | Converted, saved as a new text file |
| Word (`.docx`) | Converted inside the document; formatting kept |
| Excel (`.xlsx`) | Converted inside the workbook; formatting kept |
| PowerPoint (`.pptx`) | Converted inside the slides; formatting kept |
| PDF | Text pulled out and converted, saved as plain text — **layout is lost** |

| Older Word (`.doc`) | Converted into a new `.docx` beside it — **text only** |
| Older Excel (`.xls`) | Converted into a new `.xlsx` — **text only** |
| Older PowerPoint (`.ppt`) | Converted into a new `.pptx` — **text only** |

**About the older formats.** They store no formatting we can carry and no font
information, so only the words come across — no colours, tables or pictures —
and Mukti has to decide what is legacy Bangla from the words alone. That is the
same accuracy as a plain text file, and lower than for a `.docx`. Your original
file is never changed. Because the converted copy is a different kind of file,
*replace the original* is not offered for these.

## Things worth knowing

**Nothing leaves your computer.** Mukti works entirely offline. There is no
account, no upload and no internet connection needed.

**It errs on the side of leaving text alone.** If Mukti cannot tell whether
something is legacy Bangla, it does nothing. Missing a word is annoying but
obvious; changing a word that should not have changed is much harder to spot.

**PDFs are a best effort.** A PDF has no words or spaces inside it, only
letter shapes at positions, so spacing has to be guessed and tables come out
as ordinary running text. Some PDFs convert well, some poorly. Where Mukti
cannot read a piece of text safely, it leaves it out and tells you how much,
rather than inventing Bangla that looks convincing and is wrong.

**The first time you open the app** your computer may warn you it is from an
unidentified developer. That is because the app is not signed with a paid
certificate. On a Mac: right-click the app and choose *Open*, then *Open*
again. On Windows: click *More info*, then *Run anyway*.

## It will not write over a file it named itself

Converting `report.txt` writes `report.unicode.txt`. If that name is already
taken — usually by an earlier conversion — Mukti **stops and says so** rather
than replacing it. You chose the original's name; Mukti chose the new one, so
replacing it is not something it will do quietly.

Three ways forward:

1. Move or delete the file that is in the way.
2. `--out mine.txt` to pick your own name. Naming a file counts as asking, so
   this one is replaced without complaint.
3. `--force` to let Mukti replace the name it chose.

`--in-place` is separate: it overwrites **the original**, and has to be typed.

## If something goes wrong

**"… is empty"** — the file has nothing in it.

**"Could not read … as an older Word, Excel or PowerPoint file"** — the file may
be damaged, or it may be a newer file that has simply been given an old name.
Try renaming it with an `x` on the end: `.docx`, `.xlsx` or `.pptx`.

**Bangla shows as boxes or question marks** — this should not happen in the
app, which carries its own Bangla font. If you see it in a converted file
opened elsewhere, that programme is missing a Bangla font; install *Noto Sans
Bengali*, which is free.

**A word did not convert** — Mukti was not confident enough. This happens
most with single words, names, and text that is mostly numbers. Converting the
whole document rather than a fragment usually helps, because Mukti reads the
surrounding words to make up its mind.

**A word converted that should not have** — this should be rare: measured at
6 in every 100,000 English words. If you find one, it is worth reporting.

## How accurate is it?

Measured, not estimated:

- **99.989%** of words convert correctly (from 473,244 words tested).
- **99.951%** of legacy Bangla words are found and converted (from 154,928).
- **0.006%** of English words are wrongly converted — about 6 in 100,000.
- **0.000%** of Bangla that was already correct is touched.

The full method, including what these figures *cannot* tell you, is in the
[README](./README.md).
