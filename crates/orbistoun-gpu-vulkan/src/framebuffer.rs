//! Rendering into a colour attachment and reading the pixels back.
//!
//! # Why this exists, and why it is first
//!
//! The roadmap calls framebuffer diffing *the only cheap mechanical correctness oracle this
//! project will ever have*, and it is the one part of that sentence that was not built. Phase
//! 6's own ordering says to build the harness **before** anything it is meant to check, for a
//! reason worth repeating: everything else in that phase is verified against material this
//! project generated, so it cannot be wrong in a way its own tests would notice. A harness that
//! clears to a colour and reads that colour back is checked against *itself*, which makes it a
//! genuine oracle rather than one more self-consistent guess (D549).
//!
//! # What is here and what is not
//!
//! The attachment path: an image, a render pass, the layout transition, the copy to a
//! host-visible buffer, and the readback. **No draw and no shaders yet.** That is the next
//! step and it sits on top of this one - which is the risky half done first, because a
//! mistake in a layout transition or a copy region produces plausible pixels rather than an
//! error, and a draw built on an unverified copy would be debugged in the wrong place.
//!
//! # The same caveats as the compute path
//!
//! A missing device is reported, never quietly passed - see [`crate::compute::probe`].
//! Resources are released on the successful path only; an error abandons them, because this is
//! test infrastructure in a process that is about to exit.

use ash::vk;

use crate::compute::{DispatchBuffer, DispatchError};

/// What a cleared attachment came back as.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pixels {
    /// Width in pixels, as asked for.
    pub width: u32,
    /// Height in pixels, as asked for.
    pub height: u32,
    /// The bytes, tightly packed, four per pixel in the order the format names:
    /// `R8G8B8A8_UNORM`, so red first.
    pub bytes: Vec<u8>,
}

impl Pixels {
    /// The four bytes at `(x, y)`, or [`None`] outside the image.
    #[must_use]
    pub fn at(&self, x: u32, y: u32) -> Option<[u8; 4]> {
        if x >= self.width || y >= self.height {
            return None;
        }
        let start = ((y * self.width + x) * 4) as usize;
        self.bytes.get(start..start + 4)?.try_into().ok()
    }
}

/// The format every attachment here uses.
///
/// `R8G8B8A8_UNORM` because it is mandatory for colour attachment and blitting on every Vulkan
/// implementation, so a machine cannot fail this harness for want of a format. Eight bits a
/// channel also makes the expected bytes exact for the clear values used: `0.0` is `0` and
/// `1.0` is `255`, with no rounding to argue about.
const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// Creates a device-local image usable as a colour attachment and as a copy source.
fn create_attachment(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    width: u32,
    height: u32,
) -> Result<(vk::Image, vk::DeviceMemory), DispatchError> {
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        // TRANSFER_DST as well as SRC: a draw that starts from given pixels copies them in first
        // (worklog 822).
        .usage(
            vk::ImageUsageFlags::COLOR_ATTACHMENT
                | vk::ImageUsageFlags::TRANSFER_SRC
                | vk::ImageUsageFlags::TRANSFER_DST,
        )
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    // SAFETY: the device is live and the create info outlives the call.
    let image = unsafe { device.create_image(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image", e))?;

    // SAFETY: the image was created on this device and not yet destroyed.
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    // SAFETY: the physical device is valid.
    let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
    // Device-local: an optimally tiled image is not readable by the host whatever memory it
    // sits in, so the copy below is required regardless and there is nothing to gain by
    // asking for host-visible memory the driver may not have.
    let memory_type = (0..properties.memory_type_count)
        .find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[*i as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .ok_or(DispatchError::NoHostVisibleMemory)?;

    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    let memory = unsafe { device.allocate_memory(&allocate, None) }
        .map_err(|e| DispatchError::Vulkan("allocate_memory", e))?;
    // SAFETY: image and memory both come from this device, and the allocation is the size the
    // image asked for.
    unsafe { device.bind_image_memory(image, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_image_memory", e))?;
    Ok((image, memory))
}

/// A render pass with one colour attachment, cleared on load and kept.
///
/// The final layout is `TRANSFER_SRC_OPTIMAL`, so the render pass itself performs the
/// transition the copy needs and no separate barrier is recorded. The external dependency is
/// what makes that transition visible to the transfer stage - without it the copy may read the
/// image before the store has landed, which is the kind of error that produces *plausible*
/// pixels on one driver and correct ones on another.
///
/// **Or loaded, when the draw starts from given pixels** (`loads`, worklog 822): the attachment is
/// then copied in before the pass and arrives in `TRANSFER_DST_OPTIMAL`, which the pass takes as its
/// initial layout and loads rather than clears - so a target's earlier draws, or the guest's own
/// contents, are what this draw lands on. A second external dependency orders that copy before the
/// pass writes.
fn create_render_pass(device: &ash::Device, loads: bool) -> Result<vk::RenderPass, DispatchError> {
    let (load_op, initial_layout) = if loads {
        (
            vk::AttachmentLoadOp::LOAD,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
        )
    } else {
        (vk::AttachmentLoadOp::CLEAR, vk::ImageLayout::UNDEFINED)
    };
    let attachments = [vk::AttachmentDescription::default()
        .format(FORMAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(load_op)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(initial_layout)
        .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)];
    let references = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&references)];
    let before_copy_out = vk::SubpassDependency::default()
        .src_subpass(0)
        .dst_subpass(vk::SUBPASS_EXTERNAL)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
        .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
        .dst_access_mask(vk::AccessFlags::TRANSFER_READ);
    // The copy in, before the pass reads (loads) and writes the attachment.
    let after_copy_in = vk::SubpassDependency::default()
        .src_subpass(vk::SUBPASS_EXTERNAL)
        .dst_subpass(0)
        .src_stage_mask(vk::PipelineStageFlags::TRANSFER)
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .dst_access_mask(
            vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
        );
    let dependencies = [before_copy_out, after_copy_in];
    let dependencies = if loads {
        &dependencies[..]
    } else {
        &dependencies[..1]
    };
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(dependencies);
    // SAFETY: every slice above outlives the call and the device is live.
    unsafe { device.create_render_pass(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_render_pass", e))
}

/// The render pass a [`ResidentAttachment`] is drawn with: loaded and stored, starting and ending in
/// `COLOR_ATTACHMENT_OPTIMAL`, so consecutive draws land on each other with no copy between them
/// (worklog 839). The external dependencies order each pass after whatever earlier submission last
/// wrote the attachment (a previous draw, or the copy that seeded it) and before the readback copy.
fn create_resident_render_pass(device: &ash::Device) -> Result<vk::RenderPass, DispatchError> {
    let attachments = [vk::AttachmentDescription::default()
        .format(FORMAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::LOAD)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)
        .final_layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let references = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&references)];
    let writes = vk::AccessFlags::COLOR_ATTACHMENT_WRITE | vk::AccessFlags::TRANSFER_WRITE;
    let uses = vk::AccessFlags::COLOR_ATTACHMENT_READ
        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE
        | vk::AccessFlags::TRANSFER_READ;
    let stages = vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT | vk::PipelineStageFlags::TRANSFER;
    let dependencies = [
        vk::SubpassDependency::default()
            .src_subpass(vk::SUBPASS_EXTERNAL)
            .dst_subpass(0)
            .src_stage_mask(stages)
            .src_access_mask(writes)
            .dst_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .dst_access_mask(uses),
        vk::SubpassDependency::default()
            .src_subpass(0)
            .dst_subpass(vk::SUBPASS_EXTERNAL)
            .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
            .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
            .dst_stage_mask(stages)
            .dst_access_mask(uses),
    ];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    // SAFETY: every slice above outlives the call and the device is live.
    unsafe { device.create_render_pass(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_render_pass(resident)", e))
}

/// Records one command buffer with `body`, submits it, and waits for the device - for the resident
/// attachment's seeding and readback, which run outside any draw.
fn one_shot(
    session: &crate::compute::Session,
    body: impl FnOnce(&ash::Device, vk::CommandBuffer),
) -> Result<(), DispatchError> {
    let device = &session.device;
    // **The draws recorded so far go first** (worklog 844): whatever this does - read the attachment
    // back, seed it, set a pipeline up - comes after them in the stream the guest wrote.
    settle(session)?;
    let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(session.family);
    // SAFETY: the device is live and the create info outlives the call.
    let pool = unsafe { device.create_command_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool", e))?;
    let allocate = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: the pool was just created on this device.
    let command = unsafe { device.allocate_command_buffers(&allocate) }
        .map_err(|e| DispatchError::Vulkan("allocate_command_buffers", e))?[0];
    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: the command buffer was just allocated and is not recording.
    unsafe { device.begin_command_buffer(command, &begin) }
        .map_err(|e| DispatchError::Vulkan("begin_command_buffer", e))?;
    body(device, command);
    // SAFETY: recording is open.
    unsafe { device.end_command_buffer(command) }
        .map_err(|e| DispatchError::Vulkan("end_command_buffer", e))?;
    let buffers = [command];
    let submits = [vk::SubmitInfo::default().command_buffers(&buffers)];
    // SAFETY: recording has ended and the queue belongs to this device.
    unsafe { device.queue_submit(session.queue, &submits, vk::Fence::null()) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))?;
    // SAFETY: the device is live.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;
    // SAFETY: the device is idle, so nothing recorded from the pool is in use.
    unsafe { device.destroy_command_pool(pool, None) };
    Ok(())
}

/// [`one_shot`]'s recording, submitted **without waiting** (worklog 850): after the draws recorded
/// so far, in queue order, and finished whenever the device gets to it - the next [`settle`] waits
/// for it and frees its buffer, as it does for resident draws. For work nothing reads until then.
fn one_shot_unwaited(
    session: &crate::compute::Session,
    body: impl FnOnce(&ash::Device, vk::CommandBuffer),
) -> Result<(), DispatchError> {
    // The draws recorded so far are submitted first, so this follows them on the queue.
    close_open_pass(session)?;
    let device = &session.device;
    let pool = shared_pool(device, session.family)?;
    let allocate = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: the pool is live on this device, and the caller holds the session, so nothing else
    // allocates from it meanwhile.
    let command = unsafe { device.allocate_command_buffers(&allocate) }
        .map_err(|e| DispatchError::Vulkan("allocate_command_buffers", e))?[0];
    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: the command buffer was just allocated and is not recording.
    unsafe { device.begin_command_buffer(command, &begin) }
        .map_err(|e| DispatchError::Vulkan("begin_command_buffer", e))?;
    body(device, command);
    // SAFETY: recording is open.
    unsafe { device.end_command_buffer(command) }
        .map_err(|e| DispatchError::Vulkan("end_command_buffer", e))?;
    let buffers = [command];
    let submits = [vk::SubmitInfo::default().command_buffers(&buffers)];
    // SAFETY: recording has ended and the queue belongs to this device.
    unsafe { device.queue_submit(session.queue, &submits, vk::Fence::null()) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))?;
    in_flight()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(command);
    Ok(())
}

/// A layout transition of a whole colour image, as one barrier.
fn transition(
    device: &ash::Device,
    command: vk::CommandBuffer,
    image: vk::Image,
    (old, new): (vk::ImageLayout, vk::ImageLayout),
    (src_stage, src_access): (vk::PipelineStageFlags, vk::AccessFlags),
    (dst_stage, dst_access): (vk::PipelineStageFlags, vk::AccessFlags),
) {
    let barrier = [vk::ImageMemoryBarrier::default()
        .src_access_mask(src_access)
        .dst_access_mask(dst_access)
        .old_layout(old)
        .new_layout(new)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(image)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1),
        )];
    // SAFETY: recording is open and the image is live.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            src_stage,
            dst_stage,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &barrier,
        );
    }
}

/// A colour target kept on the device across a submission's draws (worklog 839).
///
/// Every draw used to create its own attachment, copy the target's 8 MB in and copy 8 MB back out.
/// This one is created once per target - seeded from given pixels or cleared - drawn on in place by
/// each draw, and read back only when its pixels are asked for.
#[derive(Debug)]
pub(crate) struct ResidentAttachment {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    /// Host-visible (cached where offered), the size of the image: the seed goes in through it and
    /// the readback comes out through it.
    buffer: vk::Buffer,
    buffer_memory: vk::DeviceMemory,
    width: u32,
    height: u32,
}

impl ResidentAttachment {
    /// Creates the attachment, holding `initial` (tightly packed `Rgba8`, exactly the extent) or
    /// cleared to `clear`, and leaves it ready to draw on.
    pub(crate) fn create(
        width: u32,
        height: u32,
        initial: Option<&[u8]>,
        clear: [f32; 4],
    ) -> Result<Self, DispatchError> {
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (instance, physical, device) = (&session.instance, session.physical, &session.device);
        let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
        let initial = initial.filter(|bytes| bytes.len() as u64 == size);
        let (image, memory) = create_attachment(instance, physical, device, width, height)?;
        let (buffer, buffer_memory) = create_readback_buffer(instance, physical, device, size)?;
        if let Some(bytes) = initial {
            fill_host_memory(device, buffer_memory, bytes)?;
        }
        let render_pass = create_resident_render_pass(device)?;
        let (view, framebuffer) =
            view_and_framebuffer(device, image, render_pass, (width, height))?;

        one_shot(&session, |device, command| {
            let top = (
                vk::PipelineStageFlags::TOP_OF_PIPE,
                vk::AccessFlags::empty(),
            );
            let transfer_write = (
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::TRANSFER_WRITE,
            );
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                ),
                top,
                transfer_write,
            );
            let range = vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1);
            if initial.is_some() {
                let regions = [whole_image(width, height)];
                // SAFETY: recording is open; the buffer holds the image's bytes and the image is in
                // the transfer-destination layout.
                unsafe {
                    device.cmd_copy_buffer_to_image(
                        command,
                        buffer,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &regions,
                    );
                }
            } else {
                let colour = vk::ClearColorValue { float32: clear };
                // SAFETY: recording is open and the image is in the transfer-destination layout.
                unsafe {
                    device.cmd_clear_color_image(
                        command,
                        image,
                        vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                        &colour,
                        &[range],
                    );
                }
            }
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                ),
                transfer_write,
                (
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::AccessFlags::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                ),
            );
        })?;
        Ok(Self {
            image,
            memory,
            view,
            render_pass,
            framebuffer,
            buffer,
            buffer_memory,
            width,
            height,
        })
    }

    /// **Gives it new contents in place** (worklog 857): `pixels` (tightly packed `Rgba8`, exactly
    /// the extent) copied in through its own buffer, after everything already recorded on the queue.
    /// Destroying and creating an attachment for every seed waited for the device twice and
    /// allocated two 8 MB blocks, once a frame.
    ///
    /// The buffer is written only once nothing still reads it: the device is settled first, which
    /// finishes a previous reload's copy and any readback.
    pub(crate) fn reload(&self, pixels: &[u8]) -> Result<(), DispatchError> {
        let (width, height) = self.extent();
        let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
        if pixels.len() as u64 != size {
            return Err(DispatchError::Unsupported(
                "a reload of another extent cannot fill this attachment".to_owned(),
            ));
        }
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        settle(&session)?;
        fill_host_memory(&session.device, self.buffer_memory, pixels)?;
        let (image, buffer) = (self.image, self.buffer);
        one_shot_unwaited(&session, |device, command| {
            let attachment = (
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            );
            let transfer_write = (
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::TRANSFER_WRITE,
            );
            // Its old contents are replaced whole, so they need not be kept across the transition.
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                ),
                attachment,
                transfer_write,
            );
            let regions = [whole_image(width, height)];
            // SAFETY: recording is open; the buffer holds the image's bytes and the image is in the
            // transfer-destination layout.
            unsafe {
                device.cmd_copy_buffer_to_image(
                    command,
                    buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
            }
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                ),
                transfer_write,
                attachment,
            );
        })
    }

    /// **Gives it one `Rgba8` word everywhere, in place, on the device** (worklog 863) - what a clear
    /// the guest filled seeds. The word goes into its buffer by `vkCmdFillBuffer`, exact bits rather
    /// than a float clear's rounding, and is copied in from there - all recorded after what the queue
    /// already holds, so nothing waits: the host never touches the buffer, and the device orders
    /// the fill after the buffer's last reader.
    pub(crate) fn reload_uniform(&self, word: u32) -> Result<(), DispatchError> {
        let (width, height) = self.extent();
        let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let (image, buffer) = (self.image, self.buffer);
        one_shot_unwaited(&session, |device, command| {
            let transfer = vk::PipelineStageFlags::TRANSFER;
            // After whatever last read or wrote the buffer: a reload's copy, a readback.
            let before_fill = [vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)];
            // SAFETY: recording is open.
            unsafe {
                device.cmd_pipeline_barrier(
                    command,
                    transfer,
                    transfer,
                    vk::DependencyFlags::empty(),
                    &before_fill,
                    &[],
                    &[],
                );
            }
            // SAFETY: recording is open; the buffer is `size` bytes with transfer-destination usage,
            // and the size is a multiple of four.
            unsafe { device.cmd_fill_buffer(command, buffer, 0, size, word) };
            let filled = [vk::MemoryBarrier::default()
                .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
                .dst_access_mask(vk::AccessFlags::TRANSFER_READ)];
            // SAFETY: recording is open.
            unsafe {
                device.cmd_pipeline_barrier(
                    command,
                    transfer,
                    transfer,
                    vk::DependencyFlags::empty(),
                    &filled,
                    &[],
                    &[],
                );
            }
            let attachment = (
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags::COLOR_ATTACHMENT_READ | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            );
            let transfer_write = (transfer, vk::AccessFlags::TRANSFER_WRITE);
            // Its old contents are replaced whole, so they need not be kept across the transition.
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                ),
                attachment,
                transfer_write,
            );
            let regions = [whole_image(width, height)];
            // SAFETY: recording is open; the buffer holds the image's bytes and the image is in the
            // transfer-destination layout.
            unsafe {
                device.cmd_copy_buffer_to_image(
                    command,
                    buffer,
                    image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &regions,
                );
            }
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                ),
                transfer_write,
                attachment,
            );
        })
    }

    /// The extent it was created at.
    pub(crate) const fn extent(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Copies the attachment out and returns its pixels, leaving it ready to draw on again.
    pub(crate) fn read_back(&self) -> Result<Pixels, DispatchError> {
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        self.copy_out(&session, self.buffer, true)?;
        let (width, height) = self.extent();
        let bytes = read_host_memory(&session.device, self.buffer_memory, width, height)?;
        Ok(Pixels {
            width,
            height,
            bytes,
        })
    }

    /// **The frame as it stands now, kept on the side** (worklog 850): copied on the device into a
    /// host-visible buffer of its own, so later draws on this attachment do not change it and nothing
    /// is read to the host until [`FrameSnapshot::read`] asks. What a guest's copy of its colour
    /// target sees at the point in the stream it was made.
    pub(crate) fn snapshot_into(&self, snapshot: &FrameSnapshot) -> Result<(), DispatchError> {
        if (snapshot.width, snapshot.height) != self.extent() {
            return Err(DispatchError::Unsupported(
                "a frame snapshot of another extent cannot hold this frame".to_owned(),
            ));
        }
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // Not waited for: nothing reads the snapshot until `FrameSnapshot::read`, which settles.
        self.copy_out(&session, snapshot.buffer, false)
    }

    /// Copies the image into `buffer` (the image's size) and waits for the copy.
    fn copy_out(
        &self,
        session: &crate::compute::Session,
        buffer: vk::Buffer,
        wait: bool,
    ) -> Result<(), DispatchError> {
        let (image, (width, height)) = (self.image, self.extent());
        let record = |device: &ash::Device, command: vk::CommandBuffer| {
            let attachment = (
                vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
            );
            let transfer_read = (
                vk::PipelineStageFlags::TRANSFER,
                vk::AccessFlags::TRANSFER_READ,
            );
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                ),
                attachment,
                transfer_read,
            );
            let regions = [whole_image(width, height)];
            // SAFETY: recording is open, the image is in the transfer-source layout and the buffer
            // is the image's size.
            unsafe {
                device.cmd_copy_image_to_buffer(
                    command,
                    image,
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    buffer,
                    &regions,
                );
            }
            transition(
                device,
                command,
                image,
                (
                    vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                    vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL,
                ),
                transfer_read,
                (
                    vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT,
                    vk::AccessFlags::COLOR_ATTACHMENT_READ
                        | vk::AccessFlags::COLOR_ATTACHMENT_WRITE,
                ),
            );
        };
        if wait {
            one_shot(session, record)
        } else {
            one_shot_unwaited(session, record)
        }
    }

    /// Destroys it. Best-effort, like every release here: with no session the process is ending.
    pub(crate) fn destroy(self) {
        let Ok(session) = crate::compute::session() else {
            return;
        };
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let device = &session.device;
        // Waiting idle means none of the handles below is in use - after running any draws still
        // recorded on it, and freeing their command buffers (worklogs 843, 844).
        let _ = settle(&session);
        // SAFETY: created on this device, idle, destroyed exactly once, dependants before what they
        // depend on - and so for each below.
        unsafe { device.destroy_framebuffer(self.framebuffer, None) };
        // SAFETY: as above.
        unsafe { device.destroy_image_view(self.view, None) };
        // SAFETY: as above.
        unsafe { device.destroy_render_pass(self.render_pass, None) };
        // SAFETY: as above.
        unsafe { device.destroy_image(self.image, None) };
        // SAFETY: as above; the image bound to it is destroyed.
        unsafe { device.free_memory(self.memory, None) };
        // SAFETY: as above.
        unsafe { device.destroy_buffer(self.buffer, None) };
        // SAFETY: as above; the buffer bound to it is destroyed.
        unsafe { device.free_memory(self.buffer_memory, None) };
    }
}

