#!/usr/bin/env bash
#SBATCH --job-name=ljos-herd
#SBATCH --time=00:20:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
set -uo pipefail
export PATH="$HOME/stackbin:$HOME/bin-out:$PATH"
export PACKSET_PORT=8792 PACKSET_URL=http://127.0.0.1:8792 PACKSET_HOME="$HOME/.cache/ljos-herd-pack"
export PACKSET_EMBED_MODEL=bge-small PACKSET_EMBED_CACHE="$HOME/.cache/packset-embed"
rm -rf "$PACKSET_HOME"
bash "$HOME/Git/Github/ljos/scripts/herd.sh"; rc=$?
packset stop >/dev/null 2>&1 || true
echo "herd exit $rc"; echo ONE-DONE
