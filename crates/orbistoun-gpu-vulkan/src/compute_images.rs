//! The images a guest compute dispatch fetches from and stores to.
//!
//! A translated compute module names its images at the bindings a pixel shader's use: the image it
//! fetches from at [`orbistoun_spirv::TEXTURE_BINDING`], combined with a sampler a fetch ignores,
//! and the one it stores to at [`orbistoun_spirv::STORAGE_IMAGE_BINDING`]. Each arrives as linear
//! `R8G8B8A8` texels the caller detiled from guest memory; each is uploaded before the dispatch,
//! and the stored image is copied back out after it, so the caller can tile back what changed.

use ash::vk;

use crate::compute::DispatchError;

/// A dispatch image's texel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchFormat {
    /// Four eight-bit normalised channels, the guest's `8_8_8_8_UNORM` in the same byte order.
    Rgba8,
    /// One eight-bit normalised channel, the guest's `8_UNORM`.
    R8,
    /// One eight-bit unsigned integer channel, the guest's `8_UINT`.
    R8Uint,
    /// Four thirty-two-bit unsigned channels, the guest's `32_32_32_32_UINT`: four words a texel.
    Rgba32Uint,
    /// One thirty-two-bit unsigned channel, the guest's `32_UINT`: a word a texel.
    R32Uint,
    /// One thirty-two-bit float channel, the guest's `32_FLOAT`: a word a texel, its bits as they are.
    R32Float,
    /// Ten normalised bits each of red, green and blue from the low end and two of alpha, the
    /// guest's `2_10_10_10_UNORM` in the same bit order.
    Rgb10A2,
    /// Four sixteen-bit floats, the guest's `16_16_16_16_FLOAT`: two words a texel.
    Rgba16Float,
}

impl DispatchFormat {
    const fn vulkan(self) -> vk::Format {
        match self {
            Self::Rgba8 => vk::Format::R8G8B8A8_UNORM,
            Self::R8 => vk::Format::R8_UNORM,
            Self::R8Uint => vk::Format::R8_UINT,
            Self::Rgba32Uint => vk::Format::R32G32B32A32_UINT,
            Self::R32Uint => vk::Format::R32_UINT,
            Self::R32Float => vk::Format::R32_SFLOAT,
            Self::Rgb10A2 => vk::Format::A2B10G10R10_UNORM_PACK32,
            Self::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
        }
    }

    /// Bytes one texel takes.
    const fn bytes(self) -> usize {
        match self {
            Self::Rgba8 | Self::Rgb10A2 | Self::R32Uint | Self::R32Float => 4,
            Self::R8 | Self::R8Uint => 1,
            Self::Rgba16Float => 8,
            Self::Rgba32Uint => 16,
        }
    }

    /// Words one texel takes in a dispatch image's texels, and bytes of each word that are its own.
    const fn words(self) -> (usize, usize) {
        let bytes = self.bytes();
        if bytes <= 4 {
            (1, bytes)
        } else {
            (bytes / 4, 4)
        }
    }
}

/// One image a dispatch reads or writes, as linear texels.
#[derive(Debug, Clone, Copy)]
pub struct DispatchImage<'a> {
    /// Row-major texels, `width * height` of them, each its bits in the low bytes of a word.
    pub texels: &'a [u32],
    /// What a texel is.
    pub format: DispatchFormat,
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
    /// Slices of a three-dimensional image, its texels slice after slice (D823); `None` for a
    /// two-dimensional one.
    pub depth: Option<u32>,
}

impl DispatchImage<'_> {
    /// Texels it holds: every slice's.
    fn count(&self) -> usize {
        self.width as usize * self.height as usize * self.depth.unwrap_or(1) as usize
    }
}

/// The images a dispatch binds: those it fetches from and the one it stores to.
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchImages<'a> {
    /// Bound at the texture binding: the first texture slot's.
    pub fetched: Option<DispatchImage<'a>>,
    /// The further texture slots' images, in order, each at its slot's binding
    /// ([`orbistoun_spirv::TEXTURE_BINDINGS`]).
    pub more_fetched: &'a [DispatchImage<'a>],
    /// Bound at the storage-image binding, and read back after the dispatch.
    pub stored: Option<DispatchImage<'a>>,
}

