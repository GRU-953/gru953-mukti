// GRU953 Mukti — the window's behaviour.
//
// Plain JavaScript, no framework and no build step. Everything the converter
// decides happens in Rust; this file only draws the answer and gets out of
// the way.

const invoke = window.__TAURI__.core.invoke;
const dialog = window.__TAURI__.dialog;

// Every string the user reads, in one place.
//
// English only for now, by decision — the Bangla release comes later. The
// strings live here from the start so that release is a translation job
// rather than a rebuild, which is much the cheaper way round.
const TEXT = {
  en: {
    lede: "Turn legacy Bangla into Unicode. Only the legacy words change.",
    inputLabel: "Paste legacy Bangla here",
    outputLabel: "Result",
    open: "Open a file",
    save: "Save as…",
    copy: "Copy",
    highlight: "Show what changed",
    theme: "Dark",
    themeLight: "Light",
    placeholder: "Paste text, or drop a file anywhere on this window.",
    drophint: "You can also drop a .txt, .csv or .md file onto the window.",
    empty: "The converted text will appear here.",
    copied: "Copied.",
    saved: (p) => `Saved to ${p}.`,
    nothing: "Nothing to convert yet.",
    summary: (c, t, e) =>
      `${c} of ${t} words converted; ${t - c} left exactly as they were.` +
      (e === "Windows-1252"
        ? " Read as Windows-1252, which is normal for a legacy Bangla file."
        : ""),
  },
};

let lang = "en";
const t = (key) => TEXT[lang][key];

const $ = (id) => document.getElementById(id);
const input = $("input");
const output = $("output");
const status = $("status");

let latest = { text: "", pieces: [], converted: 0, untouched: 0 };

function say(message, isError = false) {
  status.textContent = message;
  status.classList.toggle("error", isError);
}

function draw(result) {
  latest = result;
  output.replaceChildren();
  for (const piece of result.pieces) {
    if (piece.changed) {
      const mark = document.createElement("span");
      mark.className = "changed";
      // Named for screen readers, so the highlight is not colour-only.
      mark.setAttribute("aria-label", `converted: ${piece.text}`);
      mark.textContent = piece.text;
      output.append(mark);
    } else {
      output.append(document.createTextNode(piece.text));
    }
  }
  const total = result.converted + result.untouched;
  say(total === 0 ? t("nothing") : t("summary")(result.converted, total, result.encoding));
}

async function convertNow() {
  const text = input.value;
  if (!text) {
    output.replaceChildren();
    say("");
    latest = { text: "", pieces: [], converted: 0, untouched: 0 };
    return;
  }
  try {
    draw(await invoke("convert_text", { text }));
  } catch (e) {
    say(String(e), true);
  }
}

// Convert as you type, but only once typing pauses. Half a million dictionary
// lookups are fast; redrawing the window on every keystroke is not.
let timer = null;
input.addEventListener("input", () => {
  clearTimeout(timer);
  timer = setTimeout(convertNow, 120);
});

// --- Files ----------------------------------------------------------------

async function openFile(path) {
  try {
    const result = await invoke("convert_file", { path });
    // Show the original on the left and the result on the right, so the two
    // panes describe the same file.
    input.value = result.source;
    draw(result);
  } catch (e) {
    say(String(e), true);
  }
}

$("open").addEventListener("click", async () => {
  const path = await dialog.open({
    multiple: false,
    filters: [{ name: "Text", extensions: ["txt", "csv", "md", "json", "tsv"] }],
  });
  if (path) openFile(path);
});

$("save").addEventListener("click", async () => {
  if (!latest.text) {
    say(t("nothing"));
    return;
  }
  const path = await dialog.save({
    defaultPath: "converted.txt",
    filters: [{ name: "Text", extensions: ["txt"] }],
  });
  if (!path) return;
  try {
    await invoke("save_text", { path, text: latest.text });
    say(t("saved")(path));
  } catch (e) {
    say(String(e), true);
  }
});

$("copy").addEventListener("click", async () => {
  if (!latest.text) {
    say(t("nothing"));
    return;
  }
  await navigator.clipboard.writeText(latest.text);
  say(t("copied"));
});

// Drag and drop, using Tauri's own events so we get real file paths.
const webview = window.__TAURI__.webview.getCurrentWebview();
webview.onDragDropEvent((event) => {
  if (event.payload.type === "over") {
    document.body.classList.add("dropping");
  } else if (event.payload.type === "drop") {
    document.body.classList.remove("dropping");
    const [first] = event.payload.paths;
    if (first) openFile(first);
  } else {
    document.body.classList.remove("dropping");
  }
});

// --- Buttons that only change the view ------------------------------------

$("highlight").addEventListener("click", (e) => {
  const on = e.target.getAttribute("aria-pressed") === "true";
  e.target.setAttribute("aria-pressed", String(!on));
  output.classList.toggle("plain", on);
});

$("theme").addEventListener("click", (e) => {
  const dark = document.documentElement.getAttribute("data-theme") === "dark";
  document.documentElement.setAttribute("data-theme", dark ? "light" : "dark");
  e.target.setAttribute("aria-pressed", String(!dark));
  e.target.textContent = dark ? t("theme") : t("themeLight");
});

output.dataset.empty = t("empty");
