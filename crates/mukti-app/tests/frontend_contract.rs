//! The contract between the window and Rust, checked mechanically.
//!
//! # Why this file exists
//!
//! Version 0.4.0 shipped a desktop app in which nothing worked. The window opened,
//! rendered correctly, and every control was dead. The cause was one missing
//! setting: `app.withGlobalTauri` was never enabled, so `window.__TAURI__` did not
//! exist, so `app.js` threw on its first executable line and not a single event
//! listener was ever attached. A second, independent fault — no capability file at
//! all — would have blocked *Open*, *Save as…* and drag-and-drop even after that.
//!
//! Both were invisible to every check that existed. The tests passed. Clippy was
//! clean. The release built and published five installers. Nothing had ever opened
//! the window.
//!
//! # Why this, rather than a browser test
//!
//! Driving a real window needs WebDriver, which on Linux and Windows needs platform
//! drivers and on macOS does not exist at all, plus a display. That is worth having
//! and is not what stops this recurring, because the fault was never *visual*: it
//! was a **silent mismatch between two files**. A mismatch is exactly what a
//! program can check, on every platform, in milliseconds, with no display.
//!
//! So this asserts the things that were wrong, and the neighbouring things that
//! could go wrong the same way:
//!
//! 1. The bridge is switched on at all.
//! 2. A capability file exists and grants what is used.
//! 3. Every command the window calls is registered in Rust.
//! 4. Every event the window listens for is emitted by Rust.
//! 5. Every string the markup asks for exists in the table.
//! 6. Every element the script looks up exists in the markup.
//! 7. Every file the interface references is on disk.
//!
//! Any one of those failing used to be discoverable only by a person opening the
//! app. Now it fails a build.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn read(rel: &str) -> String {
    let path = crate_dir().join(rel);
    std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("could not read {}: {e}", path.display()))
}

/// Everything between `pattern(` and the closing quote, for each occurrence of a
/// call like `invoke("convert_text"` — a deliberately small scanner, because a
/// dependency for this would be a dependency in the shipped app's manifest.
fn quoted_args(source: &str, call: &str) -> HashSet<String> {
    let mut found = HashSet::new();
    let needle = format!("{call}(\"");
    let mut rest = source;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        if let Some(end) = after.find('"') {
            found.insert(after[..end].to_owned());
        }
        rest = after;
    }
    found
}

/// Values of an attribute, for each `attr="value"` in the markup.
fn attribute_values(html: &str, attr: &str) -> HashSet<String> {
    let mut found = HashSet::new();
    let needle = format!("{attr}=\"");
    let mut rest = html;
    while let Some(at) = rest.find(&needle) {
        let after = &rest[at + needle.len()..];
        if let Some(end) = after.find('"') {
            found.insert(after[..end].to_owned());
        }
        rest = after;
    }
    found
}

// ---------------------------------------------------------------------------
// 1. The bridge is switched on
// ---------------------------------------------------------------------------

/// The single setting whose absence made the whole window inert.
///
/// `app.js` reaches for `window.__TAURI__`, which only exists when this is true.
/// It defaults to false, so leaving it out is silent — the app builds, launches,
/// paints, and does nothing at all.
#[test]
fn the_bridge_the_window_depends_on_is_switched_on() {
    let config = read("tauri.conf.json");
    let uses_global = read("ui/app.js").contains("window.__TAURI__");

    assert!(
        uses_global,
        "app.js no longer uses window.__TAURI__. If it now imports the API another \
         way, this test should check THAT instead — do not simply delete it, or the \
         0.4.0 failure becomes possible again."
    );
    assert!(
        config.contains("\"withGlobalTauri\": true"),
        "app.js reads window.__TAURI__, but tauri.conf.json does not set \
         app.withGlobalTauri to true. That object will not exist, app.js will throw \
         on its first line, and NOTHING in the window will respond — which is \
         exactly what shipped in 0.4.0."
    );
}

/// The security policy has to allow the transport the bridge actually uses.
#[test]
fn the_security_policy_allows_the_bridge_to_talk() {
    let config = read("tauri.conf.json");
    assert!(
        config.contains("connect-src"),
        "the content security policy has no connect-src, so Tauri's IPC transport \
         is blocked and every call falls back to a slower path with a policy \
         violation logged on the way"
    );
    assert!(
        config.contains("font-src"),
        "the content security policy has no font-src, so the brand's webfonts may \
         not load and the window silently falls back to a system typeface"
    );
}

