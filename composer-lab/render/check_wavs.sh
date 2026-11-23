#!/usr/bin/env bash
# Usage (inside WSL with sox): check_wavs.sh <file.wav>...   prints duration and RMS amplitude
for w in "$@"; do
  d=$(sox --i -D "$w")
  rms=$(sox "$w" -n stat 2>&1 | awk '/RMS +amplitude/ {print $3; exit}')
  echo "$(basename "$w") ${d}s rms=${rms}"
done
