#!/bin/sh
# Generate product Rust LCOV using integration targets only.
set -eu

report=${1:?SONAR_RUST_REPORT_MISSING}
command -v cargo >/dev/null 2>&1 || {
  printf '%s\n' 'SONAR_CARGO_MISSING' >&2
  exit 1
}
mkdir -p "$(dirname "$report")"
export CARGO_BUILD_JOBS=${CARGO_BUILD_JOBS:-1}

cargo llvm-cov --locked -p koduck-ai --all-features \
  --test cand_11_correction_admission \
  --test cand_12_projection \
  --test postgres_cand_11 \
  --lcov --output-path "$report" -- --test-threads=3

[ -s "$report" ] && grep -q '^SF:' "$report" && \
  grep -Eq '^DA:[0-9]+,[1-9][0-9]*' "$report" || {
  printf '%s\n' 'SONAR_RUST_COVERAGE_INVALID' >&2
  exit 1
}
