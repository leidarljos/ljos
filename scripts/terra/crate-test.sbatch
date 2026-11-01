#!/usr/bin/env bash
#SBATCH --job-name=crate-test
#SBATCH --time=00:45:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
# fmt, clippy and the tests of one Rust checkout on the build host:
#   sbatch crate-test.sbatch ~/Git/Github/OmniPotentRPC/rgmin [cargo test args]
set -uo pipefail
repo=${1:?repository path}; shift || true
cd "$repo" || exit 1
echo "== $(basename "$repo") $(git log --oneline -1 | cut -c1-70)"
git pull -q --ff-only 2>&1 | tail -1
cargo fmt --all --check && echo FMT-CLEAN
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)" -A 8 | head -30
cargo test --locked --workspace --no-fail-fast "$@" 2>&1 | grep -E "^(test result:|failures:|---- )|panicked|assertion|left:|right:|stalled|error(\[|:)" -A2 | head -60
echo CRATE-TEST-DONE
