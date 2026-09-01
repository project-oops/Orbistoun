#!/bin/sh
# Regenerates the shader decoder fixtures. Needs LLVM with the AMDGPU backend, which
# is why it is a separate step rather than part of the build - the committed fixtures
# are what the tests read.
#
# On this machine, from PowerShell:
#   multipass exec obscene-build -- sh /home/ubuntu/orbistoun/tools/shader-fixtures/generate.sh
set -e
cd "$(dirname "$0")/../.."
exec cargo run -q -p orbistoun-gen -- fixtures
