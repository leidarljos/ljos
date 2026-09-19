#!/usr/bin/env bash
#SBATCH --job-name=phronesis-build
#SBATCH --time=00:40:00
#SBATCH --nodes=1
#SBATCH --ntasks-per-node=1
#SBATCH --cpus-per-task=4
#SBATCH --mem=8G
#SBATCH --partition=cpu
#SBATCH --output=%x-%j.out
# Build phronesis, the policy library the seat's argv law stands on, into
# $HOME/phronesis-prefix, so ljos-policyd links it (PHRONESIS_DIR) instead of
# its host argv table. cmocka is built first when the host has none; the
# Cap'n C runtime and capnp-janet come from phronesis's own wraps.
set -uo pipefail
PREFIX="$HOME/phronesis-prefix"
mkdir -p "$PREFIX" "$HOME/build"
export PKG_CONFIG_PATH="$PREFIX/lib/pkgconfig:$PREFIX/lib64/pkgconfig:${PKG_CONFIG_PATH:-}"
if ! pkg-config --exists cmocka; then
  echo "== cmocka"
  cd "$HOME/build" && { [ -d cmocka ] || git clone -q --depth 1 --branch cmocka-1.1.7 https://gitlab.com/cmocka/cmocka.git; }
  cmake -S cmocka -B cmocka/build -DCMAKE_INSTALL_PREFIX="$PREFIX" -DCMAKE_BUILD_TYPE=Release -DWITH_EXAMPLES=OFF >/dev/null && cmake --build cmocka/build -j4 >/dev/null && cmake --install cmocka/build >/dev/null && echo "cmocka installed"
fi
pkg-config --modversion cmocka || echo "cmocka still missing"
echo "== phronesis"
cd "$HOME/Git/Github/phronesis" || exit 1
git pull -q --ff-only 2>&1 | tail -1; git log --oneline -1
meson setup build --prefix="$PREFIX" --reconfigure 2>&1 | tail -3
meson compile -C build 2>&1 | tail -2
meson test -C build --print-errorlogs 2>&1 | grep -E "^Ok|^Fail|^Timeout|tests passed|FAIL" | head -5
meson install -C build 2>&1 | tail -1
ls "$PREFIX/lib" "$PREFIX/include/phronesis" "$PREFIX/share/phronesis" 2>&1 | head -20
echo PHRONESIS-DONE