/// Reads a `width` x `height` `Rgba8` frame out of host-visible, coherent `memory` that a waited-on
/// copy filled.
fn read_host_memory(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, DispatchError> {
    let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
    // SAFETY: the memory is host-visible and coherent, the copy into it has been waited on, and
    // nothing else maps it.
    let mapped = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    let mut bytes = vec![0u8; usize::try_from(size).unwrap_or(0)];
    // SAFETY: the mapping covers `size` bytes and the destination is exactly that long.
    unsafe {
        std::ptr::copy_nonoverlapping(mapped.cast::<u8>(), bytes.as_mut_ptr(), bytes.len());
    }
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(memory) };
    Ok(bytes)
}

/// A resident frame as it stood when [`ResidentAttachment::snapshot`] took it, held on the device in
/// a host-visible buffer of its own until it is read or dropped (worklog 850).
#[derive(Debug)]
pub(crate) struct FrameSnapshot {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    width: u32,
    height: u32,
}

impl FrameSnapshot {
    /// A snapshot buffer for a `width` x `height` frame, empty until
    /// [`ResidentAttachment::snapshot_into`] fills it.
    pub(crate) fn create(width: u32, height: u32) -> Result<Self, DispatchError> {
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
        let (buffer, memory) =
            create_readback_buffer(&session.instance, session.physical, &session.device, size)?;
        Ok(Self {
            buffer,
            memory,
            width,
            height,
        })
    }

    /// The extent it holds a frame of.
    pub(crate) const fn extent(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    /// Its pixels, `Rgba8`.
    pub(crate) fn read(&self) -> Result<Pixels, DispatchError> {
        let session = crate::compute::session()?;
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // The copy into it was submitted unwaited; this is where it is waited for.
        settle(&session)?;
        let bytes = read_host_memory(&session.device, self.memory, self.width, self.height)?;
        Ok(Pixels {
            width: self.width,
            height: self.height,
            bytes,
        })
    }

    /// Releases its buffer. Best-effort, like every release here.
    pub(crate) fn destroy(self) {
        let Ok(session) = crate::compute::session() else {
            return;
        };
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        // A copy into it may still be in flight; waited for here, so the device no longer uses it.
        let _ = settle(&session);
        // SAFETY: created on this device; the device is idle after the settle above, so nothing
        // uses it, and it is destroyed exactly once.
        unsafe { session.device.destroy_buffer(self.buffer, None) };
        // SAFETY: as above; the buffer bound to it is destroyed.
        unsafe { session.device.free_memory(self.memory, None) };
    }
}

/// A colour image's view and a framebuffer over it for `render_pass`.
fn view_and_framebuffer(
    device: &ash::Device,
    image: vk::Image,
    render_pass: vk::RenderPass,
    (width, height): (u32, u32),
) -> Result<(vk::ImageView, vk::Framebuffer), DispatchError> {
    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(FORMAT)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1),
        );
    // SAFETY: the image is live and the create info outlives the call.
    let view = unsafe { device.create_image_view(&view_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image_view", e))?;
    let views = [view];
    let framebuffer_info = vk::FramebufferCreateInfo::default()
        .render_pass(render_pass)
        .attachments(&views)
        .width(width)
        .height(height)
        .layers(1);
    // SAFETY: the render pass and view are live and the slice outlives the call.
    let framebuffer = unsafe { device.create_framebuffer(&framebuffer_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_framebuffer", e))?;
    Ok((view, framebuffer))
}

/// A copy region covering a whole tightly packed colour image.
fn whole_image(width: u32, height: u32) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
}

/// A host-visible buffer big enough to receive the image, and its mapped-readable memory.
/// Reads a host-visible allocation back as words.
fn read_words(
    device: &ash::Device,
    allocation: vk::DeviceMemory,
    words: usize,
) -> Result<Vec<u32>, DispatchError> {
    let size = words_to_bytes(words);
    // SAFETY: the allocation backs a host-visible, coherent buffer of at least this size and
    // nothing else holds a mapping of it.
    let mapped = unsafe { device.map_memory(allocation, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?
        .cast::<u32>();
    let mut out = vec![0u32; words];
    // SAFETY: the mapping covers `words` words and the destination is exactly that long.
    unsafe { std::ptr::copy_nonoverlapping(mapped, out.as_mut_ptr(), words) };
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(allocation) };
    Ok(out)
}

/// Writes `memory` into the guest-memory window a pipeline bound.
///
/// Nothing when there is nothing to write, which is every draw that does not fetch. The buffer
/// is host-visible for exactly this - these are small and a test may want to read one back.
fn seed_memory(
    device: &ash::Device,
    built: &Pipeline,
    memory: &[u32],
) -> Result<(), DispatchError> {
    if memory.is_empty() {
        return Ok(());
    }
    let (_, allocation) = built.buffers[1];
    let size = words_to_bytes(memory.len());
    // SAFETY: the allocation backs a host-visible buffer of at least this size, nothing else
    // holds a mapping of it, and the pointer is used only until it is unmapped below.
    let mapped = unsafe { device.map_memory(allocation, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?
        .cast::<u32>();
    // SAFETY: `mapped` points at `size` bytes, which is `memory.len()` words, and the source
    // and destination cannot overlap - one is host memory this process owns and the other is
    // a fresh mapping.
    unsafe { std::ptr::copy_nonoverlapping(memory.as_ptr(), mapped, memory.len()) };
    // SAFETY: the memory is mapped and no pointer into it is used after this.
    unsafe { device.unmap_memory(allocation) };
    Ok(())
}

/// Bytes for a window measured in words, as Vulkan wants it.
///
/// At least one word: a zero-sized buffer is not a legal binding, and a module that declares a
/// window it never touches would otherwise fail to build for a reason about the harness.
fn words_to_bytes(words: usize) -> vk::DeviceSize {
    let words = u64::try_from(words.max(1)).unwrap_or(1);
    words * 4
}

/// How wide and tall a storage image is, when the caller does not say.
///
/// Small, because every pipeline gets one whether or not it writes to it, and a test that means
/// to store somewhere says where. Eight by eight is the same frame the sampling tests draw, so
/// a texel index and a pixel coordinate can be the same number when a test wants that.
pub const STORAGE_IMAGE_SIZE: (u32, u32) = (8, 8);

/// A storage image: written by a shader, read back by the host afterwards.
///
/// Created for every pipeline, like the texture and the two buffers, and for the same reason: a
/// binding a module uses and the layout lacks is an invalid pipeline, and which of the two a
/// translated module is is not something this harness reads.
///
/// **Its layout is `GENERAL` for the whole recording.** That is the only layout a storage image
/// may be written in, and there is nothing to transition to afterwards - a transfer may read it
/// in `GENERAL` as well.
struct StorageImage {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    /// Where the texels are copied to after the draw, so the host can read them.
    readback: vk::Buffer,
    readback_memory: vk::DeviceMemory,
    width: u32,
    height: u32,
}

/// Creates a `size` storage image, left uninitialised.
///
/// The contents before a shader writes are **undefined and deliberately not cleared**: a test
/// asserting on what a store left behind must name the texel it wrote, and a zero-filled image
/// would let an assertion pass against a texel nothing touched.
fn create_storage_image(
    devices: Devices<'_>,
    size: (u32, u32),
) -> Result<StorageImage, DispatchError> {
    let device = devices.device;
    let (width, height) = size;
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::STORAGE | vk::ImageUsageFlags::TRANSFER_SRC)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    // SAFETY: the create info outlives the call and the device is live.
    let image = unsafe { device.create_image(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image(storage)", e))?;

    // SAFETY: the image was created on this device and not yet destroyed.
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    // SAFETY: the physical device is valid.
    let properties = unsafe {
        devices
            .instance
            .get_physical_device_memory_properties(devices.physical)
    };
    let memory_type = (0..properties.memory_type_count)
        .find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[*i as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .ok_or(DispatchError::NoHostVisibleMemory)?;
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    let memory = unsafe { device.allocate_memory(&allocate, None) }
        .map_err(|e| DispatchError::Vulkan("allocate_memory(storage)", e))?;
    // SAFETY: image and memory come from this device and the allocation is large enough.
    unsafe { device.bind_image_memory(image, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_image_memory(storage)", e))?;

    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(FORMAT)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(1)
                .layer_count(1),
        );
    // SAFETY: the image is live and the create info outlives the call.
    let view = unsafe { device.create_image_view(&view_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image_view(storage)", e))?;

    let bytes = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
    let (readback, readback_memory) =
        create_readback_buffer(devices.instance, devices.physical, device, bytes)?;

    Ok(StorageImage {
        image,
        memory,
        view,
        readback,
        readback_memory,
        width,
        height,
    })
}

/// Makes a storage image writable, into an open recording.
///
/// One barrier, discarding whatever the image held - nothing, it has never been written.
fn open_storage_image(device: &ash::Device, command: vk::CommandBuffer, storage: &StorageImage) {
    let range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1);
    let to_general = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::SHADER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(storage.image)
        .subresource_range(range)];
    // SAFETY: recording is open, the image is live, and nothing else has touched it.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_general,
        );
    }
}

/// Copies a storage image out to its readback buffer, into an open recording.
///
/// Recorded after the render pass ends, because the write happens inside it. The barrier is what
/// makes the shader's stores visible to the copy - without it the copy may read texels the
/// shader has not finished writing, which is the kind of fault that produces a correct picture
/// on one driver and not on another.
fn read_storage_image(device: &ash::Device, command: vk::CommandBuffer, storage: &StorageImage) {
    let range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1);
    let to_source = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::SHADER_WRITE)
        .dst_access_mask(vk::AccessFlags::TRANSFER_READ)
        // Still `GENERAL` on both sides: a storage image is written in it and a transfer may
        // read in it, so this barrier orders rather than transitions.
        .old_layout(vk::ImageLayout::GENERAL)
        .new_layout(vk::ImageLayout::GENERAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(storage.image)
        .subresource_range(range)];
    // SAFETY: recording is open and the render pass that wrote the image has ended.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_source,
        );
    }

    let regions = [vk::BufferImageCopy::default()
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width: storage.width,
            height: storage.height,
            depth: 1,
        })];
    // SAFETY: recording is open, the barrier above made the image readable, and the buffer is
    // large enough for the region by construction.
    unsafe {
        device.cmd_copy_image_to_buffer(
            command,
            storage.image,
            vk::ImageLayout::GENERAL,
            storage.readback,
            &regions,
        );
    }
}

/// A sampled texture: the image, what it is read through, and the bytes on their way in.
///
/// Created for every pipeline, whether or not the fragment module samples anything. A binding
/// nothing uses costs one descriptor; a binding a module uses and the layout lacks is an
/// invalid pipeline, and which of the two a translated module is is not something this harness
/// reads - the same judgement the storage buffers already record (worklog 566).
struct Texture {
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    sampler: vk::Sampler,
    /// The texels on the host, waiting to be copied in when the recording opens.
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    width: u32,
    height: u32,
    /// How many levels it has: two where the image is big enough for a second, one otherwise.
    levels: u32,
}

/// The texture bound when a caller does not ask for one: a single opaque white texel.
///
/// Not "none". An image with no extent is not a legal image and a descriptor has to be filled,
/// so the default is the smallest legal texture rather than an absent one.
static NO_TEXTURE: [u32; 1] = [0xffff_ffff];

/// A texture's upload, laid out: every level's texels in staging order, the fine level's extent, and
/// how many levels there are.
#[derive(Debug, PartialEq, Eq)]
struct Staged {
    texels: Vec<u32>,
    width: u32,
    height: u32,
    levels: u32,
}

/// Lays out `texels`, `row` to a row, for upload: the fine level padded with zeroes to whole rows,
/// then - only when `coarse_level` asks and the image is big enough - the harness's second level.
///
/// **Two levels where the harness asks and the image is big enough for a second**, so a sample naming
/// a level can be told from one naming a different level. With a single level every level answers
/// the same texel, and a test of `image_sample_l` would pass without the level operand reaching the
/// instruction at all. A one-by-one image has exactly one level - halving it reaches zero, which is
/// not an extent - so the count is derived rather than fixed.
///
/// **Only for the harness.** Given to a guest's texture it was an invented level: a minified sample
/// read the sentinel colour instead of the guest's texels, and its copy - one texel of staging for a
/// halved extent - read past the buffer, which lost the device on Neverball's first 256x256 texture
/// (worklog 833). The coarse level is now filled across its whole halved extent, so the copy reads
/// exactly what the buffer holds at any size.
fn staged(texels: &[u32], row: u32, coarse_level: bool) -> Staged {
    let width = row.max(1);
    let cells = usize::try_from(width).unwrap_or(1);
    let rows = texels.len().div_ceil(cells).max(1);
    let mut padded = texels.to_vec();
    padded.resize(cells * rows, 0);
    let height = u32::try_from(rows).unwrap_or(1);
    let levels = if coarse_level && width > 1 && height > 1 {
        2
    } else {
        1
    };
    if levels > 1 {
        let coarse = (width / 2).max(1) as usize * (height / 2).max(1) as usize;
        padded.resize(padded.len() + coarse, COARSE_TEXEL);
    }
    Staged {
        texels: padded,
        width,
        height,
        levels,
    }
}

