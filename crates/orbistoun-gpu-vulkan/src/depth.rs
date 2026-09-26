//! A draw's depth and stencil attachment, and the depth, stencil and cull state its pipeline is
//! built with.
//!
//! A guest depth target becomes a host attachment in the host's own format, sized to the colour
//! target and kept on the device per target across submissions. It never reads or writes the
//! guest's depth memory, whose tiling is unmeasured: it starts from a clear, takes each clear the
//! guest asks for, and otherwise keeps what the draws on it left. A guest that reads its depth
//! surface back sees what its own memory holds, not the host attachment.

use ash::vk;
use orbistoun_gpu::depth::{CullState, DepthStencilState, FrontFace, StencilRefMask};
use orbistoun_gpu::{CompareFunc, StencilOp};

use crate::compute::DispatchError;

/// The formats a depth attachment may take, best first: `Z_32_FLOAT`'s own precision with eight
/// stencil bits where the device combines the two, else depth alone. A guest `Z_16` surface is held
/// at the wider precision.
const FORMATS: [vk::Format; 2] = [vk::Format::D32_SFLOAT_S8_UINT, vk::Format::D32_SFLOAT];

/// What a depth attachment holds before any clear reaches it: the far plane and a zero stencil,
/// a GL context's defaults. The guest's own contents are never read (see the module comment).
const INITIAL: DepthClear = DepthClear {
    depth: Some(1.0),
    stencil: Some(0),
};

/// A clear of a depth attachment's depth, stencil, or both.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct DepthClear {
    /// The depth every sample takes, when depth is cleared.
    pub(crate) depth: Option<f32>,
    /// The stencil every sample takes, when stencil is cleared.
    pub(crate) stencil: Option<u8>,
}

impl DepthClear {
    /// This clear followed by `later`: each aspect takes the later value where it has one.
    pub(crate) fn then(self, later: Self) -> Self {
        Self {
            depth: later.depth.or(self.depth),
            stencil: later.stencil.or(self.stencil),
        }
    }
}

/// How a draw uses its depth attachment: the tests it runs, or none when the stream set no depth
/// state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct DepthDraw {
    /// The guest's depth and stencil state.
    pub(crate) state: Option<DepthStencilState>,
}

/// Whether `format` has a stencil aspect.
fn has_stencil(format: vk::Format) -> bool {
    format == vk::Format::D32_SFLOAT_S8_UINT
}

/// The depth format this device renders to, found once: the first of [`FORMATS`] it offers as an
/// optimally tiled depth-stencil attachment.
fn depth_format(session: &crate::compute::Session) -> Option<vk::Format> {
    static FORMAT: std::sync::OnceLock<Option<vk::Format>> = std::sync::OnceLock::new();
    *FORMAT.get_or_init(|| {
        FORMATS.into_iter().find(|&format| {
            // SAFETY: the physical device is valid for the session's life.
            let properties = unsafe {
                session
                    .instance
                    .get_physical_device_format_properties(session.physical, format)
            };
            properties
                .optimal_tiling_features
                .contains(vk::FormatFeatureFlags::DEPTH_STENCIL_ATTACHMENT)
        })
    })
}

/// The aspects of a depth image in `format`.
fn aspects(format: vk::Format) -> vk::ImageAspectFlags {
    if has_stencil(format) {
        vk::ImageAspectFlags::DEPTH | vk::ImageAspectFlags::STENCIL
    } else {
        vk::ImageAspectFlags::DEPTH
    }
}

/// A depth attachment kept on the device, one per guest depth target.
pub(crate) struct DepthAttachment {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    format: vk::Format,
    width: u32,
    height: u32,
}

// By hand, because this build of `ash` gives `vk::Format` no `Debug`.
impl std::fmt::Debug for DepthAttachment {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DepthAttachment")
            .field("extent", &self.extent())
            .field("stencil", &self.has_stencil())
            .finish_non_exhaustive()
    }
}