/// One image, created, with the host buffer its texels travel through.
struct Bound {
    binding: u32,
    stored: bool,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
    format: DispatchFormat,
    width: u32,
    height: u32,
    depth: Option<u32>,
}

/// A dispatch's images on the device, with the sampler the fetched one is bound with.
pub(crate) struct BoundImages {
    images: Vec<Bound>,
    sampler: Option<vk::Sampler>,
}

impl BoundImages {
    /// No images: a dispatch that reads and writes only its window.
    pub(crate) const fn none() -> Self {
        Self {
            images: Vec::new(),
            sampler: None,
        }
    }

    /// Creates each image and fills its host buffer with its texels.
    pub(crate) fn create(
        (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
        images: &DispatchImages<'_>,
    ) -> Result<Self, DispatchError> {
        let mut bound = Self::none();
        if images.more_fetched.len() + 1 > orbistoun_spirv::TEXTURE_BINDINGS.len() {
            return Err(DispatchError::Unsupported(
                "more fetched images than a stage has texture bindings".to_owned(),
            ));
        }
        let mut wanted = vec![
            (images.fetched, orbistoun_spirv::TEXTURE_BINDING, false),
            (images.stored, orbistoun_spirv::STORAGE_IMAGE_BINDING, true),
        ];
        wanted.extend(
            images
                .more_fetched
                .iter()
                .zip(&orbistoun_spirv::TEXTURE_BINDINGS[1..])
                .map(|(image, binding)| (Some(*image), *binding, false)),
        );
        for (image, binding, stored) in wanted {
            let Some(image) = image else { continue };
            let key = SpareKey {
                device: vk::Handle::as_raw(device.handle()),
                format: image.format,
                width: image.width,
                height: image.height,
                depth: image.depth,
                stored,
            };
            let created = match take_spare(device, key, image) {
                Some(reused) => reused,
                None => {
                    orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DispatchNewImage, || {
                        create_image((instance, physical, device), image, stored)
                    })
                }
            };
            match created {
                Ok((image_handle, memory, view, staging, staging_memory)) => {
                    bound.images.push(Bound {
                        binding,
                        stored,
                        image: image_handle,
                        memory,
                        view,
                        staging,
                        staging_memory,
                        format: image.format,
                        width: image.width,
                        height: image.height,
                        depth: image.depth,
                    });
                }
                Err(e) => {
                    bound.destroy(device);
                    return Err(e);
                }
            }
        }
        if bound.images.iter().any(|image| !image.stored) {
            match kept_sampler(device) {
                Ok(sampler) => bound.sampler = Some(sampler),
                Err(e) => {
                    bound.destroy(device);
                    return Err(e);
                }
            }
        }
        Ok(bound)
    }

    /// The set layout bindings the images take.
    pub(crate) fn layout_bindings(&self) -> Vec<vk::DescriptorSetLayoutBinding<'static>> {
        self.images
            .iter()
            .map(|image| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(image.binding)
                    .descriptor_type(descriptor_type(image.stored))
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect()
    }

    /// The pool room the images take.
    pub(crate) fn pool_sizes(&self) -> Vec<vk::DescriptorPoolSize> {
        self.images
            .iter()
            .map(|image| {
                vk::DescriptorPoolSize::default()
                    .ty(descriptor_type(image.stored))
                    .descriptor_count(1)
            })
            .collect()
    }