/// Creates a sampled image holding `texels`, `row` of them to a row.
///
/// **Optimally tiled, filled by a staging copy** rather than a linear image written through a
/// mapping. `R8G8B8A8_UNORM` is required to support sampling with optimal tiling on every
/// implementation and is not required to support it with linear tiling, so the guaranteed path
/// is the one a harness takes. It also leaves the row stride to the driver instead of to this
/// file, which is one fewer thing a wrong picture could be about.
///
/// A short final row is padded with zeroes, so the extent and the bytes always agree.
///
/// `coarse_level` asks for the harness's sentinel second level ([`staged`]); a guest's texture never
/// has it (worklog 833).
fn create_texture(
    devices: Devices<'_>,
    texels: &[u32],
    row: u32,
    coarse_level: bool,
) -> Result<Texture, DispatchError> {
    let device = devices.device;
    let Staged {
        texels: padded,
        width,
        height,
        levels,
    } = staged(texels, row, coarse_level);
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width,
            height,
            depth: 1,
        })
        .mip_levels(levels)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(vk::ImageUsageFlags::TRANSFER_DST | vk::ImageUsageFlags::SAMPLED)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    // SAFETY: the create info outlives the call and the device is live.
    let image = unsafe { device.create_image(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image(texture)", e))?;

    // SAFETY: the image was created on this device and not yet destroyed.
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    // SAFETY: the physical device is valid.
    let properties = unsafe {
        devices
            .instance
            .get_physical_device_memory_properties(devices.physical)
    };
    // Device-local, for the same reason the attachment is: an optimally tiled image is not
    // readable by the host whatever memory it sits in, so the copy is required regardless.
    let memory_type = (0..properties.memory_type_count)
        .find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[*i as usize]
                    .property_flags
                    .contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)
        })
        .ok_or(DispatchError::NoHostVisibleMemory)?;
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    let memory = unsafe { device.allocate_memory(&allocate, None) }
        .map_err(|e| DispatchError::Vulkan("allocate_memory(texture)", e))?;
    // SAFETY: image and memory come from this device and the allocation is the size the image
    // asked for.
    unsafe { device.bind_image_memory(image, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_image_memory(texture)", e))?;

    let size = words_to_bytes(padded.len());
    let (staging, staging_memory) = create_host_buffer(
        devices.instance,
        devices.physical,
        device,
        size,
        vk::BufferUsageFlags::TRANSFER_SRC,
    )?;
    // SAFETY: the allocation backs a host-visible, coherent buffer of exactly this size and
    // nothing else holds a mapping of it.
    let mapped = unsafe { device.map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory(texture)", e))?
        .cast::<u32>();
    // SAFETY: the mapping covers `padded.len()` words, the destination is exactly that long,
    // and the two cannot overlap - one is a fresh mapping and the other is host memory this
    // process owns.
    unsafe { std::ptr::copy_nonoverlapping(padded.as_ptr(), mapped, padded.len()) };
    // SAFETY: mapped immediately above and no pointer into it is used after this.
    unsafe { device.unmap_memory(staging_memory) };

    let view_info = vk::ImageViewCreateInfo::default()
        .image(image)
        .view_type(vk::ImageViewType::TYPE_2D)
        .format(FORMAT)
        .subresource_range(
            vk::ImageSubresourceRange::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .level_count(levels)
                .layer_count(1),
        );
    // SAFETY: the image is live and the create info outlives the call.
    let view = unsafe { device.create_image_view(&view_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image_view(texture)", e))?;

    // Nearest filtering and clamping, so a test that names a texel gets that texel. Any
    // filtering mixes neighbours, and then an exact assertion fails for a reason about the
    // sampler rather than about the shader - which is the kind of failure a harness exists to
    // not have.
    let sampler_info = vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::NEAREST)
        .min_filter(vk::Filter::NEAREST)
        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        // Without this the sampler clamps every level request to zero and a levelled sample
        // reads the fine level whatever it asked for - which looks exactly like a translation
        // that ignored the level operand.
        .max_lod(f32_from(levels - 1));
    // SAFETY: the create info outlives the call and the device is live.
    let sampler = unsafe { device.create_sampler(&sampler_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_sampler", e))?;

    Ok(Texture {
        image,
        memory,
        view,
        sampler,
        staging,
        staging_memory,
        width,
        height,
        levels,
    })
}

/// The one texel of a sampled texture's coarse level.
///
/// **Fixed by the harness rather than supplied**, because its whole purpose is to be *not* any
/// texel of the fine level: a sample naming level one comes back as this, and a sample naming
/// level zero cannot. A caller choosing it could choose one that collides, and the assertion
/// would then pass for a translation that ignored the level.
pub const COARSE_TEXEL: u32 = 0xff30_70b0;

/// Copies a texture in and leaves it readable, into an open recording.
///
/// Recorded before the render pass opens, because a render pass cannot contain a transfer. The
/// first barrier discards whatever the image held - nothing, it has never been written - and
/// the second is what makes the copy visible to the sampling that follows.
fn upload_texture(device: &ash::Device, command: vk::CommandBuffer, texture: &Texture) {
    // Every level, or the coarse one is left in an undefined layout and a sample naming it is
    // undefined behaviour rather than an answer.
    let range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(texture.levels)
        .layer_count(1);
    let to_destination = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(texture.image)
        .subresource_range(range)];
    // SAFETY: recording is open, the image is live, and nothing else has touched it.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_destination,
        );
    }

    // One region per level. The coarse level's single texel sits immediately after the fine
    // level's in the staging buffer, which is what its offset says.
    let mut regions = vec![
        vk::BufferImageCopy::default()
            // Zero means tightly packed, which is what the staging buffer is.
            .buffer_row_length(0)
            .buffer_image_height(0)
            .image_subresource(
                vk::ImageSubresourceLayers::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .layer_count(1),
            )
            .image_extent(vk::Extent3D {
                width: texture.width,
                height: texture.height,
                depth: 1,
            }),
    ];
    if texture.levels > 1 {
        let fine = vk::DeviceSize::from(texture.width) * vk::DeviceSize::from(texture.height) * 4;
        regions.push(
            vk::BufferImageCopy::default()
                .buffer_offset(fine)
                .buffer_row_length(0)
                .buffer_image_height(0)
                .image_subresource(
                    vk::ImageSubresourceLayers::default()
                        .aspect_mask(vk::ImageAspectFlags::COLOR)
                        .mip_level(1)
                        .layer_count(1),
                )
                // Halved and floored - the extent `create_texture` filled with the sentinel.
                .image_extent(vk::Extent3D {
                    width: (texture.width / 2).max(1),
                    height: (texture.height / 2).max(1),
                    depth: 1,
                }),
        );
    }
    // SAFETY: recording is open, the image is in TRANSFER_DST_OPTIMAL because of the barrier
    // above, and the staging buffer holds exactly the texels these extents describe.
    unsafe {
        device.cmd_copy_buffer_to_image(
            command,
            texture.staging,
            texture.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &regions,
        );
    }

    let to_readable = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .dst_access_mask(vk::AccessFlags::SHADER_READ)
        .old_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .new_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(texture.image)
        .subresource_range(range)];
    // SAFETY: recording is open and the copy above is the only thing that has written it.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TRANSFER,
            vk::PipelineStageFlags::FRAGMENT_SHADER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_readable,
        );
    }
}

/// A host-visible storage buffer, for one of the windows a translated module declares.
///
/// Host-visible rather than device-local because these are small and a test may want to read
/// one back; nothing here is a performance path.
fn create_storage_buffer(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    size: vk::DeviceSize,
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    create_host_buffer(
        instance,
        physical,
        device,
        size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )
}

fn create_readback_buffer(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    size: vk::DeviceSize,
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    create_host_buffer(
        instance,
        physical,
        device,
        size,
        // SRC as well: the same buffer carries a draw's starting pixels in before it carries the
        // result out (worklog 822).
        vk::BufferUsageFlags::TRANSFER_DST | vk::BufferUsageFlags::TRANSFER_SRC,
    )
}

/// One host-visible buffer of the given usage, with its memory bound.
fn create_host_buffer(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    let info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer", e))?;
    // SAFETY: the buffer was created on this device and not yet destroyed.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    // SAFETY: the physical device is valid.
    let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
    let wanted = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let with = |flags: vk::MemoryPropertyFlags| {
        (0..properties.memory_type_count).find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[*i as usize]
                    .property_flags
                    .contains(flags)
        })
    };
    // **Cached where the device offers it** (worklog 838). Every buffer here is read back by the host
    // - a frame's 8 MB of pixels after every draw - and the first host-visible type a device lists is
    // commonly write-combined, which the host reads at a small fraction of cached speed: reading one
    // 1080p frame out of it took ~20 ms of a ~25 ms draw. Coherent either way, so nothing about
    // flushing changes.
    let memory_type = with(wanted | vk::MemoryPropertyFlags::HOST_CACHED)
        .or_else(|| with(wanted))
        .ok_or(DispatchError::NoHostVisibleMemory)?;
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    let memory = unsafe { device.allocate_memory(&allocate, None) }
        .map_err(|e| DispatchError::Vulkan("allocate_memory", e))?;
    // SAFETY: buffer and memory both come from this device and the allocation is large enough.
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_buffer_memory", e))?;
    Ok((buffer, memory))
}

/// Clears an attachment to `colour` and reads the pixels back, drawing nothing.
///
/// # Errors
///
/// When no device is available, or any Vulkan call fails. A machine with no device is a skip
/// for the caller to surface loudly, never a pass.
pub fn clear_to(colour: [f32; 4], width: u32, height: u32) -> Result<Pixels, DispatchError> {
    render(
        colour,
        width,
        height,
        None,
        Geometry::Vertex(VertexDraw::TRIANGLE),
        Bound::default(),
    )
}

/// Draws a vertex+fragment pipeline over the guest's decoded vertex count into a `clear`-ed
/// attachment, and reads the pixels back.
///
/// The graphics counterpart of [`crate::compute::dispatch`]: given the two translated modules and the
/// draw's parameters, it returns the frame they drew. The backend uses it to execute a `Draw` from a
/// submission's bound vertex and fragment shaders, issuing the vertex count the guest asked for
/// rather than a fixed three; the storage buffers are the default two every module declares, until a
/// guest binds its own.
pub(crate) fn draw_vertices(
    start: &Start<'_>,
    size: (u32, u32),
    draw: VertexDraw,
    scissor: Option<vk::Rect2D>,
    vertex: &[u32],
    fragment: &[u32],
) -> Result<Pixels, DispatchError> {
    let (width, height) = size;
    render(
        start.clear,
        width,
        height,
        Some((vertex, fragment)),
        Geometry::Vertex(draw),
        Bound {
            scissor,
            initial: start.initial,
            user_data: start.user_data,
            texture: start.texture.unwrap_or((&NO_TEXTURE, 1)),
            blend: start.blend,
            viewport: start.viewport,
            second_texture: start.second_texture.unwrap_or((&NO_TEXTURE, 1)),
            ..Bound::default()
        },
    )
}

/// How a draw starts: the clear colour, the attachment's starting pixels if any (worklog 822), the
/// user-data block its shaders read at entry (worklog 826), and the texture it samples with its row
/// length, if a `BindTexture` gave one (worklog 828) - otherwise the default texel.
pub(crate) struct Start<'a> {
    /// What the attachment clears to when it has no starting pixels.
    pub(crate) clear: [f32; 4],
    /// The attachment's starting pixels (worklog 822).
    pub(crate) initial: Option<&'a [u8]>,
    /// The user-data block (worklog 826).
    pub(crate) user_data: &'a [u32; USER_DATA_BLOCK_WORDS],
    /// The texture and its row length (worklog 828).
    pub(crate) texture: Option<(&'a [u32], u32)>,
    /// Colour target zero's blend state (`REQ-...2ea9`, worklog 829); `None` draws opaque, as
    /// every draw did before.
    pub(crate) blend: Option<orbistoun_gpu::BlendControl>,
    /// The guest's clip-to-pixel transform (worklog 837); `None` maps clip space over the whole
    /// attachment, `+y` down, as every draw did before.
    pub(crate) viewport: Option<orbistoun_gpu::ViewportTransform>,
    /// The second texture and its row length (worklog 840).
    pub(crate) second_texture: Option<(&'a [u32], u32)>,
    /// What the draw's pipeline is, as the backend hashed it (worklog 843): a resident draw with a
    /// key reuses the pipeline built for the last draw with the same one. `None` builds afresh.
    pub(crate) pipeline_key: Option<std::num::NonZeroU64>,
}

/// The Vulkan viewport a guest's transform describes (worklog 837).
///
/// Vulkan maps `x_fb = (width / 2) * x_ndc + (x + width / 2)`; the guest maps
/// `x_fb = x_scale * x_ndc + x_offset`. So `width = 2 * x_scale` and `x = x_offset - x_scale`, and the
/// same for y - where a GL guest's negative `y_scale` becomes a negative height, which Vulkan takes
/// (core since 1.1) and which is exactly the flip the guest asked for.
pub(crate) fn guest_viewport(transform: orbistoun_gpu::ViewportTransform) -> vk::Viewport {
    vk::Viewport {
        x: transform.x_offset - transform.x_scale,
        y: transform.y_offset - transform.y_scale,
        width: 2.0 * transform.x_scale,
        height: 2.0 * transform.y_scale,
        min_depth: 0.0,
        max_depth: 1.0,
    }
}

/// The colour-blend attachment state a guest's `CB_BLEND0_CONTROL` asks for, or the name of what it
/// asks for that this cannot honour exactly (`REQ-...2ea9`, worklog 829).
///
/// Factors and combine functions map one to one (`gfx103.json` `BlendOp`/`CombFunc` onto Vulkan's).
/// Refused rather than approximated: the constant-colour factors (the blend constant registers are not
/// decoded), the dual-source ones, the two `BOTH_*` forms and any reserved code. With
/// `SEPARATE_ALPHA_BLEND` off, alpha is blended with the colour factors, as the hardware does.
///
/// # Errors
///
/// The name of the first part of the state with no exact Vulkan equivalent.
pub(crate) fn blend_attachment(
    blend: Option<orbistoun_gpu::BlendControl>,
) -> Result<vk::PipelineColorBlendAttachmentState, &'static str> {
    use orbistoun_gpu::{BlendFactor as F, CombineFunc as C};
    let opaque = vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(false);
    let Some(blend) = blend.filter(|b| b.enable) else {
        return Ok(opaque);
    };
    let factor = |f: F| -> Result<vk::BlendFactor, &'static str> {
        Ok(match f {
            F::Zero => vk::BlendFactor::ZERO,
            F::One => vk::BlendFactor::ONE,
            F::SrcColor => vk::BlendFactor::SRC_COLOR,
            F::OneMinusSrcColor => vk::BlendFactor::ONE_MINUS_SRC_COLOR,
            F::SrcAlpha => vk::BlendFactor::SRC_ALPHA,
            F::OneMinusSrcAlpha => vk::BlendFactor::ONE_MINUS_SRC_ALPHA,
            F::DstAlpha => vk::BlendFactor::DST_ALPHA,
            F::OneMinusDstAlpha => vk::BlendFactor::ONE_MINUS_DST_ALPHA,
            F::DstColor => vk::BlendFactor::DST_COLOR,
            F::OneMinusDstColor => vk::BlendFactor::ONE_MINUS_DST_COLOR,
            F::SrcAlphaSaturate => vk::BlendFactor::SRC_ALPHA_SATURATE,
            F::ConstantColor
            | F::OneMinusConstantColor
            | F::ConstantAlpha
            | F::OneMinusConstantAlpha => {
                return Err("a constant-colour blend factor (the blend constants are not decoded)");
            }
            F::Src1Color | F::InvSrc1Color | F::Src1Alpha | F::InvSrc1Alpha => {
                return Err("a dual-source blend factor");
            }
            F::BothSrcAlpha | F::BothInvSrcAlpha | F::Other(_) => {
                return Err("a blend factor with no Vulkan equivalent");
            }
        })
    };
    let combine = |c: C| -> Result<vk::BlendOp, &'static str> {
        Ok(match c {
            C::DstPlusSrc => vk::BlendOp::ADD,
            C::SrcMinusDst => vk::BlendOp::SUBTRACT,
            C::DstMinusSrc => vk::BlendOp::REVERSE_SUBTRACT,
            C::MinDstSrc => vk::BlendOp::MIN,
            C::MaxDstSrc => vk::BlendOp::MAX,
            C::Other(_) => return Err("a reserved blend combine function"),
        })
    };
    let (alpha_src, alpha_combine, alpha_dst) = if blend.separate_alpha_blend {
        (blend.alpha_src, blend.alpha_combine, blend.alpha_dst)
    } else {
        (blend.color_src, blend.color_combine, blend.color_dst)
    };
    Ok(opaque
        .blend_enable(true)
        .src_color_blend_factor(factor(blend.color_src)?)
        .dst_color_blend_factor(factor(blend.color_dst)?)
        .color_blend_op(combine(blend.color_combine)?)
        .src_alpha_blend_factor(factor(alpha_src)?)
        .dst_alpha_blend_factor(factor(alpha_dst)?)
        .alpha_blend_op(combine(alpha_combine)?))
}

/// The shared body: set the attachment up, record, submit, read back, release.
///
/// `shaders` decides whether anything is drawn. `None` is the clear-only path the plumbing was
/// verified with; `Some` builds a graphics pipeline and draws three vertices through it.
fn render(
    colour: [f32; 4],
    width: u32,
    height: u32,
    shaders: Option<(&[u32], &[u32])>,
    geometry: Geometry,
    bound: Bound<'_>,
) -> Result<Pixels, DispatchError> {
    render_over(colour, width, height, shaders, geometry, bound).map(|drawn| drawn.pixels)
}

