# Benchmark Scripts

These scripts help manage Criterion benchmark baselines for tracking performance changes across versions and feature branches.

## Workflow

The typical workflow is:

1. **On main branch**: Save a baseline after merging performance-critical changes
2. **On feature branch**: Run comparisons to detect regressions before merging

This ensures you catch performance issues early and have a clear record of when performance characteristics changed.

## Scripts

### `bench-baseline.sh`

Saves a Criterion benchmark baseline with a given name.

```bash
# Save baseline as "main" (default)
./scripts/bench-baseline.sh

# Save baseline as "v0.7.0"
./scripts/bench-baseline.sh --name v0.7.0

# Show help
./scripts/bench-baseline.sh --help
```

The baseline is stored in `target/criterion/` and can be compared against later.

### `bench-compare.sh`

Compares current benchmark results against a saved baseline.

```bash
# Compare against "main" baseline (default)
./scripts/bench-compare.sh

# Compare against "v0.7.0" baseline
./scripts/bench-compare.sh --name v0.7.0

# Show help
./scripts/bench-compare.sh --help
```

Results are displayed in the console and saved to `target/criterion/`. Each benchmark has an HTML report at `target/criterion/{benchmark_name}/report/index.html`.

## Example Workflow

```bash
# 1. On main branch: save a baseline after a significant optimization
git checkout main
git pull origin main
./scripts/bench-baseline.sh --name main

# 2. Create a feature branch and make changes
git checkout -b feature/optimize-search
# ... make changes ...
cargo build --release

# 3. Compare against the main baseline
./scripts/bench-compare.sh --name main

# Review the output:
# - If [FASTER] for all key benchmarks, great! Ready to merge.
# - If [SLOWER], investigate the regression and fix.
# - If [CHANGED] with high variance, run again to confirm stability.
```

## Interpreting Results

Criterion reports comparison results with tags:

- **[FASTER]** — Performance improved (lower is better)
- **[SLOWER]** — Performance regressed (higher is worse)
- **[CHANGED]** — Significant variance but unclear direction
- **[SIMILAR]** — No statistically significant change detected

Results are saved in `target/criterion/` with detailed HTML reports for each benchmark.
