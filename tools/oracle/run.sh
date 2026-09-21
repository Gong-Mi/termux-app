#!/usr/bin/env bash
# Generate the golden snapshot file for the differential oracle gate.
#
#   usage: tools/oracle/run.sh [corpus.jsonl] [golden.jsonl]
#
# The golden file records what the pinned reference implementation does for every corpus
# sequence and every step, so the Rust engine can be compared against something it did not
# author.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
OUT="${ORACLE_OUT:-$REPO/tools/oracle/build/classes}"
CORPUS="${1:-$REPO/corpus/sequences/seed.jsonl}"
GOLDEN="${2:-$REPO/corpus/golden/upstream-e634d8f.jsonl}"

bash "$HERE/build.sh"
mkdir -p "$(dirname "$GOLDEN")"
java -cp "$OUT" com.termux.terminal.OracleDump "$CORPUS" "$GOLDEN"