/// The shared body, which also hands back the guest-memory window the draw left behind.
///
/// Reading it is how a shader that writes memory is observed at a graphics stage: the
/// attachment shows what it drew and this shows what it stored, and a guest's shaders do both.
fn render_over(
    colour: [f32; 4],
    width: u32,
    height: u32,
    shaders: Option<(&[u32], &[u32])>,
    geometry: Geometry,
    bound: Bound<'_>,
) -> Result<Drawn, DispatchError> {
    let windows = bound.windows;
    let session = crate::compute::session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let instance = &session.instance;
    let physical = session.physical;
    let device = &session.device;
    let queue = session.queue;
    let family = session.family;

    // **A resident attachment is drawn on in place** (worklog 839): no attachment, render pass,
    // framebuffer or buffer of this draw's own, and no pixels in or out.
    if let Some(resident) = bound.resident {
        return render_resident(&session, resident, shaders, geometry, bound);
    }

    let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
    // Starting pixels only when they cover the attachment exactly; anything else clears, as a draw
    // always did (worklog 822).
    let initial = bound
        .initial
        .filter(|bytes| u64::try_from(bytes.len()).is_ok_and(|len| len == size));
    let (image, image_memory) = create_attachment(instance, physical, device, width, height)?;
    let render_pass = create_render_pass(device, initial.is_some())?;
    let (buffer, buffer_memory) = create_readback_buffer(instance, physical, device, size)?;
    if let Some(bytes) = initial {
        fill_host_memory(device, buffer_memory, bytes)?;
    }

    let (view, framebuffer) = view_and_framebuffer(device, image, render_pass, (width, height))?;

    let pipeline = shaders
        .map(|(vertex, fragment)| {
            build_pipeline(
                Devices {
                    instance,
                    physical,
                    device,
                },
                render_pass,
                (vertex, fragment),
                (width, height),
                geometry,
                bound,
            )
            .and_then(|built| {
                // A caller-owned resident window is uploaded once by its owner (D703), so it is not
                // seeded here; only a window this pipeline created is filled from `bound.memory`.
                if built.owns_guest_memory {
                    seed_memory(device, &built, bound.memory)?;
                }
                Ok(built)
            })
        })
        .transpose()?;

    let target = Target {
        render_pass,
        framebuffer,
        image,
        buffer,
        width,
        height,
    };
    let start = (
        colour,
        if initial.is_some() {
            Starts::Loaded
        } else {
            Starts::Cleared
        },
    );
    let command = record(instance, device, family, &target, start, pipeline.as_ref())?;
    let command_buffers = [command.buffer];
    let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
    // SAFETY: recording has ended and the queue belongs to this device.
    unsafe { device.queue_submit(queue, &submits, vk::Fence::null()) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))?;
    // One submission, so waiting on the device is as strong as a fence and one fewer object.
    // SAFETY: the device is live and nothing else is using it.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;

    // SAFETY: the memory is host-visible and coherent, was just written by the copy, and is
    // not currently mapped.
    let mapped = unsafe { device.map_memory(buffer_memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    let mut bytes = vec![0u8; usize::try_from(size).unwrap_or(0)];
    // SAFETY: the mapping covers `size` bytes and the destination is exactly that long.
    unsafe { std::ptr::copy_nonoverlapping(mapped.cast::<u8>(), bytes.as_mut_ptr(), bytes.len()) };
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(buffer_memory) };

    // The guest-memory window and the storage image as the draw left them, read before the pipeline
    // is released.
    let (left_behind, stored) = read_outputs(device, pipeline.as_ref(), windows[1])?;

    release(device, command.pool, &target, view, pipeline.as_ref());
    // SAFETY: nothing is bound to either allocation any longer and the buffer is unmapped.
    unsafe { device.free_memory(image_memory, None) };
    // SAFETY: as above.
    unsafe { device.free_memory(buffer_memory, None) };

    Ok(Drawn {
        pixels: Pixels {
            width,
            height,
            bytes,
        },
        memory: left_behind,
        stored,
    })
}

/// Everything one draw leaves behind.
///
/// Three observations of the same run: what reached the attachment, what the guest-memory window
/// holds, and what the storage image holds. A guest's shaders do all three, and telling them
/// apart is what makes "it drew nothing" different from "it drew and stored nothing".
#[derive(Debug, Clone)]
pub struct Drawn {
    /// The attachment, pixel by pixel.
    pub pixels: Pixels,
    /// The guest-memory window, word by word.
    pub memory: Vec<u32>,
    /// The storage image, texel by texel.
    pub stored: Pixels,
}

/// Reads what a draw left in the guest-memory window and in the storage image.
///
/// Empty for both when nothing drew (no pipeline). Split out of `render_over` so that function reads
/// as one sequence rather than two match arms in the middle of it.
fn read_outputs(
    device: &ash::Device,
    pipeline: Option<&Pipeline>,
    window_words: usize,
) -> Result<(Vec<u32>, Pixels), DispatchError> {
    match pipeline {
        Some(built) => Ok((
            read_words(device, built.buffers[1].1, window_words)?,
            read_image(device, &built.storage_image)?,
        )),
        None => Ok((
            Vec::new(),
            Pixels {
                width: 0,
                height: 0,
                bytes: Vec::new(),
            },
        )),
    }
}

/// Reads a storage image's readback buffer back as pixels.
fn read_image(device: &ash::Device, storage: &StorageImage) -> Result<Pixels, DispatchError> {
    let size = vk::DeviceSize::from(storage.width) * vk::DeviceSize::from(storage.height) * 4;
    // SAFETY: the allocation is host-visible and coherent, the copy that filled it has been
    // waited on, and nothing else holds a mapping of it.
    let mapped = unsafe {
        device.map_memory(
            storage.readback_memory,
            0,
            size,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|e| DispatchError::Vulkan("map_memory(storage)", e))?;
    let mut bytes = vec![0u8; usize::try_from(size).unwrap_or(0)];
    // SAFETY: the mapping covers `size` bytes and the destination is exactly that long.
    unsafe { std::ptr::copy_nonoverlapping(mapped.cast::<u8>(), bytes.as_mut_ptr(), bytes.len()) };
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(storage.readback_memory) };
    Ok(Pixels {
        width: storage.width,
        height: storage.height,
        bytes,
    })
}

/// Destroys everything one render created, on the successful path only.
///
/// An error abandons them instead - see this module's opening note. Gathered here because the
/// list is long enough that inlining it twice would be two chances to miss one.
fn release(
    device: &ash::Device,
    pool: vk::CommandPool,
    target: &Target,
    view: vk::ImageView,
    pipeline: Option<&Pipeline>,
) {
    // SAFETY: every handle below was created on this device, is no longer in use because the
    // queue has been waited on, and is destroyed exactly once.
    unsafe { device.destroy_command_pool(pool, None) };
    if let Some(built) = pipeline {
        release_pipeline(device, built);
    }
    // SAFETY: as above.
    unsafe { device.destroy_framebuffer(target.framebuffer, None) };
    // SAFETY: as above.
    unsafe { device.destroy_image_view(view, None) };
    // SAFETY: as above.
    unsafe { device.destroy_render_pass(target.render_pass, None) };
    // SAFETY: as above.
    unsafe { device.destroy_image(target.image, None) };
    // SAFETY: as above.
    unsafe { device.destroy_buffer(target.buffer, None) };
}

/// Destroys what one draw's pipeline created: the pipeline, its layout and modules, its descriptor
/// objects, the buffers it owns, its texture and its storage image. The caller has waited on the
/// device, so none is in use.
fn release_pipeline(device: &ash::Device, built: &Pipeline) {
    {
        // SAFETY: every handle below was created on this device, is no longer in use because the
        // queue has been waited on, and is destroyed exactly once.
        unsafe { device.destroy_pipeline(built.handle, None) };
        // SAFETY: as above.
        unsafe { device.destroy_pipeline_layout(built.layout, None) };
        // SAFETY: as above.
        unsafe { device.destroy_shader_module(built.vertex, None) };
        // SAFETY: as above.
        unsafe { device.destroy_shader_module(built.fragment, None) };
        // SAFETY: as above. The pool owns the set, so destroying it frees that too.
        unsafe { device.destroy_descriptor_pool(built.descriptor_pool, None) };
        // SAFETY: as above.
        unsafe { device.destroy_descriptor_set_layout(built.set_layout, None) };
        // Binding 0 is always this pipeline's; binding 1 only when it created it. A caller-owned
        // resident window (D703) is bound and read back but never destroyed here - destroying a
        // buffer its owner still holds is the use-after-free this flag exists to prevent.
        let owned: &[(vk::Buffer, vk::DeviceMemory)] = if built.owns_guest_memory {
            &built.buffers
        } else {
            &built.buffers[..1]
        };
        for &(buffer, memory) in owned {
            // SAFETY: as above.
            unsafe { device.destroy_buffer(buffer, None) };
            // SAFETY: as above, and the buffer that used it is already destroyed.
            unsafe { device.free_memory(memory, None) };
        }
        release_texture(device, &built.texture);
        release_texture(device, &built.second_texture);
        // SAFETY: as above.
        unsafe { device.destroy_image_view(built.storage_image.view, None) };
        // SAFETY: as above.
        unsafe { device.destroy_image(built.storage_image.image, None) };
        // SAFETY: as above, and the image that used it is already destroyed.
        unsafe { device.free_memory(built.storage_image.memory, None) };
        // SAFETY: as above.
        unsafe { device.destroy_buffer(built.storage_image.readback, None) };
        // SAFETY: as above, and the buffer that used it is already destroyed.
        unsafe { device.free_memory(built.storage_image.readback_memory, None) };
    }
}

/// Destroys a sampled texture's objects. The caller has waited on the device.
fn release_texture(device: &ash::Device, texture: &Texture) {
    // SAFETY: every handle was created on this device, is not in use because the device was waited
    // on, and is destroyed exactly once - and so for each below.
    unsafe { device.destroy_sampler(texture.sampler, None) };
    // SAFETY: as above.
    unsafe { device.destroy_image_view(texture.view, None) };
    // SAFETY: as above.
    unsafe { device.destroy_image(texture.image, None) };
    // SAFETY: as above, and the image that used it is already destroyed.
    unsafe { device.free_memory(texture.memory, None) };
    // SAFETY: as above.
    unsafe { device.destroy_buffer(texture.staging, None) };
    // SAFETY: as above, and the buffer that used it is already destroyed.
    unsafe { device.free_memory(texture.staging_memory, None) };
}

/// Everything one recording renders into and copies out of.
struct Target {
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    image: vk::Image,
    buffer: vk::Buffer,
    width: u32,
    height: u32,
}

/// Writes `bytes` into the start of a host-visible, coherent allocation at least that large - a
/// draw's starting pixels, into the buffer that carries them to the attachment (worklog 822).
fn fill_host_memory(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    bytes: &[u8],
) -> Result<(), DispatchError> {
    let size = vk::DeviceSize::try_from(bytes.len()).unwrap_or(vk::DeviceSize::MAX);
    // SAFETY: the caller's allocation is host-visible, coherent, at least `size` bytes, and not mapped.
    let mapped = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    // SAFETY: the mapping covers `size` bytes, which is `bytes.len()`.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), mapped.cast::<u8>(), bytes.len()) };
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(memory) };
    Ok(())
}

/// Copies a draw's starting pixels from the target's buffer into its attachment (worklog 822).
///
/// The attachment moves from `UNDEFINED` - nothing it held matters, every texel is about to be
/// written - to `TRANSFER_DST_OPTIMAL`, which is the loading render pass's initial layout; that
/// pass's external dependency orders this copy before it reads the attachment.
fn copy_in(device: &ash::Device, command: vk::CommandBuffer, target: &Target) {
    let range = vk::ImageSubresourceRange::default()
        .aspect_mask(vk::ImageAspectFlags::COLOR)
        .level_count(1)
        .layer_count(1);
    let to_destination = [vk::ImageMemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::empty())
        .dst_access_mask(vk::AccessFlags::TRANSFER_WRITE)
        .old_layout(vk::ImageLayout::UNDEFINED)
        .new_layout(vk::ImageLayout::TRANSFER_DST_OPTIMAL)
        .src_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .dst_queue_family_index(vk::QUEUE_FAMILY_IGNORED)
        .image(target.image)
        .subresource_range(range)];
    // SAFETY: recording is open, the image is live, and nothing else has touched it.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            vk::PipelineStageFlags::TRANSFER,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &to_destination,
        );
    }
    let regions = [vk::BufferImageCopy::default()
        .buffer_row_length(0)
        .buffer_image_height(0)
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width: target.width,
            height: target.height,
            depth: 1,
        })];
    // SAFETY: recording is open; the buffer holds `width * height * 4` tightly packed bytes and the
    // image is in the transfer-destination layout the barrier above put it in.
    unsafe {
        device.cmd_copy_buffer_to_image(
            command,
            target.buffer,
            target.image,
            vk::ImageLayout::TRANSFER_DST_OPTIMAL,
            &regions,
        );
    }
}

/// Words in the user-data push-constant block (worklog 826).
pub(crate) const USER_DATA_BLOCK_WORDS: usize = orbistoun_gpu::USER_DATA_BLOCK_WORDS;

/// The block's size in bytes.
const USER_DATA_BLOCK_BYTES: u32 = (USER_DATA_BLOCK_WORDS * 4) as u32;

/// The stages that may read the user-data block: the fragment stage and whichever geometry stage the
/// pipeline has - naming the mesh stage only where one is built, so a device without it never sees it.
fn user_data_stages(geometry: Geometry) -> vk::ShaderStageFlags {
    vk::ShaderStageFlags::FRAGMENT
        | match geometry {
            Geometry::Vertex(_) => vk::ShaderStageFlags::VERTEX,
            Geometry::Mesh => vk::ShaderStageFlags::MESH_EXT,
        }
}

/// What binding a draw needs of its pipeline - plain handles, so a batch can hold them while the
/// pipeline itself goes back to the cache (D718).
#[derive(Debug, Clone, Copy)]
struct BindState {
    handle: vk::Pipeline,
    layout: vk::PipelineLayout,
    set: vk::DescriptorSet,
    geometry: Geometry,
    user_data: [u32; USER_DATA_BLOCK_WORDS],
    window_offset: u32,
}

impl Pipeline {
    fn bind_state(&self) -> BindState {
        BindState {
            handle: self.handle,
            layout: self.layout,
            set: self.set,
            geometry: self.geometry,
            user_data: self.user_data,
            window_offset: self.window_offset,
        }
    }
}

/// Binds what a draw runs with, inside the open render pass: the pipeline, its descriptor set with
/// the window's and the draw data's offsets, and its user-data block.
fn bind_for_draw(device: &ash::Device, command: vk::CommandBuffer, state: &BindState, draws: u32) {
    // SAFETY: a render pass is open and the pipeline was built for it.
    unsafe {
        device.cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, state.handle);
    }
    // The set the fragment module's storage buffers live in. Bound whether or not this
    // particular module writes them: a pipeline that statically uses a set needs one
    // bound, and which sets a translated module uses is not something this harness reads.
    let sets = [state.set];
    // Which slot of the window ring this draw reads (worklog 847), and where its batch's draw data
    // starts (D718) - in binding order.
    let offsets = [state.window_offset, draws];
    // SAFETY: the set was allocated against this pipeline's layout and both are live; the two
    // dynamic bindings take the two offsets, each aligned as its binding requires.
    unsafe {
        device.cmd_bind_descriptor_sets(
            command,
            vk::PipelineBindPoint::GRAPHICS,
            state.layout,
            0,
            &sets,
            &offsets,
        );
    }
    // The draw's user data, where its shaders read it at entry (worklog 826).
    let block = zerocopy::IntoBytes::as_bytes(&state.user_data);
    // SAFETY: recording is open; the layout declares a push-constant range of exactly this many
    // bytes at offset zero for exactly these stages.
    unsafe {
        device.cmd_push_constants(
            command,
            state.layout,
            user_data_stages(state.geometry),
            0,
            block,
        );
    }
}

/// A run of guest draws that share everything but their geometry stage's user data, recorded as
/// one mesh dispatch of one workgroup each (D718). Held open in the pass until a draw that differs,
/// or anything else recorded into the pass, closes it - so draws stay in the order they came.
#[derive(Debug)]
struct MeshBatch {
    key: BatchKey,
    state: BindState,
    draws: Vec<DrawWords>,
}

/// What must be the same for a draw to join a batch: the pipeline (which carries its shaders,
/// textures, blend, viewport and scissor), the attachment, the window slot and buffer, and the
/// fragment stage's user data - everything but the geometry stage's words.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BatchKey {
    pipeline: u64,
    framebuffer: vk::Framebuffer,
    window: (vk::Buffer, u32),
    fragment: DrawWords,
}

/// The geometry stage's share of a user-data block, and the fragment stage's.
pub(crate) fn split_user_data(block: &[u32; USER_DATA_BLOCK_WORDS]) -> (DrawWords, DrawWords) {
    let mut geometry = [0u32; DRAW_DATA_STRIDE_WORDS as usize];
    let mut fragment = [0u32; DRAW_DATA_STRIDE_WORDS as usize];
    let stride = DRAW_DATA_STRIDE_WORDS as usize;
    geometry.copy_from_slice(&block[..stride]);
    fragment.copy_from_slice(&block[stride..stride * 2]);
    (geometry, fragment)
}

/// Records a batch into `command`: its draw data written, then one dispatch of a workgroup per draw.
fn record_batch(
    instance: &ash::Instance,
    device: &ash::Device,
    command: vk::CommandBuffer,
    batch: &MeshBatch,
) -> Result<(), DispatchError> {
    let offset = write_draw_data(&batch.draws).ok_or(DispatchError::Unsupported(
        "the draw-data buffer had no room for a batch it was checked to have room for".to_owned(),
    ))?;
    bind_for_draw(device, command, &batch.state, offset);
    issue_draw(
        instance,
        device,
        command,
        batch.state.geometry,
        u32::try_from(batch.draws.len()).unwrap_or(u32::MAX),
    );
    Ok(())
}

/// How a recording's attachment starts and ends.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Starts {
    /// Cleared by its render pass, then copied out.
    Cleared,
    /// Copied in from the target's buffer, then copied out (worklog 822).
    Loaded,
    /// A [`ResidentAttachment`]: loaded in place and left there (worklog 839).
    Resident,
}

