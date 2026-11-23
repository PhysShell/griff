#!/usr/bin/env bash
# Usage (inside WSL): render_all.sh <soundfont.sf2> <file.mid>...  -> writes <file>.wav next to each
set -uo pipefail
here=$(dirname "$0"); sf="$1"; shift
for m in "$@"; do bash "$here/render.sh" "$sf" "$m" "${m%.mid}.wav" || echo "render failed: $m"; done
