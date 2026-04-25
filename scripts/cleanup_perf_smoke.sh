#!/usr/bin/env bash
set -euo pipefail

bin="${1:-target/debug/rfd}"
if [[ ! -x "$bin" ]]; then
  echo "binary is not executable: $bin" >&2
  exit 2
fi

tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT

root="$tmp/tree"
mkdir -p "$root"

for top in $(seq 0 31); do
  mkdir -p "$root/dir-$top/sub-a" "$root/dir-$top/sub-b"
  for leaf in $(seq 0 31); do
    printf 'payload %s %s\n' "$top" "$leaf" > "$root/dir-$top/sub-a/file-$leaf.txt"
    printf 'payload %s %s\n' "$top" "$leaf" > "$root/dir-$top/sub-b/target-$leaf.log"
  done
done

run_case() {
  local label="$1"
  shift
  local output="$tmp/$label.out"
  local start_ns end_ns elapsed_ms bytes lines
  start_ns="$(date +%s%N)"
  "$@" > "$output"
  end_ns="$(date +%s%N)"
  elapsed_ms="$(((end_ns - start_ns) / 1000000))"
  bytes="$(wc -c < "$output" | tr -d ' ')"
  lines="$(wc -l < "$output" | tr -d ' ')"
  printf '%-18s %8sms %8s bytes %8s lines\n' "$label" "$elapsed_ms" "$bytes" "$lines"
}

echo "fixture: $root"
run_case basename "$bin" "$root" -name 'target-17.log' -print
run_case metadata "$bin" "$root" -type f -size +0 -print
run_case printf "$bin" "$root" -type f -printf '%p %s %m\n'
run_case ordered_depth "$bin" "$root" -depth -name 'file-3.txt' -print
run_case parallel_default "$bin" "$root" -type f -print
run_case parallel_explicit_4 env RUSHFIND_WORKERS=4 "$bin" "$root" -type f -print
run_case parallel_explicit_8 env RUSHFIND_WORKERS=8 "$bin" "$root" -type f -print
