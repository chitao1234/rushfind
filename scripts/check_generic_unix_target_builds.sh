#!/usr/bin/env bash
set -euo pipefail

toolchain="${RUSHFIND_TOOLCHAIN:-1.85.0}"
if [[ $# -gt 0 ]]; then
    targets=("$@")
else
    targets=(
        x86_64-unknown-illumos
        x86_64-pc-solaris
        x86_64-unknown-haiku
    )
fi

available_targets="$(rustup target list --toolchain "$toolchain")"

zig_target_for() {
    case "$1" in
        x86_64-unknown-haiku) printf '%s\n' 'x86_64-haiku' ;;
        *) return 1 ;;
    esac
}

supported_targets=()

for target in "${targets[@]}"; do
    if grep -Eq "^${target}( \\(installed\\))?$" <<<"$available_targets"; then
        if zig_target="$(zig_target_for "$target" 2>/dev/null)" && command -v zig >/dev/null 2>&1; then
            supported_targets+=("$target")
            continue
        fi

        normalized="${target//-/_}"
        normalized_cc="CC_${normalized}"
        normalized_ar="AR_${normalized}"
        if [[ -n "${!normalized_cc-}" && -n "${!normalized_ar-}" ]]; then
            supported_targets+=("$target")
            continue
        fi

        echo "== skipping $target =="
        echo "no target C toolchain is configured for $target; set ${normalized_cc} and ${normalized_ar}, or use a supported built-in path"
        continue
    fi

    echo "== skipping $target =="
    echo "toolchain $toolchain does not ship rust-std for $target; validate it on a native host instead"
done

if [[ ${#supported_targets[@]} -eq 0 ]]; then
    exit 0
fi

rustup target add --toolchain "$toolchain" "${supported_targets[@]}"

for target in "${supported_targets[@]}"; do
    echo "== cargo check --tests --target $target =="
    normalized="${target//-/_}"
    normalized_cc="CC_${normalized}"
    normalized_ar="AR_${normalized}"

    if zig_target="$(zig_target_for "$target" 2>/dev/null)" && command -v zig >/dev/null 2>&1; then
        env \
            "${normalized_cc}=zig cc -target ${zig_target}" \
            "${normalized_ar}=zig ar" \
            cargo +"$toolchain" check --tests --target "$target"
    else
        cargo +"$toolchain" check --tests --target "$target"
    fi
done
