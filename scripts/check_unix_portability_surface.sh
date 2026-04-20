#!/usr/bin/env bash
set -euo pipefail

bin="${1:-target/debug/rfd}"
workdir="$(mktemp -d)"

cleanup() {
    rm -rf "$workdir"
}

trap cleanup EXIT

if [[ ! -x "$bin" ]]; then
    echo "binary is not executable: $bin" >&2
    exit 1
fi

mkdir -p "$workdir/root/sub"
printf 'alpha\n' > "$workdir/root/file.txt"
ln -s file.txt "$workdir/root/link.txt"

echo "== print / print0 =="
"$bin" "$workdir/root" -maxdepth 1 -print
"$bin" "$workdir/root" -maxdepth 1 -print0 | hexdump -ve '1/1 "%02x"'
printf '\n'

echo
echo "== fstype =="
"$bin" "$workdir/root" -maxdepth 0 -printf '%F %p\n'

echo
echo "== birth time =="
"$bin" "$workdir/root" -maxdepth 1 -printf '%B@ %p\n' || true

echo
echo "== same filesystem =="
"$bin" "$workdir/root" -xdev -print

echo
echo "== ownership and access =="
"$bin" "$workdir/root" -maxdepth 1 -printf '%u %g %m %p\n'
"$bin" "$workdir/root" -maxdepth 1 '(' -readable -a -writable ')' -print

echo
echo "== ls/fls surface =="
"$bin" "$workdir/root" -maxdepth 1 -ls

echo
echo "== execdir path handling =="
"$bin" "$workdir/root" -maxdepth 1 -type f -execdir printf '%s\n' '{}' ';'

echo
echo "Run the interactive locale checks manually on the target host:"
echo "  LC_MESSAGES=C $bin \"$workdir/root\" -ok printf '%s\\n' '{}' ';'"
echo "  LC_MESSAGES=fr_FR.UTF-8 $bin \"$workdir/root\" -ok printf '%s\\n' '{}' ';'"
