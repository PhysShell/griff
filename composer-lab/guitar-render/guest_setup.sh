# Sourced inside the auditbox guest: unpack the nix payload and the wheels, make a venv, export tool paths.
set -euo pipefail
. ./nixextra.env
mkdir -p "$HOME/nx"
tar -xf nixextra.tar -C "$HOME/nx"
export SFIZZ_RENDER="$HOME/nx/$SFIZZ/bin/sfizz_render"
mkdir -p "$HOME/fl"; tar -xf fluid.tar -C "$HOME/fl"
export FLUIDSYNTH_BIN="$HOME/fl/$(basename "$FLUIDSYNTH_STORE")/bin/fluidsynth" SOX_BIN="$HOME/fl/$SOX/bin/sox"
export FLUID_LD_LIBRARY_PATH=$(find "$HOME/fl" -maxdepth 2 -name lib -type d | paste -sd:)
export DOTNET_BIN="$HOME/nx/$DOTNET/share/dotnet/dotnet"
[ -x "$DOTNET_BIN" ] || DOTNET_BIN=$(find "$HOME/nx/$DOTNET" -name dotnet -type f -perm -u+x | head -1)
export DOTNET_SYSTEM_GLOBALIZATION_INVARIANT=1 DOTNET_CLI_TELEMETRY_OPTOUT=1 DOTNET_LTTng=0 DOTNET_NOLOGO=1
export NAMRENDER="$AUDITBOX_IN/namrender/namrender.dll"
tar -xf wheels.tar -C "$HOME"
python3 -m venv "$HOME/v"
"$HOME/v/bin/pip" install -q --no-index --find-links "$HOME/wheels" numpy==2.1.3 scipy==1.14.1 soundfile==0.12.1 pedalboard pyloudnorm==0.1.1 > "$AUDITBOX_OUT/pip.txt" 2>&1
"$HOME/v/bin/pip" freeze > "$AUDITBOX_OUT/freeze.txt"
LIBDIRS=""
for so in libstdc++.so.6 libgcc_s.so.1 libz.so.1; do
  d=$(dirname "$(find /nix/store -maxdepth 4 -name "$so" 2>/dev/null | sort | head -1)"); LIBDIRS="$LIBDIRS:$d"
done
export GR_LD_LIBRARY_PATH="${LIBDIRS#:}"
export PY="$HOME/v/bin/python"
