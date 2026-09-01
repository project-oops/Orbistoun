#!/usr/bin/env sh
# Builds the VM the table generators run in.
#
# The generators need a reference assembler and disassembler for the target GPU. Those
# are LLVM's, they are not available on every host this project is developed on, and
# until now the machine that had them was somebody's undocumented VM - which meant the
# tables in `crates/orbistoun-shader/data` could be *read* by anyone and *re-derived* by
# nobody. REFERENCES.md claims those tables are derived by experiment; this is the
# experiment.
#
#   sh tools/toolchain/setup.sh          # create it, or bring an existing one up
#   sh tools/toolchain/run.sh <command>  # run something inside it, from the repo root
#
# Costs a VM and about 4 GB of disk. Delete it with `multipass delete --purge` and the
# name below when you are done; nothing in the repository depends on it existing.
set -e

# Git Bash on Windows rewrites anything that looks like a Unix path into a Windows one
# before the program sees it, so the guest working directory arrives as
# `C:/Program Files/Git/home/...` and the command fails somewhere confusing. Off.
MSYS_NO_PATHCONV=1
MSYS2_ARG_CONV_EXCL='*'
export MSYS_NO_PATHCONV MSYS2_ARG_CONV_EXCL

NAME=orbistoun-build
MOUNT=/home/ubuntu/orbistoun
REPO=$(cd "$(dirname "$0")/../.." && pwd)

if ! command -v multipass >/dev/null 2>&1; then
  echo "multipass is not installed - see https://multipass.run" >&2
  exit 1
fi

if multipass info "$NAME" >/dev/null 2>&1; then
  echo "$NAME exists; starting it"
  multipass start "$NAME" 2>/dev/null || true
else
  echo "creating $NAME"
  multipass launch 24.04 --name "$NAME" --cpus 2 --memory 2G --disk 12G
fi

echo "installing the reference toolchain"
multipass exec "$NAME" -- sudo apt-get update -qq
multipass exec "$NAME" -- sudo apt-get install -y -qq llvm clang

# Rust, because the generators are Rust (D209). Not apt's: Ubuntu 24.04 ships 1.75 and this
# workspace is edition 2024, which needs 1.85.
#
# `CARGO_TARGET_DIR` matters and is not a preference. The repository is a mount, and a build
# script written there cannot be executed - the build fails with a bare "Permission denied"
# naming a path inside `target/`, which reads as a corrupt checkout rather than as a mount
# without exec permission.
echo "installing Rust (the generators are Rust; apt's is too old for edition 2024)"
multipass exec "$NAME" -- sh -c "command -v cargo >/dev/null 2>&1 || curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain stable"

# Mounted rather than copied, so a generated table lands straight back in the tree and
# there is no step where the two can differ.
multipass mount "$REPO" "$NAME:$MOUNT" 2>/dev/null || true

echo
echo "ready. The toolchain reports:"
multipass exec "$NAME" -- llvm-mc --version | head -3
multipass exec "$NAME" -- sh -lc 'rustc --version'
echo
echo "Build and run a generator with, from the repository root:"
echo "  sh tools/toolchain/run.sh env CARGO_TARGET_DIR=/tmp/orb-target cargo run --release -p orbistoun-gen -- operands"
