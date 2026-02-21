# LumiType Verification Playbook

This document defines concrete checks for the remaining acceptance criteria.

## 1) Runtime Asset Check

```bash
./scripts/verify-assets.sh
```

Expected:

- exits `0`
- all required files reported as `[ok]`

## 2) 10 Consecutive Dictation Stability

Automated FSM stress check is part of unit tests:

```bash
cargo test --manifest-path src-tauri/Cargo.toml ten_consecutive_dictations_rearm_without_invalid_state
```

Expected:

- test passes

## 3) Local Runtime CPU/Memory Sampling

```bash
./scripts/bench-runtime.sh ./docs/runtime-benchmark.txt 60 1
```

Expected target thresholds:

- `avg_cpu` near or below idle target (`<6%` when idle)
- `max_rss_mb` below memory target (`<300MB`)

## 4) Manual End-to-End Checks

Run app:

```bash
cargo tauri dev --no-watch
```

Then verify manually:

1. Say wake phrase `Hey Lumi` and confirm overlay appears under menu bar.
2. Confirm mirrored waveform animates with microphone intensity.
3. Dictate into a normal text field; text should stream while speaking.
4. Stop speaking for ~1 second and confirm dictation stops and overlay fades.
5. Press `Esc` during dictation and verify current injected session is rolled back.
6. Press `Cmd+Option+Z` after a completed dictation and verify last injected block is removed.
7. Focus a secure input/password field and verify no text is injected.

## 5) Latency Measurement (Manual)

Use a stopwatch/video capture:

1. Measure wake phrase end -> overlay visible (`<75ms` target).
2. Measure speech start -> first injected characters (`<500ms` target).
3. Measure speech stop -> dictation stop event (~1.0s timeout).
