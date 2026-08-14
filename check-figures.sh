#!/usr/bin/env bash
#
# Re-measure everything, and fail if any target is missed or any published figure
# has drifted.
#
# WHY THIS IS A SCRIPT AND NOT A CI JOB
#
# The plan called for running `eval` nightly in continuous integration so the
# figures could not silently rot. That is not possible, and saying so is better
# than building something that looks like it.
#
# Every one of the six measurements needs material that is deliberately NOT in this
# repository: the word collection, the character grid, the misspelling pairs, the
# labelled answer key. All of it is real documents or unlicensed word lists, all of
# it git-ignored on purpose. A CI runner has no way to obtain any of it, and the
# only way to give it one would be to publish the very material that must not be
# published.
#
# So the check lives here, runs on the machine that HAS the corpus, and is a
# blocking step before any release rather than a nightly job that could never work.
#
#   ./check-figures.sh
#
# It exits non-zero if a target is missed OR if a figure in README.md no longer
# matches what was just measured — because a number in a document that nobody
# re-derives is exactly how 78.4% survived being wrong.

set -euo pipefail
cd "$(dirname "$0")"

if [ -z "${MUKTI_CORPUS:-}" ]; then
    echo "MUKTI_CORPUS is not set. Run: source .sandbox/activate"
    exit 1
fi
if [ ! -d "$MUKTI_CORPUS" ]; then
    echo "The corpus is not at \$MUKTI_CORPUS. It has moved four times; find it and"
    echo "update .sandbox/corpus-paths.local. Do NOT commit the new path."
    exit 1
fi

echo "Re-measuring. This needs local/labelled-corpus.tsv and local/extended-words.fst;"
echo "rebuild them with corpus-label and lexicon-build if they are missing."
echo

report=$(mktemp)
trap 'rm -f "$report"' EXIT

# eval exits non-zero by itself when a target is missed.
if ! cargo run --release -q -p eval -- --corpus "$MUKTI_CORPUS" --split test > "$report" 2>&1; then
    echo "--- A TARGET WAS MISSED ---"
    cat "$report"
    exit 1
fi

# Then check the figures the README actually publishes against what was measured.
# A percentage quoted in a document is a claim; this is what keeps it one.
fail=0
check() {
    local what="$1" pattern="$2" expected="$3"
    local measured
    measured=$(grep -m1 -oE "$pattern" "$report" | grep -oE '[0-9]+\.[0-9]+' | head -1 || true)
    if [ -z "$measured" ]; then
        echo "  ?? $what — could not be found in the report"
        fail=1
    elif [ "$measured" = "$expected" ]; then
        printf "  ok %-34s %s%%\n" "$what" "$measured"
    else
        printf "  ** %-34s measured %s%%, README says %s%%\n" "$what" "$measured" "$expected"
        fail=1
    fi
}

echo "Comparing against the figures published in README.md:"
check "conversion, word accuracy"     "Word accuracy +[0-9.]+%"           "99.989"
check "character grid"                "Combinations correct +[0-9.]+%"    "100.000"
check "detection recall"              "Recall on legacy words +99[0-9.]*%" "99.962"
check "misspellings preserved"        "Misspellings preserved +[0-9.]+%"  "99.979"
check "dictionary hit, real documents" "Output words in the dictionary +[0-9.]+%" "94.053"

echo
if [ "$fail" -ne 0 ]; then
    echo "A published figure no longer matches what was measured."
    echo "Either the code changed, or the answer key did. Update README.md and"
    echo "HANDOVER.md together with this script — never just one of them."
    exit 1
fi
echo "Every target met, and every published figure still matches. Full report above."
