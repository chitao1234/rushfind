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

run_required() {
    local label="$1"
    shift

    echo
    echo "== $label =="
    "$bin" "$@"
}

run_optional() {
    local label="$1"
    shift

    echo
    echo "== $label =="
    set +e
    "$bin" "$@"
    local status=$?
    set -e
    if [[ $status -ne 0 ]]; then
        echo "(probe ended with status $status; unsupported is acceptable on the generic Unix tier)"
    fi
}

mkdir -p "$workdir/root/sub"
printf 'alpha\n' > "$workdir/root/file.txt"
ln -s file.txt "$workdir/root/link.txt"

echo "== version =="
"$bin" -version

echo
echo "== print / print0 =="
"$bin" "$workdir/root" -maxdepth 1 -print
"$bin" "$workdir/root" -maxdepth 1 -print0 | hexdump -ve '1/1 "%02x"'
printf '\n'

run_optional "fstype" "$workdir/root" -maxdepth 0 -printf '%F %p\n'
run_optional "birth time" "$workdir/root" -maxdepth 1 -printf '%B@ %p\n'
run_required "same filesystem" "$workdir/root" -xdev -print
run_required "ownership and access" "$workdir/root" -maxdepth 1 -printf '%u %g %m %p\n'
run_required "ownership and access" "$workdir/root" -maxdepth 1 '(' -readable -a -writable ')' -print
run_required "ls/fls surface" "$workdir/root" -maxdepth 1 -ls
run_required "execdir path handling" "$workdir/root" -maxdepth 1 -type f -execdir printf '%s\n' '{}' ';'

echo
echo "Run the interactive locale checks manually on the target host:"
echo "  LC_MESSAGES=C $bin \"$workdir/root\" -ok printf '%s\\n' '{}' ';'"
echo "  LC_MESSAGES=fr_FR.UTF-8 $bin \"$workdir/root\" -ok printf '%s\\n' '{}' ';'"
