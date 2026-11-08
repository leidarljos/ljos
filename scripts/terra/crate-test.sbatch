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
# Several test filters follow a double dash: `-p ljos --lib -- tracker_push projected_card`.
set -uo pipefail
# A job inherits the submitting shell's PATH; a non-interactive one lacks the
# user bins, and the tests shell age-keygen, git and tmux from there.
export PATH="$HOME/.local/bin:$HOME/.local/pkgs/usr/bin:$HOME/.cargo/bin:$PATH"
repo=${1:?repository path}; shift || true
cd "$repo" || exit 1
echo "== $(basename "$repo") $(git log --oneline -1 | cut -c1-70)"
git pull -q --ff-only 2>&1 | tail -1
cargo fmt --all --check && echo FMT-CLEAN
cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)" -A 8 | head -30
# The whole run goes to a log beside the job output; the summary below is
# not capped, since a cap once hid the one verdict that mattered.
log="crate-test-${SLURM_JOB_ID:-local}.log"
cargo test --locked --workspace --no-fail-fast "$@" > "$log" 2>&1
status=$?
grep -E "^(test result:|failures:|---- )|panicked|assertion|left:|right:|stalled|error(\[|:)" -A2 "$log" | grep -v "0 passed; 0 failed"
echo "cargo test exit=$status (full log: $log)"
echo CRATE-TEST-DONE
