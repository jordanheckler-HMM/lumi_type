#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_FILE="${1:-$ROOT_DIR/docs/runtime-benchmark.txt}"
SAMPLE_SECONDS="${2:-60}"
SAMPLE_INTERVAL="${3:-1}"
BINARY="${LUMI_BINARY:-$ROOT_DIR/target/debug/lumitype}"

if [[ ! -x "$BINARY" ]]; then
  echo "Binary not found at $BINARY"
  echo "Build it first with: cargo tauri dev --no-watch (or cargo run --manifest-path src-tauri/Cargo.toml)"
  exit 1
fi

mkdir -p "$(dirname "$OUT_FILE")"

echo "Starting LumiType benchmark for ${SAMPLE_SECONDS}s (interval ${SAMPLE_INTERVAL}s)"
"$BINARY" >/tmp/lumitype-bench.log 2>&1 &
APP_PID=$!

cleanup() {
  kill "$APP_PID" >/dev/null 2>&1 || true
}
trap cleanup EXIT

sleep 3

{
  echo "LumiType runtime benchmark"
  echo "date: $(date -u '+%Y-%m-%dT%H:%M:%SZ')"
  echo "binary: $BINARY"
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
  awk -F',' 'NR>7 && NF==4 {cpu+=$2; rss+=$3; n+=1; if($2>cpu_max) cpu_max=$2; if($3>rss_max) rss_max=$3} END {if(n>0) {printf("samples=%d\navg_cpu=%.2f\nmax_cpu=%.2f\navg_rss_mb=%.2f\nmax_rss_mb=%.2f\n", n, cpu/n, cpu_max, (rss/n)/1024, rss_max/1024)} else {print "no_samples_collected"}}' "$OUT_FILE"
} >> "$OUT_FILE"

echo "Benchmark written to $OUT_FILE"
