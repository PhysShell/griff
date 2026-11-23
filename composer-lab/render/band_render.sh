#!/usr/bin/env bash
# Usage (inside WSL, under `nix shell nixpkgs#fluidsynth nixpkgs#sox`):
#   band_render.sh <guitar.sf2> <gm.sf2> <guitars.mid> <band.mid> <clean.wav> <out.wav>
# Mixes the guitars (same render as render.sh, before reverb) with bass and
# drums, then matches the RMS of <clean.wav> so the band version is not
# louder than the clean one.
set -euo pipefail
sfg="$1"; sfb="$2"; gmid="$3"; bmid="$4"; clean="$5"; out="$6"
tmp=$(mktemp -d)
fluidsynth -ni -g 0.5 -r 44100 -F "$tmp/g.wav" "$sfg" "$gmid" >/dev/null 2>&1
fluidsynth -ni -g 0.5 -r 44100 -F "$tmp/b.wav" "$sfb" "$bmid" >/dev/null 2>&1
sox -m -v 1.0 "$tmp/g.wav" -v 0.6 "$tmp/b.wav" "$tmp/mix.wav"
sox "$tmp/mix.wav" "$tmp/rev.wav" gain -n -3 reverb 22 50 70 trim 0 "$(sox --i -D "$clean")" fade t 0 -0 0.5 2>/dev/null
rms() { sox "$1" -n stat 2>&1 | awk '/RMS +amplitude/ {print $3; exit}'; }
db=$(awk -v t="$(rms "$clean")" -v c="$(rms "$tmp/rev.wav")" 'BEGIN { printf "%.2f", 20 * log(t / c) / log(10) }')
sox "$tmp/rev.wav" "$out" gain -l "$db" 2>/dev/null
rm -rf "$tmp"