    /// Points `set`'s image bindings at the images.
    pub(crate) fn write(&self, device: &ash::Device, set: vk::DescriptorSet) {
        let infos: Vec<[vk::DescriptorImageInfo; 1]> = self
            .images
            .iter()
            .map(|image| {
                let info = vk::DescriptorImageInfo::default()
                    .image_view(image.view)
                    .image_layout(layout(image.stored));
                [match (image.stored, self.sampler) {
                    (false, Some(sampler)) => info.sampler(sampler),
                    _ => info,
                }]
            })
            .collect();
        let writes: Vec<vk::WriteDescriptorSet<'_>> = self
            .images
            .iter()
            .zip(&infos)
            .map(|(image, info)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(image.binding)
                    .descriptor_type(descriptor_type(image.stored))
                    .image_info(info)
            })
            .collect();
        // SAFETY: the set, views and sampler are live, and the infos outlive the call.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
    }

    /// Records each image's upload from its host buffer and the move to the layout the dispatch
    /// uses it in, into an open recording.
    pub(crate) fn record_upload(&self, device: &ash::Device, command: vk::CommandBuffer) {
        for image in &self.images {
            barrier(
                device,
                command,
                image.image,
                (
                    vk::ImageLayout::UNDEFINED,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                ),
                (vk::AccessFlags::empty(), vk::AccessFlags::TRANSFER_WRITE),
                (
                    vk::PipelineStageFlags::TOP_OF_PIPE,
                    vk::PipelineStageFlags::TRANSFER,
                ),
            );
            let region = [whole(image.width, image.height, image.depth)];
            // SAFETY: recording is open; the buffer holds the image's texels and the image is in
            // the transfer-destination layout.
            unsafe {
                device.cmd_copy_buffer_to_image(
                    command,
                    image.staging,
                    image.image,
                    vk::ImageLayout::TRANSFER_DST_OPTIMAL,
                    &region,
                );
            }
            let access = if image.stored {
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE
            } else {
                vk::AccessFlags::SHADER_READ
            };
            barrier(
                device,
                command,
                image.image,
                (vk::ImageLayout::TRANSFER_DST_OPTIMAL, layout(image.stored)),
                (vk::AccessFlags::TRANSFER_WRITE, access),
                (
                    vk::PipelineStageFlags::TRANSFER,
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                ),
            );
        }
    }

    /// Records the stored image's copy back to its host buffer, after the dispatch's stores, into
    /// an open recording.
    pub(crate) fn record_readback(&self, device: &ash::Device, command: vk::CommandBuffer) {
        for image in self.images.iter().filter(|image| image.stored) {
            barrier(
                device,
                command,
                image.image,
                (vk::ImageLayout::GENERAL, vk::ImageLayout::GENERAL),
                (
                    vk::AccessFlags::SHADER_WRITE,
                    vk::AccessFlags::TRANSFER_READ,
                ),
                (
                    vk::PipelineStageFlags::COMPUTE_SHADER,
                    vk::PipelineStageFlags::TRANSFER,
                ),
            );
            let region = [whole(image.width, image.height, image.depth)];
            // SAFETY: recording is open and the barrier above ordered the shader's stores first.
            unsafe {
                device.cmd_copy_image_to_buffer(
                    command,
                    image.image,
                    vk::ImageLayout::GENERAL,
                    image.staging,
                    &region,
                );
            }
        }
    }

    /// The stored image's texels as the dispatch left them, once the recording has completed.
    pub(crate) fn read_stored(
        &self,
        device: &ash::Device,
    ) -> Result<Option<Vec<u32>>, DispatchError> {
        let Some(image) = self.images.iter().find(|image| image.stored) else {
            return Ok(None);
        };
        let texels =
            image.width as usize * image.height as usize * image.depth.unwrap_or(1) as usize;
        let size = image.format.bytes();
        let bytes = (texels * size) as vk::DeviceSize;
        // SAFETY: the memory is host-visible and coherent, holds `bytes`, and is not mapped; the
        // copy into it has completed.
        let mapped = unsafe {
            device.map_memory(image.staging_memory, 0, bytes, vk::MemoryMapFlags::empty())
        }
        .map_err(|e| DispatchError::Vulkan("map_memory(stored image)", e))?;
        // SAFETY: the mapping covers `bytes` bytes and nothing writes it while it is read.
        let raw =
            unsafe { std::slice::from_raw_parts(mapped.cast::<u8>().cast_const(), texels * size) };
        // A texel of more than a word comes back as its words, in order.
        let texels = raw
            .chunks_exact(image.format.words().1)
            .map(|texel| {
                texel
                    .iter()
                    .rev()
                    .fold(0_u32, |word, &byte| word << 8 | u32::from(byte))
            })
            .collect();
        // SAFETY: mapped just above.
        unsafe { device.unmap_memory(image.staging_memory) };
        Ok(Some(texels))
    }

    /// Releases everything created, once the dispatch that used it has finished: each image to
    /// the spares while they hold few enough bytes, else destroyed. The sampler is kept.
    pub(crate) fn destroy(self, device: &ash::Device) {
        let raw = vk::Handle::as_raw(device.handle());
        for image in self.images {
            let key = SpareKey {
                device: raw,
                format: image.format,
                width: image.width,
                height: image.height,
                depth: image.depth,
                stored: image.stored,
            };
            let kept = spare_images().lock().is_ok_and(|mut spares| {
                let held: u64 = spares.iter().map(|spare| spare.key.bytes()).sum();
                let room = held + key.bytes() <= SPARE_IMAGE_BYTES_MOST;
                if room {
                    spares.push(SpareImage {
                        key,
                        image: image.image,
                        memory: image.memory,
                        view: image.view,
                        staging: image.staging,
                        staging_memory: image.staging_memory,
                    });
                }
                room
            });
            if !kept {
                release_image(device, (image.image, image.memory, Some(image.view)));
                release_buffer(device, image.staging, image.staging_memory);
            }
        }
    }
}

