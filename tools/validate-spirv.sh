#!/bin/sh
# Asks a real validator whether the emitted modules are valid SPIR-V.
#
# The emitter's own tests check structure - magic word, bound, instruction packing -
# and cannot check validity, because a crate asserting that it likes its own output
# proves nothing. `spirv-val` is the oracle here, the same way `llvm-objdump` is for
# the shader decoder.
#
# Emitting and validating are split across two machines on purpose: the Rust toolchain
# lives on the host, the SPIR-V tools in the build VM, and neither needs the other
# installed. Emit first, then:
#
#   cargo run -q --example emit-minimal -p orbistoun-spirv -- target/spirv/minimal.spv
#   multipass exec obscene-build -- sh /home/ubuntu/orbistoun/tools/validate-spirv.sh
set -e
cd "$(dirname "$0")/.."
DIR=${DIR:-target/spirv}

if [ ! -d "$DIR" ]; then
  echo "no modules in $DIR - emit them first (see the note above)" >&2
  exit 1
fi

status=0
for module in "$DIR"/*.spv; do
  [ -e "$module" ] || { echo "no .spv files in $DIR" >&2; exit 1; }
  printf '%-28s ' "$(basename "$module")"
  if spirv-val "$module" 2>/tmp/spirv-val.err; then
    echo "valid"
  else
    echo "INVALID"
    sed 's/^/    /' /tmp/spirv-val.err
    status=1
  fi
done
exit $status
