#!/usr/bin/env bash
set -euo pipefail

repo_root=$(cd "$(dirname "$0")/.." && pwd)
root=${1:?usage: scripts/bench_parallel_engine.sh ROOT}
workers=${FINDOXIDE_WORKERS:-8}

cd "$repo_root"

for engine in legacy v2; do
  echo "== $engine =="
  /usr/bin/time -f "%E %MKB" \
    env FINDOXIDE_WORKERS="$workers" FINDOXIDE_PARALLEL_ENGINE="$engine" \
    cargo run --quiet -- "$root" -type f -printf '%p\n' >/dev/null
done
