#!/bin/sh
set -eu
if [ "$#" -ne 2 ]; then
  printf '%s\n' 'Usage: tools/build-marcher.sh EXTERNAL_CHECKOUT OUTPUT_BINARY' >&2
  exit 2
fi
repo=$1
out=$2
expected=1fa785edbe8003445163a63f00f04a4c0b77254b
test "$(git -C "$repo" rev-parse HEAD)" = "$expected"
git -C "$repo" diff-index --quiet HEAD --
test -z "$(git -C "$repo" ls-files --others --exclude-standard)"
cc -O3 -std=gnu11 -DNDEBUG -DPRINT_OUTPUT=0 -DUSE_ENDGAME_DB=0 \
  -DNNUE_NO_SIMD=1 -DTT_ENTRIES_LOG2=15 -I "$repo/src/engine" \
  "$(dirname "$0")/marcher_bridge.c" -lm -o "$out"
