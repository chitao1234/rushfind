#!/usr/bin/env bash
set -euo pipefail

args=()
for arg in "$@"; do
  case "$arg" in
    --target=x86_64-apple-macosx)
      args+=(-target x86_64-macos)
      ;;
    --target=arm64-apple-macosx)
      args+=(-target aarch64-macos)
      ;;
    *)
      args+=("$arg")
      ;;
  esac
done

exec zig cc "${args[@]}"
