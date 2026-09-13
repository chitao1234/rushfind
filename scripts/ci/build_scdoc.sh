#!/usr/bin/env bash
set -euo pipefail

prefix="${1:?install prefix required}"
source_env="${2:-ci/scdoc-source.env}"
work_root="${RUNNER_TEMP:-$(mktemp -d)}"
jobs="$(getconf _NPROCESSORS_ONLN 2>/dev/null || echo 4)"

# shellcheck source=/dev/null
source "${source_env}"

src_dir="${work_root}/scdoc-src"

rm -rf "${src_dir}"
mkdir -p "${src_dir}" "${prefix}"

curl -fsSL "${SCDOC_REPO}/archive/${SCDOC_REF}.tar.gz" \
    | tar -xz -C "${src_dir}" --strip-components=1

make -C "${src_dir}" -j"${jobs}"
make -C "${src_dir}" install PREFIX="${prefix}"
