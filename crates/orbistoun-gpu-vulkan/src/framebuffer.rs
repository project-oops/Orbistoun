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

use crate::compute::DispatchError;

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
        .usage(vk::ImageUsageFlags::COLOR_ATTACHMENT | vk::ImageUsageFlags::TRANSFER_SRC)
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
fn create_render_pass(device: &ash::Device) -> Result<vk::RenderPass, DispatchError> {
    let attachments = [vk::AttachmentDescription::default()
        .format(FORMAT)
        .samples(vk::SampleCountFlags::TYPE_1)
        .load_op(vk::AttachmentLoadOp::CLEAR)
        .store_op(vk::AttachmentStoreOp::STORE)
        .stencil_load_op(vk::AttachmentLoadOp::DONT_CARE)
        .stencil_store_op(vk::AttachmentStoreOp::DONT_CARE)
        .initial_layout(vk::ImageLayout::UNDEFINED)
        .final_layout(vk::ImageLayout::TRANSFER_SRC_OPTIMAL)];
    let references = [vk::AttachmentReference::default()
        .attachment(0)
        .layout(vk::ImageLayout::COLOR_ATTACHMENT_OPTIMAL)];
    let subpasses = [vk::SubpassDescription::default()
        .pipeline_bind_point(vk::PipelineBindPoint::GRAPHICS)
        .color_attachments(&references)];
    let dependencies = [vk::SubpassDependency::default()
        .src_subpass(0)
        .dst_subpass(vk::SUBPASS_EXTERNAL)
        .src_stage_mask(vk::PipelineStageFlags::COLOR_ATTACHMENT_OUTPUT)
        .src_access_mask(vk::AccessFlags::COLOR_ATTACHMENT_WRITE)
        .dst_stage_mask(vk::PipelineStageFlags::TRANSFER)
        .dst_access_mask(vk::AccessFlags::TRANSFER_READ)];
    let info = vk::RenderPassCreateInfo::default()
        .attachments(&attachments)
        .subpasses(&subpasses)
        .dependencies(&dependencies);
    // SAFETY: every slice above outlives the call and the device is live.
    unsafe { device.create_render_pass(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_render_pass", e))
}

/// A host-visible buffer big enough to receive the image, and its mapped-readable memory.
fn create_readback_buffer(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    size: vk::DeviceSize,
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    let info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer", e))?;
    // SAFETY: the buffer was created on this device and not yet destroyed.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    // SAFETY: the physical device is valid.
    let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
    let wanted = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let memory_type = (0..properties.memory_type_count)
        .find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[*i as usize]
                    .property_flags
                    .contains(wanted)
        })
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
    render(colour, width, height, None)
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
) -> Result<Pixels, DispatchError> {
    let session = crate::compute::session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let instance = &session.instance;
    let physical = session.physical;
    let device = &session.device;
    let queue = session.queue;
    let family = session.family;

    let size = vk::DeviceSize::from(width) * vk::DeviceSize::from(height) * 4;
    let (image, image_memory) = create_attachment(instance, physical, device, width, height)?;
    let render_pass = create_render_pass(device)?;
    let (buffer, buffer_memory) = create_readback_buffer(instance, physical, device, size)?;

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

    let pipeline = shaders
        .map(|(vertex, fragment)| {
            build_pipeline(device, render_pass, vertex, fragment, width, height)
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
    let command = record(device, family, &target, colour, pipeline.as_ref())?;
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

    release(device, command.pool, &target, view, pipeline.as_ref());
    // SAFETY: nothing is bound to either allocation any longer and the buffer is unmapped.
    unsafe { device.free_memory(image_memory, None) };
    // SAFETY: as above.
    unsafe { device.free_memory(buffer_memory, None) };

    Ok(Pixels {
        width,
        height,
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
        // SAFETY: as above.
        unsafe { device.destroy_pipeline(built.handle, None) };
        // SAFETY: as above.
        unsafe { device.destroy_pipeline_layout(built.layout, None) };
        // SAFETY: as above.
        unsafe { device.destroy_shader_module(built.vertex, None) };
        // SAFETY: as above.
        unsafe { device.destroy_shader_module(built.fragment, None) };
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

/// Everything one recording renders into and copies out of.
struct Target {
    render_pass: vk::RenderPass,
    framebuffer: vk::Framebuffer,
    image: vk::Image,
    buffer: vk::Buffer,
    width: u32,
    height: u32,
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
    device: &ash::Device,
    family: u32,
    target: &Target,
    colour: [f32; 4],
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
        // SAFETY: a render pass is open and the pipeline was built for it.
        unsafe {
            device.cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, built.handle);
        }
        // Three vertices, one instance. The vertex shader indexes its own table by
        // `gl_VertexIndex`, so there is nothing bound to draw from.
        // SAFETY: a pipeline is bound inside an open render pass.
        unsafe { device.cmd_draw(command, 3, 1, 0, 0) };
    }
    // SAFETY: a render pass is open.
    unsafe { device.cmd_end_render_pass(command) };

    let regions = [vk::BufferImageCopy::default()
        // Zero means tightly packed, which is what the readback expects.
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
fn build_pipeline(
    device: &ash::Device,
    render_pass: vk::RenderPass,
    vertex_words: &[u32],
    fragment_words: &[u32],
    width: u32,
    height: u32,
) -> Result<Pipeline, DispatchError> {
    let vertex_info = vk::ShaderModuleCreateInfo::default().code(vertex_words);
    // SAFETY: the words outlive the call and the device is live.
    let vertex = unsafe { device.create_shader_module(&vertex_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module(vertex)", e))?;
    let fragment_info = vk::ShaderModuleCreateInfo::default().code(fragment_words);
    // SAFETY: as above.
    let fragment = unsafe { device.create_shader_module(&fragment_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module(fragment)", e))?;

    let layout_info = vk::PipelineLayoutCreateInfo::default();
    // SAFETY: the create info outlives the call and the device is live.
    let layout = unsafe { device.create_pipeline_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_pipeline_layout", e))?;

    let stages = [
        vk::PipelineShaderStageCreateInfo::default()
            .stage(vk::ShaderStageFlags::VERTEX)
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
    let viewports = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: f32_from(width),
        height: f32_from(height),
        min_depth: 0.0,
        max_depth: 1.0,
    }];
    let scissors = [vk::Rect2D {
        offset: vk::Offset2D { x: 0, y: 0 },
        extent: vk::Extent2D { width, height },
    }];
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
    // Blending off and every channel written, so what the shader stores is what lands.
    let blend_attachments = [vk::PipelineColorBlendAttachmentState::default()
        .color_write_mask(vk::ColorComponentFlags::RGBA)
        .blend_enable(false)];
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
    render(clear, width, height, Some((vertex_words, fragment_words)))
}