/// Draws once into a [`ResidentAttachment`] (worklog 839): the pipeline and its bindings are this
/// draw's, the attachment, its render pass and framebuffer are the resident one's and are left as
/// they are. Returns the guest-memory window and storage image the draw left, with no pixels - the
/// attachment keeps those until [`ResidentAttachment::read_back`].
fn render_resident(
    session: &crate::compute::Session,
    resident: &ResidentAttachment,
    shaders: Option<(&[u32], &[u32])>,
    geometry: Geometry,
    bound: Bound<'_>,
) -> Result<Drawn, DispatchError> {
    let (instance, physical, device) = (&session.instance, session.physical, &session.device);
    orbistoun_gpu::perf::count(orbistoun_gpu::perf::Count::Draws);
    // **A pipeline built for an earlier draw with the same key is reused** (worklog 843): building
    // and releasing one per draw was 80% of a frame. Only with a resident guest-memory buffer, which
    // is re-bound here because the buffer is the one thing the key leaves out.
    let cacheable = bound
        .pipeline_key
        .map(std::num::NonZeroU64::get)
        .zip(bound.guest_buffer.copied());
    let cached = cacheable.and_then(|(key, guest)| {
        let mut built = cached_pipelines().lock().ok()?.remove(&key)?;
        if built.buffers[1].0 != guest.buffer {
            // A set may not be updated while a recorded draw still uses it.
            settle(session).ok()?;
            rebind_guest_memory(device, &built, guest, bound.windows[1]);
            // And recorded as its binding 1: the buffer it was built with belonged to an earlier
            // window and is gone.
            built.buffers[1] = (guest.buffer, guest.memory);
        }
        // The slot this draw's window is in - an offset at bind, not a change to the set.
        built.window_offset = u32::try_from(guest.offset).unwrap_or(0);
        built.geometry = geometry;
        built.user_data = *bound.user_data;
        Some(built)
    });
    let pipeline =
        orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::WholePipeline, || match cached {
            Some(built) => Ok(Some(built)),
            None => build_resident_pipeline(
                Devices {
                    instance,
                    physical,
                    device,
                },
                resident,
                shaders,
                geometry,
                bound,
            ),
        })?;
    orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::WholeRecord, || {
        render_resident_with(session, resident, pipeline, cacheable.map(|(key, _)| key))
    })
}

/// Pipelines resident draws built, by the key the backend gave them (worklog 843). Kept across
/// draws and submissions; emptied when it passes [`PIPELINE_CACHE_LIMIT`].
fn cached_pipelines() -> &'static std::sync::Mutex<std::collections::HashMap<u64, Pipeline>> {
    static CACHE: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<u64, Pipeline>>> =
        std::sync::OnceLock::new();
    CACHE.get_or_init(Default::default)
}

/// The command buffers of resident draws submitted and not yet waited on (worklog 843), all from
/// [`shared_pool`].
fn in_flight() -> &'static std::sync::Mutex<Vec<vk::CommandBuffer>> {
    static BUFFERS: std::sync::Mutex<Vec<vk::CommandBuffer>> = std::sync::Mutex::new(Vec::new());
    &BUFFERS
}

/// The one command pool resident draws allocate from (worklog 844): a pool made and destroyed for
/// every draw was a third of a draw's cost. Made on first use and kept for the process, like the
/// session it belongs to.
fn shared_pool(device: &ash::Device, family: u32) -> Result<vk::CommandPool, DispatchError> {
    static POOL: std::sync::Mutex<Option<vk::CommandPool>> = std::sync::Mutex::new(None);
    let mut pool = POOL
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(pool) = *pool {
        return Ok(pool);
    }
    let info = vk::CommandPoolCreateInfo::default().queue_family_index(family);
    // SAFETY: the device is live and the create info outlives the call.
    let made = unsafe { device.create_command_pool(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool", e))?;
    *pool = Some(made);
    Ok(made)
}

/// The render pass resident draws are being recorded into (worklog 844): one command buffer and one
/// pass for as many draws as land on one attachment in a row, rather than a pass per draw - which
/// loaded and stored the whole attachment around every draw, twelve thousand times a frame.
struct OpenPass {
    command: vk::CommandBuffer,
    /// Which attachment it draws on.
    framebuffer: vk::Framebuffer,
    /// The pass's first timestamp query, when the device clock stamps it (worklog 847).
    clock: Option<u32>,
    /// Draws waiting to be recorded as one dispatch (D718).
    batch: Option<MeshBatch>,
}

fn open_pass() -> &'static std::sync::Mutex<Option<OpenPass>> {
    static OPEN: std::sync::Mutex<Option<OpenPass>> = std::sync::Mutex::new(None);
    &OPEN
}

/// The device's own clock around each pass of resident draws (worklog 847): how long the GPU was
/// busy, which the host's timers cannot see - a host that waits at a flip has measured the wait, not
/// the work. Two timestamps per pass, read when the device is idle.
struct GpuClock {
    pool: vk::QueryPool,
    /// Nanoseconds per timestamp tick.
    period: f64,
    /// The next unused query.
    next: u32,
    /// The first query of each pass stamped since the last read.
    pairs: Vec<u32>,
}

/// How many timestamps a clock holds between reads: a read happens at every flip, and a frame is a
/// handful of passes, so this is never reached in practice - and when it is, passes go unstamped.
const GPU_QUERIES: u32 = 4096;

fn gpu_clock() -> &'static std::sync::Mutex<Option<GpuClock>> {
    static CLOCK: std::sync::Mutex<Option<GpuClock>> = std::sync::Mutex::new(None);
    &CLOCK
}

/// Stamps the start of a pass into `command`, answering the pair's first query - or `None` when the
/// queue cannot take timestamps or the clock is full. Recorded outside the render pass.
fn gpu_clock_start(session: &crate::compute::Session, command: vk::CommandBuffer) -> Option<u32> {
    let device = &session.device;
    let mut clock = gpu_clock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if clock.is_none() {
        // SAFETY: the physical device belongs to the live instance.
        let families = unsafe {
            session
                .instance
                .get_physical_device_queue_family_properties(session.physical)
        };
        let valid = families
            .get(session.family as usize)
            .is_some_and(|family| family.timestamp_valid_bits > 0);
        if !valid {
            return None;
        }
        // SAFETY: as above.
        let limits = unsafe {
            session
                .instance
                .get_physical_device_properties(session.physical)
        }
        .limits;
        let info = vk::QueryPoolCreateInfo::default()
            .query_type(vk::QueryType::TIMESTAMP)
            .query_count(GPU_QUERIES);
        // SAFETY: the device is live and the create info outlives the call.
        let pool = unsafe { device.create_query_pool(&info, None) }.ok()?;
        *clock = Some(GpuClock {
            pool,
            period: f64::from(limits.timestamp_period),
            next: 0,
            pairs: Vec::new(),
        });
    }
    let clock = clock.as_mut()?;
    if clock.next + 2 > GPU_QUERIES {
        return None;
    }
    let first = clock.next;
    clock.next += 2;
    // SAFETY: recording is open and outside any render pass; the two queries are this pass's own.
    unsafe { device.cmd_reset_query_pool(command, clock.pool, first, 2) };
    // SAFETY: as above; the first query was just reset.
    unsafe {
        device.cmd_write_timestamp(
            command,
            vk::PipelineStageFlags::TOP_OF_PIPE,
            clock.pool,
            first,
        );
    }
    Some(first)
}

/// Stamps the end of a pass whose start was `first`.
fn gpu_clock_end(device: &ash::Device, command: vk::CommandBuffer, first: u32) {
    let mut clock = gpu_clock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(clock) = clock.as_mut() {
        // SAFETY: recording is open, the render pass has ended, and the query was reset with its pair.
        unsafe {
            device.cmd_write_timestamp(
                command,
                vk::PipelineStageFlags::BOTTOM_OF_PIPE,
                clock.pool,
                first + 1,
            );
        }
        clock.pairs.push(first);
    }
}

/// Adds every stamped pass's device time to the `gpu` phase and starts the clock again. Called with
/// the device idle, so every stamp has been written.
fn gpu_clock_read(device: &ash::Device) {
    let mut clock = gpu_clock()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(clock) = clock.as_mut() else {
        return;
    };
    let mut ticks = 0u64;
    for &first in &clock.pairs {
        let mut stamps = [0u64; 2];
        // SAFETY: both queries were written by a submit the idle device has finished; the output
        // holds exactly two 64-bit results.
        let read = unsafe {
            device.get_query_pool_results(
                clock.pool,
                first,
                &mut stamps,
                vk::QueryResultFlags::TYPE_64,
            )
        };
        if read.is_ok() {
            ticks += stamps[1].saturating_sub(stamps[0]);
        }
    }
    clock.pairs.clear();
    clock.next = 0;
    // Nanoseconds as a float product, then whole: a tick period is a fraction on some devices.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let nanos = (ticks as f64 * clock.period) as u64;
    orbistoun_gpu::perf::add(
        orbistoun_gpu::perf::Phase::Gpu,
        std::time::Duration::from_nanos(nanos),
    );
}

/// How many slots the guest-memory window rotates through (worklog 847): a submission writes the
/// next slot while the device may still be drawing from the last two.
pub(crate) const WINDOW_SLOTS: usize = 3;

/// The fences of the submits made while each window slot was current, and which slot that is. A slot
/// is written again only once its own fences have signalled - not once the whole device is idle.
struct SlotFences {
    current: usize,
    fences: [Vec<vk::Fence>; WINDOW_SLOTS],
}

fn slot_fences() -> &'static std::sync::Mutex<SlotFences> {
    static SLOTS: std::sync::Mutex<SlotFences> = std::sync::Mutex::new(SlotFences {
        current: 0,
        fences: [Vec::new(), Vec::new(), Vec::new()],
    });
    &SLOTS
}

/// A fence for a submit, kept against the window slot current as it is made.
fn submit_fence(device: &ash::Device) -> Result<vk::Fence, DispatchError> {
    // SAFETY: the device is live and the create info outlives the call.
    let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
        .map_err(|e| DispatchError::Vulkan("create_fence", e))?;
    let mut slots = slot_fences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let current = slots.current;
    slots.fences[current].push(fence);
    Ok(fence)
}

/// Makes `slot` the window slot draws read, once the device has finished every submit made while it
/// was last current (worklog 847). What has been recorded so far is submitted first, against the
/// slot it read. Waits only when the device is `WINDOW_SLOTS` submissions behind, which it rarely is.
///
/// # Errors
///
/// When a submit or the wait fails.
pub(crate) fn enter_window_slot(
    session: &crate::compute::Session,
    slot: usize,
) -> Result<(), DispatchError> {
    close_open_pass(session)?;
    let device = &session.device;
    let mut slots = slot_fences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let waiting = std::mem::take(&mut slots.fences[slot]);
    if !waiting.is_empty() {
        // SAFETY: every fence is live, created on this device and submitted with a queue submit.
        unsafe { device.wait_for_fences(&waiting, true, u64::MAX) }
            .map_err(|e| DispatchError::Vulkan("wait_for_fences", e))?;
        for fence in waiting {
            // SAFETY: signalled, so no pending submit uses it; taken out of the list, so destroyed once.
            unsafe { device.destroy_fence(fence, None) };
        }
    }
    slots.current = slot;
    Ok(())
}

/// Destroys every slot's fences - called when the device is idle, so all have signalled.
fn forget_slot_fences(device: &ash::Device) {
    let mut slots = slot_fences()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    for fences in &mut slots.fences {
        for fence in std::mem::take(fences) {
            // SAFETY: the device is idle, so the fence has signalled and no submit uses it.
            unsafe { device.destroy_fence(fence, None) };
        }
    }
}

/// Ends the open pass, if there is one, and submits it - without waiting. The session is held.
fn close_open_pass(session: &crate::compute::Session) -> Result<(), DispatchError> {
    let Some(open) = open_pass()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .take()
    else {
        return Ok(());
    };
    let device = &session.device;
    // The batch still waiting is the pass's last work (D718).
    if let Some(batch) = &open.batch {
        record_batch(&session.instance, device, open.command, batch)?;
    }
    // SAFETY: the pass and the recording were opened by `begin_pass` and nothing else ended them.
    unsafe { device.cmd_end_render_pass(open.command) };
    if let Some(first) = open.clock {
        gpu_clock_end(device, open.command, first);
    }
    // SAFETY: recording is open.
    unsafe { device.end_command_buffer(open.command) }
        .map_err(|e| DispatchError::Vulkan("end_command_buffer", e))?;
    let buffers = [open.command];
    let submits = [vk::SubmitInfo::default().command_buffers(&buffers)];
    let fence = submit_fence(device)?;
    // SAFETY: recording has ended, the queue belongs to this device, and the fence is new.
    unsafe { device.queue_submit(session.queue, &submits, fence) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))?;
    if let Ok(mut pending) = in_flight().lock() {
        pending.push(open.command);
    }
    Ok(())
}

/// The command buffer draws on `resident` record into: the open pass when it is on this attachment,
/// else a new one - closing any other first, so draws stay in the order they were asked for.
fn pass_on(
    session: &crate::compute::Session,
    resident: &ResidentAttachment,
) -> Result<vk::CommandBuffer, DispatchError> {
    if let Some(open) = open_pass()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .as_ref()
        && open.framebuffer == resident.framebuffer
    {
        return Ok(open.command);
    }
    close_open_pass(session)?;
    let device = &session.device;
    let allocate = vk::CommandBufferAllocateInfo::default()
        .command_pool(shared_pool(device, session.family)?)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: the pool is live on this device.
    let command = unsafe { device.allocate_command_buffers(&allocate) }
        .map_err(|e| DispatchError::Vulkan("allocate_command_buffers", e))?[0];
    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: the command buffer was just allocated and is not recording.
    unsafe { device.begin_command_buffer(command, &begin) }
        .map_err(|e| DispatchError::Vulkan("begin_command_buffer", e))?;
    // After everything submitted before it: the attachment, the window and the storage images were
    // all written by what came before.
    let after_all = [vk::MemoryBarrier::default()
        .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
        .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)];
    // SAFETY: recording is open.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::PipelineStageFlags::ALL_COMMANDS,
            vk::DependencyFlags::empty(),
            &after_all,
            &[],
            &[],
        );
    }
    let clock = gpu_clock_start(session, command);
    let (width, height) = resident.extent();
    let pass_begin = vk::RenderPassBeginInfo::default()
        .render_pass(resident.render_pass)
        .framebuffer(resident.framebuffer)
        .render_area(vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D { width, height },
        });
    // SAFETY: recording is open; the render pass and framebuffer are the attachment's own and live,
    // and the pass loads what the attachment holds.
    unsafe { device.cmd_begin_render_pass(command, &pass_begin, vk::SubpassContents::INLINE) };
    *open_pass()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner) = Some(OpenPass {
        command,
        framebuffer: resident.framebuffer,
        clock,
        batch: None,
    });
    Ok(command)
}

/// Records the open pass's batch, if it has one, so what is recorded next comes after it (D718).
fn flush_batch(session: &crate::compute::Session) -> Result<(), DispatchError> {
    let mut open = open_pass()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(open) = open.as_mut() else {
        return Ok(());
    };
    match open.batch.take() {
        Some(batch) => record_batch(&session.instance, &session.device, open.command, &batch),
        None => Ok(()),
    }
}

/// Adds a mesh draw to the open pass's batch - recording the batch there first when this draw cannot
/// join it (D718). The pass must already be on `framebuffer` ([`pass_on`]).
fn batch_draw(
    session: &crate::compute::Session,
    key: BatchKey,
    state: BindState,
    draw: DrawWords,
) -> Result<(), DispatchError> {
    let mut open = open_pass()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(open) = open.as_mut() else {
        return Err(DispatchError::Unsupported(
            "a batched draw with no pass open to record it in".to_owned(),
        ));
    };
    if let Some(batch) = open.batch.as_mut()
        && batch.key == key
        && batch.draws.len() < DRAW_DATA_MOST_DRAWS as usize
    {
        batch.draws.push(draw);
        return Ok(());
    }
    if let Some(batch) = open.batch.take() {
        record_batch(&session.instance, &session.device, open.command, &batch)?;
    }
    open.batch = Some(MeshBatch {
        key,
        state,
        draws: vec![draw],
    });
    Ok(())
}

/// Adds a draw to the open batch without touching the pipeline or the session, when the pass is on
/// `key`'s attachment, the batch is `key`'s and has room (D718) - `false` when it cannot, and the draw
/// takes the whole path instead.
pub(crate) fn join_open_batch(key: &BatchKey, draw: DrawWords) -> bool {
    let mut open = open_pass()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(open) = open.as_mut() else {
        return false;
    };
    let Some(batch) = open.batch.as_mut() else {
        return false;
    };
    if open.framebuffer != key.framebuffer
        || batch.key != *key
        || batch.draws.len() >= DRAW_DATA_MOST_DRAWS as usize
        || !draw_data_room(batch.draws.len() + 1)
    {
        return false;
    }
    batch.draws.push(draw);
    // A draw carried out is a draw, whether it took the whole path or joined a batch.
    orbistoun_gpu::perf::count(orbistoun_gpu::perf::Count::Draws);
    true
}

/// Runs every resident draw recorded or submitted so far, waits for them, and frees their command
/// buffers.
///
/// Called, with the session held, wherever the host touches what those draws use: reading a buffer
/// or target back, replacing the guest-memory window, updating or releasing a cached pipeline.
///
/// # Errors
///
/// When the submit or the wait fails.
pub(crate) fn settle(session: &crate::compute::Session) -> Result<(), DispatchError> {
    close_open_pass(session)?;
    let device = &session.device;
    let buffers = in_flight()
        .lock()
        .map(|mut pending| std::mem::take(&mut *pending))
        .unwrap_or_default();
    // SAFETY: the device is live and the caller holds the session, so nothing submits meanwhile.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;
    forget_slot_fences(device);
    gpu_clock_read(device);
    // Nothing recorded reads the draw data now (D718).
    reset_draw_data();
    if buffers.is_empty() {
        return Ok(());
    }
    // Only resident draws put buffers here, and only after the pool exists.
    let pool = shared_pool(device, 0)?;
    // SAFETY: the device is idle, so none of the buffers is in use; each was allocated from the
    // shared pool and is freed once, having been taken out of the list.
    unsafe { device.free_command_buffers(pool, &buffers) };
    Ok(())
}

/// Submits the draws recorded so far without waiting for them (D714) - so the device works on a
/// submission while the guest builds the next, and nothing waits until something reads the frame.
///
/// # Errors
///
/// When there is no device, or the submit fails.
pub(crate) fn submit_recorded() -> Result<(), DispatchError> {
    let session = crate::compute::session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    close_open_pass(&session)
}

