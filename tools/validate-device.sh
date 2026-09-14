#!/bin/sh
# Runs the device tests under the Khronos validation layer.
#
# The tests themselves only ask a driver for an answer, and a driver is entitled to give a
# correct-looking one for a module it should have refused. That is not hypothetical here: the
# console's pixel shader drew the right colour while producing five validation errors - two
# SPIR-V capabilities the device had never been asked for, a pipeline layout that did not
# declare what the shader used, a fragment stage writing a storage buffer without the feature
# that permits it, and a draw with no descriptor set bound (worklog 554). Every one of them
# was invisible to the tests, and every one was a real fault in this repository.
#
#   sh tools/validate-device.sh                       # every device test
#   sh tools/validate-device.sh -p orbistoun-gpu-vulkan --test console_fragment
#
# Needs the Vulkan SDK's layer. It is not a build dependency and CI has no device anyway, so
# this is something a person runs when they have changed what the emitter declares, what the
# device is created with, or what a pipeline binds.
set -e
cd "$(dirname "$0")/.."

# Where the layer manifest lives. Overridable, because an SDK is somebody's own install.
LAYER_PATH=${LAYER_PATH:-X:\\toolchains\\VulkanSDK\\1.4.357.0\\Bin}

export VK_LAYER_PATH="$LAYER_PATH"
export VK_LOADER_LAYERS_ENABLE=VK_LAYER_KHRONOS_validation
export VK_LAYER_KHRONOS_VALIDATION_DEBUG_ACTION=VK_DBG_LAYER_ACTION_LOG_MSG
export VK_LAYER_KHRONOS_VALIDATION_LOG_FILENAME=stdout

# Default to the tests that put a module on a device. Anything passed through goes to cargo
# instead, so one test can be run on its own while chasing a message.
if [ $# -eq 0 ]; then
  set -- -p orbistoun-gpu-vulkan
fi

# **One test is meant to produce a validation error and is skipped here.**
# `a_malformed_module_is_rejected_rather_than_run` hands the driver a module containing a
# deliberate nonsense opcode and asserts it is refused - so the layer reports it every time,
# correctly, and a verdict that counted it could never reach zero. Named rather than filtered
# by message, so skipping it is a decision somebody can see rather than a pattern that quietly
# swallows a real error that happens to look similar.
EXPECTED_FAILURE=a_malformed_module_is_rejected_rather_than_run

output=$(cargo test "$@" -- --nocapture --skip "$EXPECTED_FAILURE" 2>&1) || {
  printf '%s\n' "$output"
  echo "the tests themselves failed - fix that before reading the layer" >&2
  exit 1
}
printf '%s\n' "$output"

# **The layer's verdict is the point, and it does not fail the test run on its own**, so it is
# read out of the output here. A run that says nothing is the one to want.
if printf '%s\n' "$output" | grep -q "Validation Error"; then
  echo
  echo "validation errors above - the driver ran it anyway, which is what makes them easy to miss" >&2
  exit 1
fi
echo
echo "no validation errors (skipped $EXPECTED_FAILURE, which exists to cause one)"
