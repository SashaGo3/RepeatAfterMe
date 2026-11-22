#!/usr/bin/env bash
# Downloads what the app bundles but git doesn't keep: the speech model and the PDF engine (pdfium).
#   scripts/fetch-vendor.sh            # for this computer
#   scripts/fetch-vendor.sh mac        # universal macOS (arm64 + x64), as the release uses
set -euo pipefail
cd "$(dirname "$0")/.."
PDFIUM_BUILD=8125   # must match pdfium-render's "pdfium_latest" feature (see core/Cargo.toml)
MODEL=ggml-small.en-q5_1.bin

mkdir -p vendor/models
if [ ! -s "vendor/models/$MODEL" ]; then
  echo "Speech model ($MODEL, ~190 MB)…"
  curl -fSL --retry 3 -o "vendor/models/$MODEL.part" "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$MODEL"
  mv "vendor/models/$MODEL.part" "vendor/models/$MODEL"
fi

pdfium() { # <platform name in pdfium-binaries> <folder>
  if [ ! -d "vendor/pdfium/$2" ]; then
    echo "pdfium $1…"
    mkdir -p "vendor/pdfium/$2"
    curl -fSL --retry 3 "https://github.com/bblanchon/pdfium-binaries/releases/download/chromium%2F$PDFIUM_BUILD/pdfium-$1.tgz" | tar xz -C "vendor/pdfium/$2"
  fi
}

target="${1:-}"
if [ -z "$target" ]; then
  case "$(uname -s)-$(uname -m)" in
    Darwin-*) target=mac ;;
    Linux-x86_64) target=linux-x64 ;;
    Linux-aarch64) target=linux-arm64 ;;
    MINGW*|MSYS*|CYGWIN*) target=win-x64 ;;
    *) echo "Unknown platform; pass one of: mac linux-x64 linux-arm64 win-x64" >&2; exit 1 ;;
  esac
fi
case "$target" in
  mac)
    pdfium mac-arm64 mac-arm64
    pdfium mac-x64 mac-x64
    mkdir -p vendor/pdfium/mac
    lipo -create vendor/pdfium/mac-arm64/lib/libpdfium.dylib vendor/pdfium/mac-x64/lib/libpdfium.dylib -output vendor/pdfium/mac/libpdfium.dylib
    ;;
  linux-x64) pdfium linux-x64 linux-x64 ;;
  linux-arm64) pdfium linux-arm64 linux-arm64 ;;
  win-x64) pdfium win-x64 win-x64 ;;
  *) echo "Unknown target $target" >&2; exit 1 ;;
esac
echo "vendor/ is ready."
