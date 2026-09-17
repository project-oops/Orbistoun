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

/// Creates a sampled image holding `texels`, `row` of them to a row.
///
/// **Optimally tiled, filled by a staging copy** rather than a linear image written through a
/// mapping. `R8G8B8A8_UNORM` is required to support sampling with optimal tiling on every
/// implementation and is not required to support it with linear tiling, so the guaranteed path
/// is the one a harness takes. It also leaves the row stride to the driver instead of to this
/// file, which is one fewer thing a wrong picture could be about.
///
/// A short final row is padded with zeroes, so the extent and the bytes always agree.
fn create_texture(
    devices: Devices<'_>,
    texels: &[u32],
    row: u32,
) -> Result<Texture, DispatchError> {
    let device = devices.device;
    let width = row.max(1);
    let cells = usize::try_from(width).unwrap_or(1);
    let rows = texels.len().div_ceil(cells).max(1);
    let mut padded = texels.to_vec();
    padded.resize(cells * rows, 0);
    let height = u32::try_from(rows).unwrap_or(1);

    // **Two levels where the image is big enough for a second**, so a sample naming a level can
    // be told from one naming a different level. With a single level every level answers the
    // same texel, and a test of `image_sample_l` would pass without the level operand reaching
    // the instruction at all.
    //
    // A one-by-one image has exactly one level - halving it reaches zero, which is not an
    // extent - so the count is derived rather than fixed.
    let levels = if width > 1 && height > 1 { 2 } else { 1 };
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

    // The coarse level's one texel, appended after the fine level's. Both go in one staging
    // buffer and out in two copies, because they are one upload of one image.
    if levels > 1 {
        padded.push(COARSE_TEXEL);
    }

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
                // Halved and floored, which for every texture this harness binds is one by one.
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
        vk::BufferUsageFlags::TRANSFER_DST,
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
    clear: [f32; 4],
    size: (u32, u32),
    draw: VertexDraw,
    scissor: Option<vk::Rect2D>,
    vertex: &[u32],
    fragment: &[u32],
) -> Result<Pixels, DispatchError> {
    let (width, height) = size;
    render(
        clear,
        width,
        height,
        Some((vertex, fragment)),
        Geometry::Vertex(draw),
        Bound {
            scissor,
            ..Bound::default()
        },
    )
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
    let command = record(instance, device, family, &target, colour, pipeline.as_ref())?;
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
        // SAFETY: as above.
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
        // SAFETY: as above.
        unsafe { device.destroy_sampler(built.texture.sampler, None) };
        // SAFETY: as above.
        unsafe { device.destroy_image_view(built.texture.view, None) };
        // SAFETY: as above.
        unsafe { device.destroy_image(built.texture.image, None) };
        // SAFETY: as above, and the image that used it is already destroyed.
        unsafe { device.free_memory(built.texture.memory, None) };
        // SAFETY: as above.
        unsafe { device.destroy_buffer(built.texture.staging, None) };
        // SAFETY: as above, and the buffer that used it is already destroyed.
        unsafe { device.free_memory(built.texture.staging_memory, None) };
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
    instance: &ash::Instance,
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

    // The texture, copied in and made readable, before the render pass that samples it opens -
    // a render pass cannot contain a transfer. Recorded for every pipeline, because every
    // pipeline has a texture whether or not its fragment module reads one.
    if let Some(built) = pipeline {
        upload_texture(device, command, &built.texture);
        open_storage_image(device, command, &built.storage_image);
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
        // SAFETY: a render pass is open and the pipeline was built for it.
        unsafe {
            device.cmd_bind_pipeline(command, vk::PipelineBindPoint::GRAPHICS, built.handle);
        }
        // The set the fragment module's storage buffers live in. Bound whether or not this
        // particular module writes them: a pipeline that statically uses a set needs one
        // bound, and which sets a translated module uses is not something this harness reads.
        let sets = [built.set];
        // SAFETY: the set was allocated against this pipeline's layout and both are live.
        unsafe {
            device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::GRAPHICS,
                built.layout,
                0,
                &sets,
                &[],
            );
        }
        match built.geometry {
            // The guest's decoded count: a vertex shader is called once per vertex, indexing its
            // source by `gl_VertexIndex`, so this is how much geometry it draws. `first_instance`
            // stays zero - nothing decodes it yet, and zero is what it means.
            // SAFETY: a pipeline is bound inside an open render pass.
            Geometry::Vertex(draw) => unsafe {
                device.cmd_draw(command, draw.vertices, draw.instances, draw.first_vertex, 0);
            },
            // **One workgroup**, which is the whole draw: a mesh shader decides for itself how
            // many vertices and primitives come out of it, so the count here is groups rather
            // than vertices. One is what a guest's primitive shader is - a single wave that
            // announces what it will emit and then emits it.
            Geometry::Mesh => {
                let mesh = ash::ext::mesh_shader::Device::new(instance, device);
                // SAFETY: a mesh pipeline is bound inside an open render pass, and a mesh
                // pipeline exists only where the device was created with the extension this
                // loader dispatches through.
                unsafe { mesh.cmd_draw_mesh_tasks(command, 1, 1, 1) };
            }
        }
    }
    // SAFETY: a render pass is open.
    unsafe { device.cmd_end_render_pass(command) };

    // The storage image, copied out now the render pass that wrote it has ended. After the pass
    // rather than inside it, because a render pass may not contain a transfer.
    if let Some(built) = pipeline {
        read_storage_image(device, command, &built.storage_image);
    }

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
    let texture = create_texture(devices, bound.texture.0, bound.texture.1)?;
    let storage_image = create_storage_image(devices, STORAGE_IMAGE_SIZE)?;

    // Both stages, because either may use a binding: a guest's pixel shader writes its canary
    // and so does its primitive shader. Declaring only the fragment stage made a mesh pipeline
    // invalid, which the layer said and the picture did not (worklog 559).
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT | vk::ShaderStageFlags::MESH_EXT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
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
    ];
    let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    // SAFETY: the create info outlives the call.
    let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?;

    let set_layouts = [set_layout];
    let layout_info = vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts);
    // SAFETY: the create info outlives the call and the device is live.
    let layout = unsafe { device.create_pipeline_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_pipeline_layout", e))?;

    let pool_sizes = [
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(2),
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::COMBINED_IMAGE_SAMPLER)
            .descriptor_count(1),
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
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
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
    let viewports = [vk::Viewport {
        x: 0.0,
        y: 0.0,
        width: f32_from(width),
        height: f32_from(height),
        min_depth: 0.0,
        max_depth: 1.0,
    }];
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
        set_layout,
        descriptor_pool,
        set,
        buffers,
        texture,
        storage_image,
        geometry,
        owns_guest_memory,
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

/// Draws with a mesh stage over a **resident** guest-memory buffer, bound directly.
///
/// The direct-bind counterpart of [`draw_mesh_over_clipped`], which creates and seeds a fresh binding
/// from bytes every draw; this binds a buffer its owner uploaded once (D703, worklog 645) and leaves
/// it intact - the window is read back the same way, so a shader that wrote guest memory is still
/// observed. The window's length is the resident buffer's own, which is the mask a module built
/// against it applies (worklog 641).
///
/// # Errors
///
/// When no device is available, when it has no mesh stage, or when any Vulkan call fails.
pub(crate) fn draw_mesh_into(
    mesh_words: &[u32],
    fragment_words: &[u32],
    clear: [f32; 4],
    width: u32,
    height: u32,
    guest_buffer: &DispatchBuffer,
    scissor: Option<vk::Rect2D>,
) -> Result<(Pixels, Vec<u32>), DispatchError> {
    render_over(
        clear,
        width,
        height,
        Some((mesh_words, fragment_words)),
        Geometry::Mesh,
        Bound {
            windows: [DEFAULT_WINDOWS[0], guest_buffer.words],
            guest_buffer: Some(*guest_buffer),
            scissor,
            ..Bound::default()
        },
    )
    .map(|drawn| (drawn.pixels, drawn.memory))
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
    guest_buffer: Option<DispatchBuffer>,
}

impl Default for Bound<'_> {
    fn default() -> Self {
        Self {
            windows: DEFAULT_WINDOWS,
            memory: &[],
            texture: (&NO_TEXTURE, 1),
            scissor: None,
            guest_buffer: None,
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
