#!/bin/bash
# NeoDOS Boot Stress Test — runs N QEMU TCG boots and classifies failures
# Usage: stress_boot.sh [count=100] [timeout_per_boot=30]

set -euo pipefail

COUNT=${1:-100}
TIMEOUT=${2:-30}
PASS=0
FAIL=0
RESULTS_DIR="/tmp/neodos-stress-$(date +%Y%m%d-%H%M%S)"
mkdir -p "$RESULTS_DIR"

echo "=== NeoDOS Boot Stress Test ==="
echo "Boots:     $COUNT"
echo "Timeout:   ${TIMEOUT}s"
echo "Results:   $RESULTS_DIR"
echo ""

run_one_boot() {
    local boot_num=$1
    local logfile="$RESULTS_DIR/boot-$(printf '%04d' $boot_num).log"
    local serialfile="$RESULTS_DIR/serial-$(printf '%04d' $boot_num).txt"

    # Use timeout to prevent hangs
    timeout $TIMEOUT \
        neodev run 2>&1 | tee "$serialfile" > "$logfile" || true

    # Analyze result
    local last_progress=""
    local last_sched_switch=""
    local last_thread=""
    local last_lock=""

    # Extract BOOT_PROGRESS markers
    last_progress=$(grep -oP '\[BOOT_PROGRESS\] \K.*' "$serialfile" | tail -1 || echo "NONE")

    # Extract SCHED_SWITCH traces
    last_sched_switch=$(grep -oP '\[SCHED\] SWITCH.*' "$serialfile" | tail -1 || echo "NONE")

    # Extract panic info
    local panic=""
    if grep -q '!!! KERNEL PANIC' "$serialfile"; then
        panic=$(grep -oP '!!! KERNEL PANIC \(CLASS: \K[^)]*' "$serialfile" | head -1 || echo "UNKNOWN")
    fi

    # Determine outcome
    local outcome=""
    if grep -q 'NeoShell\|NeoInit\|SHELL_READY\|shell started\|NeoDOS' "$serialfile"; then
        outcome="PASS"
        echo "PASS" > "$RESULTS_DIR/boot-$(printf '%04d' $boot_num).result"
    elif grep -q '!!! KERNEL PANIC' "$serialfile"; then
        outcome="PANIC:$panic"
        echo "PANIC:$panic" > "$RESULTS_DIR/boot-$(printf '%04d' $boot_num).result"
    elif grep -q 'DOUBLE FAULT\|TRIPLE FAULT\|#DF\|#PF' "$serialfile"; then
        outcome="CRASH"
        echo "CRASH" > "$RESULTS_DIR/boot-$(printf '%04d' $boot_num).result"
    elif timeout EXITCODE; then
        outcome="TIMEOUT"
        echo "TIMEOUT" > "$RESULTS_DIR/boot-$(printf '%04d' $boot_num).result"
    else
        outcome="HANG"
        echo "HANG" > "$RESULTS_DIR/boot-$(printf '%04d' $boot_num).result"
    fi

    # Write summary
    cat >> "$RESULTS_DIR/boot-$(printf '%04d' $boot_num).result" << EOF2
progress=$last_progress
sched_switch=$last_sched_switch
EOF2

    echo "$outcome"
}

echo "Running $COUNT boot iterations..."
for i in $(seq 1 $COUNT); do
    printf "[%04d/%04d] " $i $COUNT
    result=$(run_one_boot $i)

    case "$result" in
        PASS)
            echo "PASS"
            PASS=$((PASS + 1))
            ;;
        PANIC:*)
            echo "${result#PANIC:}"
            FAIL=$((FAIL + 1))
            echo "    FAIL: ${result#PANIC:}" >> "$RESULTS_DIR/summary.txt"
            echo "    boot #$i" >> "$RESULTS_DIR/summary.txt"
            tail -5 "$RESULTS_DIR/serial-$(printf '%04d' $i).txt" >> "$RESULTS_DIR/summary.txt"
            echo "" >> "$RESULTS_DIR/summary.txt"
            ;;
        *)
            echo "$result"
            FAIL=$((FAIL + 1))
            echo "    $result" >> "$RESULTS_DIR/summary.txt"
            echo "    boot #$i" >> "$RESULTS_DIR/summary.txt"
            tail -5 "$RESULTS_DIR/serial-$(printf '%04d' $i).txt" >> "$RESULTS_DIR/summary.txt"
            echo "" >> "$RESULTS_DIR/summary.txt"
            ;;
    esac
done

# Generate final report
echo ""
echo "=== RESULTS ==="
echo "Pass:  $PASS/$COUNT"
echo "Fail:  $FAIL/$COUNT"
echo ""

if [ $FAIL -gt 0 ]; then
    echo "=== FAILURE CLASSIFICATION ==="
    echo ""
    echo "Analyzing $FAIL failures..."

    # Group by last BOOT_PROGRESS
    echo "--- By Boot Progress ---"
    grep 'progress=' "$RESULTS_DIR"/*.result | sort | uniq -c | sort -rn || true

    echo ""
    echo "--- By Panic Class ---"
    grep 'PANIC:' "$RESULTS_DIR"/*.result | sort | uniq -c | sort -rn || true

    echo ""
    echo "--- By Scheduler State ---"
    grep 'sched_switch=' "$RESULTS_DIR"/*.result | sort | uniq -c | sort -rn || true

    echo ""
    echo "=== DETAILED FAILURE LOGS ==="
    echo "See: $RESULTS_DIR/summary.txt"
    cat "$RESULTS_DIR/summary.txt" || true
fi

echo ""
echo "Results saved to: $RESULTS_DIR"
