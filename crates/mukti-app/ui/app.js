// GRU953 Mukti — the window's behaviour.
//
// Plain JavaScript, no framework and no build step. Every decision about what is
// legacy Bangla happens in Rust; this file draws the answer and gets out of the
// way.
//
// Three things it does differently from the version before 13 August 2026, each
// fixing something measured rather than suspected:
//
//   1. Every string comes from ONE table, applied through the data-i18n
//      attributes in the markup. Those attributes already existed and nothing
//      read them, so each string lived in two places with nothing keeping them
//      in step.
//   2. The theme is resolved from what is actually on screen and remembered.
//      Before, the button reported "not pressed" while the window rendered dark,
//      and its first click changed nothing visible.
//   3. No file path is ever sent to Rust. Rust owns the dialogs, so there is no
//      path parameter to get wrong.

const invoke = window.__TAURI__.core.invoke;
const listen = window.__TAURI__.event.listen;

// Every string a person reads, in one place.
//
// English only, by decision. The structure is here so a Bangla release is a
// translation rather than a rebuild — which is much the cheaper way round. The
// tagline is NOT in this table: it is locked by the brand in both languages and
// lives in the markup, because translating it is not permitted.
const TEXT = {
  en: {
    skip: "Skip to the text box",
    railLabel: "Actions",
    panesLabel: "Convert text",
    lede: "Turn legacy Bangla into Unicode. Only the legacy words change.",
    inputLabel: "Legacy Bangla",
    outputLabel: "Unicode",
    open: "Open",
    save: "Save",
    copy: "Copy",
    highlightShort: "Changes",
    theme: "Dark",
    themeLight: "Light",
    placeholder: "Paste text here, or drop a file anywhere on this window.",
    drophint: "Or drop a file on the window: text, Word, Excel, PowerPoint or PDF.",
    empty: "The converted text will appear here.",

    // Words that prefix the status line, so colour is never the only signal.
    wordError: "Error",
    wordDone: "Done",

    copied: "Copied to the clipboard.",
    saved: (name) => `Saved as ${name}.`,
    nothing: "Nothing to convert yet.",
    nothingToSave: "There is nothing converted to save yet.",
    cancelled: "Cancelled — nothing was written.",
    readonly: "This is a document, so it is shown rather than edited. Save to keep the result.",
    pdfReadonly: "PDF text is read out, never written back. Save to keep it as a text file.",
    noText: (n) =>
      `No text could be read from that PDF. ${n} piece(s) were in fonts that store ` +
      `shapes rather than characters, so they were skipped rather than guessed at.`,
    summary: (changed, total, encoding) =>
      `${changed} of ${total} words converted; ${total - changed} left exactly as ` +
      `they were.` +
      (encoding === "Windows-1252"
        ? " Read as Windows-1252, which is normal for a legacy Bangla file."
        : ""),
    counted: (n) => `${n} changed`,
  },
};

let lang = "en";
const t = (key) => TEXT[lang][key];

const $ = (id) => document.getElementById(id);
const input = $("input");
const output = $("output");
const status = $("status");
const statusWord = $("statusword");
const statusText = $("statustext");
const count = $("count");
const fileMeta = $("filemeta");
const outHint = $("outhint");

let latest = { text: "", pieces: [], converted: 0, untouched: 0, kind: "text" };

// --- Strings, applied once from the table --------------------------------

function applyText() {
  for (const el of document.querySelectorAll("[data-i18n]")) {
    el.textContent = t(el.dataset.i18n);
  }
  for (const el of document.querySelectorAll("[data-i18n-placeholder]")) {
    el.placeholder = t(el.dataset.i18nPlaceholder);
  }
  for (const el of document.querySelectorAll("[data-i18n-label]")) {
    el.setAttribute("aria-label", t(el.dataset.i18nLabel));
  }
  output.dataset.empty = t("empty");
  document.documentElement.lang = lang;
}

// --- Status --------------------------------------------------------------

function say(message, kind = "plain") {
  statusText.textContent = message;
  status.classList.toggle("error", kind === "error");
  status.classList.toggle("good", kind === "good");
  if (kind === "error") {
    statusWord.textContent = t("wordError");
    statusWord.hidden = false;
  } else if (kind === "good") {
    statusWord.textContent = t("wordDone");
    statusWord.hidden = false;
  } else {
    statusWord.hidden = true;
  }
}

// --- Drawing the result --------------------------------------------------

