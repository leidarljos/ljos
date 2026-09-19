#!/usr/bin/env bash
#SBATCH --job-name=hook-scale
#SBATCH --time=01:00:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
# A timing run would own the node (--exclusive): shared with other work the
# fill reads two to three times slower. Three exclusive runs on 2026-09-19
# were cancelled by SIGTERM ten minutes in, from the sibling sessions whose
# jobs they held back, so the run shares the node and the write trace
# (per-stage milliseconds by pack size) carries the comparison instead.
#SBATCH --output=%x-%j.out
# The seat's hook, search and a dry consolidation timed on a scratch pack of
# a thousand, five thousand and ten thousand distinct lessons.
set -uo pipefail
export PATH="$HOME/stackbin:$HOME/bin-out:$HOME/.local/bin:$PATH"
# The port and the pack home carry the job id: two scale runs on one host
# must not share a daemon or wipe each other's pack.
port=$((8700 + ${SLURM_JOB_ID:-97} % 100))
export PACKSET_PORT=$port PACKSET_URL=http://127.0.0.1:$port PACKSET_HOME="$HOME/.cache/ljos-hook-scale-pack-${SLURM_JOB_ID:-local}"
export PACKSET_EMBED_MODEL=bge-small PACKSET_EMBED_CACHE="$HOME/.cache/packset-embed" PYTHONUNBUFFERED=1
rm -rf "$PACKSET_HOME"
# One stage-clock line per write, so a fill that slows with the pack says
# which stage grew.
export PACKSET_TRACE_WRITES="$HOME/hook-scale-writes-${SLURM_JOB_ID:-local}.log"
rm -f "$PACKSET_TRACE_WRITES"
packset ensure
ljos doctor | grep -E "^(ok|--)	(pack|encoder)" || true
uv run --script "$HOME/Git/Github/ljos/scripts/terra/hook_scale.py" --sizes 1000 --sizes 5000 --sizes 10000; rc=$?
packset stop >/dev/null 2>&1 || true
rm -rf "$PACKSET_HOME"
echo "scale exit $rc"; echo ONE-DONE
