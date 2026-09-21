#!/usr/bin/env bash
# Build the *reference* side of the differential oracle gate.
#
# The reference is upstream Termux's own terminal implementation at the pinned baseline
# commit, compiled against three tiny Android stubs so the harness runs without the Android
# SDK (ordinary CI and Termux both have javac, neither has the SDK).
#
# Refusing to build against an unpinned tree is deliberate: if the reference could drift,
# every historical comparison would silently change meaning.
set -euo pipefail

PIN=e634d8f981f48b6b89202cf0e04533f0889e03b3
HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO="$(cd "$HERE/../.." && pwd)"
UPSTREAM_SRC="${UPSTREAM_TERMUX_SRC:-$HOME/termux-src/termux-app}"
OUT="${ORACLE_OUT:-$REPO/tools/oracle/build/classes}"

if [ ! -d "$UPSTREAM_SRC/.git" ]; then
  echo "error: $UPSTREAM_SRC is not a git checkout of termux/termux-app" >&2
  echo "hint:  git clone https://github.com/termux/termux-app \"$UPSTREAM_SRC\"" >&2
  echo "       git -C \"$UPSTREAM_SRC\" checkout $PIN" >&2
  exit 2
fi

ACTUAL="$(git -C "$UPSTREAM_SRC" rev-parse HEAD)"
if [ "$ACTUAL" != "$PIN" ]; then
  echo "error: reference tree is at $ACTUAL, pin is $PIN" >&2
  exit 2
fi

# Only the pure-Java subset is compiled: KeyHandler/TerminalSession need the real Android
# SDK (KeyEvent, Looper, Service) and are not part of terminal-state semantics.
TERM_DIR="$UPSTREAM_SRC/terminal-emulator/src/main/java/com/termux/terminal"
CORE="TerminalEmulator TerminalBuffer TerminalRow TextStyle WcWidth ByteQueue TerminalOutput TerminalColorScheme TerminalColors Logger TerminalSessionClient KeyHandler"

rm -rf "$OUT"
mkdir -p "$OUT"

SRCS=()
for cls in $CORE; do
  SRCS+=("$TERM_DIR/$cls.java")
done
while IFS= read -r f; do
  SRCS+=("$f")
done < <(find "$HERE/stubs" "$HERE/src" -name '*.java' | sort)

javac -encoding UTF-8 -nowarn -d "$OUT" "${SRCS[@]}"
echo "oracle: compiled reference + harness -> $OUT"