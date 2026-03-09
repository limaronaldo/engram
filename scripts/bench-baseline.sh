#!/bin/bash
set -euo pipefail

# Benchmark Baseline Script
# Creates and saves a Criterion benchmark baseline for performance tracking.
#
# Usage:
#   ./scripts/bench-baseline.sh [--name <baseline>] [--help]
#
# Examples:
#   ./scripts/bench-baseline.sh                    # Save baseline as "main"
#   ./scripts/bench-baseline.sh --name v0.7.0    # Save baseline as "v0.7.0"

BASELINE_NAME="main"

# Parse arguments
while [[ $# -gt 0 ]]; do
  case "$1" in
    --help)
      echo "Usage: ./scripts/bench-baseline.sh [--name <baseline>] [--help]"
      echo ""
      echo "Saves a Criterion benchmark baseline for later comparison."
      echo ""
      echo "Options:"
      echo "  --name <baseline>   Override baseline name (default: main)"
      echo "  --help              Show this help message"
      echo ""
      echo "Examples:"
      echo "  ./scripts/bench-baseline.sh                # Save as 'main'"
      echo "  ./scripts/bench-baseline.sh --name v0.7.0 # Save as 'v0.7.0'"
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

echo "Saving benchmark baseline as '${BASELINE_NAME}'..."
echo ""

cargo bench -- --save-baseline "${BASELINE_NAME}"

echo ""
echo "Baseline '${BASELINE_NAME}' saved successfully!"
echo "You can now compare against this baseline using:"
echo "  ./scripts/bench-compare.sh --name ${BASELINE_NAME}"
