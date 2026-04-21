#!/usr/bin/env bash
set -euo pipefail

prefix="${1:?install prefix required}"
source_env="${2:-ci/gnu-findutils-source.env}"
work_root="${RUNNER_TEMP:-$(mktemp -d)}"
src_dir="${work_root}/gnu-findutils-src"
jobs="$(sysctl -n hw.ncpu 2>/dev/null || getconf _NPROCESSORS_ONLN || echo 4)"

# shellcheck source=/dev/null
source "${source_env}"

rm -rf "${src_dir}"
mkdir -p "${prefix}"

git clone "${GNU_FINDUTILS_REPO}" "${src_dir}"
git -C "${src_dir}" checkout --detach "${GNU_FINDUTILS_REF}"
git -C "${src_dir}" submodule update --init --recursive gnulib

cd "${src_dir}"
./bootstrap
./configure --prefix="${prefix}" --program-prefix=g
make -j"${jobs}"
make install
