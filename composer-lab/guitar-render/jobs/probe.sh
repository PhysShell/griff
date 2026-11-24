#!/usr/bin/env bash
# P0 probe: sfizz_render CLI, dotnet + NAudio.Nam, pedalboard, ffmpeg MP3 encoder, all inside the offline auditbox.
set -euo pipefail
. "$(dirname "$0")/stage_common.sh"
J=$HOME/auditbox-jobs/gr01-probe
rm -rf "$J" "$J-out"; stage_payload "$J"
mkdir -p "$J/nam"; cp "$OUT_WIN"/assets/nam/*.nam "$J/nam/"
cat > "$J/sine.sfz" <<'S'
<region> sample=*sine ampeg_release=0.2
S
cat > "$J/run.sh" <<'R'
#!/usr/bin/env bash
set -uo pipefail
. ./guest_setup.sh; set +e
O=$AUDITBOX_OUT
"$SFIZZ_RENDER" --help > $O/sfizz_help.txt 2>&1; echo "sfizz help rc=$?"
"$SFIZZ_RENDER" --version >> $O/sfizz_help.txt 2>&1
"$DOTNET_BIN" --info > $O/dotnet_info.txt 2>&1; echo "dotnet rc=$?"; head -5 $O/dotnet_info.txt
ffmpeg -hide_banner -encoders 2>/dev/null | grep -i -E "mp3|lame" > $O/ffmpeg_mp3.txt; cat $O/ffmpeg_mp3.txt
env LD_LIBRARY_PATH="$GR_LD_LIBRARY_PATH" "$PY" -I - > $O/py.txt 2>&1 <<'P'
import pedalboard, numpy, scipy, soundfile, pyloudnorm
print("pedalboard", pedalboard.__version__, "numpy", numpy.__version__, "scipy", scipy.__version__, "soundfile", soundfile.__version__, soundfile.__libsndfile_version__)
print([n for n in dir(pedalboard) if n[0].isupper()])
import numpy as np, soundfile as sf
sr=48000; t=np.arange(sr*2)/sr
x=(0.3*np.sin(2*np.pi*110*t)*np.exp(-t*1.5)).astype(np.float32)
sf.write("/tmp/sine.wav", x, sr, subtype="FLOAT")
P
echo "py rc=$?"; cat $O/py.txt | head -3
"$DOTNET_BIN" "$NAMRENDER" nam/SB50-Ch2-Trans-DI.nam 1.0 /tmp/sine.wav $O/sine_ch2_full.wav 0 /tmp/sine.wav $O/sine_ch2_full_b.wav 0 > $O/nam.json 2> $O/nam.err; echo "nam rc=$?"; cat $O/nam.json $O/nam.err | head -c 1500; echo
"$DOTNET_BIN" "$NAMRENDER" nam/SB50-Ch2-Trans-DI.nam 0.0 /tmp/sine.wav $O/sine_ch2_lite.wav 0 > $O/nam_lite.json 2>&1; echo "nam lite rc=$?"; head -c 600 $O/nam_lite.json; echo
cmp $O/sine_ch2_full.wav $O/sine_ch2_full_b.wav && echo "nam same-process repeat: byte-identical"
printf '\x4d\x54\x68\x64\x00\x00\x00\x06\x00\x00\x00\x01\x01\xe0\x4d\x54\x72\x6b\x00\x00\x00\x16\x00\x90\x2d\x64\x83\x60\x80\x2d\x00\x00\x90\x34\x64\x83\x60\x80\x34\x00\x00\xff\x2f\x00' > /tmp/t.mid
time "$SFIZZ_RENDER" --sfz sine.sfz --midi /tmp/t.mid --wav $O/sfizz_sine.wav > $O/sfizz_run.txt 2>&1; echo "sfizz render rc=$?"; tail -25 $O/sfizz_run.txt
ls -la $O
exit 0
R
run_box "$J" 900 200