/// [`settle`], taking the session - for a caller that does not hold it.
///
/// # Errors
///
/// When there is no device, or the wait fails.
pub(crate) fn settle_session() -> Result<(), DispatchError> {
    let session = crate::compute::session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    settle(&session)
}

/// How many pipelines the cache keeps before it releases them all. A frame uses tens; a limit
/// keeps a title whose textures change every frame from growing device memory without end.
const PIPELINE_CACHE_LIMIT: usize = 512;

/// Points a cached pipeline's binding 1 at this draw's guest-memory buffer.
fn rebind_guest_memory(
    device: &ash::Device,
    built: &Pipeline,
    guest: DispatchBuffer,
    words: usize,
) {
    let memory = [vk::DescriptorBufferInfo::default()
        .buffer(guest.buffer)
        .offset(0)
        .range(words_to_bytes(words))];
    let writes = [vk::WriteDescriptorSet::default()
        .dst_set(built.set)
        .dst_binding(1)
        .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
        .buffer_info(&memory)];
    // SAFETY: the set is live and not in use - the caller settled every recorded draw first - and
    // the buffer info outlives the call.
    unsafe { device.update_descriptor_sets(&writes, &[]) };
}

/// Builds a resident draw's pipeline, seeding its own guest memory when it made one.
fn build_resident_pipeline(
    devices: Devices<'_>,
    resident: &ResidentAttachment,
    shaders: Option<(&[u32], &[u32])>,
    geometry: Geometry,
    bound: Bound<'_>,
) -> Result<Option<Pipeline>, DispatchError> {
    let (width, height) = resident.extent();
    let (instance, physical, device) = (devices.instance, devices.physical, devices.device);
    shaders
        .map(|(vertex, fragment)| {
            orbistoun_gpu::perf::measure(orbistoun_gpu::perf::Phase::Build, || {
                build_pipeline(
                    Devices {
                        instance,
                        physical,
                        device,
                    },
                    resident.render_pass,
                    (vertex, fragment),
                    (width, height),
                    geometry,
                    bound,
                )
            })
            .and_then(|built| {
                if built.owns_guest_memory {
                    orbistoun_gpu::perf::measure(orbistoun_gpu::perf::Phase::Seed, || {
                        seed_memory(device, &built, bound.memory)
                    })?;
                }
                Ok(built)
            })
        })
        .transpose()
}

/// Records, runs and reads back a resident draw with `pipeline`, then keeps the pipeline under
/// `key` when it has one - or releases it when it does not.
fn render_resident_with(
    session: &crate::compute::Session,
    resident: &ResidentAttachment,
    pipeline: Option<Pipeline>,
    key: Option<u64>,
) -> Result<Drawn, DispatchError> {
    let (instance, device) = (&session.instance, &session.device);
    let nothing = || Pixels {
        width: 0,
        height: 0,
        bytes: Vec::new(),
    };
    let drawn = || Drawn {
        pixels: nothing(),
        memory: Vec::new(),
        stored: nothing(),
    };
    // A draw with no shaders leaves a resident attachment as it is.
    let Some(mut built) = pipeline else {
        return Ok(drawn());
    };
    // **A pipeline's own setup happens once, outside any pass** (worklog 844): its textures copied in
    // and its storage image made writable. A render pass may not contain a transfer, and a pipeline
    // taken from the cache has had this done already.
    if !built.textures_uploaded {
        one_shot(session, |device, command| {
            upload_texture(device, command, &built.texture);
            upload_texture(device, command, &built.second_texture);
            open_storage_image(device, command, &built.storage_image);
        })?;
        built.textures_uploaded = true;
    }
    // **Recorded into the open pass, not submitted** (worklog 844): draws on one attachment share a
    // command buffer and a render pass, and run when something needs what they drew - [`settle`].
    // Nothing is read back per draw either (worklog 843): the window is read once when asked for
    // ([`read_buffer`]), and nothing reads a resident draw's storage image.
    orbistoun_gpu::perf::measure(orbistoun_gpu::perf::Phase::Execute, || {
        ensure_draw_room(session)?;
        let command = pass_on(session, resident)?;
        let (geometry_words, fragment) = split_user_data(&built.user_data);
        // **A cached mesh pipeline's draw joins a batch** (D718), recorded when a draw that
        // differs or anything else comes along.
        if let (Geometry::Mesh, Some(key)) = (built.geometry, key) {
            batch_draw(
                session,
                BatchKey {
                    pipeline: key,
                    framebuffer: resident.framebuffer,
                    window: (built.buffers[1].0, built.window_offset),
                    fragment,
                },
                built.bind_state(),
                geometry_words,
            )?;
        } else {
            flush_batch(session)?;
            let offset = write_draw_data(&[geometry_words]).ok_or(DispatchError::Unsupported(
                "the draw-data buffer had no room for one draw after it was made room for"
                    .to_owned(),
            ))?;
            bind_for_draw(device, command, &built.bind_state(), offset);
            issue_draw(instance, device, command, built.geometry, 1);
        }
        Ok::<_, DispatchError>(())
    })?;
    orbistoun_gpu::perf::measure(orbistoun_gpu::perf::Phase::Release, || {
        let Some(key) = key else {
            settle(session)?;
            release_pipeline(device, &built);
            return Ok(());
        };
        let Ok(mut cache) = cached_pipelines().lock() else {
            settle(session)?;
            release_pipeline(device, &built);
            return Ok(());
        };
        if cache.len() >= PIPELINE_CACHE_LIMIT {
            settle(session)?;
            for (_, old) in cache.drain() {
                release_pipeline(device, &old);
            }
        }
        if let Some(replaced) = cache.insert(key, built) {
            settle(session)?;
            release_pipeline(device, &replaced);
        }
        Ok::<_, DispatchError>(())
    })?;
    Ok(drawn())
}

/// Issues a pipeline's draw into an open render pass, with the pipeline and its bindings bound.
fn issue_draw(
    instance: &ash::Instance,
    device: &ash::Device,
    command: vk::CommandBuffer,
    geometry: Geometry,
    draws: u32,
) {
    match geometry {
        // The guest's decoded count: a vertex shader is called once per vertex, indexing its
        // source by `gl_VertexIndex`, so this is how much geometry it draws. `first_instance`
        // stays zero - nothing decodes it yet, and zero is what it means.
        // SAFETY: a pipeline is bound inside an open render pass.
        Geometry::Vertex(draw) => unsafe {
            device.cmd_draw(command, draw.vertices, draw.instances, draw.first_vertex, 0);
        },
        // **One workgroup per guest draw**: a mesh shader decides for itself how many vertices
        // and primitives come out of it, so the count here is groups rather than vertices, and a
        // guest's primitive shader is a single wave that announces what it will emit and then
        // emits it. A batch of draws is that many workgroups, each reading its own draw's words
        // (D718) - and the primitives of a lower workgroup reach the rasteriser before those of a
        // higher one (the Vulkan specification's mesh shader primitive ordering), so the batch
        // blends exactly as its draws one by one would.
        Geometry::Mesh => {
            // The extension's entry points, looked up once for the process's one device
            // rather than on every draw (worklog 844).
            static MESH: std::sync::OnceLock<ash::ext::mesh_shader::Device> =
                std::sync::OnceLock::new();
            let mesh = MESH.get_or_init(|| ash::ext::mesh_shader::Device::new(instance, device));
            // SAFETY: a mesh pipeline is bound inside an open render pass, and a mesh
            // pipeline exists only where the device was created with the extension this
            // loader dispatches through.
            unsafe { mesh.cmd_draw_mesh_tasks(command, draws, 1, 1) };
        }
    }
}

/// Reads a resident buffer's words back from the device - the guest-memory window after a frame's
/// resident draws, read once rather than after each (worklog 843).
///
/// # Errors
///
/// When no device is available or the mapping fails.
pub(crate) fn read_buffer(buffer: &DispatchBuffer) -> Result<Vec<u32>, DispatchError> {
    let session = crate::compute::session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    // Every draw that wrote the buffer, finished.
    settle(&session)?;
    orbistoun_gpu::perf::measure(orbistoun_gpu::perf::Phase::ReadBack, || {
        crate::compute::read_back(&session.device, buffer)
    })
}

/// A command pool and the one buffer recorded into it.
struct Recorded {
    pool: vk::CommandPool,
    buffer: vk::CommandBuffer,
}

/// Records the clear, an optional draw, and the copy out.
///
/// With no pipeline the render pass does the whole of the work: `LOAD_OP_CLEAR` writes the
/// colour and the final layout transitions the image for the copy, so nothing is drawn and no
/// barrier is recorded by hand. With one, three vertices are drawn between those two points -
/// enough for a triangle, and the vertex shader is expected to produce its own positions.
fn record(
    instance: &ash::Instance,
    device: &ash::Device,
    family: u32,
    target: &Target,
    (colour, starts): ([f32; 4], Starts),
    pipeline: Option<&Pipeline>,
) -> Result<Recorded, DispatchError> {
    let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(family);
    // SAFETY: the device is live and the create info outlives the call.
    let pool = unsafe { device.create_command_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool", e))?;
    let allocate = vk::CommandBufferAllocateInfo::default()
        .command_pool(pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: the pool was just created on this device.
    let buffers = unsafe { device.allocate_command_buffers(&allocate) }
        .map_err(|e| DispatchError::Vulkan("allocate_command_buffers", e))?;
    let command = buffers[0];

    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: the command buffer was just allocated and is not recording.
    unsafe { device.begin_command_buffer(command, &begin) }
        .map_err(|e| DispatchError::Vulkan("begin_command_buffer", e))?;

    // **After everything submitted before it** (worklog 843): resident draws are no longer waited on
    // one by one, so each orders itself after the last - its attachment, the guest-memory window and
    // the storage image are all written by the draw before.
    if starts == Starts::Resident {
        let after_all = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::MEMORY_WRITE)];
        // SAFETY: recording is open.
        unsafe {
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::PipelineStageFlags::ALL_COMMANDS,
                vk::DependencyFlags::empty(),
                &after_all,
                &[],
                &[],
            );
        }
    }
    // The texture, copied in and made readable, before the render pass that samples it opens -
    // a render pass cannot contain a transfer. Recorded for every pipeline, because every
    // pipeline has a texture whether or not its fragment module reads one - once: a pipeline reused
    // from the cache already holds its textures (worklog 843).
    if let Some(built) = pipeline {
        if !built.textures_uploaded {
            upload_texture(device, command, &built.texture);
            upload_texture(device, command, &built.second_texture);
        }
        open_storage_image(device, command, &built.storage_image);
    }
    // The starting pixels, copied from the readback buffer (which holds them until the copy out
    // overwrites it) into the attachment, leaving it in the layout the loading pass expects.
    if starts == Starts::Loaded {
        copy_in(device, command, target);
    }

    let clears = [vk::ClearValue {
        color: vk::ClearColorValue { float32: colour },
    }];
    let pass_begin = vk::RenderPassBeginInfo::default()
        .render_pass(target.render_pass)
        .framebuffer(target.framebuffer)
        .render_area(vk::Rect2D {
            offset: vk::Offset2D { x: 0, y: 0 },
            extent: vk::Extent2D {
                width: target.width,
                height: target.height,
            },
        })
        .clear_values(&clears);
    // SAFETY: recording is open; the render pass and framebuffer are live and agree about the
    // attachment.
    unsafe { device.cmd_begin_render_pass(command, &pass_begin, vk::SubpassContents::INLINE) };
    if let Some(built) = pipeline {
        let (geometry_words, _) = split_user_data(&built.user_data);
        let offset = write_one_draw_waiting(device, geometry_words)?;
        bind_for_draw(device, command, &built.bind_state(), offset);
        issue_draw(instance, device, command, built.geometry, 1);
    }
    // SAFETY: a render pass is open.
    unsafe { device.cmd_end_render_pass(command) };

    // The storage image, copied out now the render pass that wrote it has ended. After the pass
    // rather than inside it, because a render pass may not contain a transfer. Not for a resident
    // draw, whose storage image nothing reads: an attachment-sized copy after every draw was most
    // of a resident draw's device work (worklog 844).
    if let Some(built) = pipeline
        && starts != Starts::Resident
    {
        read_storage_image(device, command, &built.storage_image);
    }

    // A resident attachment stays where it is; it is read back when its pixels are asked for.
    if starts != Starts::Resident {
        let regions = [whole_image(target.width, target.height)];
        // SAFETY: recording is open, the render pass left the image in TRANSFER_SRC_OPTIMAL, and
        // the buffer is large enough for the region by construction.
        unsafe {
            device.cmd_copy_image_to_buffer(
                command,
                target.image,
                vk::ImageLayout::TRANSFER_SRC_OPTIMAL,
                target.buffer,
                &regions,
            );
        }
    }
    // SAFETY: recording is open.
    unsafe { device.end_command_buffer(command) }
        .map_err(|e| DispatchError::Vulkan("end_command_buffer", e))?;

    Ok(Recorded {
        pool,
        buffer: command,
    })
}

/// The entry point every module here declares.
const ENTRY: &std::ffi::CStr = c"main";

/// A graphics pipeline and the shader modules it was built from.
struct Pipeline {
    vertex: vk::ShaderModule,
    fragment: vk::ShaderModule,
    layout: vk::PipelineLayout,
    handle: vk::Pipeline,
    /// The set layout, pool and set the fragment module's storage buffers are bound through.
    ///
    /// Every module this project's translator emits declares two of them - the observation
    /// window and the guest-memory window - because the models declare both in their headers.
    /// The draw path used an empty pipeline layout and bound nothing, which the Khronos
    /// validation layer calls two separate errors: a layout that does not declare what the
    /// shader uses, and a draw with no set bound (worklog 554). It drew anyway on this
    /// driver, which is what made it worth measuring rather than assuming.
    set_layout: vk::DescriptorSetLayout,
    descriptor_pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
    buffers: [(vk::Buffer, vk::DeviceMemory); 2],
    /// The sampled image bound at binding 2, which a guest's textured pixel shader needs and
    /// every other module ignores.
    texture: Texture,
    /// The sampled image bound at binding 4: a pixel shader's second texture (worklog 840).
    second_texture: Texture,
    /// The storage image bound at binding 3, which a guest's `image_store` writes.
    ///
    /// Created for every pipeline for the same reason the texture is: which bindings a
    /// translated module declares is not something this harness reads, and a binding a module
    /// uses and the layout lacks is an invalid pipeline.
    storage_image: StorageImage,
    /// Which stage feeds the fragment stage: the two are drawn by different calls, and the
    /// pipeline is the thing that knows which it was built for.
    geometry: Geometry,
    /// Whether binding 1 (the guest-memory buffer, `buffers[1]`) was created here. `false` when a
    /// caller bound its own resident buffer (D703), so `release` leaves it for its owner rather than
    /// destroying a buffer this pipeline does not own.
    owns_guest_memory: bool,
    /// The draw's user-data block, pushed before it (worklog 826).
    user_data: [u32; USER_DATA_BLOCK_WORDS],
    /// Whether [`Self::texture`] and [`Self::second_texture`] already hold their texels on the
    /// device - true once a cached pipeline has drawn, so its reuse copies nothing (worklog 843).
    textures_uploaded: bool,
    /// The dynamic offset of binding 1 - which slot of the window ring the draw reads (worklog
    /// 847). Zero for a pipeline that made its own guest-memory buffer.
    window_offset: u32,
}

/// An attachment dimension as a float, exactly.
///
/// Every value that reaches this is a pixel count bounded by the device's maximum image
/// dimension, which is far inside the integers `f32` holds exactly - so this is a widening in
/// practice, and saying so is cheaper than a cast nobody can check at the call site.
// The debug assertion below is the check the lint asks for, made where the value is known
// rather than at every call site.
#[allow(clippy::cast_precision_loss)]
fn f32_from(pixels: u32) -> f32 {
    debug_assert!(
        pixels <= (1 << f32::MANTISSA_DIGITS),
        "an attachment larger than f32 represents exactly: {pixels}"
    );
    pixels as f32
}

/// Builds a pipeline that draws a triangle list into the render pass's one attachment.
///
/// Fixed viewport and scissor rather than dynamic state: this draws once, into an attachment
/// whose size is known when the pipeline is built, and a dynamic state is a second place the
/// size could be wrong.
///
/// No vertex input. The vertex shader is expected to produce its own positions from
/// `gl_VertexIndex` - see `orbistoun_spirv::fullscreen_triangle_vertex_module` - which keeps a
/// vertex buffer, its memory and its binding description out of a harness whose whole job is to
/// have as few moving parts as possible.
/// The device handles the buffer helpers need, passed as one thing.
///
/// Three values that always travel together and are never chosen independently. Separately
/// they were three of nine parameters, which is past the point where a call site tells a
/// reader anything.
#[derive(Clone, Copy)]
struct Devices<'a> {
    instance: &'a ash::Instance,
    physical: vk::PhysicalDevice,
    device: &'a ash::Device,
}

