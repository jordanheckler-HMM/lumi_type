#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="${LUMI_MODEL_DIR:-$ROOT_DIR/src-tauri/models}"
PORCUPINE_LIB_PATH="${LUMI_PORCUPINE_DYLIB:-/opt/homebrew/lib/libpv_porcupine.dylib}"
TMP_DIR="$(mktemp -d /tmp/lumitype-porcupine-XXXXXX)"

cleanup() {
  rm -rf "$TMP_DIR"
}
trap cleanup EXIT

mkdir -p "$MODEL_DIR"
mkdir -p "$(dirname "$PORCUPINE_LIB_PATH")"

git clone --depth 1 https://github.com/Picovoice/porcupine.git "$TMP_DIR/porcupine" >/dev/null 2>&1

cp "$TMP_DIR/porcupine/lib/common/porcupine_params.pv" "$MODEL_DIR/porcupine_params.pv"
cp "$TMP_DIR/porcupine/resources/keyword_files/mac/porcupine_mac.ppn" "$MODEL_DIR/porcupine_mac.ppn"
cp "$TMP_DIR/porcupine/lib/mac/arm64/libpv_porcupine.dylib" "$PORCUPINE_LIB_PATH"

echo "Installed Porcupine fallback assets:"
echo "  $MODEL_DIR/porcupine_params.pv"
echo "  $MODEL_DIR/porcupine_mac.ppn"
echo "  $PORCUPINE_LIB_PATH"
echo
echo "Next: generate a custom Hey Lumi keyword as $MODEL_DIR/hey-lumi-mac.ppn"
echo "using Picovoice Console for production wake phrase behavior."
