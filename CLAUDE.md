# Working on Mukti

Mukti converts legacy Bijoy/SutonnyMJ Bangla into Unicode, **word by word**, so
English, numbers and Bengali that is already Unicode come through unchanged.
Rust, no runtime dependencies beyond the converter itself, macOS arm64 only
since 0.9.0.

This file is read at the start of every session in this repository. It is
deliberately short. The reasoning archive is `Dev-Memory/`, which is
**git-ignored** — see "If you cloned this repo" below.

## The three rules that must not drift

1. **Measure before improving.** No accuracy or speed claim ships without its
   method and sample size beside it. `./check-figures.sh` fails the build on a
   drifted README figure, and it is a release blocker.
2. **When the evidence runs out, leave the text alone.** Missing a legacy word
   is visible and fixable. Converting a word that was not legacy destroys
   readable text and may never be noticed. Every threshold here is asymmetric
   for that reason. Do not "improve" recall without reading the false-positive
   number in the same breath.
3. **Nothing derived from the private archive is ever committed.** `local/`,
   `Dev-Memory/` and `.sandbox/` are git-ignored and this repository is public.
   Only aggregate figures may leave `local/`. Scan the exact file list `git`
   would add before every commit.

## Commit identity

Every commit is the owner's work, authored and committed as
`Aninda Sundar Howlader <225374119+GRU-953@users.noreply.github.com>`. **Never
add a `Co-Authored-By` line naming an assistant**, and never a "generated with"
line. All 79 commits are clean today and the `attribution` job in `ci.yml` keeps
them that way.

Enable the local guards once per clone — they cannot be enabled for you, because
`.git/config` is outside what a sandboxed session may write:

```bash
git config core.hooksPath .githooks
```

`pre-commit` then refuses a commit under the wrong identity, and `commit-msg`
refuses an AI attribution line. Both are stated as an **allow-list on purpose**:
listing the addresses that must not appear would write them into a tracked file
in a public repository. The sibling MTA project had no such guard, and clearing
four trailers from it cost a full history rewrite, a force-push, and one commit
that GitHub pins at `refs/pull/N/head` and will never release.

## Build and test

```bash
source .sandbox/activate     # project-local Rust toolchain, pinned 1.97.1
cargo test --workspace       # 246 tests, no network needed
```

**Do not set `RUSTFLAGS`.** Warnings-as-errors for *builds* is configured in
`.cargo/config.toml` and nowhere else. (`ci.yml` also passes `-D warnings` to
clippy on the command line, which is a separate, intentional thing.) A
`RUSTFLAGS` environment variable silently *overrides* the config file rather
than merging with it, which once left the release build as the only build not
enforcing warnings-as-errors — and it took two passes to find the third place
it was still being exported. `ci.yml` fails if `RUSTFLAGS` is set at all.
When in doubt, run cargo as `env -u RUSTFLAGS cargo ...`.

**`cargo fmt` IS enforced here** (unlike the sibling MTA project, where it is
deliberately not).

## What you cannot verify, and must not pretend to

The accuracy figures come from a private document archive that cannot be
shipped: real programme documents belonging to a third party. So:

- `cargo test` proves the code works. It does **not** reproduce any published
  accuracy figure.
- `eval`, `corpus-label`, `corpus-verify` and `bench` all need corpus paths from
  `.sandbox/corpus-paths.local`, which is git-ignored and machine-specific.
- If you do not have the archive, **say so** rather than quoting the numbers as
  if you had checked them. `HANDOVER.md` §6 explains what kind of document set
  would substitute.

## Where things are

| | |
|---|---|
| `crates/mukti-core` | conversion, detection, embedded dictionaries |
| `crates/mukti-formats` | `.docx`/`.xlsx`/`.pptx` plus the pre-2007 reader |
| `crates/mukti-cli` | the `mukti` command, eight modules — `words.rs` holds **every** user-facing string, with the brand tests |
| `devtools/` | measurement only. Not shipped, not published |
| `Dev-Memory/` | why every decision was made. Git-ignored |

Read the code in the order `HANDOVER.md` §4 gives. Comments naming a defect are
there because the defect was real — do not tidy them away.

## Gotchas that have cost real time

- **`CARGO_TARGET_DIR` may be pinned to this repo's `target/`.** It is set in
  `.claude/settings.local.json`, which is **git-ignored** — so the owner's
  machine has it and a fresh clone does not. Where it is set it applies to the
  whole session: `cd` to another project, build, and the output lands *here*.
  It cost 6.7 GB of one project's artefacts landing in the other's folder, and
  a `cargo clean` then deleting the wrong target. Check with
  `echo $CARGO_TARGET_DIR` before building anything outside this repo.
- **Pushing does not trigger the GitHub workflows.** Every run this repository
  has ever had was started by hand. Use the Actions tab → Run workflow. The
  cause is not in the workflow files; they have been checked repeatedly.
- **`gh` may not authenticate** from a sandboxed session (invalid keyring token,
  blocked TLS). `git push` over HTTPS still works. Releases must be dispatched
  and published from the web UI.
- **Plan mode reuses one file path** for the active plan. A record citing
  `~/.claude/plans/...` may point at a completely different, later plan. Check
  the plan's own date line before trusting it.

## If you cloned this repo and have no `Dev-Memory/`

You are missing the reasoning behind roughly every non-obvious choice in the
codebase. The committed documents carry the conclusions; `Dev-Memory/` carries
the arguments, the measurements that failed, and the things deliberately not
built. Ask the owner for it. Without it, treat any change to `classify.rs`
thresholds as unsafe: several were tried, measured, and reverted, and the
record of that lives only there.

`HANDOVER.md` is the long-form version of this file and assumes no prior
knowledge.
