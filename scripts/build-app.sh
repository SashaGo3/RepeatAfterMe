#!/usr/bin/env bash
# Builds the app and installer for this computer, with nothing in it about who built it or where:
# every source path the compilers record (for crash messages) is rewritten, this folder → "."
# and your home folder → "~". Without this, a build contains e.g. /Users/<you>/.cargo/....
#
#   scripts/build-app.sh                                    # app + installer for this computer
#   scripts/build-app.sh --bundles app                      # macOS: just the .app
#   scripts/build-app.sh --target universal-apple-darwin    # macOS: Apple Silicon + Intel
#
# The result is in target/release/bundle/ (or target/<target>/release/bundle/).
set -euo pipefail
cd "$(dirname "$0")/.."
ROOT="$PWD"
CARGO_DIR="${CARGO_HOME:-$HOME/.cargo}"

# Rust: the last matching rule wins, so the most specific folders come last
export RUSTFLAGS="${RUSTFLAGS:-} --remap-path-prefix=$HOME=~ --remap-path-prefix=$CARGO_DIR=~/.cargo --remap-path-prefix=$ROOT=."
# macOS: the linker leaves out debug info and local symbols (they hold the build folder's paths).
# Cargo's own `strip` needs a working rust-objcopy, which isn't always installed.
if [ "$(uname -s)" = Darwin ]; then
  RUSTFLAGS="$RUSTFLAGS -C link-arg=-Wl,-S,-x"
fi
# C/C++ (whisper.cpp): __FILE__ in its error messages
MAP="-ffile-prefix-map=$HOME=~ -ffile-prefix-map=$CARGO_DIR=~/.cargo -ffile-prefix-map=$ROOT=."
export CFLAGS="${CFLAGS:-} $MAP" CXXFLAGS="${CXXFLAGS:-} $MAP"

[ -s vendor/models/ggml-small.en-q5_1.bin ] || scripts/fetch-vendor.sh
[ -d ui/node_modules ] || npm --prefix ui ci

cd app
../ui/node_modules/.bin/tauri build "$@"

# check: no trace of this computer's user in what was built
cd "$ROOT"
leaks=$(find target -path '*/release/bundle/*' -type f \( -perm -u+x -o -name '*.dylib' -o -name '*.exe' \) -print0 2>/dev/null |
  xargs -0 grep -l -a -F "$HOME" 2>/dev/null || true)
if [ -n "$leaks" ]; then
  echo "Warning: these files still contain $HOME:" >&2
  echo "$leaks" >&2
  exit 1
fi
echo "Built. No trace of $USER in it."
