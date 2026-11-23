#!/usr/bin/env bash
# Build 98 guitar samples (MIDI pitches 40-88 x velocities 72, 100) for Studio Web.
# Usage (from composer-lab/web/ or anywhere):
#   nix shell --offline nixpkgs#fluidsynth nixpkgs#sox nixpkgs#ffmpeg -c bash build_samples.sh
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
OUT_DIR="${SCRIPT_DIR}/samples"
SF_DIR="$(cd "${SCRIPT_DIR}/../../.." && pwd)/_griff_out/sf"
SF_PATH="${GRIFF_SOUNDFONT:-${SF_DIR}/fsbs_clean.sf2}"
LICENSE_PATH="${GRIFF_SOUNDFONT_LICENSE:-${SF_DIR}/fsbs_LICENSE.txt}"

if [[ ! -f "$SF_PATH" ]]; then
  echo "SoundFont not found: $SF_PATH" >&2
  exit 1
fi
if [[ ! -f "$LICENSE_PATH" ]]; then
  echo "License file not found: $LICENSE_PATH" >&2
  exit 1
fi

for cmd in fluidsynth sox ffmpeg; do
  if ! command -v "$cmd" >/dev/null 2>&1; then
    echo "Missing required tool: $cmd (run inside nix shell nixpkgs#fluidsynth nixpkgs#sox nixpkgs#ffmpeg)" >&2
    exit 1
  fi
done

mkdir -p "$OUT_DIR"
rm -f "$OUT_DIR"/*.mp3 "$OUT_DIR/manifest.json"
cp -f "$LICENSE_PATH" "$OUT_DIR/fsbs_LICENSE.txt"

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "$TMP_DIR"' EXIT

MID_FILE="$TMP_DIR/all_notes.mid"
RAW_WAV="$TMP_DIR/raw.wav"
NORM_WAV="$TMP_DIR/norm.wav"
NOTE_WAV="$TMP_DIR/note.wav"
TRACK_BODY="$TMP_DIR/track.bin"

# Build format-0 single-track MIDI at 120 BPM (PPQ=480, 960 ticks/sec).
# Each slot = 3840 ticks (4.0 s): NoteOn at 0, NoteOff at +2400 (2.5 s),
# CC120 All Sound Off at +1248 (+1.3 s), +192 tick gap before next NoteOn.
{
  # Delta 0, Tempo 500000 us/quarter (120 BPM); Delta 0, Program Change ch0 -> 0
  printf '\x00\xff\x51\x03\x07\xa1\x20'
  printf '\x00\xc0\x00'
  first=1
  for pitch in $(seq 40 88); do
    for vel in 72 100; do
      p_hex=$(printf '\\x%02x' "$pitch")
      v_hex=$(printf '\\x%02x' "$vel")
      if [[ "$first" -eq 1 ]]; then
        delta_on='\x00'
        first=0
      else
        delta_on='\x81\x40'
      fi
      # Note On (ch 0)
      printf "${delta_on}\x90${p_hex}${v_hex}"
      # +2400 ticks (0x960 -> VLQ 92 60): Note Off (ch 0)
      printf "\x92\x60\x80${p_hex}\x00"
      # +1248 ticks (0x4e0 -> VLQ 89 60): CC 120 All Sound Off
      printf '\x89\x60\xb0\x78\x00'
    done
  done
  # +192 ticks (VLQ 81 40): End of Track
  printf '\x81\x40\xff\x2f\x00'
} > "$TRACK_BODY"

TRACK_LEN=$(wc -c < "$TRACK_BODY")
{
  # MThd: len=6, format=0, tracks=1, ppq=480 (0x01e0)
  printf 'MThd\x00\x00\x00\x06\x00\x00\x00\x01\x01\xe0'
  printf 'MTrk'
  len_esc=$(printf '\\x%02x\\x%02x\\x%02x\\x%02x' \
    $(((TRACK_LEN >> 24) & 255)) \
    $(((TRACK_LEN >> 16) & 255)) \
    $(((TRACK_LEN >> 8) & 255)) \
    $((TRACK_LEN & 255)))
  printf '%b' "$len_esc"
  cat "$TRACK_BODY"
} > "$MID_FILE"

echo "Rendering 98 notes with FluidSynth..."
fluidsynth -ni -g 0.5 -r 44100 -F "$RAW_WAV" "$SF_PATH" "$MID_FILE" >/dev/null 2>&1
raw_dur=$(soxi -D "$RAW_WAV")
echo "Rendered raw WAV duration: ${raw_dur}s"
if awk "BEGIN { exit !($raw_dur < 390) }"; then
  echo "ERROR: rendered WAV too short (${raw_dur}s < 390s)" >&2
  exit 1
fi

echo "Mixing to mono and normalizing..."
sox "$RAW_WAV" "$NORM_WAV" channels 1 gain -n -1

echo "Slicing and encoding MP3 samples..."
MANIFEST="$OUT_DIR/manifest.json"
{
  printf '{\n'
  printf '  "min_pitch": 40,\n'
  printf '  "max_pitch": 88,\n'
  printf '  "velocities": [72, 100],\n'
  printf '  "duration_sec": 2.5,\n'
  printf '  "sample_rate": 44100,\n'
  printf '  "bitrate_kbps": 96,\n'
  printf '  "license": "fsbs_LICENSE.txt",\n'
  printf '  "files": {\n'

  idx=0
  for pitch in $(seq 40 88); do
    printf '    "%d": {\n' "$pitch"
    for vi in 0 1; do
      vel=72
      if [[ "$vi" -eq 1 ]]; then
        vel=100
      fi
      start_sec=$((idx * 4))
      fname=$(printf 'p%03d_v%03d.mp3' "$pitch" "$vel")
      sox "$NORM_WAV" "$NOTE_WAV" trim "$start_sec" 2.5 fade 0 2.5 0.03
      ffmpeg -loglevel error -y -i "$NOTE_WAV" -ac 1 -ar 44100 -codec:a libmp3lame -b:a 96k "$OUT_DIR/$fname"
      comma=","
      if [[ "$vi" -eq 1 ]]; then
        comma=""
      fi
      printf '      "%d": "%s"%s\n' "$vel" "$fname" "$comma"
      idx=$((idx + 1))
    done
    p_comma=","
    if [[ "$pitch" -eq 88 ]]; then
      p_comma=""
    fi
    printf '    }%s\n' "$p_comma"
  done

  printf '  }\n'
  printf '}\n'
} > "$MANIFEST"

total_bytes=$(du -sb "$OUT_DIR" | awk '{print $1}')
echo "Done: $idx MP3 samples in $OUT_DIR (total ${total_bytes} bytes)"
