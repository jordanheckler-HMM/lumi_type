#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
MODEL_DIR="${LUMI_MODEL_DIR:-$ROOT_DIR/src-tauri/models}"
PORCUPINE_DYLIB="${LUMI_PORCUPINE_DYLIB:-/opt/homebrew/lib/libpv_porcupine.dylib}"

missing=0
fallback_keyword=0

check_file() {
  local path="$1"
  local label="$2"

  if [[ -f "$path" ]]; then
    echo "[ok] $label: $path"
  else
    echo "[missing] $label: $path"
    missing=1
  fi
}

check_file "$MODEL_DIR/ggml-base.en.bin" "Whisper base.en"
check_file "$MODEL_DIR/ggml-tiny.en.bin" "Whisper tiny.en"
check_file "$MODEL_DIR/porcupine_params.pv" "Porcupine params"

if [[ -f "$MODEL_DIR/hey-lumi-mac.ppn" ]]; then
  echo "[ok] Porcupine wake keyword (Hey Lumi): $MODEL_DIR/hey-lumi-mac.ppn"
elif [[ -f "$MODEL_DIR/porcupine_mac.ppn" ]]; then
  echo "[warn] Hey Lumi keyword missing; fallback keyword present: $MODEL_DIR/porcupine_mac.ppn"
  fallback_keyword=1
else
  echo "[missing] Porcupine wake keyword: $MODEL_DIR/hey-lumi-mac.ppn"
  echo "[missing] Porcupine fallback keyword: $MODEL_DIR/porcupine_mac.ppn"
  missing=1
fi

check_file "$PORCUPINE_DYLIB" "Porcupine dynamic library"

if [[ "$missing" -ne 0 ]]; then
  echo
  echo "One or more required runtime assets are missing."
  echo "Populate files and rerun scripts/verify-assets.sh."
  exit 1
fi

echo

if [[ "$fallback_keyword" -eq 1 ]]; then
  echo "Runtime assets are present with fallback wake word only."
  echo "For production \"Hey Lumi\", replace with a custom keyword file at:"
  echo "  $MODEL_DIR/hey-lumi-mac.ppn"
else
  echo "All required runtime assets are present."
fi
