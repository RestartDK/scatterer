#!/usr/bin/env bash
set -euo pipefail

root="${HERDR_PLUGIN_ROOT:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
cd "$root"

# Nix-provided Rust on macOS may need an explicit libiconv path when this
# launcher falls back to `cargo run` during development.
if [[ "$(uname -s)" == "Darwin" ]]; then
  iconv_lib="$(find /nix/store -maxdepth 3 -path '*/lib/libiconv.dylib' -print 2>/dev/null | sort | tail -n 1 || true)"
  if [[ -n "$iconv_lib" ]]; then
    iconv_dir="$(dirname "$iconv_lib")"
    export LIBRARY_PATH="$iconv_dir${LIBRARY_PATH:+:$LIBRARY_PATH}"
  fi
fi

binary_is_fresh() {
  local binary="$1"
  [[ -x "$binary" ]] || return 1

  # If any source/manifest file is newer than the binary, do not run the stale
  # binary during local plugin development. `find -newer` works on macOS + Linux.
  local newer
  newer="$({ find src -type f -newer "$binary" -print -quit 2>/dev/null; find Cargo.toml Cargo.lock -newer "$binary" -print -quit 2>/dev/null; } | head -n 1)"
  [[ -z "$newer" ]]
}

if binary_is_fresh "$root/target/release/scatterer"; then
  exec "$root/target/release/scatterer" "$@"
fi

if binary_is_fresh "$root/target/debug/scatterer"; then
  exec "$root/target/debug/scatterer" "$@"
fi

exec cargo run --quiet -- "$@"
