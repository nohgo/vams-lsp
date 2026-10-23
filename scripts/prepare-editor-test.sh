#!/bin/sh
set -eu
root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
grammar=${VAMS_TEST_GRAMMAR:-"$root/build/test-deps/tree-sitter-verilog-ams"}
mkdir -p "$root/.test-work/parser"
${CC:-cc} -O2 -shared -fPIC -I "$grammar/src" "$grammar/src/parser.c" -o "$root/.test-work/parser/verilog_ams.so"