impl DepthAttachment {
    /// Creates one of `width` x `height`, holding `clear` over [`INITIAL`], ready to draw on.
    ///
    /// # Errors
    ///
    /// When there is no device, it offers no depth format, or a Vulkan call fails.
    pub(crate) fn create(
        width: u32,
        height: u32,
        clear: Option<DepthClear>,
    ) -> Result<Self, DispatchError> {
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let format = depth_format(&session).ok_or_else(|| {
            DispatchError::Unsupported("a depth target on a device with no depth format".to_owned())
        })?;
        let (instance, physical, device) = (&session.instance, session.physical, &session.device);
        let info = vk::ImageCreateInfo::default()
            .image_type(vk::ImageType::TYPE_2D)
            .format(format)
            .extent(vk::Extent3D {
                width,
                height,
                depth: 1,
            })
            .mip_levels(1)
            .array_layers(1)
            .samples(vk::SampleCountFlags::TYPE_1)
            .tiling(vk::ImageTiling::OPTIMAL)
            // TRANSFER_DST for the clear it is created with.
            .usage(
                vk::ImageUsageFlags::DEPTH_STENCIL_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .initial_layout(vk::ImageLayout::UNDEFINED);
        // SAFETY: the device is live and the create info outlives the call.
        let image = unsafe { device.create_image(&info, None) }
            .map_err(|e| DispatchError::Vulkan("create_image(depth)", e))?;
        let memory = crate::framebuffer::bind_device_local(instance, physical, device, image)?;
        let range = vk::ImageSubresourceRange::default()
            .aspect_mask(aspects(format))
            .level_count(1)
            .layer_count(1);
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(format)
            .subresource_range(range);
        // SAFETY: the image is live and the create info outlives the call.
        let view = unsafe { device.create_image_view(&view_info, None) }
            .map_err(|e| DispatchError::Vulkan("create_image_view(depth)", e))?;
        let value = INITIAL.then(clear.unwrap_or(INITIAL));
        crate::framebuffer::one_shot(&session, |device, command| {
            initialise(device, command, image, range, value);
        })?;
        Ok(Self {
            image,
            memory,
            view,
            format,
            width,
            height,
        })
    }

    /// Its view, which a framebuffer over it names.
    pub(crate) const fn view(&self) -> vk::ImageView {
        self.view
    }

    /// Its format.
    pub(crate) const fn format(&self) -> vk::Format {
        self.format
    }

    /// Whether it holds stencil as well as depth.
    pub(crate) fn has_stencil(&self) -> bool {
        has_stencil(self.format)
    }

    /// The extent it was created at.
    pub(crate) const fn extent(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Destroys it. Every framebuffer naming its view must be destroyed first. Best-effort, like
    /// every release in this crate.
    pub(crate) fn destroy(self) {
        let Ok(session) = crate::compute::session() else {
            return;
        };
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let _ = crate::framebuffer::settle(&session);
        let device = &session.device;
        // SAFETY: created on this device, idle after the settle, destroyed exactly once, the view
        // before its image and the image before its memory.
        unsafe { device.destroy_image_view(self.view, None) };
        // SAFETY: as above.
        unsafe { device.destroy_image(self.image, None) };
        // SAFETY: as above; the image bound to it is destroyed.
        unsafe { device.free_memory(self.memory, None) };
    }
}

/// Records a new depth image's first contents: `value` in every sample, left in the layout a depth
/// pass expects.
fn initialise(
    device: &ash::Device,
    command: vk::CommandBuffer,
    image: vk::Image,
    range: vk::ImageSubresourceRange,
    value: DepthClear,
) {
    let barrier = |old, new, src_access, dst_access| {
        [vk::ImageMemoryBarrier::default()
            .src_access_mask(src_access)
            .dst_access_mask(dst_access)
            .old_layout(old)
            .new_layout(new)
            .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
            .image(image)
            .subresource_range(range)]
    };
    let to_transfer = barrier(
        vk::ImageLayout::UNDEFINED,
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        vk::AccessFlags::empty(),
        vk::AccessFlags::TRANSFER_WRITE,
    );
    // SAFETY: recording is open and the image is live.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_transfer,
        );
    }
    let clear = vk::ClearDepthStencilValue {
        depth: value.depth.unwrap_or(1.0),
        stencil: u32::from(value.stencil.unwrap_or(0)),
    };
    // SAFETY: recording is open and the image is in the transfer-destination layout.
    unsafe {
        device.cmd_clear_depth_stencil_image(
            command,
            image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &clear,
            &[range],
        );
    }
    let to_attachment = barrier(
        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL,
        vk::AccessFlags::TRANSFER_WRITE,
        vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ
            | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
    );
    // SAFETY: as above.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS
                | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_attachment,
        );
    }
}