function draw(result) {
  latest = result;
  output.replaceChildren();

  for (const piece of result.pieces) {
    if (piece.changed) {
      const mark = document.createElement("span");
      mark.className = "changed";
      // Marked as Bangla per word, not per pane. The result is a mixture of
      // converted Bangla and English deliberately left alone, so declaring the
      // whole pane Bangla told a screen reader to read the English with a
      // Bengali voice. The kit calls this the single highest-value
      // accessibility line of code there is for a bilingual product.
      mark.lang = "bn";
      mark.textContent = piece.text;
      output.append(mark);
    } else {
      output.append(document.createTextNode(piece.text));
    }
  }

  const total = result.converted + result.untouched;

  count.hidden = result.converted === 0;
  count.textContent = t("counted")(result.converted);

  fileMeta.hidden = !result.filename;
  fileMeta.textContent = result.filename || "";

  // A document or a PDF is shown, not edited: there is no way to type a .docx.
  const editable = result.kind === "text";
  input.readOnly = !editable;
  outHint.hidden = editable;
  if (!editable) {
    outHint.textContent = result.kind === "pdf" ? t("pdfReadonly") : t("readonly");
  }

  if (result.unreadable !== null && result.unreadable !== undefined) {
    say(t("noText")(result.unreadable), "error");
  } else if (total === 0) {
    say(t("nothing"));
  } else {
    say(t("summary")(result.converted, total, result.encoding));
  }
}

function clearAll() {
  output.replaceChildren();
  latest = { text: "", pieces: [], converted: 0, untouched: 0, kind: "text" };
  count.hidden = true;
  fileMeta.hidden = true;
  outHint.hidden = true;
  input.readOnly = false;
  say("");
}

// --- Typing --------------------------------------------------------------

async function convertNow() {
  const text = input.value;
  if (!text) {
    clearAll();
    return;
  }
  try {
    draw(await invoke("convert_text", { text }));
  } catch (e) {
    say(String(e), "error");
  }
}

// Convert as you type, but only once typing pauses. Half a million dictionary
// lookups are fast; redrawing the window on every keystroke is not.
let timer = null;
input.addEventListener("input", () => {
  if (input.readOnly) return;
  clearTimeout(timer);
  timer = setTimeout(convertNow, 120);
});

// --- Files ---------------------------------------------------------------
// Rust puts the dialogs up and reads what was chosen, so nothing here handles a
// path. `null` back means the dialog was closed without choosing.

$("open").addEventListener("click", async () => {
  try {
    const result = await invoke("open_and_convert");
    if (!result) return;
    input.value = result.source;
    draw(result);
  } catch (e) {
    say(String(e), "error");
  }
});

$("save").addEventListener("click", async () => {
  if (!latest.text) {
    say(t("nothingToSave"));
    return;
  }
  try {
    const name = await invoke("save_result");
    say(name ? t("saved")(name) : t("cancelled"), name ? "good" : "plain");
  } catch (e) {
    say(String(e), "error");
  }
});

$("copy").addEventListener("click", async () => {
  if (!latest.text) {
    say(t("nothing"));
    return;
  }
  try {
    await navigator.clipboard.writeText(latest.text);
    say(t("copied"), "good");
  } catch (e) {
    say(String(e), "error");
  }
});

// Dropped files are converted in Rust, which then sends the result here — again
// so no path crosses the boundary.
listen("file-converted", (event) => {
  input.value = event.payload.source;
  draw(event.payload);
});
listen("file-failed", (event) => say(String(event.payload), "error"));
listen("drop-state", (event) => {
  document.body.classList.toggle("dropping", event.payload === "over");
});

// --- View controls -------------------------------------------------------

$("highlight").addEventListener("click", (e) => {
  const button = e.currentTarget;
  const on = button.getAttribute("aria-pressed") === "true";
  button.setAttribute("aria-pressed", String(!on));
  output.classList.toggle("plain", on);
});

// The theme, resolved from what is actually rendered rather than assumed.
//
// The old version read data-theme, which is unset on first load, so on a
// dark-mode machine the button said "Dark" and reported aria-pressed="false"
// while the window was already dark — and the first click set data-theme="dark",
// changing nothing visible while announcing a state change. Now the starting
// state comes from the media query, and the choice is remembered.
const STORE = "gru953-mukti-theme";

function setTheme(theme) {
  document.documentElement.setAttribute("data-theme", theme);
  const dark = theme === "dark";
  const button = $("theme");
  button.setAttribute("aria-pressed", String(dark));
  // The label says what pressing it will DO, which is the opposite of the
  // current state.
  button.querySelector("[data-i18n]").textContent = dark ? t("themeLight") : t("theme");
  button.querySelector("[data-i18n]").dataset.i18n = dark ? "themeLight" : "theme";
  try {
    localStorage.setItem(STORE, theme);
  } catch {
    // Private mode, or storage full. Not worth telling anyone about: the theme
    // simply will not be remembered next time.
  }
}

function startingTheme() {
  let saved = null;
  try {
    saved = localStorage.getItem(STORE);
  } catch {
    saved = null;
  }
  if (saved === "dark" || saved === "light") return saved;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

$("theme").addEventListener("click", () => {
  const dark = document.documentElement.getAttribute("data-theme") === "dark";
  setTheme(dark ? "light" : "dark");
});

// --- Start ---------------------------------------------------------------

applyText();
setTheme(startingTheme());
clearAll();
input.focus();
