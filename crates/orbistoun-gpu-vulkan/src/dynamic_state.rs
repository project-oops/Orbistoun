//! Draw state set while recording rather than built into the pipeline (D995).
//!
//! A draw's device pipeline is keyed by its shape (D856), and every piece of state in that key
//! multiplies the pipelines a shader pair is built into. What Vulkan can set on the command buffer
//! is left out of the key and set after each bind instead: the viewport, scissor, line width, depth
//! bias, blend constants, stencil masks and reference, and depth bounds always (core dynamic
//! state); cull mode, front face and the depth and stencil tests where the device has
//! `VK_EXT_extended_dynamic_state`; the depth-bias switch where it has `_state2`; and each colour
//! attachment's blend switch, equation and write mask where it has `_state3`'s three features.
//! Whatever the device cannot set stays in the key ([`Offered`]).

use ash::vk;

/// The most colour attachments a draw writes: the guest's eight colour targets.
pub(crate) const MAX_ATTACHMENTS: usize = 8;

/// Which extended dynamic states the session's device was created with.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Offered {
    /// `VK_EXT_extended_dynamic_state`: cull mode, front face, the depth and stencil tests.
    pub(crate) tests: bool,
    /// `VK_EXT_extended_dynamic_state2`: the depth-bias switch.
    pub(crate) bias_enable: bool,
    /// `VK_EXT_extended_dynamic_state3`'s blend switch, equation and write mask, all three.
    pub(crate) blend: bool,
}

/// What a device offers of the three extensions, from its extension list and features.
pub(crate) fn offered(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    extensions: &[vk::ExtensionProperties],
) -> Offered {
    let named = |name| crate::compute::names(extensions, name);
    let (first, second, third) = (
        named(ash::ext::extended_dynamic_state::NAME),
        named(ash::ext::extended_dynamic_state2::NAME),
        named(ash::ext::extended_dynamic_state3::NAME),
    );
    let mut tests = vk::PhysicalDeviceExtendedDynamicStateFeaturesEXT::default();
    let mut bias = vk::PhysicalDeviceExtendedDynamicState2FeaturesEXT::default();
    let mut blend = vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT::default();
    let mut chained = vk::PhysicalDeviceFeatures2::default();
    if first {
        chained = chained.push_next(&mut tests);
    }
    if second {
        chained = chained.push_next(&mut bias);
    }
    if third {
        chained = chained.push_next(&mut blend);
    }
    // SAFETY: the handle came from enumeration on this live instance, each chained structure is
    // one its extension, which the device offers, defines, and all outlive the call.
    unsafe { instance.get_physical_device_features2(physical, &mut chained) };
    Offered {
        tests: first && tests.extended_dynamic_state == vk::TRUE,
        bias_enable: second && bias.extended_dynamic_state2 == vk::TRUE,
        blend: third
            && blend.extended_dynamic_state3_color_blend_enable == vk::TRUE
            && blend.extended_dynamic_state3_color_blend_equation == vk::TRUE
            && blend.extended_dynamic_state3_color_write_mask == vk::TRUE,
    }
}

/// The extensions `offered` enables, by name.
pub(crate) fn extension_names(offered: Offered) -> [(bool, *const core::ffi::c_char); 3] {
    [
        (
            offered.tests,
            ash::ext::extended_dynamic_state::NAME.as_ptr(),
        ),
        (
            offered.bias_enable,
            ash::ext::extended_dynamic_state2::NAME.as_ptr(),
        ),
        (
            offered.blend,
            ash::ext::extended_dynamic_state3::NAME.as_ptr(),
        ),
    ]
}

/// The feature structures that enable what `offered` names, for the device's create info.
fn features(
    offered: Offered,
) -> (
    vk::PhysicalDeviceExtendedDynamicStateFeaturesEXT<'static>,
    vk::PhysicalDeviceExtendedDynamicState2FeaturesEXT<'static>,
    vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT<'static>,
) {
    (
        vk::PhysicalDeviceExtendedDynamicStateFeaturesEXT::default()
            .extended_dynamic_state(offered.tests),
        vk::PhysicalDeviceExtendedDynamicState2FeaturesEXT::default()
            .extended_dynamic_state2(offered.bias_enable),
        vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT::default()
            .extended_dynamic_state3_color_blend_enable(offered.blend)
            .extended_dynamic_state3_color_blend_equation(offered.blend)
            .extended_dynamic_state3_color_write_mask(offered.blend),
    )
}

