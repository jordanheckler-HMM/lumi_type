# LumiType

LumiType is a macOS-only, offline dictation app built with Tauri + Rust.

## Stack

- Wake word: Porcupine (Rust FFI binding in `wake_word.rs`)
- Audio capture: `cpal`
- VAD: `webrtc-vad`
- STT: `whisper-rs` (`whisper.cpp`, Metal enabled on macOS)
- Injection: `enigo`
- Runtime: Tokio + channel-driven finite state machine

## Prerequisites (Apple Silicon macOS)

```bash
brew install cmake pkg-config
rustup toolchain install stable
cargo install tauri-cli --locked
```

## Required model/runtime assets

Create and populate `src-tauri/models`:

```bash
mkdir -p src-tauri/models
```

### Whisper models

```bash
curl -L -o src-tauri/models/ggml-base.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin

curl -L -o src-tauri/models/ggml-tiny.en.bin \
  https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.en.bin
```

### Porcupine assets

Place these files in `src-tauri/models`:

- `porcupine_params.pv`
- `hey-lumi-mac.ppn` (custom keyword model for `"Hey Lumi"`)

Install Porcupine dynamic library at one of:

- `/opt/homebrew/lib/libpv_porcupine.dylib` (default Apple Silicon path)
- or set `LUMI_PORCUPINE_DYLIB` to your custom path.

Optional overrides:

```bash
export LUMI_MODEL_DIR="$(pwd)/src-tauri/models"
export LUMI_PORCUPINE_MODEL="$(pwd)/src-tauri/models/porcupine_params.pv"
export LUMI_PORCUPINE_KEYWORD="$(pwd)/src-tauri/models/hey-lumi-mac.ppn"
```

## Build and run

### Test

```bash
cargo test --manifest-path src-tauri/Cargo.toml
```

### Verify assets

```bash
./scripts/verify-assets.sh
```

### Benchmark runtime CPU/memory

```bash
./scripts/bench-runtime.sh ./docs/runtime-benchmark.txt 60 1
```

### Development

```bash
cargo tauri dev --no-watch
```

### Production build

```bash
cargo tauri build
```

## Privacy behavior

- No network calls in runtime path
- No audio persistence
- No transcript logging
- No analytics
