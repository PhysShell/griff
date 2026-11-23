#!/usr/bin/env bash
# Usage (inside WSL, under `nix shell nixpkgs#fluidsynth nixpkgs#sox`):
#   render.sh <soundfont.sf2> <in.mid> <out.wav>
# The MIDI must already be remapped by audition_midi.py.
set -euo pipefail
sf="$1"; in="$2"; out="$3"
tmp=$(mktemp --suffix=.wav)
fluidsynth -ni -g 0.5 -r 44100 -F "$tmp" "$sf" "$in" >/dev/null 2>&1
sox "$tmp" "$out" gain -n -3 reverb 22 50 70 gain -n -1 2>/dev/null
rm -f "$tmp"
