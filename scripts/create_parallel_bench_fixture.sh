#!/usr/bin/env bash
set -euo pipefail

root=${1:?usage: scripts/create_parallel_bench_fixture.sh ROOT}
dirs=${RUSHFIND_BENCH_DIRS:-96}
subdirs=${RUSHFIND_BENCH_SUBDIRS:-8}
files=${RUSHFIND_BENCH_FILES:-16}

rm -rf "$root"
mkdir -p "$root"

for dir_index in $(seq 0 $((dirs - 1))); do
    dir="$root/dir-$(printf '%03d' "$dir_index")"
    mkdir -p "$dir"
    for sub_index in $(seq 0 $((subdirs - 1))); do
        sub="$dir/sub-$(printf '%03d' "$sub_index")"
        mkdir -p "$sub"
        for file_index in $(seq 0 $((files - 1))); do
            printf 'x\n' >"$sub/file-$(printf '%03d' "$file_index").txt"
        done
    done
done