/// A fetched image is a combined image sampler and a stored one a storage image.
const fn descriptor_type(stored: bool) -> vk::DescriptorType {
    if stored {
        vk::DescriptorType::STORAGE_IMAGE
    } else {
        vk::DescriptorType::COMBINED_IMAGE_SAMPLER
    }
}

/// The layout each is used in: `GENERAL` for a storage image, read-only for a fetched one.
const fn layout(stored: bool) -> vk::ImageLayout {
    if stored {
        vk::ImageLayout::GENERAL
    } else {
        vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL
    }
}

/// The whole of a single-level image, every slice of a volume, tightly packed in its buffer.
fn whole(width: u32, height: u32, depth: Option<u32>) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(1),
        )
        .image_extent(vk::Extent3D {
            width,
            height,
            depth: depth.unwrap_or(1),
        })
}

/// One image barrier, into an open recording.
fn barrier(
    device: &ash::Device,
    command: vk::CommandBuffer,
    image: vk::Image,
    (old, new): (vk::ImageLayout, vk::ImageLayout),
    (from, to): (vk::AccessFlags, vk::AccessFlags),
    (after, before): (vk::PipelineStageFlags, vk::PipelineStageFlags),
) {
    let barriers = [vk::ImageMemoryBarrier::default()
        .src_access_mask(from)
        .dst_access_mask(to)
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
            after,
            before,
            vk::DependencyFlags::empty(),
            &[],
            &[],
            &barriers,
        );
    }
}

/// The handles one image takes: the image, its memory and view, and its host buffer.
type Created = (
    vk::Image,
    vk::DeviceMemory,
    vk::ImageView,
    vk::Buffer,
    vk::DeviceMemory,
);

