#!/usr/bin/env bash
#SBATCH --job-name=seat-stack
#SBATCH --time=01:00:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
# Build the habitats the smoke stands on (tracker, deed store, claim graph,
# policy daemon) on the build host and put the binaries in $HOME/stackbin,
# beside the pack's. Each repository is pulled first; a repository that
# fails to build is reported and the rest still build.
set -uo pipefail
mkdir -p "$HOME/stackbin"
declare -A bins=(
  [vissue]="vissue vissue-mcp"
  [deedar]="deedar deedar-mcp"
  [claimdag]="claimdag claimdag-mcp"
  [ljos-policyd]="ljos-policyd"
)
for r in vissue deedar claimdag ljos-policyd; do
  cd "$HOME/Git/Github/$r" || { echo "== $r: no checkout"; continue; }
  echo "== $r $(git log --oneline -1 2>/dev/null | cut -c1-70)"
  git pull -q --ff-only 2>&1 | tail -1
  cargo build --release 2>&1 | tail -1
  for b in ${bins[$r]}; do
    if [ -x "target/release/$b" ]; then cp -f "target/release/$b" "$HOME/stackbin/" && echo "copied $b"; else echo "missing $b"; fi
  done
done
ls -la "$HOME/stackbin" | tail -12
echo SEAT-STACK-DONE
