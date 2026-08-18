# Using GRU953 Mukti

A short guide for anyone who has old Bangla documents that come out as
gibberish. No technical knowledge assumed.

> ## ⚠️ There is no longer a window — Mukti is a typed command
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

1. Go to the project's **Releases** page and download the file for your computer:
   `mukti-macos` for a Mac, `mukti-windows.exe` for Windows, `mukti-linux` for
   Linux.
2. Put it somewhere you can find again — your home folder is fine.
3. Open your computer's command window: **Terminal** on a Mac (press ⌘ + Space,
   type `Terminal`, press Return), or **PowerShell** on Windows (press the Start
   key, type `PowerShell`, press Return).
4. On a Mac or Linux, allow the file to run by typing this once, then Return:

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

**The first time you run it** your computer may say it cannot check who made it.
That is because the file is not signed with a paid certificate, not because
anything is wrong with it. On a Mac, open **System Settings → Privacy &
Security**, find the message about `mukti`, and click *Open Anyway*. On Windows,
click *More info*, then *Run anyway*.

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

**Bangla shows as boxes or question marks** — the conversion is fine; whatever
you are viewing it in has no Bangla font. Install *Noto Sans Bengali*, which is
free. Some command windows cannot draw Bangla at all, so check a converted file
in Word or a browser rather than on screen.

**A word did not convert** — Mukti was not confident enough. This happens
most with single words, names, and text that is mostly numbers. Converting the
whole document rather than a fragment usually helps, because Mukti reads the
surrounding words to make up its mind.

**A word converted that should not have** — this should be rare: measured at
14 in every 100,000 English words. If you find one, it is worth reporting.

## How accurate is it?

Measured, not estimated:

- **99.989%** of words convert correctly (from 473,244 words tested).
- **99.962%** of legacy Bangla words are found and converted (from 177,079).
- **0.014%** of English words are wrongly converted — about 14 in 100,000.
- **0.000%** of Bangla that was already correct is touched.

The full method, including what these figures *cannot* tell you, is in the
[README](./README.md).