// ---------------------------------------------------------------------------
// 2. Permissions exist and cover what is used
// ---------------------------------------------------------------------------

/// Without a file in `capabilities/`, Tauri grants nothing.
///
/// The generated permission set comes out as literally `{}` and every
/// plugin-prefixed call is refused at runtime. That is the second, independent
/// reason Open, Save and drag-and-drop did not work in 0.4.0 — and it would still
/// have failed after `withGlobalTauri` was fixed.
#[test]
fn a_capability_file_exists_and_grants_the_core_permissions() {
    let dir = crate_dir().join("capabilities");
    assert!(
        dir.is_dir(),
        "there is no capabilities/ directory. Tauri looks for ./capabilities/**/* \
         and grants NOTHING when it finds none — the generated permission set is \
         then `{{}}` and every plugin call is refused at runtime."
    );

    let mut granted = String::new();
    let mut windows = String::new();
    for entry in std::fs::read_dir(&dir).expect("capabilities/ should be readable") {
        let path = entry.expect("a readable directory entry").path();
        if path.extension().and_then(|e| e.to_str()) == Some("json") {
            let body = std::fs::read_to_string(&path).expect("a readable capability");
            granted.push_str(&body);
            windows.push_str(&body);
        }
    }
    assert!(
        !granted.is_empty(),
        "capabilities/ contains no .json file, so nothing is granted"
    );
    assert!(
        granted.contains("core:default"),
        "no capability grants core:default. The window listens for events, and \
         event listening lives behind core:event — without it the drag-and-drop \
         and file-converted listeners are refused silently."
    );
    assert!(
        windows.contains("\"main\""),
        "no capability names the window \"main\". A capability applies only to the \
         windows it lists, so one that names a different label grants nothing to \
         the window that actually exists."
    );
}

// ---------------------------------------------------------------------------
// 3 and 4. Command and event names agree across the boundary
// ---------------------------------------------------------------------------

/// Every command the window calls must be a command Rust registered.
///
/// A typo here is invisible until somebody clicks the button: the front end asks
/// for a name that does not exist and the call is rejected at runtime.
#[test]
fn every_command_the_window_calls_is_registered_in_rust() {
    let js = read("ui/app.js");
    let rs = read("src/main.rs");

    let called = quoted_args(&js, "invoke");
    assert!(
        !called.is_empty(),
        "no invoke() calls found in app.js — if the window no longer calls Rust at \
         all, this test needs rewriting rather than removing"
    );

    // The handler list is what Tauri actually exposes; a #[tauri::command] that is
    // never added to it is not callable.
    let handler = rs
        .split_once("generate_handler![")
        .map(|(_, after)| {
            after
                .split_once(']')
                .map(|(inside, _)| inside)
                .unwrap_or("")
        })
        .unwrap_or("");
    assert!(
        !handler.is_empty(),
        "could not find generate_handler![...] in main.rs"
    );

    for name in &called {
        assert!(
            handler.contains(name.as_str()),
            "app.js calls invoke(\"{name}\"), but that command is not in \
             generate_handler![...]. The call would be rejected at runtime, and \
             only when somebody used that control."
        );
    }
}

/// Every event the window listens for must be an event Rust emits.
///
/// The reverse of the command check, and the same class of silence: a listener for
/// an event nobody sends simply never fires, which looks exactly like a feature
/// that was never wired up.
#[test]
fn every_event_the_window_listens_for_is_emitted_by_rust() {
    let js = read("ui/app.js");
    let rs = read("src/main.rs");

    let listened = quoted_args(&js, "listen");
    assert!(
        !listened.is_empty(),
        "no listen() calls found in app.js — dropped files are delivered by event, \
         so if that has changed this test should follow it"
    );

    let emitted = quoted_args(&rs, "emit");
    for name in &listened {
        assert!(
            emitted.contains(name),
            "app.js listens for \"{name}\", but main.rs never emits it. The \
             listener will never fire, which is indistinguishable from the feature \
             not being built."
        );
    }
}

