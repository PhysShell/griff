#!/usr/bin/env bash
# Usage (inside WSL, under `nix shell nixpkgs#fluidsynth nixpkgs#sox nixpkgs#ffmpeg`):
#   render_pack.sh <guitar.sf2> <gm.sf2> <dir>
# For every <name>.mid with a <name>.band.mid beside it in <dir>: clean render
# <name>.wav, band render <name>.b.wav (RMS-matched), MP3s of both.
set -uo pipefail
here=$(dirname "$0"); sfg="$1"; sfb="$2"; dir="$3"
cd "$dir"
for m in *.mid; do
  case "$m" in *.band.mid) continue ;; esac
  b="${m%.mid}"
  bash "$here/render.sh" "$sfg" "$m" "$b.wav" || echo "clean render failed: $m"
  if [ -f "$b.band.mid" ]; then
    bash "$here/band_render.sh" "$sfg" "$sfb" "$m" "$b.band.mid" "$b.wav" "$b.b.wav" || echo "band render failed: $m"
  fi
done
bash "$here/to_mp3.sh" *.wav
bash "$here/check_wavs.sh" *.wav
