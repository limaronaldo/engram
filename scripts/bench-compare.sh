#!/bin/bash
set -euo pipefail

# Benchmark Comparison Script
# Compares current benchmark results against a saved baseline.
#
# Usage:
#   ./scripts/bench-compare.sh [--name <baseline>] [--help]
#
# Examples:
#   ./scripts/bench-compare.sh                    # Compare against "main"
#   ./scripts/bench-compare.sh --name v0.7.0    # Compare against "v0.7.0"

BASELINE_NAME="main"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --help)
      echo "Usage: ./scripts/bench-compare.sh [--name <baseline>] [--help]"
      echo ""
      echo "Compares current benchmark results against a saved baseline."
      echo ""
      echo "Options:"
      echo "  --name <baseline>   Baseline to compare against (default: main)"
      echo "  --help              Show this help message"
      echo ""
      echo "Examples:"
      echo "  ./scripts/bench-compare.sh                # Compare against 'main'"
      echo "  ./scripts/bench-compare.sh --name v0.7.0 # Compare against 'v0.7.0'"
      exit 0
      ;;
    --name)
      BASELINE_NAME="$2"
      shift 2
      ;;
    *)
      echo "Error: Unknown option '$1'" >&2
      echo "Use --help for usage information." >&2
      exit 1
      ;;
  esac
done

echo "Running benchmarks and comparing against baseline '${BASELINE_NAME}'..."
echo ""

cargo bench -- --baseline "${BASELINE_NAME}"

echo ""
echo "=========================================="
echo "Benchmark Comparison Complete"
echo "=========================================="
echo ""
echo "How to interpret the results:"
echo ""
echo "  * [FASTER]  — Performance improved (positive change)"
echo "  * [SLOWER]  — Performance regressed (negative change)"
echo "  * [CHANGED] — Variance indicates a change, investigate further"
echo "  * [SIMILAR] — No significant change detected"
echo ""
echo "Results are saved to: target/criterion/"
echo "Review individual reports: target/criterion/{benchmark_name}/report/index.html"
echo ""
echo "To create a new baseline, use:"
echo "  ./scripts/bench-baseline.sh --name ${BASELINE_NAME}"
