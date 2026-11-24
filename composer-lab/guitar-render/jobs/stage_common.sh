# Sourced by the job scripts (WSL host side). Stages trusted inputs into an auditbox job directory.
export PATH=/run/current-system/sw/bin:/run/wrappers/bin
NIXPKGS=github:NixOS/nixpkgs/26ef669cffa904b6f6832ab57b77892a37c1a671
OUT_WIN=/mnt/c/Users/f.shelipov/source/repos/_griff_out/guitar-render-01
EXP=/mnt/c/Users/f.shelipov/source/repos/_guitar-render-wt/composer-lab/guitar-render
SFIZZ=$(nix build --no-link --print-out-paths $NIXPKGS#sfizz.out)
DOTNET=$(nix build --no-link --print-out-paths $NIXPKGS#dotnet-runtime_9.unwrapped 2>/dev/null || true)
[ -n "$DOTNET" ] || DOTNET=/nix/store/9hmkr5l6qgzm2whhf4gz1n9ppvrix6sb-dotnet-runtime-9.0.15
SOX=$(nix build --no-link --print-out-paths $NIXPKGS#sox.out)
FLUIDSYNTH=$(nix build --no-link --print-out-paths $NIXPKGS#fluidsynth.out)
stage_payload() {  # $1 job dir
  local j=$1
  mkdir -p "$j"
  if [ ! -f "$HOME/gr01/nixextra.tar" ]; then
    tar -C /nix/store -cf "$HOME/gr01/nixextra.tar" "$(basename "$SFIZZ")" "$(basename "$DOTNET")" "$(basename "$SOX")"
  fi
  cp "$HOME/gr01/nixextra.tar" "$j/"
  if [ ! -f "$HOME/gr01/fluid.tar" ]; then
    nix path-info -r "$FLUIDSYNTH" "$SOX" | sort -u | grep -v -- "-glibc-2" | xargs -n1 basename > "$HOME/gr01/fluid.list"
    tar -C /nix/store -cf "$HOME/gr01/fluid.tar" -T "$HOME/gr01/fluid.list"
  fi
  cp "$HOME/gr01/fluid.tar" "$j/"
  printf 'SFIZZ=%s\nDOTNET=%s\nSOX=%s\nFLUIDSYNTH_STORE=%s\n' "$(basename "$SFIZZ")" "$(basename "$DOTNET")" "$(basename "$SOX")" "$FLUIDSYNTH" > "$j/nixextra.env"
  cp "$HOME/auditbox-jobs/gr01-wheels-out/wheels.tar" "$j/"
  cp -r "$OUT_WIN/bin/namrender" "$j/namrender"
  cp -r "$EXP/grender" "$EXP/render.py" "$EXP/analyze.py" "$EXP/make_clips.py" "$EXP/guest_setup.sh" "$j/"
  [ -d "$EXP/presets" ] && cp -r "$EXP/presets" "$j/"
  return 0
}
run_box() {  # $1 job dir, $2 timeout seconds, $3 out MB
  cd ~/qubix-auditbox
  set +e
  tools/auditbox-run.sh --timeout "$2" --out-mb "$3" "$1" "$1-out" > "$1.console.txt" 2>&1
  local rc=$?
  set -e
  echo "auditbox rc=$rc"
  tail -40 "$1-out/auditbox.log" 2>/dev/null
  return 0
}
