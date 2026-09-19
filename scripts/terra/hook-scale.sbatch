#!/usr/bin/env bash
#SBATCH --job-name=hook-scale
#SBATCH --time=01:00:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
# The seat's hook, search and a dry consolidation timed on a scratch pack of
# a thousand, five thousand and ten thousand distinct lessons.
set -uo pipefail
export PATH="$HOME/stackbin:$HOME/bin-out:$HOME/.local/bin:$PATH"
export PACKSET_PORT=8797 PACKSET_URL=http://127.0.0.1:8797 PACKSET_HOME="$HOME/.cache/ljos-hook-scale-pack"
export PACKSET_EMBED_MODEL=bge-small PACKSET_EMBED_CACHE="$HOME/.cache/packset-embed" PYTHONUNBUFFERED=1
rm -rf "$PACKSET_HOME"
packset ensure
ljos doctor | grep -E "^(ok|--)	(pack|encoder)" || true
uv run --script "$HOME/Git/Github/ljos/scripts/terra/hook_scale.py" --sizes 1000 --sizes 5000 --sizes 10000; rc=$?
packset stop >/dev/null 2>&1 || true
echo "scale exit $rc"; echo ONE-DONE
