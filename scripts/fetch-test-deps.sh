#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
fetch() {
  name=$1 url=$2 revision=$3
  dir="$root/build/test-deps/$name"
  if [ ! -d "$dir/.git" ]; then mkdir -p "$dir"; git -C "$dir" init -q; fi
  if ! git -C "$dir" cat-file -e "$revision^{commit}" 2>/dev/null; then
    git -C "$dir" fetch --depth 1 "$url" "$revision"
  fi
  git -C "$dir" checkout -q --detach "$revision"
}
fetch blink.cmp https://github.com/saghen/blink.cmp.git 78336bc89ee5365633bcf754d93df01678b5c08f
fetch LuaSnip https://github.com/L3MON4D3/LuaSnip.git 642b0c595e11608b4c18219e93b88d7637af27bc
fetch tree-sitter-verilog-ams https://github.com/nohgo/tree-sitter-verilog-ams.git 0b2672e76ff2941fb62081b5d42b0ae84a0f6c46
fetch cadence.nvim https://github.com/nohgo/cadence.nvim.git a342689256906fb9957a22353a8e804b7747f583