// A pipeline is a linear sequence of create-infos, each needed by the one after it: the
// descriptor layout the pipeline layout names, the pool the set comes from, the stages the
// pipeline is built out of. Extracting halves means helpers passing six handles apiece, which
// moves the length rather than removing it and hides the order - the one property a builder
// has to show. The same judgement `Wavefront::for_stage` records for the same reason.
#[allow(clippy::too_many_lines)]
fn build_pipeline(
    devices: Devices<'_>,
    render_pass: vk::RenderPass,
    shaders: (&[u32], &[u32]),
    size: (u32, u32),
    geometry: Geometry,
    bound: Bound<'_>,
) -> Result<Pipeline, DispatchError> {
    let device = devices.device;
    let windows = bound.windows;
    let (vertex_words, fragment_words) = shaders;
    let (width, height) = size;
    let vertex_info = vk::ShaderModuleCreateInfo::default().code(vertex_words);
    // SAFETY: the words outlive the call and the device is live.
    let vertex = unsafe { device.create_shader_module(&vertex_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module(vertex)", e))?;
    let fragment_info = vk::ShaderModuleCreateInfo::default().code(fragment_words);
    // SAFETY: as above.
    let fragment = unsafe { device.create_shader_module(&fragment_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module(fragment)", e))?;

    // The two storage buffers every translated module declares, at set 0 bindings 0 and 1:
    // the observation window and the guest-memory window. A fragment module writes neither by
    // default - its epilogue is skipped (D553) - but a guest's pixel shader writes guest
    // memory, so the binding is real and has to exist.
    // Binding 0 (the observation window) is always created and owned here. Binding 1 (the guest
    // memory) is either a caller-owned resident buffer bound directly - uploaded once (D703) - or one
    // created and seeded here, the path every caller but the direct-bind one takes. `owns_guest_memory`
    // records which, so `release` destroys binding 1 only when this built it.
    let observation = create_storage_buffer(
        devices.instance,
        devices.physical,
        device,
        words_to_bytes(windows[0]),
    )?;
    let (guest_memory, owns_guest_memory) = match bound.guest_buffer {
        Some(resident) => ((resident.buffer, resident.memory), false),
        None => (
            create_storage_buffer(
                devices.instance,
                devices.physical,
                device,
                words_to_bytes(windows[1]),
            )?,
            true,
        ),
    };
    let buffers = [observation, guest_memory];
    let texture = create_texture(
        devices,
        bound.texture.0,
        bound.texture.1,
        bound.coarse_level,
    )?;
    // The second texture unit (worklog 840): a guest pixel shader that samples two textures reads
    // the second here. The default white texel when nothing asked for one.
    let second_texture = create_texture(
        devices,
        bound.second_texture.0,
        bound.second_texture.1,
        false,
    )?;
    let storage_image = create_storage_image(devices, STORAGE_IMAGE_SIZE)?;
    // One dispatch's span of the draw-data buffer; the batch's offset is given at bind (D718).
    let draw_data_info = [vk::DescriptorBufferInfo::default()
        .buffer(ensure_draw_data(devices)?)
        .offset(0)
        .range(DRAW_DATA_RANGE)];

    // Both stages, because either may use a binding: a guest's pixel shader writes its canary
    // and so does its primitive shader. Declaring only the fragment stage made a mesh pipeline
    // invalid, which the layer said and the picture did not (worklog 559).
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::MESH_EXT),
        // **Dynamic** (worklog 847): the guest-memory window is one slot of a ring, and which slot
        // is a per-draw offset given at bind - so a cached pipeline's set never changes as the
        // window moves between submissions, and nothing waits to change it.
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::MESH_EXT),
        // The fragment stage only. A sampled image is here because a guest's *pixel* shader
        // reads one; a primitive shader that fetched from a texture would be a second claim,
        // and declaring a stage nothing uses would assert it without measuring it.
        vk::DescriptorSetLayoutBinding::default()
            .binding(TEXTURE_BINDING)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        // A storage image, which is a different binding from the sampled one because they are
        // different things: one is read through a sampler and cannot be written, the other is
        // written and has no sampler. A guest's `image_store` names an image descriptor exactly
        // as its load does, and only the host cares that the two are separate.
        vk::DescriptorSetLayoutBinding::default()
            .binding(STORAGE_IMAGE_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(SECOND_TEXTURE_BINDING)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
        // Each draw's user data, for a mesh module that carries a batch of draws (D718): dynamic,
        // because every batch is at its own offset of the one buffer.
        vk::DescriptorSetLayoutBinding::default()
            .binding(DRAW_DATA_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::MESH_EXT),
    ];
    let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    // SAFETY: the create info outlives the call.
    let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?;

    let set_layouts = [set_layout];
    // The user-data block every stage may read at entry (worklog 826): 128 bytes, the size every
    // device takes, visible to the geometry stage this pipeline has and to the fragment stage. A
    // module that declares no block ignores it.
    let push_ranges = [vk::PushConstantRange::default()
        .stage_flags(user_data_stages(geometry))
        .offset(0)
        .size(USER_DATA_BLOCK_BYTES)];
    let layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&set_layouts)
        .push_constant_ranges(&push_ranges);
    // SAFETY: the create info outlives the call and the device is live.
    let layout = unsafe { device.create_pipeline_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_pipeline_layout", e))?;

    let pool_sizes = [
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
            .descriptor_count(2),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(2),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_IMAGE)
            .descriptor_count(1),
    ];
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(1);
    // SAFETY: the create info outlives the call.
    let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_pool", e))?;
    let allocate_sets = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&set_layouts);
    // SAFETY: the pool has room for exactly this one set.
    let sets = unsafe { device.allocate_descriptor_sets(&allocate_sets) }
        .map_err(|e| DispatchError::Vulkan("allocate_descriptor_sets", e))?;
    let set = sets[0];

    let observation = [vk::DescriptorBufferInfo::default()
        .buffer(buffers[0].0)
        .offset(0)
        .range(words_to_bytes(windows[0]))];
    let memory = [vk::DescriptorBufferInfo::default()
        .buffer(buffers[1].0)
        .offset(0)
        .range(words_to_bytes(windows[1]))];
    // The layout the barrier in `upload_texture` leaves the image in, named here because a
    // descriptor records the layout the image will be in when it is read, not the one it is in
    // when the descriptor is written.
    let sampled = [vk::DescriptorImageInfo::default()
        .sampler(texture.sampler)
        .image_view(texture.view)
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    let second_sampled = [vk::DescriptorImageInfo::default()
        .sampler(second_texture.sampler)
        .image_view(second_texture.view)
        .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)];
    // `GENERAL`, which is the only layout a storage image may be written in - and the one the
    // barrier before the render pass puts it in.
    let stored = [vk::DescriptorImageInfo::default()
        .image_view(storage_image.view)
        .image_layout(vk::ImageLayout::GENERAL)];
    let writes = [
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&observation),
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
            .buffer_info(&memory),
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(TEXTURE_BINDING)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&sampled),
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(STORAGE_IMAGE_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_IMAGE)
            .image_info(&stored),
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(SECOND_TEXTURE_BINDING)
            .descriptor_type(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .image_info(&second_sampled),
        vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(DRAW_DATA_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER_DYNAMIC)
            .buffer_info(&draw_data_info),
    ];
    // SAFETY: the set came from the pool above and the buffers outlive the call.
    unsafe { device.update_descriptor_sets(&writes, &[]) };

    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(match geometry {
                Geometry::Vertex(_) => vk::ShaderStageFlags::VERTEX,
                Geometry::Mesh => vk::ShaderStageFlags::MESH_EXT,
            })
            .module(vertex)
            .name(ENTRY),
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::FRAGMENT)
            .module(fragment)
            .name(ENTRY),
    ];
    let vertex_input = vk::PipelineVertexInputStateCreateInfo::default();
    let assembly = vk::PipelineInputAssemblyStateCreateInfo::default()
        .topology(vk::PrimitiveTopology::TRIANGLE_LIST);
    // An attachment is at most the device's maximum dimension, which is orders of magnitude
    // inside the range `f32` represents exactly, so the conversion is lossless here.
    // The guest's own transform when it gave one (worklog 837); otherwise clip space over the whole
    // attachment.
    let viewports = [bound.viewport.map_or(
        vk::Viewport {
            x: 0.0,
            y: 0.0,
            width: f32_from(width),
            height: f32_from(height),
            min_depth: 0.0,
            max_depth: 1.0,
        },
        guest_viewport,
    )];
    // The whole attachment unless the draw set a scissor - a guest's viewport (worklog 644) - which
    // restricts rasterisation to a rectangle, so pixels outside it keep the clear. Clamped to the
    // attachment, because Vulkan refuses a scissor that reaches past it.
    let full = vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D { width, height },
    };
    let scissors = [bound
        .scissor
        .map_or(full, |rect| clamp_scissor(rect, width, height))];
    let viewport_state = vk::PipelineViewportStateCreateInfo::default()
        .viewports(&viewports)
        .scissors(&scissors);
    // Culling off: a harness should not fail because a triangle was wound the other way.
    let raster = vk::PipelineRasterizationStateCreateInfo::default()
        .polygon_mode(vk::PolygonMode::FILL)
        .cull_mode(vk::CullModeFlags::NONE)
        .front_face(vk::FrontFace::COUNTER_CLOCKWISE)
        .line_width(1.0);
    let multisample = vk::PipelineMultisampleStateCreateInfo::default()
        .rasterization_samples(vk::SampleCountFlags::TYPE_1);
    // The guest's blend state (`REQ-...2ea9`), or opaque with every channel written when it set
    // none. A state that does not map is refused before a draw reaches here (`VulkanBackend`), so
    // this never builds a pipeline blending approximately.
    let blend_attachments = [blend_attachment(bound.blend)
        .map_err(|what| DispatchError::Unsupported(what.to_owned()))?];
    let blend = vk::PipelineColorBlendStateCreateInfo::default().attachments(&blend_attachments);

    let infos = [vk::GraphicsPipelineCreateInfo::default()
        .stages(&stages)
        .vertex_input_state(&vertex_input)
        .input_assembly_state(&assembly)
        .viewport_state(&viewport_state)
        .rasterization_state(&raster)
        .multisample_state(&multisample)
        .color_blend_state(&blend)
        .layout(layout)
        .render_pass(render_pass)
        .subpass(0)];
    // SAFETY: every referenced object is live and every slice outlives the call.
    let pipelines =
        unsafe { device.create_graphics_pipelines(vk::PipelineCache::null(), &infos, None) }
            .map_err(|(_, e)| DispatchError::Vulkan("create_graphics_pipelines", e))?;

    Ok(Pipeline {
        vertex,
        fragment,
        layout,
        handle: pipelines[0],
        set_layout,
        descriptor_pool,
        set,
        buffers,
        texture,
        second_texture,
        storage_image,
        geometry,
        owns_guest_memory,
        user_data: *bound.user_data,
        textures_uploaded: false,
        window_offset: bound
            .guest_buffer
            .map_or(0, |guest| u32::try_from(guest.offset).unwrap_or(0)),
    })
}

/// Clears an attachment to `clear`, draws three vertices through the given shaders, and reads
/// the pixels back.
///
/// # What the pair of colours buys
///
/// `clear` and whatever the fragment shader writes should **differ**. Then a result matching the
/// fragment shader's colour proves the shader ran and its output reached the attachment, where a
/// result matching the clear proves it did not - and neither can be mistaken for the other. A
/// harness that cleared and drew the same colour would pass with no pipeline at all (D550).
///
/// # Errors
///
/// When no device is available, or any Vulkan call fails. A machine with no device is a skip for
/// the caller to surface loudly, never a pass.
pub fn draw_with(
    vertex_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    width: u32,
    height: u32,
) -> Result<Pixels, DispatchError> {
    draw_with_windows(
        vertex_words,
        fragment_words,
        clear,
        width,
        height,
        DEFAULT_WINDOWS,
    )
}

/// Draws with a **mesh** stage rather than a vertex stage.
///
/// One workgroup, which is what a guest's primitive shader is: a wave that declares how many
/// vertices and primitives it will emit and then emits them (D688). The fragment module is
/// whatever should shade them.
///
/// # Errors
///
/// When no device is available, when the device has no mesh stage - which
/// [`Properties::mesh_shading`](crate::compute::Properties::mesh_shading) reports before it is
/// asked for - or when any Vulkan call fails.
pub fn draw_mesh_with(
    mesh_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    width: u32,
    height: u32,
) -> Result<Pixels, DispatchError> {
    render(
        clear,
        width,
        height,
        Some((mesh_words, fragment_words)),
        Geometry::Mesh,
        Bound::default(),
    )
}

/// Draws with a mesh stage over guest memory seeded with `memory`.
///
/// The words are written into the guest-memory window before the draw, so a translated shader
/// that fetches its vertices finds something there. A window is addressed by the module's own
/// mask, so where a guest address lands in it is arithmetic the caller can do and this cannot.
///
/// # Errors
///
/// When no device is available, when it has no mesh stage, or when any Vulkan call fails.
pub fn draw_mesh_over(
    mesh_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    width: u32,
    height: u32,
    memory: &[u32],
) -> Result<(Pixels, Vec<u32>), DispatchError> {
    draw_mesh_over_clipped(
        mesh_words,
        fragment_words,
        clear,
        width,
        height,
        memory,
        None,
    )
}

/// Draws with a mesh stage over seeded guest memory, **restricted to a scissor rectangle**.
///
/// The clipped counterpart of [`draw_mesh_over`], which is this with no scissor. The backend uses it
/// to execute a `Draw` of a guest's mesh geometry that also set a viewport (worklog 644): the
/// rectangle restricts rasterisation, so pixels outside it keep the clear.
///
/// # Errors
///
/// When no device is available, when it has no mesh stage, or when any Vulkan call fails.
pub(crate) fn draw_mesh_over_clipped(
    mesh_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    width: u32,
    height: u32,
    memory: &[u32],
    scissor: Option<vk::Rect2D>,
) -> Result<(Pixels, Vec<u32>), DispatchError> {
    render_over(
        clear,
        width,
        height,
        Some((mesh_words, fragment_words)),
        Geometry::Mesh,
        Bound {
            windows: [DEFAULT_WINDOWS[0], memory.len().max(1)],
            memory,
            scissor,
            ..Bound::default()
        },
    )
    .map(|drawn| (drawn.pixels, drawn.memory))
}

/// Draws with a mesh stage over a **resident** guest-memory buffer, bound directly, **into a
/// [`ResidentAttachment`] in place** (worklog 839).
///
/// The guest buffer is one its owner uploaded once (D703, worklog 645) and is left intact; the window
/// is read back after the draw, so a shader that wrote guest memory is still observed, and its length
/// is the resident buffer's own - the mask a module built against it applies (worklog 641). The
/// attachment takes no pixels in or out, so a submission's draws cost their draw and not two 8 MB
/// copies each. Returns the guest-memory window as the draw left it.
///
/// # Errors
///
/// When no device is available, when it has no mesh stage, or when any Vulkan call fails.
pub(crate) fn draw_mesh_resident(
    (mesh_words, fragment_words): (&[u32], &[u32]),
    start: &Start<'_>,
    resident: &ResidentAttachment,
    guest_buffer: &DispatchBuffer,
    scissor: Option<vk::Rect2D>,
) -> Result<Option<BatchKey>, DispatchError> {
    // **A draw the open batch already describes joins it and does nothing else** (D718): same
    // pipeline - which is its shaders, textures, blend, viewport and scissor - same attachment,
    // same window, same fragment words. Only its geometry stage's words are its own.
    let key = start.pipeline_key.map(|pipeline| {
        let (_, fragment) = split_user_data(start.user_data);
        BatchKey {
            pipeline: pipeline.get(),
            framebuffer: resident.framebuffer,
            window: (
                guest_buffer.buffer,
                u32::try_from(guest_buffer.offset).unwrap_or(0),
            ),
            fragment,
        }
    });
    if let Some(key) = key {
        let (geometry_words, _) = split_user_data(start.user_data);
        if join_open_batch(&key, geometry_words) {
            return Ok(Some(key));
        }
    }
    let (width, height) = resident.extent();
    render_over(
        start.clear,
        width,
        height,
        Some((mesh_words, fragment_words)),
        Geometry::Mesh,
        Bound {
            windows: [DEFAULT_WINDOWS[0], guest_buffer.words],
            guest_buffer: Some(guest_buffer),
            scissor,
            user_data: start.user_data,
            texture: start.texture.unwrap_or((&NO_TEXTURE, 1)),
            blend: start.blend,
            viewport: start.viewport,
            second_texture: start.second_texture.unwrap_or((&NO_TEXTURE, 1)),
            resident: Some(resident),
            pipeline_key: start.pipeline_key,
            ..Bound::default()
        },
    )
    .map(|_| key)
}

/// Draws with a mesh stage over seeded guest memory, **and a texture bound**.
///
/// What a textured frame needs: the geometry comes from a guest's primitive shader reading
/// vertices out of guest memory, and its pixel shader samples a texture with a coordinate that
/// shader exported. Every other mesh path binds the default one white texel, which is enough
/// for a pipeline to be valid and not enough to see.
///
/// # Errors
///
/// When no device is available, when it has no mesh stage, or when any Vulkan call fails.
pub fn draw_mesh_over_texture(
    mesh_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    size: (u32, u32),
    memory: &[u32],
    texture: (&[u32], u32),
) -> Result<Drawn, DispatchError> {
    let (width, height) = size;
    render_over(
        clear,
        width,
        height,
        Some((mesh_words, fragment_words)),
        Geometry::Mesh,
        Bound {
            windows: [DEFAULT_WINDOWS[0], memory.len().max(1)],
            memory,
            texture,
            ..Bound::default()
        },
    )
}

/// Draws and hands back the storage image the fragment module wrote, with the frame.
///
/// The image is [`STORAGE_IMAGE_SIZE`] and starts **uninitialised**: a test asserting on what a
/// store left behind names the texel it wrote, and a cleared image would let an assertion pass
/// against a texel nothing touched.
///
/// # Errors
///
/// When no device is available, or any Vulkan call fails. A module that writes a format-less
/// storage image also needs a device offering `shaderStorageImageWriteWithoutFormat`, which
/// [`Properties::storage_image_write`](crate::compute::Properties::storage_image_write) reports
/// before it is asked for.
pub fn draw_storing(
    vertex_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    size: (u32, u32),
) -> Result<Drawn, DispatchError> {
    let (width, height) = size;
    render_over(
        clear,
        width,
        height,
        Some((vertex_words, fragment_words)),
        Geometry::Vertex(VertexDraw::TRIANGLE),
        Bound::default(),
    )
}

