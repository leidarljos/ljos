#!/usr/bin/env bash
#SBATCH --job-name=ebstack-build
#SBATCH --time=00:40:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
# Release build of eb-stack from its main branch into $HOME/stackbin, the
# binary the seat's demonstration drives; the laptop copies it from there.
set -uo pipefail
export TMPDIR=/var/lib/builds/tmp
export PATH="$HOME/.cargo/bin:$PATH"
# A clone of its own: the developer checkout under Tools carries local
# changes on another branch and is not this job's to move.
SRC="$HOME/Git/tmp/eb-stack-main"
if [ -d "$SRC/.git" ]; then
  cd "$SRC" && git fetch -q origin && git reset -q --hard origin/main
else
  git clone -q --branch main git@github.com:HaoZeke/eb-stack.git "$SRC" && cd "$SRC"
fi
echo "== eb-stack $(git log --oneline -1 | cut -c1-70)"
cargo build --release --locked 2>&1 | tail -2
mkdir -p "$HOME/stackbin" && cp -f target/release/eb-stack "$HOME/stackbin/" && echo "copied eb-stack $("$HOME/stackbin/eb-stack" --version)"
echo BUILD-DONE