/// A render pass over a colour attachment in `colour` and a depth attachment in `depth`, both
/// loaded and stored in their attachment layouts: the resident colour pass with depth beside it.
///
/// # Errors
///
/// When the device refuses it.
pub(crate) fn create_render_pass(
    device: &ash::Device,
    colour: vk::Format,
    depth: vk::Format,
) -> Result<vk::RenderPass, DispatchError> {
    let stencil_op = if has_stencil(depth) {
        (vk::AttachmentLoadOp::LOAD, vk::AttachmentStoreOp::STORE)
    } else {
        (
            vk::AttachmentLoadOp::DONT_CARE,
            vk::AttachmentStoreOp::DONT_CARE,
        )
    };
    let attachments = [
        vk::AttachmentDescription::default()
            .format(colour)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
            .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
            .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
            .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL),
        vk::AttachmentDescription::default()
            .format(depth)
            .samples(vk::SampleCountFlags::TYPE_1)
            .load_op(vk::AttachmentLoadOp::LOAD)
            .store_op(vk::AttachmentStoreOp::STORE)
            .stencil_load_op(stencil_op.0)
            .stencil_store_op(stencil_op.1)
            .initial_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL)
            .final_layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL),
    ];
    let colour_references = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let depth_reference = vk::AttachmentReference::default()
        .attachment(1)
        .layout(vk::ImageLayout::DEPTH_STENCIL_ATTACHMENT_OPTIMAL);
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&colour_references)
        .depth_stencil_attachment(&depth_reference)];
    // As the colour-only resident pass orders itself, with the depth tests' stages and accesses.
    let tests =
        vk::PipelineStageFlags::EARLY_FRAGMENT_TESTS | vk::PipelineStageFlags::LATE_FRAGMENT_TESTS;
    let stages =
        vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::TRANSFER | tests;
    let writes = vk::AccessFlags::COLOR_ATTACHMENT_WRITE
        | vk::AccessFlags::TRANSFER_WRITE
        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE;
    let uses = writes
        | vk::AccessFlags::COLOR_ATTACHMENT_READ
        | vk::AccessFlags::TRANSFER_READ
        | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_READ;
    let dependencies = [
        vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(stages)
            .src_access_mask(writes)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | tests)
            .dst_access_mask(uses),
        vk::SubpassDependency::default()
            .src_subpass(0)
            .dst_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | tests)
            .src_access_mask(
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE
                    | vk::AccessFlags::DEPTH_STENCIL_ATTACHMENT_WRITE,
            )
            .dst_stage_mask(stages)
            .dst_access_mask(uses),
    ];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    // SAFETY: every slice above outlives the call and the device is live.
    unsafe { device.create_render_pass(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_render_pass(depth)", e))
}

/// Records a clear of the open depth pass's depth attachment over the whole `extent`.
pub(crate) fn record_clear(
    device: &ash::Device,
    command: vk::CommandBuffer,
    (width, height): (u32, u32),
    clear: DepthClear,
) {
    let mut aspect = vk::ImageAspectFlags::empty();
    if clear.depth.is_some() {
        aspect |= vk::ImageAspectFlags::DEPTH;
    }
    if clear.stencil.is_some() {
        aspect |= vk::ImageAspectFlags::STENCIL;
    }
    if aspect.is_empty() {
        return;
    }
    let attachments = [vk::ClearAttachment {
        aspect_mask: aspect,
        color_attachment: 0,
        clear_value: vk::ClearValue {
            depth_stencil: vk::ClearDepthStencilValue {
                depth: clear.depth.unwrap_or(1.0),
                stencil: u32::from(clear.stencil.unwrap_or(0)),
            },
        },
    }];
    let rects = [vk::ClearRect {
        rect: vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width, height },
        },
        base_array_layer: 0,
        layer_count: 1,
    }];
    // SAFETY: recording is open inside a depth pass whose render area is `extent`, and the clear
    // names only aspects the pass's depth attachment has (a stencil clear is refused before it
    // reaches a depth-only attachment).
    unsafe { device.cmd_clear_attachments(command, &attachments, &rects) };
}

