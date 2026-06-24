#!/usr/bin/env bash
set -euo pipefail

root="${HERDR_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$root"

# Nix-provided Rust on macOS can pass -liconv without an SDK-visible
# libiconv. If a Nix libiconv is installed, expose it to the linker for local
# plugin development and GitHub plugin installs.
if [[ "$(uname -s)" == "Darwin" ]]; then
  iconv_lib="$(find /nix/store -maxdepth 3 -path '*/lib/libiconv.dylib' -print 2>/dev/null | sort | tail -n 1 || true)"
  if [[ -n "$iconv_lib" ]]; then
    iconv_dir="$(dirname "$iconv_lib")"
    export LIBRARY_PATH="$iconv_dir${LIBRARY_PATH:+:$LIBRARY_PATH}"
  fi
fi

exec cargo build "$@"
