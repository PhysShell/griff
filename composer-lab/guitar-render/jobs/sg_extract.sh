#!/usr/bin/env bash
# Usage (WSL): sg_extract.sh <archive.rar> <dest-dir>
# Tests and unpacks an untrusted RAR inside the offline auditbox with unar/lsar (nixpkgs, LGPL),
# then copies the unpacked tree to <dest-dir>, which must not exist yet.
set -euo pipefail
. "$(dirname "$0")/stage_common.sh"
ARCHIVE=$1 DEST=$2
[ ! -e "$DEST" ] || { echo "$DEST exists" >&2; exit 2; }
UNAR=$(nix build --no-link --print-out-paths $NIXPKGS#unar.out)
J=$HOME/auditbox-jobs/gr01-sg-extract
rm -rf "${J:?}" "${J:?}-out"; mkdir -p "$J"
cp "$ARCHIVE" "$J/archive.rar"
nix path-info -r "$UNAR" | grep -v -- "-glibc-2" | xargs -n1 basename > "$J/unar.list"
tar -C /nix/store -cf "$J/unar.tar" -T "$J/unar.list"
cat > "$J/run.sh" <<R
#!/usr/bin/env bash
set -uo pipefail
mkdir -p "\$HOME/u"; tar -xf unar.tar -C "\$HOME/u"
export LD_LIBRARY_PATH=\$(find "\$HOME/u" -maxdepth 2 -name lib -type d | paste -sd:)
B="\$HOME/u/$(basename "$UNAR")/bin"
sha256sum archive.rar | tee "\$AUDITBOX_OUT/archive.sha256"
"\$B/lsar" -t archive.rar > "\$AUDITBOX_OUT/lsar-test.txt" 2>&1; echo "lsar test rc=\$?"; tail -3 "\$AUDITBOX_OUT/lsar-test.txt"
"\$B/lsar" -l archive.rar > "\$AUDITBOX_OUT/lsar-list.txt" 2>&1; echo "lsar list rc=\$?"
"\$B/unar" -q -o "\$AUDITBOX_OUT/x" -D archive.rar; echo "unar rc=\$?"
find "\$AUDITBOX_OUT/x" -type f | sed 's/.*\.//' | sort | uniq -c
du -sh "\$AUDITBOX_OUT/x"
exit 0
R
cd ~/qubix-auditbox
tools/auditbox-run.sh --timeout 1800 --out-mb 3000 "$J" "$J-out" > "$J.console.txt" 2>&1 || true
tail -20 "$J-out/auditbox.log"
mkdir -p "$DEST"
cp -r "$J-out"/x/. "$DEST/"
cp "$J-out"/archive.sha256 "$J-out"/lsar-*.txt "$DEST/../" 2>/dev/null || true
rm -rf "${J:?}" "${J:?}-out"
echo "unpacked to $DEST"