/// A `CompareFrag` comparison as Vulkan's; the two enumerate the same eight.
const fn compare_op(func: CompareFunc) -> vk::CompareOp {
    match func {
        CompareFunc::Never => vk::CompareOp::NEVER,
        CompareFunc::Less => vk::CompareOp::LESS,
        CompareFunc::Equal => vk::CompareOp::EQUAL,
        CompareFunc::LessEqual => vk::CompareOp::LESS_OR_EQUAL,
        CompareFunc::Greater => vk::CompareOp::GREATER,
        CompareFunc::NotEqual => vk::CompareOp::NOT_EQUAL,
        CompareFunc::GreaterEqual => vk::CompareOp::GREATER_OR_EQUAL,
        CompareFunc::Always => vk::CompareOp::ALWAYS,
    }
}

/// A `StencilOp` as Vulkan's, or the name of what has no exact equivalent.
///
/// The add and subtract operations step by `STENCILOPVAL`, which Vulkan fixes at one (radeonsi sets
/// one, `si_state.c:1329`); `REPLACE_OP` writes `STENCILOPVAL` where Vulkan writes the reference.
/// The logic operations and `ONES` have no Vulkan form.
fn stencil_op(op: StencilOp, face: StencilRefMask) -> Result<vk::StencilOp, &'static str> {
    let stepped = |vk_op| {
        if face.op_value == 1 {
            Ok(vk_op)
        } else {
            Err("a stencil increment or decrement by other than one")
        }
    };
    match op {
        StencilOp::Keep => Ok(vk::StencilOp::KEEP),
        StencilOp::Zero => Ok(vk::StencilOp::ZERO),
        StencilOp::ReplaceTest => Ok(vk::StencilOp::REPLACE),
        StencilOp::ReplaceOp if face.op_value == face.reference => Ok(vk::StencilOp::REPLACE),
        StencilOp::ReplaceOp => Err("a stencil replace by a value other than the reference"),
        StencilOp::Invert => Ok(vk::StencilOp::INVERT),
        StencilOp::AddClamp => stepped(vk::StencilOp::INCREMENT_AND_CLAMP),
        StencilOp::SubClamp => stepped(vk::StencilOp::DECREMENT_AND_CLAMP),
        StencilOp::AddWrap => stepped(vk::StencilOp::INCREMENT_AND_WRAP),
        StencilOp::SubWrap => stepped(vk::StencilOp::DECREMENT_AND_WRAP),
        StencilOp::Ones
        | StencilOp::And
        | StencilOp::Or
        | StencilOp::Xor
        | StencilOp::Nand
        | StencilOp::Nor
        | StencilOp::Xnor => Err("a stencil operation with no Vulkan equivalent"),
    }
}

/// One face's Vulkan stencil state from its comparison, three operations and reference and masks.
fn face_state(
    func: CompareFunc,
    (fail, pass, depth_fail): (StencilOp, StencilOp, StencilOp),
    face: StencilRefMask,
) -> Result<vk::StencilOpState, &'static str> {
    Ok(vk::StencilOpState {
        fail_op: stencil_op(fail, face)?,
        pass_op: stencil_op(pass, face)?,
        depth_fail_op: stencil_op(depth_fail, face)?,
        compare_op: compare_op(func),
        compare_mask: u32::from(face.compare_mask),
        write_mask: u32::from(face.write_mask),
        reference: u32::from(face.reference),
    })
}

/// The depth-stencil state a guest's registers ask for, or the name of what they ask for that has
/// no exact Vulkan form. `None` - no depth state set - runs no test and writes nothing.
///
/// With `BACKFACE_ENABLE` clear the hardware gives back faces the front's stencil state
/// (`DB_DEPTH_CONTROL`); the depth-bounds test is refused, its bounds registers not being decoded.
///
/// # Errors
///
/// The name of the first part of the state with no exact Vulkan equivalent.
pub(crate) fn depth_stencil_state(
    state: Option<DepthStencilState>,
) -> Result<vk::PipelineDepthStencilStateCreateInfo<'static>, &'static str> {
    let off = vk::PipelineDepthStencilStateCreateInfo::default();
    let Some(DepthStencilState {
        control,
        ops,
        front,
        back,
    }) = state
    else {
        return Ok(off);
    };
    if control.depth_bounds_test_enable {
        return Err("a depth-bounds test (its bounds are not decoded)");
    }
    // A stencil test that is off leaves its operations unused, so they are not refused.
    let (front_state, back_state) = if control.stencil_test_enable {
        let front_state = face_state(
            control.stencil_func,
            (ops.fail_op, ops.depth_pass_op, ops.depth_fail_op),
            front,
        )?;
        let back_state = if control.backface_enable {
            face_state(
                control.stencil_func_backface,
                (
                    ops.back_fail_op,
                    ops.back_depth_pass_op,
                    ops.back_depth_fail_op,
                ),
                back,
            )?
        } else {
            front_state
        };
        (front_state, back_state)
    } else {
        (vk::StencilOpState::default(), vk::StencilOpState::default())
    };
    Ok(off
        .depth_test_enable(control.depth_test_enable)
        .depth_write_enable(control.depth_write_enable)
        .depth_compare_op(compare_op(control.depth_func))
        .stencil_test_enable(control.stencil_test_enable)
        .front(front_state)
        .back(back_state))
}

