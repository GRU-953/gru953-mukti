# Using GRU953 Scribe

A short guide for anyone who has old Bangla documents that come out as
gibberish. No technical knowledge assumed.

## What problem this solves

Bangla typed years ago in Bijoy or SutonnyMJ is not really Bangla to a
computer. It is English letters that *look* Bangla because a special font
draws Bangla shapes on top of them. So the text cannot be searched, cannot be
spell-checked, and turns into nonsense on any machine without that font.

Scribe turns it into proper Unicode Bangla, which every modern computer and
phone understands.

**It changes only the legacy Bangla.** English, numbers, and Bangla that is
already correct are left exactly as they were.

## Using the app

1. Open **GRU953 Scribe**.
2. Paste your text into the box on the left, or drag a file onto the window.
3. The result appears on the right, straight away.
4. Click **Show what changed** to see exactly which words Scribe touched.
   Converted words are tinted and underlined; everything else it left alone.
5. Click **Copy** to take the result, or **Save as…** to write a file.

The line at the bottom always tells you how many words changed. If it says
"3 of 16 words converted", the other 13 came through untouched.

### Converting a whole document

Drag a Word, Excel or PowerPoint file onto the window, or use **Open a file**.
Scribe converts the Bangla inside it and leaves the formatting, tables and
pictures exactly as they were.

## Using it from the command line

If you have many files, this is much faster.

**See what would change, without changing anything:**

```bash
scribe check report.docx
```

**Convert a file.** The original is never touched — you get a new file beside
it, called `report.unicode.docx`:

```bash
scribe convert report.docx
```

**Convert many files at once:**

```bash
scribe convert *.docx
```

**Replace the original instead** (only if you are sure, and have a backup):

```bash
scribe convert report.docx --in-place
```

## What Scribe can open

| Kind of file | What you get |
|---|---|
| Text (`.txt`, `.csv`, `.md`) | Converted, saved as a new text file |
| Word (`.docx`) | Converted inside the document; formatting kept |
| Excel (`.xlsx`) | Converted inside the workbook; formatting kept |
| PowerPoint (`.pptx`) | Converted inside the slides; formatting kept |
| PDF | Text pulled out and converted, saved as plain text — **layout is lost** |

**Older `.doc`, `.xls` and `.ppt` files** cannot be read directly. Open one in
Word, Excel or PowerPoint and use *Save As* to make a `.docx`, `.xlsx` or
`.pptx` first.

## Things worth knowing

**Nothing leaves your computer.** Scribe works entirely offline. There is no
account, no upload and no internet connection needed.

**It errs on the side of leaving text alone.** If Scribe cannot tell whether
something is legacy Bangla, it does nothing. Missing a word is annoying but
obvious; changing a word that should not have changed is much harder to spot.

**PDFs are a best effort.** A PDF has no words or spaces inside it, only
letter shapes at positions, so spacing has to be guessed and tables come out
as ordinary running text. Some PDFs convert well, some poorly. Where Scribe
cannot read a piece of text safely, it leaves it out and tells you how much,
rather than inventing Bangla that looks convincing and is wrong.

**The first time you open the app** your computer may warn you it is from an
unidentified developer. That is because the app is not signed with a paid
certificate. On a Mac: right-click the app and choose *Open*, then *Open*
again. On Windows: click *More info*, then *Run anyway*.

## If something goes wrong

**"… is empty"** — the file has nothing in it.

**"Could not read … as a Word, Excel or PowerPoint file"** — it is probably an
older `.doc`, `.xls` or `.ppt`. Save it in the newer format first.

**Bangla shows as boxes or question marks** — this should not happen in the
app, which carries its own Bangla font. If you see it in a converted file
opened elsewhere, that programme is missing a Bangla font; install *Noto Sans
Bengali*, which is free.

**A word did not convert** — Scribe was not confident enough. This happens
most with single words, names, and text that is mostly numbers. Converting the
whole document rather than a fragment usually helps, because Scribe reads the
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