/// Creates one image and the host buffer holding its texels.
fn create_image(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    texels: DispatchImage<'_>,
    stored: bool,
) -> Result<Created, DispatchError> {
    let words = texels.count() * texels.format.words().0;
    if texels.texels.len() != words || words == 0 {
        return Err(DispatchError::Unsupported(format!(
            "a dispatch image's texels are not its extent: {} words for {}x{} {:?}",
            texels.texels.len(),
            texels.width,
            texels.height,
            texels.format
        )));
    }
    let usage = if stored {
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::TRANSFER_SRC
    } else {
        vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST
    };
    // A volume is a three-dimensional image of its slices, seen through a view of the same type,
    // which is what a module's three-dimensional storage image binds (D823).
    let (image_type, view_type) = if texels.depth.is_some() {
        (vk::ImageType::TYPE_3D, vk::ImageViewType::TYPE_3D)
    } else {
        (vk::ImageType::TYPE_2D, vk::ImageViewType::TYPE_2D)
    };
    let info = vk::ImageCreateInfo::default()
        .image_type(image_type)
        .format(texels.format.vulkan())
        .extent(vk::Extent3D {
            width: texels.width,
            height: texels.height,
            depth: texels.depth.unwrap_or(1),
        })
        .mip_levels(1)
        .array_layers(1)
        .samples(vk::SampleCountFlags::TYPE_1)
        .tiling(vk::ImageTiling::OPTIMAL)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE)
        .initial_layout(vk::ImageLayout::UNDEFINED);
    // SAFETY: the create info outlives the call and the device is live.
    let image = unsafe { device.create_image(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_image(dispatch)", e))?;
    // SAFETY: the image was created on this device.
    let requirements = unsafe { device.get_image_memory_requirements(image) };
    let memory = match allocate(
        (instance, physical, device),
        requirements,
        vk::MemoryPropertyFlags::DEVICE_LOCAL,
    ) {
        Ok(memory) => memory,
        Err(e) => {
            // SAFETY: created above and never used.
            unsafe { device.destroy_image(image, None) };
            return Err(e);
        }
    };
    // SAFETY: image and memory come from this device and the allocation is large enough.
    let bound = unsafe { device.bind_image_memory(image, memory, 0) };
    let view = bound.and_then(|()| {
        let view_info = vk::ImageViewCreateInfo::default()
            .image(image)
            .view_type(view_type)
            .format(texels.format.vulkan())
            .subresource_range(
                vk::ImageSubresourceRange::default()
                    .aspect_mask(vk::ImageAspectFlags::COLOR)
                    .level_count(1)
                    .layer_count(1),
            );
        // SAFETY: the image is live and bound, and the create info outlives the call.
        unsafe { device.create_image_view(&view_info, None) }
    });
    let view = match view {
        Ok(view) => view,
        Err(e) => {
            release_image(device, (image, memory, None));
            return Err(DispatchError::Vulkan("create_image_view(dispatch)", e));
        }
    };
    let staged = staging_buffer((instance, physical, device), texels);
    match staged {
        Ok((staging, staging_memory)) => Ok((image, memory, view, staging, staging_memory)),
        Err(e) => {
            release_image(device, (image, memory, Some(view)));
            Err(e)
        }
    }
}

/// Allocates memory of a type `requirements` allows with `flags`.
fn allocate(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    requirements: vk::MemoryRequirements,
    flags: vk::MemoryPropertyFlags,
) -> Result<vk::DeviceMemory, DispatchError> {
    // SAFETY: the physical device is valid.
    let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
    let memory_type = (0..properties.memory_type_count)
        .find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && properties.memory_types[*i as usize]
                    .property_flags
                    .contains(flags)
        })
        .ok_or(DispatchError::NoHostVisibleMemory)?;
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    unsafe { device.allocate_memory(&allocate, None) }
        .map_err(|e| DispatchError::Vulkan("allocate_memory(dispatch image)", e))
}

/// A host-visible buffer holding `texels`, a transfer source and destination.
fn staging_buffer(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    image: DispatchImage<'_>,
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    let packed = packed_texels(image);
    let bytes = packed.len() as vk::DeviceSize;
    let info = vk::BufferCreateInfo::default()
        .size(bytes)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer(dispatch image)", e))?;
    // SAFETY: the buffer was created on this device.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    // Cached where the device offers it: a stored image is read back texel by texel, and an
    // uncached read costs a bus round trip each.
    let coherent = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let context = (instance, physical, device);
    let memory = match allocate(
        context,
        requirements,
        coherent | vk::MemoryPropertyFlags::HOST_CACHED,
    )
    .or_else(|_| allocate(context, requirements, coherent))
    {
        Ok(memory) => memory,
        Err(e) => {
            // SAFETY: created above and never used.
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(e);
        }
    };
    // SAFETY: buffer and memory come from this device and the allocation is large enough.
    let filled = unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .and_then(|()| fill_staging(device, memory, &packed));
    match filled {
        Ok(()) => Ok((buffer, memory)),
        Err(e) => {
            release_buffer(device, buffer, memory);
            Err(DispatchError::Vulkan("fill(dispatch image)", e))
        }
    }
}