/// Whether a draw with `state` can run on `depth`, or the name of what it asks for that cannot.
///
/// # Errors
///
/// The name of the first part of the state that has no exact form here.
pub(crate) fn check(
    state: Option<DepthStencilState>,
    depth: &DepthAttachment,
) -> Result<(), &'static str> {
    let Some(state) = state else {
        return Ok(());
    };
    if state.control.stencil_test_enable && !depth.has_stencil() {
        return Err("a stencil test on a device with no combined depth-stencil format");
    }
    depth_stencil_state(Some(state)).map(|_| ())
}

/// The Vulkan cull mode and front face a guest's `PA_SU_SC_MODE_CNTL` asks for; `None` culls
/// nothing.
///
/// Both APIs decide a triangle's facing from its winding in window coordinates after the viewport
/// transform, and the host viewport mirrors the guest's (a negative height for its y flip), so
/// `FACE` maps straight across.
pub(crate) fn rasterisation(cull: Option<CullState>) -> (vk::CullModeFlags, vk::FrontFace) {
    let Some(cull) = cull else {
        return (vk::CullModeFlags::NONE, vk::FrontFace::COUNTER_CLOCKWISE);
    };
    let mut mode = vk::CullModeFlags::NONE;
    if cull.cull_front {
        mode |= vk::CullModeFlags::FRONT;
    }
    if cull.cull_back {
        mode |= vk::CullModeFlags::BACK;
    }
    let face = match cull.front_face {
        FrontFace::CounterClockwise => vk::FrontFace::COUNTER_CLOCKWISE,
        FrontFace::Clockwise => vk::FrontFace::CLOCKWISE,
    };
    (mode, face)
}

#[cfg(test)]
mod tests {
    use super::{DepthClear, depth_stencil_state, rasterisation, stencil_op};
    use ash::vk;
    use orbistoun_gpu::StencilOp;
    use orbistoun_gpu::depth::{
        DepthStencilState, decode_cull, decode_stencil_ref_mask, depth_stencil_from,
    };

    fn state(values: &[(u32, u32)]) -> DepthStencilState {
        depth_stencil_from(|register| values.iter().find(|(r, _)| *r == register).map(|(_, v)| *v))
            .expect("DB_DEPTH_CONTROL is written")
    }

    /// A depth test on with writes and `LESS` maps to Vulkan's; no state is no test.
    #[test]
    fn the_depth_test_maps_across() {
        let on = depth_stencil_state(Some(state(&[(0xA200, 0x16)]))).expect("maps");
        assert_eq!(on.depth_test_enable, vk::TRUE);
        assert_eq!(on.depth_write_enable, vk::TRUE);
        assert_eq!(on.depth_compare_op.as_raw(), vk::CompareOp::LESS.as_raw());
        assert_eq!(on.stencil_test_enable, vk::FALSE);
        let off = depth_stencil_state(None).expect("maps");
        assert_eq!(off.depth_test_enable, vk::FALSE);
        assert_eq!(off.depth_write_enable, vk::FALSE);
    }

