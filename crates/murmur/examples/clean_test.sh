#!/bin/bash
# Kill all cli_dev_scope processes and clean up storage
# Usage: ./examples/clean_test.sh

set -e

echo "=== Murmur CLI Test Cleanup ==="

# Find and kill all cli_dev_scope processes
PIDS=$(pgrep -f "cli_dev_scope" 2>/dev/null || true)

if [ -n "$PIDS" ]; then
    echo "Killing cli_dev_scope processes:"
    for pid in $PIDS; do
        CMD=$(ps -p "$pid" -o args= 2>/dev/null || echo "(unknown)")
        echo "  PID $pid: $CMD"
        kill "$pid" 2>/dev/null || true
    done
    sleep 1
    # Force kill if still alive
    for pid in $PIDS; do
        if kill -0 "$pid" 2>/dev/null; then
            echo "  Force killing PID $pid"
            kill -9 "$pid" 2>/dev/null || true
        fi
    done
else
    echo "No cli_dev_scope processes found."
fi

# Clean up storage directories
echo ""
echo "Cleaning storage:"
DIRS=$(ls -d /tmp/murmur-cli-test-* 2>/dev/null || true)
if [ -n "$DIRS" ]; then
    for d in $DIRS; do
        echo "  rm -rf $d"
        rm -rf "$d"
    done
else
    echo "  No /tmp/murmur-cli-test-* directories found."
fi

# Also clean bridge storage
BRIDGE_STORE="$HOME/data0/public_work/imi/murmur-scope/.murmur-scope-data"
if [ -d "$BRIDGE_STORE" ]; then
    echo "  rm -rf $BRIDGE_STORE"
    rm -rf "$BRIDGE_STORE"
fi

echo ""
echo "Done. You can now start a fresh test:"
echo "  cargo run --example cli_dev_scope -- alice"
