set -e
# Assemble each probe file and show what the reference made of it.
#
# The target comes from `orbistoun-gen target`, so this cannot drift from the generators
# that read the same constants.
GEN="cargo run -q -p orbistoun-gen --"
MCPU=$($GEN target mcpu)
MATTR=$($GEN target mattr)
TRIPLE=$($GEN target triple)
for f in tools/shader-fixtures/probes/*.s; do
  echo "=== $f ==="
  llvm-mc "-triple=$TRIPLE" -mcpu "$MCPU" "-mattr=$MATTR" -show-encoding "$f" 2>&1 | head -20
done
