#!/usr/bin/env sh
# Runs a command inside the generator VM, from the repository root.
#
#   sh tools/toolchain/run.sh cargo run --release -p orbistoun-gen -- operands
#
# Fails rather than falling back to the host: a generator that silently ran against
# whatever assembler happened to be on PATH is how a table acquires an unrecorded
# provenance.
set -e

# Git Bash on Windows rewrites anything that looks like a Unix path into a Windows one
# before the program sees it, so the guest working directory arrives as
# `C:/Program Files/Git/home/...` and the command fails somewhere confusing. Off.
MSYS_NO_PATHCONV=1
MSYS2_ARG_CONV_EXCL='*'
export MSYS_NO_PATHCONV MSYS2_ARG_CONV_EXCL

NAME=orbistoun-build
MOUNT=/home/ubuntu/orbistoun

if ! multipass info "$NAME" >/dev/null 2>&1; then
  echo "$NAME does not exist - run tools/toolchain/setup.sh first" >&2
  exit 1
fi

multipass exec "$NAME" --working-directory "$MOUNT" -- "$@"