/// The session device's extended dynamic states and the commands that set them.
struct Enabled {
    offered: Offered,
    tests: Option<ash::ext::extended_dynamic_state::Device>,
    bias: Option<ash::ext::extended_dynamic_state2::Device>,
    blend: Option<ash::ext::extended_dynamic_state3::Device>,
}

static ENABLED: std::sync::OnceLock<Enabled> = std::sync::OnceLock::new();

/// Records that the session's `device` was created with what `offered` names, and loads the
/// commands that set it.
pub(crate) fn note_enabled(offered: Offered, instance: &ash::Instance, device: &ash::Device) {
    let _ = ENABLED.set(Enabled {
        offered,
        tests: offered
            .tests
            .then(|| ash::ext::extended_dynamic_state::Device::new(instance, device)),
        bias: offered
            .bias_enable
            .then(|| ash::ext::extended_dynamic_state2::Device::new(instance, device)),
        blend: offered
            .blend
            .then(|| ash::ext::extended_dynamic_state3::Device::new(instance, device)),
    });
}

/// The extended dynamic states the session's device has; none before it opens.
pub(crate) fn enabled() -> Offered {
    ENABLED.get().map_or_else(Offered::default, |e| e.offered)
}

/// The states a draw's pipeline leaves to the command buffer on this device.
pub(crate) fn pipeline_states() -> Vec<vk::DynamicState> {
    let offered = enabled();
    let mut states = vec![
        vk::DynamicState::VIEWPORT,
        vk::DynamicState::SCISSOR,
        vk::DynamicState::LINE_WIDTH,
        vk::DynamicState::DEPTH_BIAS,
        vk::DynamicState::BLEND_CONSTANTS,
        vk::DynamicState::DEPTH_BOUNDS,
        vk::DynamicState::STENCIL_COMPARE_MASK,
        vk::DynamicState::STENCIL_WRITE_MASK,
        vk::DynamicState::STENCIL_REFERENCE,
    ];
    if offered.tests {
        states.extend([
            vk::DynamicState::CULL_MODE_EXT,
            vk::DynamicState::FRONT_FACE_EXT,
            vk::DynamicState::DEPTH_TEST_ENABLE_EXT,
            vk::DynamicState::DEPTH_WRITE_ENABLE_EXT,
            vk::DynamicState::DEPTH_COMPARE_OP_EXT,
            vk::DynamicState::DEPTH_BOUNDS_TEST_ENABLE_EXT,
            vk::DynamicState::STENCIL_TEST_ENABLE_EXT,
            vk::DynamicState::STENCIL_OP_EXT,
        ]);
    }
    if offered.bias_enable {
        states.push(vk::DynamicState::DEPTH_BIAS_ENABLE_EXT);
    }
    if offered.blend {
        states.extend([
            vk::DynamicState::COLOR_BLEND_ENABLE_EXT,
            vk::DynamicState::COLOR_BLEND_EQUATION_EXT,
            vk::DynamicState::COLOR_WRITE_MASK_EXT,
        ]);
    }
    states
}

/// A draw's state that its pipeline leaves to the command buffer, as Vulkan takes it. Set after
/// every bind of the pipeline, so a pipeline shared by draws of differing state draws each with its
/// own; what the device cannot set is the same in all of them, because it is in their key.
#[derive(Clone, Copy)]
pub(crate) struct DrawState {
    /// The viewport, in drawn pixels.
    pub(crate) viewport: vk::Viewport,
    /// The rectangle drawn into, within the attachment.
    pub(crate) scissor: vk::Rect2D,
    /// The width lines are drawn at.
    pub(crate) line_width: f32,
    /// Whether the depth bias applies, and its constant factor, clamp and slope factor.
    pub(crate) bias: (bool, [f32; 3]),
    /// The constants a constant-colour blend factor reads.
    pub(crate) blend_constants: [f32; 4],
    /// The faces culled, and which winding faces front.
    pub(crate) cull: (vk::CullModeFlags, vk::FrontFace),
    /// The depth and stencil tests; all off for a pass without a depth attachment.
    pub(crate) depth: vk::PipelineDepthStencilStateCreateInfo<'static>,
    /// Each colour attachment's blend state, the first [`Self::attachments`] of them.
    pub(crate) blend: [vk::PipelineColorBlendAttachmentState; MAX_ATTACHMENTS],
    /// How many colour attachments the pass has.
    pub(crate) attachments: usize,
}