/// An image's texels packed as its staging buffer holds them: each texel's own bytes of each word.
/// Where every byte of a word is the texel's, that is the words as they lie, and nothing is copied.
fn packed_texels(image: DispatchImage<'_>) -> std::borrow::Cow<'_, [u8]> {
    let (_, size) = image.format.words();
    if size == 4 {
        return std::borrow::Cow::Borrowed(zerocopy::IntoBytes::as_bytes(image.texels));
    }
    std::borrow::Cow::Owned(
        image
            .texels
            .iter()
            .flat_map(|word| word.to_le_bytes().into_iter().take(size))
            .collect(),
    )
}

/// Copies `packed` into a staging buffer's host-visible `memory`, which holds at least that many
/// bytes and is not mapped.
fn fill_staging(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    packed: &[u8],
) -> Result<(), vk::Result> {
    let bytes = packed.len() as vk::DeviceSize;
    // SAFETY: the memory is host-visible, holds `bytes`, and is not mapped.
    let mapped = unsafe { device.map_memory(memory, 0, bytes, vk::MemoryMapFlags::empty()) }?;
    // SAFETY: the mapping covers `packed.len()` bytes, exclusively ours while mapped.
    unsafe {
        std::ptr::copy_nonoverlapping(packed.as_ptr(), mapped.cast::<u8>(), packed.len());
    }
    // SAFETY: mapped just above, and the copy into it has finished.
    unsafe { device.unmap_memory(memory) };
    Ok(())
}

/// A dispatch image a finished dispatch released, kept for the next of the same device, format,
/// extent and use, as a dispatch's buffers are (D805): every reuse uploads the whole image from its
/// staging buffer, refilled with the new texels, from an undefined layout, so nothing it held is
/// ever read.
struct SpareImage {
    key: SpareKey,
    image: vk::Image,
    memory: vk::DeviceMemory,
    view: vk::ImageView,
    staging: vk::Buffer,
    staging_memory: vk::DeviceMemory,
}

/// What a spare image must match to be reused: the device, the format, the extent and whether it
/// is stored to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct SpareKey {
    device: u64,
    format: DispatchFormat,
    width: u32,
    height: u32,
    depth: Option<u32>,
    stored: bool,
}

impl SpareKey {
    /// Bytes it holds, on the device and in its staging buffer together.
    fn bytes(self) -> u64 {
        u64::from(self.width)
            * u64::from(self.height)
            * u64::from(self.depth.unwrap_or(1))
            * self.format.bytes() as u64
            * 2
    }
}

/// The spare images.
fn spare_images() -> &'static std::sync::Mutex<Vec<SpareImage>> {
    static SPARES: std::sync::Mutex<Vec<SpareImage>> = std::sync::Mutex::new(Vec::new());
    &SPARES
}

/// The most bytes the spare images hold together: a frame's few dispatch images, with room to
/// spare.
const SPARE_IMAGE_BYTES_MOST: u64 = 128 << 20;

/// A spare image matching `key`, its staging buffer refilled with `texels`; `None` where none is
/// kept, or the refill failed (the spare is then released).
fn take_spare(
    device: &ash::Device,
    key: SpareKey,
    texels: DispatchImage<'_>,
) -> Option<Result<Created, DispatchError>> {
    let words = texels.count() * texels.format.words().0;
    if texels.texels.len() != words || words == 0 {
        return None;
    }
    let spare = {
        let mut spares = spare_images().lock().ok()?;
        let at = spares.iter().position(|spare| spare.key == key)?;
        spares.swap_remove(at)
    };
    if fill_staging(device, spare.staging_memory, &packed_texels(texels)).is_err() {
        release_image(device, (spare.image, spare.memory, Some(spare.view)));
        release_buffer(device, spare.staging, spare.staging_memory);
        return None;
    }
    Some(Ok((
        spare.image,
        spare.memory,
        spare.view,
        spare.staging,
        spare.staging_memory,
    )))
}

