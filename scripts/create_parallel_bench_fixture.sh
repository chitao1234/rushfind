#!/usr/bin/env bash
set -euo pipefail

root=${1:?usage: scripts/create_parallel_bench_fixture.sh ROOT}
dirs=${FINDOXIDE_BENCH_DIRS:-96}
files=${FINDOXIDE_BENCH_FILES:-32}

rm -rf "$root"
mkdir -p "$root"

for dir_index in $(seq 0 $((dirs - 1))); do
    dir="$root/dir-$(printf '%03d' "$dir_index")"
    mkdir -p "$dir"
    for file_index in $(seq 0 $((files - 1))); do
        printf 'x\n' >"$dir/file-$(printf '%03d' "$file_index").txt"
    done
done