// ---------------------------------------------------------------------------
// 5, 6 and 7. The interface is wired to itself
// ---------------------------------------------------------------------------

/// Every string the markup asks for must exist in the table.
///
/// A missing key renders the word "undefined" in the interface. These attributes
/// existed from 0.3.0 and *nothing read them*, so every string lived in two places
/// with nothing keeping them in step; now one table fills them all, which is only
/// safe if the names match.
#[test]
fn every_string_the_markup_asks_for_exists_in_the_table() {
    let html = read("ui/index.html");
    let js = read("ui/app.js");

    let mut wanted = attribute_values(&html, "data-i18n");
    wanted.extend(attribute_values(&html, "data-i18n-placeholder"));
    wanted.extend(attribute_values(&html, "data-i18n-label"));
    assert!(
        !wanted.is_empty(),
        "no data-i18n attributes found in index.html"
    );

    for key in &wanted {
        // The table is written as `key: "..."` or `key: (args) => ...`.
        let declared = js.contains(&format!("\n    {key}:"));
        assert!(
            declared,
            "the markup asks for the string \"{key}\", which is not in the TEXT \
             table in app.js. That renders the literal word \"undefined\" where a \
             label should be."
        );
    }
}

/// Every element the script looks up must exist in the markup.
///
/// `$("save")` returning null throws the moment a listener is attached to it —
/// and because that happens during start-up, it takes every listener after it
/// down as well. Which is precisely how one fault made the whole window inert.
#[test]
fn every_element_the_script_looks_up_exists_in_the_markup() {
    let html = read("ui/index.html");
    let js = read("ui/app.js");

    let wanted = quoted_args(&js, "$");
    assert!(!wanted.is_empty(), "no $(\"id\") lookups found in app.js");

    let present = attribute_values(&html, "id");
    for id in &wanted {
        assert!(
            present.contains(id),
            "app.js looks up the element \"{id}\", which does not exist in \
             index.html. Attaching a listener to it throws during start-up, and \
             every listener registered after that point is lost too."
        );
    }
}

/// Every file the interface references must be on disk.
///
/// A missing webfont degrades quietly to a system typeface; a missing stylesheet
/// or mark is far more obvious but just as silent at build time, because the
/// bundler copies the whole `ui/` folder without checking what is in it.
#[test]
fn every_file_the_interface_references_is_present() {
    let html = read("ui/index.html");
    let css = read("ui/app.css");
    let ui = crate_dir().join("ui");

    let mut referenced: Vec<String> = attribute_values(&html, "href")
        .into_iter()
        .filter(|h| !h.starts_with('#') && !h.contains("://"))
        .collect();

    // url("...") in the stylesheet: the fonts and the mark.
    let mut rest = css.as_str();
    while let Some(at) = rest.find("url(\"") {
        let after = &rest[at + 5..];
        if let Some(end) = after.find('"') {
            referenced.push(after[..end].to_owned());
        }
        rest = after;
    }

    assert!(
        referenced.len() >= 4,
        "expected the interface to reference a stylesheet, fonts and the mark; \
         found only {} references, which suggests this test is looking in the \
         wrong place",
        referenced.len()
    );

    for rel in &referenced {
        let path: &Path = Path::new(rel);
        assert!(
            ui.join(path).exists(),
            "the interface references {rel}, which is not in ui/. The bundler \
             copies the folder without checking, so this ships as a missing font \
             or a missing mark rather than as a build failure."
        );
    }
}

/// The marks must be the kit's own files, unmodified.
///
/// The design rules are explicit: do not re-draw, re-colour or add effects to the
/// mark. Shipping the file as-is and colouring it with CSS is the only approved
/// way, and a file that is never edited cannot drift from the original.
#[test]
fn the_brand_mark_is_present_and_carries_its_own_description() {
    let bird = read("ui/brand/GRU953-bird.svg");
    assert!(
        bird.contains("<title") && bird.contains("<desc"),
        "the mark has lost its <title>/<desc>. Those are what make an inline SVG \
         announce itself correctly, and their absence suggests the file has been \
         edited rather than shipped as the kit provides it."
    );
    assert!(
        bird.contains("currentColor"),
        "the mark no longer uses currentColor, so it cannot be given an approved \
         colour without editing the file — which the design rules forbid."
    );
}