/// What a draw binds beside its shaders.
///
/// Three values chosen together and never independently, so they travel together - the same
/// judgement [`Devices`] records, and for the same reason. Separately they were three of nine
/// parameters, which is past the point where a call site tells a reader anything.
#[derive(Clone, Copy)]
struct Bound<'a> {
    /// Word counts for the two storage buffers, in binding order.
    windows: [usize; 2],
    /// What the guest-memory window holds before the draw.
    memory: &'a [u32],
    /// The texels of the sampled image, and how many of them make one row.
    texture: (&'a [u32], u32),
    /// The rectangle rasterisation is restricted to - a guest's viewport (worklog 644) - or [`None`]
    /// for the whole attachment.
    scissor: Option<vk::Rect2D>,
    /// A caller-owned buffer to bind as the guest-memory window (binding 1) instead of creating and
    /// seeding one - a resident buffer uploaded once (worklog 645, D703). [`None`] creates and seeds
    /// its own from [`Self::memory`], the path every existing caller takes. When [`Some`], the buffer
    /// is **not** owned here: it is bound, read back, and left for its owner, never destroyed.
    guest_buffer: Option<&'a DispatchBuffer>,
    /// The attachment's contents before the draw, as tightly packed `Rgba8` rows - a target's
    /// earlier draws, or the guest's own surface (worklog 822). [`None`], or bytes that do not cover
    /// the attachment exactly, clears it as every draw did before.
    initial: Option<&'a [u8]>,
    /// The user-data block the draw's shaders read at entry (worklog 826); zeros when no command set
    /// any, which a module declaring no block never reads. By reference, so `Bound` stays cheap to
    /// pass by value.
    user_data: &'a [u32; USER_DATA_BLOCK_WORDS],
    /// Colour target zero's blend state (`REQ-...2ea9`, worklog 829); `None` draws opaque.
    blend: Option<orbistoun_gpu::BlendControl>,
    /// Whether the texture gets the harness's sentinel second level ([`COARSE_TEXEL`]) - only for a
    /// test that must tell a sample naming level one from one naming level zero. A guest's texture
    /// has the levels it carries and no invented one (worklog 833).
    coarse_level: bool,
    /// The guest's clip-to-pixel transform (worklog 837); `None` maps clip space over the whole
    /// attachment.
    viewport: Option<orbistoun_gpu::ViewportTransform>,
    /// Draw into this attachment in place rather than a fresh one (worklog 839); `None` is the fresh
    /// attachment every path used before.
    resident: Option<&'a ResidentAttachment>,
    /// The second sampled image's texels and row length (worklog 840); the default texel otherwise.
    second_texture: (&'a [u32], u32),
    /// The key a resident draw's pipeline is cached under (worklog 843); `None` builds and releases
    /// one per draw. Non-zero so the option costs no space beside the key.
    pipeline_key: Option<std::num::NonZeroU64>,
}

/// The block a draw that sets no user data pushes.
static NO_USER_DATA: [u32; USER_DATA_BLOCK_WORDS] = [0; USER_DATA_BLOCK_WORDS];

impl Default for Bound<'_> {
    fn default() -> Self {
        Self {
            windows: DEFAULT_WINDOWS,
            memory: &[],
            texture: (&NO_TEXTURE, 1),
            scissor: None,
            guest_buffer: None,
            initial: None,
            user_data: &NO_USER_DATA,
            blend: None,
            coarse_level: false,
            viewport: None,
            resident: None,
            second_texture: (&NO_TEXTURE, 1),
            pipeline_key: None,
        }
    }
}

/// Clamps a scissor to an attachment, since Vulkan refuses one that reaches past it.
///
/// A guest names its viewport in its own target's pixels, which should already lie inside the
/// attachment, but a decode that went wrong or a target smaller than the guest expected could put an
/// edge outside - and a scissor Vulkan rejects would fail the draw for a reason about the harness
/// rather than the guest. Clamping keeps the rectangle legal; where it had to move, it drew a
/// smaller region than the guest asked, which is visible rather than a validation error.
fn clamp_scissor(rect: vk::Rect2D, width: u32, height: u32) -> vk::Rect2D {
    let left = rect.offset.x.clamp(0, width as i32);
    let top = rect.offset.y.clamp(0, height as i32);
    let right =
        (i64::from(rect.offset.x) + i64::from(rect.extent.width)).clamp(0, i64::from(width));
    let bottom =
        (i64::from(rect.offset.y) + i64::from(rect.extent.height)).clamp(0, i64::from(height));
    vk::Rect2D {
        offset: vk::Offset2D { x: left, y: top },
        extent: vk::Extent2D {
            width: u32::try_from(right - i64::from(left)).unwrap_or(0),
            height: u32::try_from(bottom - i64::from(top)).unwrap_or(0),
        },
    }
}

/// A vertex draw's parameters: how many vertices it issues, how many instances, and the first
/// vertex, as the guest's `Draw` decoded them (`orbistoun_gpu::RenderCommand::Draw`).
///
/// Carried rather than fixed at three so the executor issues the guest's count - a vertex shader
/// that fetches from guest memory is called once per vertex, so the count is what decides how much
/// geometry is drawn, and hardcoding three drew the wrong amount of every guest's.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VertexDraw {
    /// Vertices to issue.
    pub vertices: u32,
    /// Instances to issue.
    pub instances: u32,
    /// The first vertex index.
    pub first_vertex: u32,
}

impl VertexDraw {
    /// The three-vertex, one-instance draw the fullscreen-triangle harness entries issue: a vertex
    /// shader that indexes its own three positions by `gl_VertexIndex` and needs nothing bound.
    const TRIANGLE: Self = Self {
        vertices: 3,
        instances: 1,
        first_vertex: 0,
    };
}

/// Which stage feeds the fragment stage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Geometry {
    /// A vertex shader over the vertices the draw supplies.
    Vertex(VertexDraw),
    /// A mesh shader, one workgroup, which supplies its own.
    Mesh,
}

/// Which binding a sampled image is bound at.
///
/// Two, because zero and one are the storage buffers every translated module declares. The
/// other half of this number is [`orbistoun_spirv::TEXTURE_BINDING`], which is what a module
/// *says* it reads; a harness cannot import it - the module builder is a test dependency here,
/// deliberately, because a backend that needed one would have the layering wrong. The two
/// agreeing is asserted by the device test rather than left to a comment.
///
/// [`orbistoun_spirv::TEXTURE_BINDING`]: https://docs.rs/orbistoun-spirv
pub const TEXTURE_BINDING: u32 = 2;

/// Which binding a pixel shader's second sampled image is bound at (worklog 840): four, after the
/// storage image. Mirrors `orbistoun_spirv::SECOND_TEXTURE_BINDING`, as [`TEXTURE_BINDING`] mirrors
/// its own.
pub const SECOND_TEXTURE_BINDING: u32 = 4;

/// Which binding a mesh module reads its per-draw user data from, how many words each draw has
/// there, and how many draws one dispatch carries (D718). Mirrors `orbistoun_spirv`'s
/// `DRAW_DATA_BINDING`, `DRAW_DATA_STRIDE_WORDS` and `DRAW_DATA_MOST_DRAWS`.
pub const DRAW_DATA_BINDING: u32 = 5;
/// See [`DRAW_DATA_BINDING`].
pub const DRAW_DATA_STRIDE_WORDS: u32 = 16;
/// See [`DRAW_DATA_BINDING`].
pub const DRAW_DATA_MOST_DRAWS: u32 = 4096;

/// One draw's user data in the draw-data buffer.
pub(crate) type DrawWords = [u32; DRAW_DATA_STRIDE_WORDS as usize];

/// Bytes one dispatch's draw data may span: what the binding's range covers.
const DRAW_DATA_RANGE: u64 = DRAW_DATA_MOST_DRAWS as u64 * DRAW_DATA_STRIDE_WORDS as u64 * 4;

/// Bytes the draw-data buffer gives batches between two settles - well over a frame of draws.
const DRAW_DATA_BYTES: u64 = 16 << 20;

/// What a batch's offset into the buffer is aligned to: the largest storage-buffer offset alignment
/// a device may ask for.
const DRAW_DATA_ALIGN: u64 = 256;

/// The draw-data buffer: host-visible, mapped for the process's life, written by the host as each
/// batch is recorded, and read by the batch's workgroups (D718). Its space is handed out in order and
/// taken back all at once when [`settle`] leaves the device idle.
struct DrawData {
    buffer: vk::Buffer,
    mapped: usize,
    cursor: u64,
}

fn draw_data() -> &'static std::sync::Mutex<Option<DrawData>> {
    static DATA: std::sync::Mutex<Option<DrawData>> = std::sync::Mutex::new(None);
    &DATA
}

/// The draw-data buffer, made on first use.
fn ensure_draw_data(devices: Devices<'_>) -> Result<vk::Buffer, DispatchError> {
    let mut data = draw_data()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(existing) = data.as_ref() {
        return Ok(existing.buffer);
    }
    let size = DRAW_DATA_BYTES + DRAW_DATA_RANGE;
    let (buffer, memory) = create_host_buffer(
        devices.instance,
        devices.physical,
        devices.device,
        size,
        vk::BufferUsageFlags::STORAGE_BUFFER,
    )?;
    // SAFETY: the memory is host-visible and was just bound to the buffer; mapped once, for the
    // process's life, and never unmapped - the buffer lives as long as the device does.
    let mapped = unsafe {
        devices
            .device
            .map_memory(memory, 0, size, vk::MemoryMapFlags::empty())
    }
    .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    *data = Some(DrawData {
        buffer,
        mapped: mapped as usize,
        cursor: 0,
    });
    Ok(buffer)
}

/// Whether `draws` more draws fit before the buffer must be taken back.
fn draw_data_room(draws: usize) -> bool {
    draw_data()
        .lock()
        .ok()
        .and_then(|data| {
            let data = data.as_ref()?;
            let start = data.cursor.next_multiple_of(DRAW_DATA_ALIGN);
            Some(start + draws as u64 * u64::from(DRAW_DATA_STRIDE_WORDS) * 4 <= DRAW_DATA_BYTES)
        })
        .unwrap_or(true)
}

/// Writes a batch's draws into the buffer and answers the offset its dispatch binds - `None` when
/// the buffer is not made yet or has no room.
fn write_draw_data(draws: &[DrawWords]) -> Option<u32> {
    let mut data = draw_data()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let data = data.as_mut()?;
    let start = data.cursor.next_multiple_of(DRAW_DATA_ALIGN);
    let bytes = zerocopy::IntoBytes::as_bytes(draws);
    let end = start + bytes.len() as u64;
    if end > DRAW_DATA_BYTES || draws.len() > DRAW_DATA_MOST_DRAWS as usize {
        return None;
    }
    let at = std::ptr::with_exposed_provenance_mut::<u8>(data.mapped + start as usize);
    // SAFETY: `[start, end)` is inside the mapped buffer (checked above), no batch recorded since
    // the last settle was given it, and the device reads it only once the batch is submitted.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), at, bytes.len()) };
    data.cursor = end;
    u32::try_from(start).ok()
}

/// Makes room for the open batch and one more draw: when the buffer has none, the pass is closed
/// (recording its batch), the device waited for, and the buffer taken back (D718).
fn ensure_draw_room(session: &crate::compute::Session) -> Result<(), DispatchError> {
    let pending = open_pass()
        .lock()
        .ok()
        .and_then(|open| {
            open.as_ref()
                .and_then(|o| o.batch.as_ref().map(|b| b.draws.len()))
        })
        .unwrap_or(0);
    if !draw_data_room(pending + 1) {
        close_open_pass(session)?;
        settle(session)?;
    }
    Ok(())
}

/// Writes one draw's words for a recording outside the resident pass, waiting for the device and
/// taking the buffer back when it is full.
fn write_one_draw_waiting(device: &ash::Device, words: DrawWords) -> Result<u32, DispatchError> {
    if let Some(offset) = write_draw_data(&[words]) {
        return Ok(offset);
    }
    // SAFETY: the device is live; waiting for it idle means no recorded work still reads the
    // buffer.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;
    reset_draw_data();
    write_draw_data(&[words]).ok_or(DispatchError::Unsupported(
        "the draw-data buffer has no room for one draw when empty".to_owned(),
    ))
}

/// Takes the whole buffer back: the device is idle, so no recorded batch still reads it.
fn reset_draw_data() {
    if let Ok(mut data) = draw_data().lock()
        && let Some(data) = data.as_mut()
    {
        data.cursor = 0;
    }
}

/// Which binding a storage image is bound at.
///
/// Three, after the sampled image, and it is a separate binding because it is a separate thing:
/// a sampled image is read through a sampler and cannot be written, and a storage image is
/// written and declares no sampler. Its other half is
/// `orbistoun_spirv::STORAGE_IMAGE_BINDING`, asserted equal by the device test for the same
/// reason the sampled one is.
pub const STORAGE_IMAGE_BINDING: u32 = 3;

/// The window sizes a module's two storage buffers get when the caller does not say.
///
/// Both are counts of words. They are only ever indexed through a mask the module itself
/// applies, so a window larger than the module uses is wasted and one smaller is a read past
/// the end - which is why the caller can say, and why the default is the one the translator's
/// own models declare.
pub const DEFAULT_WINDOWS: [usize; 2] = [16, 1024];

/// Draws with a texture bound, for a fragment module that samples one.
///
/// `texture` is the texels and how many of them make a row; they are uploaded before the draw
/// and read through a sampler that does no filtering, so a coordinate inside a texel gives back
/// that texel exactly and an assertion can name it.
///
/// Every other draw path binds a texture too - one white texel - because the binding exists
/// whether or not the module reads it. This is the path that puts something worth reading there.
///
/// # Errors
///
/// When no device is available, or any Vulkan call fails.
pub fn draw_with_texture(
    vertex_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    size: (u32, u32),
    texture: (&[u32], u32),
) -> Result<Pixels, DispatchError> {
    let (width, height) = size;
    render(
        clear,
        width,
        height,
        Some((vertex_words, fragment_words)),
        Geometry::Vertex(VertexDraw::TRIANGLE),
        Bound {
            texture,
            coarse_level: true,
            ..Bound::default()
        },
    )
}

/// Draws with explicit window sizes for the two storage buffers a module declares.
///
/// # Errors
///
/// When no device is available, or any Vulkan call fails.
pub fn draw_with_windows(
    vertex_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    width: u32,
    height: u32,
    windows: [usize; 2],
) -> Result<Pixels, DispatchError> {
    render(
        clear,
        width,
        height,
        Some((vertex_words, fragment_words)),
        Geometry::Vertex(VertexDraw::TRIANGLE),
        Bound {
            windows,
            ..Bound::default()
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{COARSE_TEXEL, guest_viewport, staged};

    /// **A GL guest's transform becomes a flipped Vulkan viewport over the whole target** (worklog
    /// 837): x from 0 across 1920, y starting at the bottom row with a negative height - so NDC
    /// `+y` lands on row 0, where the guest's own transform puts it - and a positive y scale (the AGC
    /// path's) is the unflipped viewport Vulkan draws by default.
    #[test]
    fn a_negative_y_scale_is_a_negative_height() {
        let flipped = guest_viewport(orbistoun_gpu::ViewportTransform {
            x_scale: 960.0,
            x_offset: 960.0,
            y_scale: -540.0,
            y_offset: 540.0,
        });
        assert_eq!(
            (flipped.x, flipped.width, flipped.y, flipped.height),
            (0.0, 1920.0, 1080.0, -1080.0)
        );
        let upright = guest_viewport(orbistoun_gpu::ViewportTransform {
            x_scale: 32.0,
            x_offset: 32.0,
            y_scale: 32.0,
            y_offset: 32.0,
        });
        assert_eq!(
            (upright.x, upright.width, upright.y, upright.height),
            (0.0, 64.0, 0.0, 64.0)
        );
    }

    /// **A guest's texture is uploaded with the one level it carries** (worklog 833): no invented
    /// second level, and nothing staged past its own texels - the invented level's copy read past
    /// the staging buffer and lost the device on Neverball's first 256x256 texture.
    #[test]
    fn a_guest_texture_has_one_level_and_only_its_own_texels() {
        let texels = vec![0x1234_5678; 256 * 256];
        let laid = staged(&texels, 256, false);
        assert_eq!((laid.width, laid.height, laid.levels), (256, 256, 1));
        assert_eq!(laid.texels, texels);
    }

    /// **A uniform reload leaves exactly that word in every pixel** (worklog 863): an attachment
    /// seeded with a ramp, refilled on the device with a word whose four bytes differ, reads back as
    /// that word everywhere - bit for bit, and in byte order red first. Skipped, and says so, where
    /// no device is present.
    #[test]
    fn a_uniform_reload_leaves_exactly_its_word_everywhere() {
        if crate::compute::session().is_err() {
            eprintln!("no Vulkan device: the uniform reload is not exercised here");
            return;
        }
        let (width, height) = (8, 4);
        let ramp: Vec<u8> = (0..width * height * 4).map(|i| (i % 251) as u8).collect();
        let resident = super::ResidentAttachment::create(width, height, Some(&ramp), [0.0; 4])
            .expect("an attachment");
        resident.reload_uniform(0x8040_2010).expect("reloaded");
        let read = resident.read_back().expect("read back");
        resident.destroy();
        assert_eq!(
            read.bytes,
            [0x10, 0x20, 0x40, 0x80].repeat((width * height) as usize),
            "every pixel the word, red in the first byte"
        );
    }

    /// **The harness's coarse level stages every texel its copy reads**: a 256x256 image with the
    /// sentinel level holds 128x128 sentinels after its own texels - exactly the halved extent the
    /// level-one copy names - and a 2x2 one still holds the single sentinel the sampling tests expect.
    #[test]
    fn the_harness_coarse_level_stages_its_whole_extent() {
        let laid = staged(&vec![7; 256 * 256], 256, true);
        assert_eq!(laid.levels, 2);
        assert_eq!(laid.texels.len(), 256 * 256 + 128 * 128);
        assert!(laid.texels[256 * 256..].iter().all(|&t| t == COARSE_TEXEL));
        let small = staged(&[1, 2, 3, 4], 2, true);
        assert_eq!(small.texels, vec![1, 2, 3, 4, COARSE_TEXEL]);
        assert_eq!(
            staged(&[9], 1, true).levels,
            1,
            "a one-by-one image has one level"
        );
    }
}
