#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
baseline_ref=${1:?usage: scripts/bench_parallel_engine.sh BASELINE_REF}
workers=${FINDOXIDE_WORKERS:-8}
tmpdir=$(mktemp -d)
baseline_tree="$tmpdir/baseline"
fixture="$tmpdir/wide-tree"

cleanup() {
    git -C "$repo_root" worktree remove --force "$baseline_tree" >/dev/null 2>&1 || true
    rm -rf "$tmpdir"
}
trap cleanup EXIT

bash "$repo_root/scripts/create_parallel_bench_fixture.sh" "$fixture"
git -C "$repo_root" worktree add --detach "$baseline_tree" "$baseline_ref" >/dev/null

run_case() {
    local label=$1
    local tree=$2

    echo "== $label =="
    (
        cd "$tree"
        cargo build --quiet >/dev/null
        /usr/bin/time -f "%E %MKB" \
            env FINDOXIDE_WORKERS="$workers" \
            cargo run --quiet -- "$fixture" -type f -printf '%p\n' >/dev/null
    )
}

run_case baseline "$baseline_tree"
run_case current "$repo_root"
