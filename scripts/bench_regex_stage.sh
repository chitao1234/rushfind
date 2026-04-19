#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
baseline_ref=${1:?usage: scripts/bench_regex_stage.sh BASELINE_REF}
workers=${RUSHFIND_WORKERS:-8}
repeats=${RUSHFIND_BENCH_REPEATS:-5}
tmpdir=$(mktemp -d)
baseline_tree="$tmpdir/baseline"
fixture="$tmpdir/fixture"

cleanup() {
    git -C "$repo_root" worktree remove --force "$baseline_tree" >/dev/null 2>&1 || true
    rm -rf "$tmpdir"
}
trap cleanup EXIT

bash "$repo_root/scripts/create_regex_bench_fixture.sh" "$fixture"
git -C "$repo_root" worktree add --detach "$baseline_tree" "$baseline_ref" >/dev/null

build_binary() {
    local tree=$1
    (
        cd "$tree"
        cargo build --quiet --release >/dev/null
    )
    printf '%s\n' "$tree/target/release/rfd"
}

median() {
    printf '%s\n' "$@" | sort -g | awk '
        { values[NR] = $1 }
        END {
            if (NR == 0) {
                exit 1
            }
            if (NR % 2 == 1) {
                print values[(NR + 1) / 2]
            } else {
                printf "%.6f\n", (values[NR / 2] + values[NR / 2 + 1]) / 2
            }
        }
    '
}

time_case() {
    local case_workers=$1
    local binary=$2
    shift 2
    local output
    output=$(
        {
            /usr/bin/time -f '%e' env RUSHFIND_WORKERS="$case_workers" "$binary" "$@" >/dev/null
        } 2>&1
    )
    printf '%s\n' "$output"
}

run_series() {
    local case_workers=$1
    local binary=$2
    shift 2
    local samples=()
    local run
    for run in $(seq 1 "$repeats"); do
        samples+=("$(time_case "$case_workers" "$binary" "$@")")
    done
    median "${samples[@]}"
}

emit_case() {
    local label=$1
    local mode=$2
    local case_workers=$3
    shift 3
    local baseline_median current_median delta
    baseline_median=$(run_series "$case_workers" "$baseline_bin" "$@")
    current_median=$(run_series "$case_workers" "$current_bin" "$@")
    delta=$(
        awk -v baseline="$baseline_median" -v current="$current_median" '
            BEGIN {
                if (baseline == 0 && current == 0) {
                    printf "0.00%%"
                } else if (baseline == 0) {
                    printf "n/a"
                } else {
                    printf "%.2f%%", ((current - baseline) / baseline) * 100
                }
            }
        '
    )
    printf 'case=%s mode=%s baseline=%ss current=%ss delta=%s\n' \
        "$label" "$mode" "$baseline_median" "$current_median" "$delta"
}

baseline_bin=$(build_binary "$baseline_tree")
current_bin=$(build_binary "$repo_root")

echo "Regex benchmark baseline ref: $baseline_ref"

emit_case light ordered 1 \
    "$fixture/light" \
    -type f \
    -regextype posix-extended \
    -regex '.*/(src|docs)/[^/]+\.(rs|MD)'
emit_case light parallel "$workers" \
    "$fixture/light" \
    -type f \
    -regextype posix-extended \
    -regex '.*/(src|docs)/[^/]+\.(rs|MD)'

emit_case heavy ordered 1 \
    "$fixture/heavy" \
    -type f \
    -regextype posix-extended \
    -regex '.*/(alpha|beta|gamma)/.*' \
    -iregex '.*/(module|readme|guide)[0-9]{2}-[a-z0-9]+\.(rs|md|txt)' \
    -regex '.*/[^/]+[0-9]{2}-[A-Za-z0-9]+\.(rs|md|txt)' \
    -regex '.*/(module|readme|guide).*'
emit_case heavy parallel "$workers" \
    "$fixture/heavy" \
    -type f \
    -regextype posix-extended \
    -regex '.*/(alpha|beta|gamma)/.*' \
    -iregex '.*/(module|readme|guide)[0-9]{2}-[a-z0-9]+\.(rs|md|txt)' \
    -regex '.*/[^/]+[0-9]{2}-[A-Za-z0-9]+\.(rs|md|txt)' \
    -regex '.*/(module|readme|guide).*'

emit_case fallback ordered 1 \
    "$fixture/fallback" \
    -type f \
    '(' \
    -regextype posix-basic \
    -regex '.*/repeats/\(a\|b\)\1-[0-9][0-9]\.txt' \
    -o \
    -regextype posix-extended \
    -regex '.*/words/\<(foo|bar)\>-[0-9][0-9]\.txt' \
    ')'
emit_case fallback parallel "$workers" \
    "$fixture/fallback" \
    -type f \
    '(' \
    -regextype posix-basic \
    -regex '.*/repeats/\(a\|b\)\1-[0-9][0-9]\.txt' \
    -o \
    -regextype posix-extended \
    -regex '.*/words/\<(foo|bar)\>-[0-9][0-9]\.txt' \
    ')'