impl std::fmt::Debug for DrawState {
    // The Vulkan structures it holds do not print themselves, so it says what tells draws apart.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let v = &self.viewport;
        f.debug_struct("DrawState")
            .field(
                "viewport",
                &[v.x, v.y, v.width, v.height, v.min_depth, v.max_depth],
            )
            .field("line_width", &self.line_width)
            .field("bias", &self.bias)
            .field("blend_constants", &self.blend_constants)
            .field("attachments", &self.attachments)
            .finish_non_exhaustive()
    }
}

/// Sets `state` on `command`, for the graphics pipeline just bound there.
pub(crate) fn record(device: &ash::Device, command: vk::CommandBuffer, state: &DrawState) {
    let Some(loaded) = ENABLED.get() else {
        return;
    };
    record_core(device, command, state);
    if let Some(tests) = &loaded.tests {
        record_tests(tests, command, state);
    }
    if let Some(bias) = &loaded.bias {
        // SAFETY: recording is open and the bound pipeline was built with the depth-bias switch
        // dynamic, as this device has its extension.
        unsafe { bias.cmd_set_depth_bias_enable(command, state.bias.0) };
    }
    if let Some(blend) = &loaded.blend {
        record_blend(blend, command, state);
    }
}

// Each `SAFETY` below rests on the same invariant: recording is open on `command`, a graphics
// pipeline built with the state dynamic is bound, and every value was checked against the device's
// features before that pipeline was built.

/// Sets the core dynamic states, which every draw pipeline leaves to the command buffer.
fn record_core(device: &ash::Device, command: vk::CommandBuffer, state: &DrawState) {
    let depth = &state.depth;
    let [constant, clamp, slope] = if state.bias.0 { state.bias.1 } else { [0.0; 3] };
    // SAFETY: see above.
    unsafe { device.cmd_set_viewport(command, 0, &[state.viewport]) };
    // SAFETY: see above; the scissor lies within the attachment.
    unsafe { device.cmd_set_scissor(command, 0, &[state.scissor]) };
    // SAFETY: see above; the width is one pixel unless the device has `wideLines`.
    unsafe { device.cmd_set_line_width(command, state.line_width) };
    // SAFETY: see above; the clamp is zero unless the device has `depthBiasClamp`.
    unsafe { device.cmd_set_depth_bias(command, constant, clamp, slope) };
    // SAFETY: see above.
    unsafe { device.cmd_set_blend_constants(command, &state.blend_constants) };
    // SAFETY: see above; the bounds were clamped to `0..1`.
    unsafe { device.cmd_set_depth_bounds(command, depth.min_depth_bounds, depth.max_depth_bounds) };
    for (face, ops) in [
        (vk::StencilFaceFlags::FRONT, depth.front),
        (vk::StencilFaceFlags::BACK, depth.back),
    ] {
        // SAFETY: see above.
        unsafe { device.cmd_set_stencil_compare_mask(command, face, ops.compare_mask) };
        // SAFETY: see above.
        unsafe { device.cmd_set_stencil_write_mask(command, face, ops.write_mask) };
        // SAFETY: see above.
        unsafe { device.cmd_set_stencil_reference(command, face, ops.reference) };
    }
}

