#!/usr/bin/env bash
# Builds equation.cpp against Mesa's addrlib from the oops-mesa submodule beside orbistoun.
#
#   tools/addrlib/build.sh            # builds target/addrlib/equation
#   target/addrlib/equation 64KB_R_X 8
#
# A local clang 21 on PATH, as for every C build in the collection (CONVENTIONS section 8). The
# addrlib sources are compiled as Mesa's own meson file lists them, with its flags.
set -euo pipefail

HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)
REPO=$(cd "$HERE/../.." && pwd)
ADDRLIB=${ORBISTOUN_ADDRLIB:-$REPO/../oops-mesa/mesa/src/amd/addrlib}
OUT=$REPO/target/addrlib

if [ ! -f "$ADDRLIB/inc/addrinterface.h" ]; then
    echo "no addrlib at $ADDRLIB - check out oops-mesa beside orbistoun, or set ORBISTOUN_ADDRLIB" >&2
    exit 1
fi
mkdir -p "$OUT"

sources=(
    src/addrinterface.cpp
    src/core/addrelemlib.cpp src/core/addrlib.cpp src/core/addrlib1.cpp src/core/addrlib2.cpp
    src/core/addrlib3.cpp src/core/addrobject.cpp src/core/addrswizzler.cpp src/core/coord.cpp
    src/gfx9/gfx9addrlib.cpp src/gfx10/gfx10addrlib.cpp src/gfx11/gfx11addrlib.cpp
    src/gfx12/gfx12addrlib.cpp
    src/r800/ciaddrlib.cpp src/r800/egbaddrlib.cpp src/r800/siaddrlib.cpp
)
includes=(inc src src/core src/chip/gfx9 src/chip/r800 src/chip/gfx10 src/chip/gfx11 src/chip/gfx12)

args=(-std=c++17 -O1 -DADDR_FASTCALL= -DLITTLEENDIAN_CPU -DDEBUG=0 -w)
for dir in "${includes[@]}"; do
    args+=(-I "$ADDRLIB/$dir")
done
for source in "${sources[@]}"; do
    args+=("$ADDRLIB/$source")
done

clang++ "${args[@]}" "$HERE/equation.cpp" -o "$OUT/equation"
echo "built $OUT/equation"
