#!/usr/bin/env bash
# Usage (inside WSL with ffmpeg): to_mp3.sh <file.wav>...  -> <file>.mp3 at 192 kbit/s
for w in "$@"; do ffmpeg -loglevel error -y -i "$w" -codec:a libmp3lame -b:a 192k "${w%.wav}.mp3"; done
