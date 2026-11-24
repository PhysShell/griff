#!/usr/bin/env bash
# Host side (WSL): stage and run the auditbox FETCH job that downloads the Python wheels; nothing is installed or run.
set -euo pipefail
J=$HOME/auditbox-jobs/gr01-wheels
rm -rf "$J" "$J-out"; mkdir -p "$J"
cat > "$J/run.sh" <<'R'
#!/usr/bin/env bash
set -euo pipefail
python3 --version
python3 -m venv "$HOME/v"
"$HOME/v/bin/pip" download --only-binary=:all: --dest wheels --python-version 3.12 \
  numpy==2.1.3 scipy==1.14.1 soundfile==0.12.1 pedalboard pyloudnorm==0.1.1 2>&1 | tail -12
ls -l wheels
sha256sum wheels/*.whl > "$AUDITBOX_OUT/wheels.sha256"
tar -cf "$AUDITBOX_OUT/wheels.tar" wheels
R
cd ~/qubix-auditbox
tools/auditbox-run.sh --fetch --timeout 900 --out-mb 600 "$J" "$J-out"; echo rc=$?
tail -25 "$J-out/auditbox.log"