/// Sets cull mode, front face and the depth and stencil tests.
fn record_tests(
    tests: &ash::ext::extended_dynamic_state::Device,
    command: vk::CommandBuffer,
    state: &DrawState,
) {
    let depth = &state.depth;
    // SAFETY: see above.
    unsafe { tests.cmd_set_cull_mode(command, state.cull.0) };
    // SAFETY: see above.
    unsafe { tests.cmd_set_front_face(command, state.cull.1) };
    // SAFETY: see above.
    unsafe { tests.cmd_set_depth_test_enable(command, depth.depth_test_enable == vk::TRUE) };
    // SAFETY: see above.
    unsafe { tests.cmd_set_depth_write_enable(command, depth.depth_write_enable == vk::TRUE) };
    // SAFETY: see above.
    unsafe { tests.cmd_set_depth_compare_op(command, depth.depth_compare_op) };
    let bounds = depth.depth_bounds_test_enable == vk::TRUE;
    // SAFETY: see above; a bounds test is on only where the device has `depthBounds`.
    unsafe { tests.cmd_set_depth_bounds_test_enable(command, bounds) };
    let stencil = depth.stencil_test_enable == vk::TRUE;
    // SAFETY: see above; a stencil test is on only for a target with a stencil aspect.
    unsafe { tests.cmd_set_stencil_test_enable(command, stencil) };
    for (face, ops) in [
        (vk::StencilFaceFlags::FRONT, depth.front),
        (vk::StencilFaceFlags::BACK, depth.back),
    ] {
        // SAFETY: see above.
        unsafe {
            tests.cmd_set_stencil_op(
                command,
                face,
                ops.fail_op,
                ops.pass_op,
                ops.depth_fail_op,
                ops.compare_op,
            );
        }
    }
}

/// Sets each colour attachment's blend switch, equation and write mask.
fn record_blend(
    blend: &ash::ext::extended_dynamic_state3::Device,
    command: vk::CommandBuffer,
    state: &DrawState,
) {
    let attachments = &state.blend[..state.attachments.min(MAX_ATTACHMENTS)];
    let switches: Vec<vk::Bool32> = attachments.iter().map(|a| a.blend_enable).collect();
    let equations: Vec<vk::ColorBlendEquationEXT> = attachments
        .iter()
        .map(|a| vk::ColorBlendEquationEXT {
            src_color_blend_factor: a.src_color_blend_factor,
            dst_color_blend_factor: a.dst_color_blend_factor,
            color_blend_op: a.color_blend_op,
            src_alpha_blend_factor: a.src_alpha_blend_factor,
            dst_alpha_blend_factor: a.dst_alpha_blend_factor,
            alpha_blend_op: a.alpha_blend_op,
        })
        .collect();
    let masks: Vec<vk::ColorComponentFlags> =
        attachments.iter().map(|a| a.color_write_mask).collect();
    // SAFETY: see above; one entry for each of the pass's colour attachments.
    unsafe { blend.cmd_set_color_blend_enable(command, 0, &switches) };
    // SAFETY: as for the switches.
    unsafe { blend.cmd_set_color_blend_equation(command, 0, &equations) };
    // SAFETY: as for the switches.
    unsafe { blend.cmd_set_color_write_mask(command, 0, &masks) };
}

/// The extended dynamic states' features to enable, held where the device's create info can point.
pub(crate) struct Wanted {
    tests: vk::PhysicalDeviceExtendedDynamicStateFeaturesEXT<'static>,
    bias: vk::PhysicalDeviceExtendedDynamicState2FeaturesEXT<'static>,
    blend: vk::PhysicalDeviceExtendedDynamicState3FeaturesEXT<'static>,
}

impl Wanted {
    /// What enables `offered`.
    pub(crate) fn of(offered: Offered) -> Self {
        let (tests, bias, blend) = features(offered);
        Self { tests, bias, blend }
    }

    /// `info` with the features of each extension `offered` enables chained on, and only those.
    pub(crate) fn chained<'a>(
        &'a mut self,
        info: vk::DeviceCreateInfo<'a>,
        offered: Offered,
    ) -> vk::DeviceCreateInfo<'a> {
        let info = if offered.tests {
            info.push_next(&mut self.tests)
        } else {
            info
        };
        let info = if offered.bias_enable {
            info.push_next(&mut self.bias)
        } else {
            info
        };
        if offered.blend {
            info.push_next(&mut self.blend)
        } else {
            info
        }
    }
}
