#!/bin/sh
# Runs the device tests under the Khronos validation layer.
#
# A driver can draw correctly from a module it should have refused, so the tests alone do not
# show that the emitter declares every capability, layout and binding a shader uses. The layer
# does.
#
#   LAYER_PATH=<the Vulkan SDK's Bin directory> sh tools/validate-device.sh
#   LAYER_PATH=... sh tools/validate-device.sh -p orbistoun-gpu-vulkan --test console_fragment
#
# Needs the Vulkan SDK's layer, which is not a build dependency; CI has no device. Run it after
# changing what the emitter declares, how the device is created, or what a pipeline binds.
set -e
cd "$(dirname "$0")/.."

# Where the layer manifest lives: the SDK is the user's own install.
LAYER_PATH=${LAYER_PATH:-${VK_LAYER_PATH:-}}
if [ -z "$LAYER_PATH" ]; then
  echo "set LAYER_PATH (or VK_LAYER_PATH) to the Vulkan SDK directory holding the layer manifest" >&2
  exit 2
fi

export VK_LAYER_PATH="$LAYER_PATH"
export VK_LOADER_LAYERS_ENABLE=VK_LAYER_KHRONOS_validation
export VK_LAYER_KHRONOS_VALIDATION_DEBUG_ACTION=VK_DBG_LAYER_ACTION_LOG_MSG
export VK_LAYER_KHRONOS_VALIDATION_LOG_FILENAME=stdout

# Default to the tests that put a module on a device. Anything passed through goes to cargo
# instead, so one test can be run on its own while chasing a message.
if [ $# -eq 0 ]; then
  set -- -p orbistoun-gpu-vulkan
fi

# One test hands the driver a module with a deliberate invalid opcode and asserts it is refused,
# so the layer always reports it. It is skipped by name, so no real error is filtered by message.
EXPECTED_FAILURE=a_malformed_module_is_rejected_rather_than_run

output=$(cargo test "$@" -- --nocapture --skip "$EXPECTED_FAILURE" 2>&1) || {
  printf '%s\n' "$output"
  echo "the tests themselves failed - fix that before reading the layer" >&2
  exit 1
}
printf '%s\n' "$output"

# The layer's verdict does not fail the test run, so it is read from the output.
if printf '%s\n' "$output" | grep -q "Validation Error"; then
  echo
  echo "validation errors above - the driver ran it anyway, which is what makes them easy to miss" >&2
  exit 1
fi
echo
echo "no validation errors (skipped $EXPECTED_FAILURE, which exists to cause one)"
