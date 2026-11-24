#!/usr/bin/env bash
# Usage (WSL): REPEATS=n render_job.sh <result-dir> <prep-dir>...
# Stages every prepared plan with the sample libraries, NAM captures, IRs and tools into ONE offline
# auditbox job, renders each plan REPEATS times in fresh processes and copies the results back.
# A prep dir may also hold extra.sh: guest commands run after the renders (cwd = that plan's dir).
set -euo pipefail
. "$(dirname "$0")/stage_common.sh"
RES=$1; shift
REPEATS=${REPEATS:-1}
name=gr01-$(basename "$RES")
J=$HOME/auditbox-jobs/$name
rm -rf "$J" "$J-out"; stage_payload "$J"
mkdir -p "$J/nam" "$J/ir" "$J/libs" "$J/plans"
cp "$OUT_WIN"/assets/nam/*.nam "$J/nam/"
cp "$OUT_WIN"/assets/dl-god-ir/x/free_impulse_response_pack_01/Harley_Benton_Celestion_V30/PA_AMP/*.wav \
   "$OUT_WIN"/assets/dl-god-ir/x/free_impulse_response_pack_01/Laney_Ironheart_2x12/PA_AMP/*.wav "$J/ir/"
cp /mnt/c/Users/f.shelipov/source/repos/_griff_out/sf/fsbs_clean.sf2 "$J/" 2>/dev/null || true
names=()
for PREP in "$@"; do
  n=$(basename "$PREP"); names+=("$n")
  cp -r "$PREP" "$J/plans/$n"
  for lib in $(grep -o '"library_dir": "[^"]*"' "$PREP/plan.json" 2>/dev/null | cut -d'"' -f4 | sort -u); do
    [ -d "$J/libs/$lib" ] && continue
    [ -d "$HOME/gr01/libs/$lib" ] || { mkdir -p "$HOME/gr01/libs"; cp -r "$OUT_WIN/assets/libs/$lib" "$HOME/gr01/libs/$lib"; }
    cp -al "$HOME/gr01/libs/$lib" "$J/libs/$lib"
  done
done
cat > "$J/run.sh" <<R
#!/usr/bin/env bash
set -uo pipefail
. ./guest_setup.sh; set +e
export ROOT=\$PWD
export PYRUN="env LD_LIBRARY_PATH=\$GR_LD_LIBRARY_PATH \$PY -I"
for n in ${names[*]}; do
  P=\$HOME/plans/\$n; mkdir -p "\$P"; cp -r "\$ROOT/plans/\$n/." "\$P/"
  for d in libs nam ir; do ln -s "\$ROOT/\$d" "\$P/\$d"; done
  cp "\$ROOT/render.py" "\$ROOT/analyze.py" "\$P/"; cp -r "\$ROOT/grender" "\$P/"
  [ -f "\$ROOT/fsbs_clean.sf2" ] && ln -s "\$ROOT/fsbs_clean.sf2" "\$P/fsbs_clean.sf2"
  O=\$AUDITBOX_OUT/\$n; mkdir -p "\$O"
  cd "\$P"
  if [ -f plan.json ]; then
    for r in \$(seq 1 $REPEATS); do
      \$PYRUN render.py plan.json "\$O/run\$r" > "\$O/run\$r.console.txt" 2>&1
      echo "\$n run\$r rc=\$?"; tail -2 "\$O/run\$r.console.txt"
    done
    if [ -f expected.json ]; then
      for f in "\$O"/run1/*.di.wav; do \$PYRUN analyze.py onsets "\$f" expected.json > "\$O/onsets_\$(basename "\$f" .wav).json" 2>&1; done
    fi
    if [ $REPEATS -gt 1 ]; then
      \$PYRUN analyze.py compare \$(seq -f "\$O/run%g" 1 $REPEATS) > "\$O/repeat_compare.json" 2>&1
    fi
  fi
  if [ -f extra.sh ]; then OUT=\$O bash extra.sh > "\$O/extra.console.txt" 2>&1; echo "\$n extra rc=\$?"; tail -3 "\$O/extra.console.txt"; fi
  cd "\$ROOT"
done
exit 0
R
run_box "$J" 5400 6000
mkdir -p "$RES"
cp -r "$J-out"/. "$RES/"
cp "$J.console.txt" "$RES/auditbox.console.txt"
rm -rf "$J"
echo "results in $RES"