    /// Two-sided stencil takes the back registers; one-sided gives the back face the front's.
    #[test]
    fn stencil_faces_follow_backface_enable() {
        // STENCIL_ENABLE, STENCILFUNC EQUAL (2 << 8), STENCILFUNC_BF NOTEQUAL (5 << 20); ops: front
        // ZPASS REPLACE_TEST (3 << 4), back ZPASS ADD_WRAP (8 << 16); references 7 and 9, op value 1.
        let two = [
            (0xA200, 1 | (2 << 8) | (1 << 7) | (5 << 20)),
            (0xA10B, (3 << 4) | (8 << 16)),
            (0xA10C, 0x0100_ff07),
            (0xA10D, 0x0100_ff09),
        ];
        let mapped = depth_stencil_state(Some(state(&two))).expect("maps");
        assert_eq!(
            mapped.front.compare_op.as_raw(),
            vk::CompareOp::EQUAL.as_raw()
        );
        assert_eq!(
            mapped.front.pass_op.as_raw(),
            vk::StencilOp::REPLACE.as_raw()
        );
        assert_eq!(mapped.front.reference, 7);
        assert_eq!(
            mapped.back.compare_op.as_raw(),
            vk::CompareOp::NOT_EQUAL.as_raw()
        );
        assert_eq!(
            mapped.back.pass_op.as_raw(),
            vk::StencilOp::INCREMENT_AND_WRAP.as_raw()
        );
        assert_eq!(mapped.back.reference, 9);

        let one = [
            (0xA200, 1 | (2 << 8)),
            (0xA10B, 3 << 4),
            (0xA10C, 0x0100_ff07),
        ];
        let mapped = depth_stencil_state(Some(state(&one))).expect("maps");
        assert_eq!(
            mapped.back.compare_op.as_raw(),
            mapped.front.compare_op.as_raw()
        );
        assert_eq!(mapped.back.pass_op.as_raw(), mapped.front.pass_op.as_raw());
        assert_eq!(mapped.back.reference, 7);
    }

    /// An operation Vulkan cannot express is named, not approximated.
    #[test]
    fn a_stencil_operation_without_an_equivalent_is_refused() {
        let face = decode_stencil_ref_mask(0x0100_ff07);
        assert!(stencil_op(StencilOp::Xor, face).is_err());
        assert!(stencil_op(StencilOp::Ones, face).is_err());
        assert_eq!(
            stencil_op(StencilOp::AddClamp, face).map(vk::StencilOp::as_raw),
            Ok(vk::StencilOp::INCREMENT_AND_CLAMP.as_raw())
        );
        let by_two = decode_stencil_ref_mask(0x0200_ff07);
        assert!(stencil_op(StencilOp::AddClamp, by_two).is_err());
        assert!(stencil_op(StencilOp::ReplaceOp, face).is_err());
        let same = decode_stencil_ref_mask(0x0700_ff07);
        assert_eq!(
            stencil_op(StencilOp::ReplaceOp, same).map(vk::StencilOp::as_raw),
            Ok(vk::StencilOp::REPLACE.as_raw())
        );
        assert!(
            depth_stencil_state(Some(state(&[(0xA200, 0x16 | 0x8)]))).is_err(),
            "the depth-bounds test"
        );
    }

    /// A rasterisation answer as numbers: `ash` implements `Debug` only when a workspace build
    /// turns its feature on, so the tests compare raw values.
    fn raw((cull, face): (vk::CullModeFlags, vk::FrontFace)) -> (u32, i32) {
        (cull.as_raw(), face.as_raw())
    }

    /// Cull bits and `FACE` map one to one; no cull state culls nothing.
    #[test]
    fn cull_state_maps_across() {
        assert_eq!(
            raw(rasterisation(Some(decode_cull(0x6)))),
            raw((vk::CullModeFlags::BACK, vk::FrontFace::CLOCKWISE))
        );
        assert_eq!(
            raw(rasterisation(Some(decode_cull(0x3)))),
            raw((
                vk::CullModeFlags::FRONT_AND_BACK,
                vk::FrontFace::COUNTER_CLOCKWISE
            ))
        );
        assert_eq!(
            rasterisation(None).0.as_raw(),
            vk::CullModeFlags::NONE.as_raw()
        );
    }

    /// A later clear replaces only the aspects it names.
    #[test]
    fn a_later_clear_overrides_per_aspect() {
        let first = DepthClear {
            depth: Some(1.0),
            stencil: Some(3),
        };
        let later = DepthClear {
            depth: Some(0.5),
            stencil: None,
        };
        assert_eq!(
            first.then(later),
            DepthClear {
                depth: Some(0.5),
                stencil: Some(3),
            }
        );
    }
}
