#!/usr/bin/env bash
set -euo pipefail

root=${1:?usage: scripts/create_regex_bench_fixture.sh ROOT}
dirs=${FINDOXIDE_REGEX_BENCH_DIRS:-48}
files=${FINDOXIDE_REGEX_BENCH_FILES:-24}

rm -rf "$root"
mkdir -p "$root/light" "$root/heavy" "$root/fallback"

for dir_index in $(seq 0 $((dirs - 1))); do
    dir_name=$(printf '%03d' "$dir_index")

    light_root="$root/light/tree-$dir_name"
    mkdir -p "$light_root/src" "$light_root/docs" "$light_root/misc"
    printf 'pub fn lib_%s() {}\n' "$dir_name" >"$light_root/src/lib-$dir_name.rs"
    printf '# guide %s\n' "$dir_name" >"$light_root/docs/GUIDE-$dir_name.MD"
    printf 'blob %s\n' "$dir_name" >"$light_root/misc/blob-$dir_name.bin"

    heavy_root="$root/heavy/set-$dir_name"
    mkdir -p "$heavy_root/alpha" "$heavy_root/beta" "$heavy_root/gamma" "$heavy_root/delta"
    for file_index in $(seq 0 $((files - 1))); do
        token=$(printf '%02d' "$file_index")
        printf 'module %s %s\n' "$dir_name" "$token" >"$heavy_root/alpha/module${token}-feature${dir_name}.rs"
        printf 'readme %s %s\n' "$dir_name" "$token" >"$heavy_root/beta/readme${token}-topic${dir_name}.md"
        printf 'guide %s %s\n' "$dir_name" "$token" >"$heavy_root/gamma/guide${token}-ALPHA${dir_name}.txt"
        printf 'miss %s %s\n' "$dir_name" "$token" >"$heavy_root/delta/blob${token}-${dir_name}.dat"
    done

    fallback_root="$root/fallback/case-$dir_name"
    mkdir -p "$fallback_root/words" "$fallback_root/repeats" "$fallback_root/miss"
    for file_index in $(seq 0 $((files - 1))); do
        token=$(printf '%02d' "$file_index")
        printf 'word %s %s\n' "$dir_name" "$token" >"$fallback_root/words/foo-$token.txt"
        printf 'word %s %s\n' "$dir_name" "$token" >"$fallback_root/words/bar-$token.txt"
        printf 'repeat %s %s\n' "$dir_name" "$token" >"$fallback_root/repeats/aa-$token.txt"
        printf 'repeat %s %s\n' "$dir_name" "$token" >"$fallback_root/repeats/bb-$token.txt"
        printf 'miss %s %s\n' "$dir_name" "$token" >"$fallback_root/miss/qux_$token.log"
    done
done
