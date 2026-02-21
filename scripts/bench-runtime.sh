#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_FILE="${1:-$ROOT_DIR/docs/runtime-benchmark.txt}"
SAMPLE_SECONDS="${2:-60}"
SAMPLE_INTERVAL="${3:-1}"
BINARY="${LUMI_BINARY:-$ROOT_DIR/target/debug/lumitype}"
LOG_FILE="${LUMI_BENCH_LOG:-/tmp/lumitype-bench.log}"
ATTACH_EXISTING="${LUMI_ATTACH_EXISTING:-0}"

if [[ ! -x "$BINARY" ]]; then
  echo "Binary not found at $BINARY"
  echo "Build it first with: cargo tauri dev --no-watch (or cargo run --manifest-path src-tauri/Cargo.toml)"
  exit 1
fi

mkdir -p "$(dirname "$OUT_FILE")"

APP_PID=""
DRIVER_PID=""
LAUNCH_MODE="binary"
OWN_PROCESS=1

cleanup() {
  if [[ "$OWN_PROCESS" -ne 1 ]]; then
    return
  fi
  if [[ -n "$APP_PID" ]]; then
    kill "$APP_PID" >/dev/null 2>&1 || true
  fi
  if [[ -n "$DRIVER_PID" && "$DRIVER_PID" != "$APP_PID" ]]; then
    kill "$DRIVER_PID" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

launch_binary() {
  "$BINARY" >"$LOG_FILE" 2>&1 &
  APP_PID=$!
  DRIVER_PID=$APP_PID
  sleep 3
  ps -p "$APP_PID" -o pid= >/dev/null 2>&1
}

launch_via_tauri_dev() {
  LAUNCH_MODE="tauri_dev"
  (cd "$ROOT_DIR" && cargo tauri dev --no-watch >"$LOG_FILE" 2>&1) &
  DRIVER_PID=$!

  local tries=0
  while [[ "$tries" -lt 45 ]]; do
    APP_PID="$(pgrep -fn "$BINARY" || true)"
    if [[ -n "$APP_PID" ]] && ps -p "$APP_PID" -o pid= >/dev/null 2>&1; then
      sleep 3
      return 0
    fi
    tries=$((tries + 1))
    sleep 1
  done

  return 1
}

echo "Starting LumiType benchmark for ${SAMPLE_SECONDS}s (interval ${SAMPLE_INTERVAL}s)"

if [[ "$ATTACH_EXISTING" == "1" ]]; then
  APP_PID="$(pgrep -fn "$BINARY" || true)"
  if [[ -z "$APP_PID" ]]; then
    echo "No running LumiType process found for attach mode." >&2
    exit 1
  fi
  LAUNCH_MODE="attach_existing"
  OWN_PROCESS=0
else
  if ! launch_binary; then
    echo "Direct binary launch exited early; retrying with cargo tauri dev launcher" >&2
    if ! launch_via_tauri_dev; then
      echo "Failed to start LumiType for benchmarking. Check $LOG_FILE" >&2
      exit 1
    fi
  fi
fi

{
  echo "LumiType runtime benchmark"
  echo "date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "binary: $BINARY"
  echo "launch_mode: $LAUNCH_MODE"
  echo "sample_seconds: $SAMPLE_SECONDS"
  echo "sample_interval_seconds: $SAMPLE_INTERVAL"
  echo
  echo "timestamp,pcpu,rss_kb,vsz_kb"

  elapsed=0
  while [[ "$elapsed" -lt "$SAMPLE_SECONDS" ]]; do
    if ! ps -p "$APP_PID" -o pcpu= -o rss= -o vsz= >/dev/null 2>&1; then
      echo "process_exited_before_benchmark_complete"
      break
    fi

    stats="$(ps -p "$APP_PID" -o pcpu= -o rss= -o vsz= | awk '{$1=$1;print}')"
    now="$(date '+%H:%M:%S')"
    echo "${now},${stats// /,}"

    sleep "$SAMPLE_INTERVAL"
    elapsed=$((elapsed + SAMPLE_INTERVAL))
  done
} > "$OUT_FILE"

{
  echo
  echo "summary"
  awk -F',' 'NR>8 && NF==4 {cpu+=$2; rss+=$3; n+=1; if($2>cpu_max) cpu_max=$2; if($3>rss_max) rss_max=$3} END {if(n>0) {printf("samples=%d\navg_cpu=%.2f\nmax_cpu=%.2f\navg_rss_mb=%.2f\nmax_rss_mb=%.2f\n", n, cpu/n, cpu_max, (rss/n)/1024, rss_max/1024)} else {print "no_samples_collected"}}' "$OUT_FILE"
} >> "$OUT_FILE"

echo "Benchmark written to $OUT_FILE"

if grep -q 'no_samples_collected' "$OUT_FILE"; then
  echo "Benchmark failed to collect samples. Check $LOG_FILE" >&2
  exit 1
fi