/// The sampler every fetched image is bound with, kept per device: its settings never change.
fn kept_sampler(device: &ash::Device) -> Result<vk::Sampler, DispatchError> {
    static KEPT: std::sync::Mutex<Option<(u64, vk::Sampler)>> = std::sync::Mutex::new(None);
    let mut kept = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let raw = vk::Handle::as_raw(device.handle());
    if let Some((on, sampler)) = *kept
        && on == raw
    {
        return Ok(sampler);
    }
    let info = vk::SamplerCreateInfo::default()
        .mag_filter(vk::Filter::NEAREST)
        .min_filter(vk::Filter::NEAREST)
        .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
        .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
        .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
    // SAFETY: the device is live and the create info outlives the call.
    let sampler = unsafe { device.create_sampler(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_sampler", e))?;
    *kept = Some((raw, sampler));
    Ok(sampler)
}

/// Releases an image, its memory and its view, each created on `device` and no longer in use: never
/// submitted, or the device idle since.
fn release_image(
    device: &ash::Device,
    (image, memory, view): (vk::Image, vk::DeviceMemory, Option<vk::ImageView>),
) {
    if let Some(view) = view {
        // SAFETY: as the caller vouches, the view is this device's and unused.
        unsafe { device.destroy_image_view(view, None) };
    }
    // SAFETY: as above, for the image.
    unsafe { device.destroy_image(image, None) };
    // SAFETY: as above, for its memory, which nothing is bound to once the image is gone.
    unsafe { device.free_memory(memory, None) };
}

/// Releases a host buffer and its memory, under the same conditions.
fn release_buffer(device: &ash::Device, buffer: vk::Buffer, memory: vk::DeviceMemory) {
    // SAFETY: the buffer is this device's and unused.
    unsafe { device.destroy_buffer(buffer, None) };
    // SAFETY: as above, for its memory.
    unsafe { device.free_memory(memory, None) };
}

#[cfg(test)]
mod tests {
    use super::{BoundImages, DispatchFormat, DispatchImage, DispatchImages};
    use crate::compute::{Availability, Session, probe, session};

    /// An extent no other test uses, so the spares hold none of it before.
    const WIDTH: u32 = 37;
    const HEIGHT: u32 = 11;

    /// A dispatch that fetches `texels` as one image of that extent.
    fn images(texels: &[u32]) -> DispatchImages<'_> {
        DispatchImages {
            more_fetched: &[],
            fetched: Some(DispatchImage {
                texels,
                format: DispatchFormat::Rgba8,
                width: WIDTH,
                height: HEIGHT,
                depth: None,
            }),
            stored: None,
        }
    }

    /// A finished dispatch's images are kept for the next of the same format, extent and use: the
    /// second binds the very image the first released, its staging buffer holds its own texels, and
    /// both fetch through the one kept sampler.
    #[test]
    fn a_dispatch_s_images_are_reused_by_the_next() {
        if let Availability::Unavailable { reason } = probe() {
            eprintln!("skipped: {reason}");
            return;
        }
        let session = session().expect("a session");
        let session = session
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let Session {
            ref instance,
            physical,
            ref device,
            ..
        } = *session;
        let context = (instance, physical, device);
        let (width, height) = (WIDTH, HEIGHT);
        let first_texels = vec![1; (width * height) as usize];
        let first = BoundImages::create(context, &images(&first_texels)).expect("created");
        let (image, sampler) = (first.images[0].image, first.sampler);
        first.destroy(device);
        let second_texels = vec![0x0203_0405; (width * height) as usize];
        let second = BoundImages::create(context, &images(&second_texels)).expect("created");
        let reused = (second.images[0].image, second.sampler);
        let staged = crate::compute::read_back(
            device,
            &crate::compute::DispatchBuffer {
                buffer: second.images[0].staging,
                memory: second.images[0].staging_memory,
                size: u64::from(width * height) * 4,
                words: (width * height) as usize,
                offset: 0,
            },
        );
        second.destroy(device);
        assert_eq!(reused, (image, sampler), "the image and sampler kept");
        assert!(
            staged
                .expect("read")
                .iter()
                .all(|&texel| texel == 0x0203_0405),
            "the second's own texels"
        );
    }
}
