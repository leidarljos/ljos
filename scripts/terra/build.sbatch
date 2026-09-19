#!/usr/bin/env bash
#SBATCH --job-name=ljos-build
#SBATCH --time=00:30:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
set -uo pipefail
mkdir -p "$HOME/bin-out"
status=0
for r in ljos consensus; do
  cd "$HOME/Git/Github/$r" || continue
  echo "== $r"
  cargo fmt --all --check && echo FMT-CLEAN
  cargo clippy --workspace --all-targets -- -D warnings 2>&1 | grep -E "^(error|warning)" -A 8 | head -30
  cargo build --release > "$HOME/$r-build-${SLURM_JOB_ID}.log" 2>&1 || status=1
  tail -1 "$HOME/$r-build-${SLURM_JOB_ID}.log"
  cargo test --workspace --no-fail-fast > "$HOME/$r-test-${SLURM_JOB_ID}.log" 2>&1 || status=1
  grep -E "^(test result:|failures:|---- )|panicked|assertion|left:|right:" -A2 "$HOME/$r-test-${SLURM_JOB_ID}.log" | head -40
done
cp -f "$HOME/Git/Github/ljos/target/release/ljos" "$HOME/Git/Github/ljos/target/release/ljos-mcp" "$HOME/Git/Github/consensus/target/release/ljos-consensus" "$HOME/bin-out/" 2>&1
ls -la "$HOME/bin-out" | tail -4
echo ONE-DONE
# A chained job (--dependency=afterok) must not run on a red build.
exit $status
