//! The images a guest compute dispatch fetches from and stores to.
//!
//! A translated compute module names its images at the bindings a pixel shader's use: the image it
//! fetches from at [`orbistoun_spirv::TEXTURE_BINDING`], combined with a sampler a fetch ignores,
//! and the one it stores to at [`orbistoun_spirv::STORAGE_IMAGE_BINDING`]. Each arrives as linear
//! `R8G8B8A8` texels the caller detiled from guest memory; each is uploaded before the dispatch,
//! and the stored image is copied back out after it, so the caller can tile back what changed.

use ash::vk;

use crate::compute::DispatchError;

/// The format both images are created in: four eight-bit normalised channels, the guest's
/// `8_8_8_8_UNORM` in the same byte order.
const FORMAT: vk::Format = vk::Format::R8G8B8A8_UNORM;

/// One image a dispatch reads or writes, as linear texels.
#[derive(Debug, Clone, Copy)]
pub struct DispatchImage<'a> {
    /// Row-major texels, `width * height` of them.
    pub texels: &'a [u32],
    /// Width in texels.
    pub width: u32,
    /// Height in texels.
    pub height: u32,
}

/// The images a dispatch binds: the one it fetches from and the one it stores to.
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchImages<'a> {
    /// Bound at the texture binding.
    pub fetched: Option<DispatchImage<'a>>,
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
    width: u32,
    height: u32,
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
        let wanted = [
            (images.fetched, orbistoun_spirv::TEXTURE_BINDING, false),
            (images.stored, orbistoun_spirv::STORAGE_IMAGE_BINDING, true),
        ];
        for (image, binding, stored) in wanted {
            let Some(image) = image else { continue };
            let created = create_image((instance, physical, device), image, stored);
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
                        width: image.width,
                        height: image.height,
                    });
                }
                Err(e) => {
                    bound.destroy(device);
                    return Err(e);
                }
            }
        }
        if bound.images.iter().any(|image| !image.stored) {
            let info = vk::SamplerCreateInfo::default()
                .mag_filter(vk::Filter::NEAREST)
                .min_filter(vk::Filter::NEAREST)
                .mipmap_mode(vk::SamplerMipmapMode::NEAREST)
                .address_mode_u(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_v(vk::SamplerAddressMode::CLAMP_TO_EDGE)
                .address_mode_w(vk::SamplerAddressMode::CLAMP_TO_EDGE);
            // SAFETY: the device is live and the create info outlives the call.
            match unsafe { device.create_sampler(&info, None) } {
                Ok(sampler) => bound.sampler = Some(sampler),
                Err(e) => {
                    bound.destroy(device);
                    return Err(DispatchError::Vulkan("create_sampler", e));
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
            let region = [whole(image.width, image.height)];
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
            let region = [whole(image.width, image.height)];
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
        let words = image.width as usize * image.height as usize;
        let bytes = (words * 4) as vk::DeviceSize;
        // SAFETY: the memory is host-visible and coherent, holds `bytes`, and is not mapped; the
        // copy into it has completed.
        let mapped = unsafe {
            device.map_memory(image.staging_memory, 0, bytes, vk::MemoryMapFlags::empty())
        }
        .map_err(|e| DispatchError::Vulkan("map_memory(stored image)", e))?;
        // SAFETY: the mapping covers `words` whole `u32`s and nothing writes it while it is read.
        let texels =
            unsafe { std::slice::from_raw_parts(mapped.cast::<u32>().cast_const(), words) }
                .to_vec();
        // SAFETY: mapped just above.
        unsafe { device.unmap_memory(image.staging_memory) };
        Ok(Some(texels))
    }

    /// Releases everything created.
    pub(crate) fn destroy(self, device: &ash::Device) {
        for image in self.images {
            release_image(device, (image.image, image.memory, Some(image.view)));
            release_buffer(device, image.staging, image.staging_memory);
        }
        if let Some(sampler) = self.sampler {
            // SAFETY: as above.
            unsafe { device.destroy_sampler(sampler, None) };
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

/// The whole of a single-level image, tightly packed in its buffer.
fn whole(width: u32, height: u32) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
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
    let words = texels.width as usize * texels.height as usize;
    if texels.texels.len() != words || words == 0 {
        return Err(DispatchError::Unsupported(
            "a dispatch image's texels are not its extent".to_owned(),
        ));
    }
    let usage = if stored {
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::TRANSFER_SRC
    } else {
        vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST
    };
    let info = vk::ImageCreateInfo::default()
        .image_type(vk::ImageType::TYPE_2D)
        .format(FORMAT)
        .extent(vk::Extent3D {
            width: texels.width,
            height: texels.height,
            depth: 1,
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
            .view_type(vk::ImageViewType::TYPE_2D)
            .format(FORMAT)
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
    let staged = staging_buffer((instance, physical, device), texels.texels);
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
    texels: &[u32],
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    let bytes = (texels.len() * 4) as vk::DeviceSize;
    let info = vk::BufferCreateInfo::default()
        .size(bytes)
        .usage(vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer(dispatch image)", e))?;
    // SAFETY: the buffer was created on this device.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    let memory = match allocate(
        (instance, physical, device),
        requirements,
        vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT,
    ) {
        Ok(memory) => memory,
        Err(e) => {
            // SAFETY: created above and never used.
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(e);
        }
    };
    // SAFETY: buffer and memory come from this device and the allocation is large enough.
    let filled = unsafe { device.bind_buffer_memory(buffer, memory, 0) }.and_then(|()| {
        // SAFETY: the memory is host-visible, holds `bytes`, and is not mapped.
        let mapped = unsafe { device.map_memory(memory, 0, bytes, vk::MemoryMapFlags::empty()) }?;
        // SAFETY: the mapping covers `texels.len()` whole `u32`s, exclusively ours while mapped.
        unsafe {
            std::ptr::copy_nonoverlapping(texels.as_ptr(), mapped.cast::<u32>(), texels.len());
        }
        // SAFETY: mapped just above, and the copy into it has finished.
        unsafe { device.unmap_memory(memory) };
        Ok(())
    });
    match filled {
        Ok(()) => Ok((buffer, memory)),
        Err(e) => {
            release_buffer(device, buffer, memory);
            Err(DispatchError::Vulkan("fill(dispatch image)", e))
        }
    }
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
