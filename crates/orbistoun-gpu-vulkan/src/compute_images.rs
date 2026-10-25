//! The images a guest compute dispatch fetches from and stores to.
//!
//! A translated compute module names its images at the bindings a pixel shader's use: the image it
//! fetches from at [`orbistoun_spirv::TEXTURE_BINDING`], combined with a sampler a fetch ignores,
//! and the one it stores to at [`orbistoun_spirv::STORAGE_IMAGE_BINDING`]. Each arrives as linear
//! `R8G8B8A8` texels the caller detiled from guest memory; each is uploaded before the dispatch,
//! and the stored image is copied back out after it, so the caller can tile back what changed.

use ash::vk;

use crate::compute::DispatchError;

mod resident;
pub use resident::{Hints, with_hints};

/// A dispatch image's texel format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DispatchFormat {
    /// Four eight-bit normalised channels, the guest's `8_8_8_8_UNORM` in the same byte order.
    Rgba8,
    /// One eight-bit normalised channel, the guest's `8_UNORM`.
    R8,
    /// One eight-bit unsigned integer channel, the guest's `8_UINT`.
    R8Uint,
    /// Four eight-bit unsigned integer channels, the guest's `8_8_8_8_UINT` in the same byte order.
    Rgba8Uint,
    /// Four thirty-two-bit unsigned channels, the guest's `32_32_32_32_UINT`: four words a texel.
    Rgba32Uint,
    /// One thirty-two-bit unsigned channel, the guest's `32_UINT`: a word a texel.
    R32Uint,
    /// One sixteen-bit unsigned channel, the guest's `16_UINT`: the low half of a word a texel.
    R16Uint,
    /// One sixteen-bit normalised channel, the guest's `16_UNORM`: the low half of a word a texel
    /// (D916).
    R16Unorm,
    /// One sixteen-bit float channel, the guest's `16_FLOAT`: the low half of a word a texel.
    R16Float,
    /// Two sixteen-bit float channels, the guest's `16_16_FLOAT`: a word a texel, red lowest.
    Rg16Float,
    /// Two normalised eight-bit channels, the guest's `8_8_UNORM`: the low half of a word a texel.
    Rg8Unorm,
    /// Two thirty-two-bit unsigned channels, the guest's `32_32_UINT`: two words a texel (D916).
    Rg32Uint,
    /// One thirty-two-bit float channel, the guest's `32_FLOAT`: a word a texel, its bits as they are.
    R32Float,
    /// Ten normalised bits each of red, green and blue from the low end and two of alpha, the
    /// guest's `2_10_10_10_UNORM` in the same bit order.
    Rgb10A2,
    /// Four sixteen-bit floats, the guest's `16_16_16_16_FLOAT`: two words a texel.
    Rgba16Float,
    /// Four normalised sixteen-bit channels, the guest's `16_16_16_16_UNORM`: two words a texel,
    /// red lowest (D923).
    Rgba16Unorm,
    /// Four signed normalised eight-bit channels, the guest's `8_8_8_8_SNORM`: a word a texel, red
    /// lowest.
    Rgba8Snorm,
    /// Three packed unsigned floats, the guest's `10_11_11_FLOAT`: eleven bits of red from the low
    /// end, eleven of green, ten of blue, a word a texel (D908).
    Float11_11_10,
}

impl DispatchFormat {
    const fn vulkan(self) -> vk::Format {
        match self {
            Self::Rgba8 => vk::Format::R8G8B8A8_UNORM,
            Self::R8 => vk::Format::R8_UNORM,
            Self::R8Uint => vk::Format::R8_UINT,
            Self::Rgba8Uint => vk::Format::R8G8B8A8_UINT,
            Self::Rgba32Uint => vk::Format::R32G32B32A32_UINT,
            Self::R32Uint => vk::Format::R32_UINT,
            Self::R16Uint => vk::Format::R16_UINT,
            Self::R16Unorm => vk::Format::R16_UNORM,
            Self::R16Float => vk::Format::R16_SFLOAT,
            Self::Rg16Float => vk::Format::R16G16_SFLOAT,
            Self::Rg8Unorm => vk::Format::R8G8_UNORM,
            Self::Rg32Uint => vk::Format::R32G32_UINT,
            Self::R32Float => vk::Format::R32_SFLOAT,
            Self::Rgb10A2 => vk::Format::A2B10G10R10_UNORM_PACK32,
            Self::Rgba16Float => vk::Format::R16G16B16A16_SFLOAT,
            Self::Rgba16Unorm => vk::Format::R16G16B16A16_UNORM,
            Self::Rgba8Snorm => vk::Format::R8G8B8A8_SNORM,
            Self::Float11_11_10 => vk::Format::B10G11R11_UFLOAT_PACK32,
        }
    }

    /// Bytes one texel takes.
    const fn bytes(self) -> usize {
        match self {
            Self::Rgba8
            | Self::Rgba8Uint
            | Self::Rgb10A2
            | Self::Float11_11_10
            | Self::R32Uint
            | Self::Rg16Float
            | Self::R32Float
            | Self::Rgba8Snorm => 4,
            Self::R8 | Self::R8Uint => 1,
            Self::R16Uint | Self::R16Unorm | Self::R16Float | Self::Rg8Unorm => 2,
            Self::Rgba16Float | Self::Rgba16Unorm | Self::Rg32Uint => 8,
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
    /// Layers of a 2D array the program addresses as one, its texels layer after layer (D895,
    /// D897); `None` for an image it does not.
    pub layers: Option<u32>,
    /// The sampler a fetched image is sampled through, where its program names one (D904); `None`
    /// for one only fetched, bound with the kept sampler a fetch ignores.
    pub sampling: Option<orbistoun_gpu::TextureSampling>,
    /// Whether the program samples it as a cube, its six layers the faces, seen through a cube view
    /// (D907).
    pub cube: bool,
}

impl DispatchImage<'_> {
    /// Texels it holds: every slice's.
    fn count(&self) -> usize {
        self.width as usize
            * self.height as usize
            * self.depth.unwrap_or(1) as usize
            * self.layers.unwrap_or(1) as usize
    }
}

/// The images a dispatch binds: those it fetches from and those it stores to.
#[derive(Debug, Clone, Copy, Default)]
pub struct DispatchImages<'a> {
    /// Bound at the texture binding: the first texture slot's.
    pub fetched: Option<DispatchImage<'a>>,
    /// The further texture slots' images, in order, each at its slot's binding
    /// ([`orbistoun_spirv::TEXTURE_BINDINGS`]).
    pub more_fetched: &'a [DispatchImage<'a>],
    /// The images it stores to, in order, each at its storage-image binding
    /// ([`orbistoun_spirv::STORAGE_IMAGE_BINDINGS`]) and read back after the dispatch (D893, D963).
    pub stored: &'a [DispatchImage<'a>],
    /// The images its program selects from tables at run time, and the directory its module finds
    /// them in (D983); `None` where it selects none.
    pub selected: Option<SelectedImageSet<'a>>,
}

/// The images a dispatch's program selects from tables at run time (D983): every one the tables
/// name, each at its place in the arrays at [`orbistoun_spirv::SELECTED_IMAGES_BINDING`] and
/// [`orbistoun_spirv::SELECTED_CUBES_BINDING`], and the directory at
/// [`orbistoun_spirv::SELECTED_DIRECTORY_BINDING`].
#[derive(Debug, Clone, Copy)]
pub struct SelectedImageSet<'a> {
    /// The texture slots, one bit each, whose own binding holds their sampler alone: the image
    /// [`DispatchImages::fetched`] or [`DispatchImages::more_fetched`] gives the slot is not bound.
    pub slots: u32,
    /// Each image, at its place: a cube where its `cube` says so, bound in the array of cubes, and
    /// any other in the array of flat images.
    pub images: &'a [DispatchImage<'a>],
    /// The directory's words.
    pub directory: &'a [u32],
}

/// Whether the device indexes an array of sampled images by a value each invocation computes,
/// which a dispatch reading images selected at run time needs (D983): set when the session's
/// device is made.
static SELECTED_IMAGES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Records whether the session's device was made with `features` indexing an array of sampled
/// images by a value each invocation computes (D983).
pub(crate) fn note_enabled(features: &vk::PhysicalDeviceVulkan12Features<'_>) {
    SELECTED_IMAGES.store(
        features.shader_sampled_image_array_non_uniform_indexing == vk::TRUE,
        std::sync::atomic::Ordering::Relaxed,
    );
}

/// The binding an image read only as one of a dispatch's selected images carries: none of its own.
const ARRAYED: u32 = u32::MAX;

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
    layers: Option<u32>,
    /// The sampler a fetched image is bound with; `None` for a stored one.
    sampler: Option<vk::Sampler>,
    /// Whether its binding holds its sampler alone: a slot reading images selected at run time,
    /// whose image the arrays hold (D983).
    sampler_only: bool,
    /// Where it is kept after the dispatch, where it has a place among the images kept (D985).
    place: Option<resident::Place>,
    /// The layout its contents were kept in, where they are taken as they are and nothing is
    /// uploaded (D985).
    kept_layout: Option<vk::ImageLayout>,
}

/// A dispatch's selected images on the device (D983): where each lies among the images, whether it
/// is a cube, the images filling the arrays' other places, and the directory.
struct SelectedBound {
    /// Each selected image's index among [`BoundImages::images`], and whether it is a cube.
    placed: Vec<(usize, bool)>,
    /// The flat image and the cube every other place of each array holds.
    fillers: (usize, usize),
    /// The directory's buffer and memory.
    directory: (vk::Buffer, vk::DeviceMemory),
}

/// A dispatch's images on the device, each fetched one with the sampler it is bound with.
pub(crate) struct BoundImages {
    images: Vec<Bound>,
    /// Its selected images, where it reads any (D983).
    selected: Option<SelectedBound>,
}

impl BoundImages {
    /// No images: a dispatch that reads and writes only its window.
    pub(crate) const fn none() -> Self {
        Self {
            images: Vec::new(),
            selected: None,
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
        if images.stored.len() > orbistoun_spirv::STORAGE_IMAGE_BINDINGS.len() {
            return Err(DispatchError::Unsupported(
                "more stored images than a stage has storage-image bindings".to_owned(),
            ));
        }
        // The device's own bound, which may be as low as four (D963).
        // SAFETY: the physical device came from this instance.
        let reach = unsafe { instance.get_physical_device_properties(physical) }
            .limits
            .max_per_stage_descriptor_storage_images;
        if images.stored.len() > reach as usize {
            return Err(DispatchError::Unsupported(format!(
                "{} stored images, more than the {reach} storage images this device lets one stage reach (D963)",
                images.stored.len()
            )));
        }
        if let Some(set) = images.selected {
            selected_room((instance, physical), set)?;
        }
        let fillers = selected_fillers();
        // Each image's place among those the device keeps, where it has one (D985).
        let (places, dropped) = resident::wanted(images);
        resident::let_go(device, &dropped);
        let wanted = wanted_images(images, &fillers);
        let places = places.into_iter().chain(std::iter::repeat(None));
        for ((image, binding, stored, sampler_only), used) in wanted.into_iter().zip(places) {
            let Some(image) = image else { continue };
            let key = SpareKey {
                device: vk::Handle::as_raw(device.handle()),
                format: image.format,
                width: image.width,
                height: image.height,
                depth: image.depth,
                layers: image.layers,
                stored,
            };
            let (mut place, mut kept_layout) = (None, None);
            let taken = resident::take((instance, physical, device), used, image).map(|taken| {
                taken.map(|(created, layout, at)| {
                    (place, kept_layout) = (Some(at), layout);
                    created
                })
            });
            let created = match taken.or_else(|| take_spare(device, key, image)) {
                Some(reused) => reused,
                None => {
                    orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DispatchNewImage, || {
                        create_image((instance, physical, device), image, stored, None)
                    })
                }
            };
            let sampler = if stored || binding == ARRAYED {
                Ok(None)
            } else {
                image_sampler(device, image.sampling).map(Some)
            };
            let created = created.and_then(|made| sampler.map(|sampler| (made, sampler)));
            match created {
                Ok(((image_handle, memory, view, staging, staging_memory), sampler)) => {
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
                        layers: image.layers,
                        sampler,
                        sampler_only,
                        place,
                        kept_layout,
                    });
                }
                Err(e) => {
                    bound.destroy(device);
                    return Err(e);
                }
            }
        }
        if let Some(set) = images.selected
            && let Err(e) = bound.place_selected((instance, physical, device), set)
        {
            bound.destroy(device);
            return Err(e);
        }
        Ok(bound)
    }

    /// Notes where each of `set`'s images, created after every image bound singly, lies among the
    /// images, and makes its directory (D983).
    fn place_selected(
        &mut self,
        context: (&ash::Instance, vk::PhysicalDevice, &ash::Device),
        set: SelectedImageSet<'_>,
    ) -> Result<(), DispatchError> {
        let arrayed: Vec<usize> = (0..self.images.len())
            .filter(|&at| self.images[at].binding == ARRAYED)
            .collect();
        let (placed, filling) = arrayed.split_at(set.images.len());
        let directory = directory_buffer(context, set.directory)?;
        self.selected = Some(SelectedBound {
            placed: placed
                .iter()
                .zip(set.images)
                .map(|(&at, image)| (at, image.cube))
                .collect(),
            fillers: (filling[0], filling[1]),
            directory,
        });
        Ok(())
    }

    /// The set layout bindings the images take.
    pub(crate) fn layout_bindings(&self) -> Vec<vk::DescriptorSetLayoutBinding<'static>> {
        let mut bindings: Vec<vk::DescriptorSetLayoutBinding<'static>> = self
            .bound_singly()
            .map(|image| {
                vk::DescriptorSetLayoutBinding::default()
                    .binding(image.binding)
                    .descriptor_type(image.descriptor_type())
                    .descriptor_count(1)
                    .stage_flags(vk::ShaderStageFlags::COMPUTE)
            })
            .collect();
        if self.selected.is_some() {
            bindings.extend(selected_layout_bindings());
        }
        bindings
    }

    /// The images bound each at a binding of its own: all but the selected images (D983).
    fn bound_singly(&self) -> impl Iterator<Item = &Bound> {
        self.images.iter().filter(|image| image.binding != ARRAYED)
    }

    /// The pool room the images take.
    pub(crate) fn pool_sizes(&self) -> Vec<vk::DescriptorPoolSize> {
        let mut sizes: Vec<vk::DescriptorPoolSize> = self
            .bound_singly()
            .map(|image| {
                vk::DescriptorPoolSize::default()
                    .ty(image.descriptor_type())
                    .descriptor_count(1)
            })
            .collect();
        if self.selected.is_some() {
            sizes.extend(selected_layout_bindings().iter().map(|binding| {
                vk::DescriptorPoolSize::default()
                    .ty(binding.descriptor_type)
                    .descriptor_count(binding.descriptor_count)
            }));
        }
        sizes
    }

    /// Points `set`'s image bindings at the images.
    pub(crate) fn write(&self, device: &ash::Device, set: vk::DescriptorSet) {
        let singly: Vec<&Bound> = self.bound_singly().collect();
        let infos: Vec<[vk::DescriptorImageInfo; 1]> = singly
            .iter()
            .map(|image| {
                let info = vk::DescriptorImageInfo::default()
                    .image_view(image.view)
                    .image_layout(layout(image.stored));
                [match (image.stored, image.sampler) {
                    // A sampler alone names no image (D983).
                    (false, Some(sampler)) if image.sampler_only => {
                        vk::DescriptorImageInfo::default().sampler(sampler)
                    }
                    (false, Some(sampler)) => info.sampler(sampler),
                    _ => info,
                }]
            })
            .collect();
        let mut writes: Vec<vk::WriteDescriptorSet<'_>> = singly
            .iter()
            .zip(&infos)
            .map(|(image, info)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(image.binding)
                    .descriptor_type(image.descriptor_type())
                    .image_info(info)
            })
            .collect();
        // Every place of both arrays: each selected image in its own shape's array, the fillers in
        // every other place (D983).
        let arrays = self.selected.as_ref().map(|selected| {
            let view = |at: usize| {
                vk::DescriptorImageInfo::default()
                    .image_view(self.images[at].view)
                    .image_layout(vk::ImageLayout::SHADER_READ_ONLY_OPTIMAL)
            };
            let most = orbistoun_spirv::MOST_SELECTED_IMAGES as usize;
            let (mut flat, mut cube) = (
                vec![view(selected.fillers.0); most],
                vec![view(selected.fillers.1); most],
            );
            for (place, &(at, is_cube)) in selected.placed.iter().enumerate().take(most) {
                if is_cube {
                    cube[place] = view(at);
                } else {
                    flat[place] = view(at);
                }
            }
            let directory = [vk::DescriptorBufferInfo::default()
                .buffer(selected.directory.0)
                .offset(0)
                .range(vk::WHOLE_SIZE)];
            (flat, cube, directory)
        });
        if let Some((flat, cube, directory)) = &arrays {
            for (binding, infos) in [
                (orbistoun_spirv::SELECTED_IMAGES_BINDING, flat),
                (orbistoun_spirv::SELECTED_CUBES_BINDING, cube),
            ] {
                writes.push(
                    vk::WriteDescriptorSet::default()
                        .dst_set(set)
                        .dst_binding(binding)
                        .descriptor_type(vk::DescriptorType::SAMPLED_IMAGE)
                        .image_info(infos),
                );
            }
            writes.push(
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(orbistoun_spirv::SELECTED_DIRECTORY_BINDING)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(directory),
            );
        }
        // SAFETY: the set, views, samplers and directory are live, and the infos outlive the call.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
    }

    /// Records each image's upload from its host buffer and the move to the layout the dispatch
    /// uses it in, into an open recording.
    pub(crate) fn record_upload(&self, device: &ash::Device, command: vk::CommandBuffer) {
        for image in &self.images {
            let access = if image.stored {
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE
            } else {
                vk::AccessFlags::SHADER_READ
            };
            // An image kept as it was left is moved to the layout the dispatch uses it in, its
            // contents kept, after whatever last wrote it (D985).
            if let Some(kept) = image.kept_layout {
                barrier(
                    device,
                    command,
                    image.image,
                    (kept, layout(image.stored)),
                    (
                        vk::AccessFlags::SHADER_WRITE | vk::AccessFlags::TRANSFER_WRITE,
                        access,
                    ),
                    (
                        vk::PipelineStageFlags::ALL_COMMANDS,
                        vk::PipelineStageFlags::COMPUTE_SHADER,
                    ),
                );
                continue;
            }
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
            let region = [whole(
                image.width,
                image.height,
                (image.depth, image.layers),
            )];
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
            let region = [whole(
                image.width,
                image.height,
                (image.depth, image.layers),
            )];
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

    /// Each stored image's texels as the dispatch left them, in the order they were bound, once
    /// the recording has completed.
    pub(crate) fn read_stored(&self, device: &ash::Device) -> Result<Vec<Vec<u32>>, DispatchError> {
        self.images
            .iter()
            .filter(|image| image.stored)
            .map(|image| read_back_image(device, image))
            .collect()
    }

    /// Releases everything created, once the dispatch that used it has finished: each image to
    /// the spares while they hold few enough bytes, else destroyed. The sampler is kept.
    pub(crate) fn destroy(self, device: &ash::Device) {
        let raw = vk::Handle::as_raw(device.handle());
        if let Some(selected) = &self.selected {
            release_buffer(device, selected.directory.0, selected.directory.1);
        }
        for image in self.images {
            // An image with a place among those kept is kept, in the layout the dispatch left it
            // in (D985).
            if let Some(place) = image.place {
                let created = (
                    image.image,
                    image.memory,
                    image.view,
                    image.staging,
                    image.staging_memory,
                );
                resident::keep(device, place, (created, layout(image.stored)));
                continue;
            }
            let key = SpareKey {
                device: raw,
                format: image.format,
                width: image.width,
                height: image.height,
                depth: image.depth,
                layers: image.layers,
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

/// One stored image's texels as the dispatch left them, once the recording has completed.
fn read_back_image(device: &ash::Device, image: &Bound) -> Result<Vec<u32>, DispatchError> {
    let texels = image.width as usize
        * image.height as usize
        * image.depth.unwrap_or(1) as usize
        * image.layers.unwrap_or(1) as usize;
    let size = image.format.bytes();
    let bytes = (texels * size) as vk::DeviceSize;
    // SAFETY: the memory is host-visible and coherent, holds `bytes`, and is not mapped; the
    // copy into it has completed.
    let mapped =
        unsafe { device.map_memory(image.staging_memory, 0, bytes, vk::MemoryMapFlags::empty()) }
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
    Ok(texels)
}

/// The set layout bindings of a dispatch fetching `fetched` images and storing to `stored`, at the
/// bindings [`BoundImages::create`] gives them: a pipeline's layout before any dispatch's images
/// exist.
pub(crate) fn layout_bindings_for(
    fetched: usize,
    stored: usize,
) -> Vec<vk::DescriptorSetLayoutBinding<'static>> {
    let fetched = orbistoun_spirv::TEXTURE_BINDINGS
        .iter()
        .take(fetched)
        .map(|binding| (*binding, false));
    let stored = orbistoun_spirv::STORAGE_IMAGE_BINDINGS
        .iter()
        .take(stored)
        .map(|binding| (*binding, true));
    fetched
        .chain(stored)
        .map(|(binding, stored)| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(binding)
                .descriptor_type(descriptor_type(stored))
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        })
        .collect()
}

/// A fetched image is a combined image sampler and a stored one a storage image.
const fn descriptor_type(stored: bool) -> vk::DescriptorType {
    if stored {
        vk::DescriptorType::STORAGE_IMAGE
    } else {
        vk::DescriptorType::COMBINED_IMAGE_SAMPLER
    }
}

/// Each image a dispatch binds, in the order they are made: its binding, whether it is stored to,
/// and whether its binding holds its sampler alone - a slot reading images selected at run time
/// (D983). The selected images follow at no binding of their own, then the `fillers` the arrays'
/// other places hold.
fn wanted_images<'a>(
    images: &DispatchImages<'a>,
    fillers: &'a [DispatchImage<'a>; 2],
) -> Vec<(Option<DispatchImage<'a>>, u32, bool, bool)> {
    let sampler_only = |slot: usize| {
        images
            .selected
            .is_some_and(|set| slot < 32 && set.slots >> slot & 1 != 0)
    };
    let mut wanted = vec![(
        images.fetched,
        orbistoun_spirv::TEXTURE_BINDING,
        false,
        sampler_only(0),
    )];
    wanted.extend(
        images
            .stored
            .iter()
            .zip(orbistoun_spirv::STORAGE_IMAGE_BINDINGS)
            .map(|(image, binding)| (Some(*image), binding, true, false)),
    );
    wanted.extend(
        images
            .more_fetched
            .iter()
            .zip(&orbistoun_spirv::TEXTURE_BINDINGS[1..])
            .enumerate()
            .map(|(n, (image, binding))| (Some(*image), *binding, false, sampler_only(n + 1))),
    );
    if let Some(set) = images.selected {
        wanted.extend(
            set.images
                .iter()
                .chain(fillers)
                .map(|image| (Some(*image), ARRAYED, false, false)),
        );
    }
    wanted
}

impl Bound {
    /// What its binding holds: a storage image, a sampler alone for a slot reading images selected
    /// at run time (D983), or an image with its sampler.
    const fn descriptor_type(&self) -> vk::DescriptorType {
        if self.sampler_only {
            vk::DescriptorType::SAMPLER
        } else {
            descriptor_type(self.stored)
        }
    }
}

/// The bindings a dispatch reading images selected at run time adds (D983): the arrays of flat
/// images and of cubes, each [`orbistoun_spirv::MOST_SELECTED_IMAGES`] long, and the directory.
fn selected_layout_bindings() -> [vk::DescriptorSetLayoutBinding<'static>; 3] {
    let binding = |binding: u32, kind: vk::DescriptorType, count: u32| {
        vk::DescriptorSetLayoutBinding::default()
            .binding(binding)
            .descriptor_type(kind)
            .descriptor_count(count)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
    };
    let most = orbistoun_spirv::MOST_SELECTED_IMAGES;
    [
        binding(
            orbistoun_spirv::SELECTED_IMAGES_BINDING,
            vk::DescriptorType::SAMPLED_IMAGE,
            most,
        ),
        binding(
            orbistoun_spirv::SELECTED_CUBES_BINDING,
            vk::DescriptorType::SAMPLED_IMAGE,
            most,
        ),
        binding(
            orbistoun_spirv::SELECTED_DIRECTORY_BINDING,
            vk::DescriptorType::STORAGE_BUFFER,
            1,
        ),
    ]
}

/// The flat image and the cube the arrays' places no selected image of their shape takes hold: one
/// texel of zero, and a cube of six. A lookup never reaches them; the directory names no place
/// in the other shape's array (D983).
fn selected_fillers() -> [DispatchImage<'static>; 2] {
    static FLAT: [u32; 1] = [0];
    static CUBE: [u32; 6] = [0; 6];
    let filler = |texels: &'static [u32], cube: bool| DispatchImage {
        texels,
        format: DispatchFormat::Rgba8,
        width: 1,
        height: 1,
        depth: None,
        layers: cube.then_some(6),
        sampling: None,
        cube,
    };
    [filler(&FLAT, false), filler(&CUBE, true)]
}

/// Refuses a dispatch reading images selected at run time that this device cannot bind: one that
/// does not index an array of sampled images by a value each invocation computes, or lets one
/// stage reach fewer sampled images than the two arrays and the texture slots hold (D983).
fn selected_room(
    (instance, physical): (&ash::Instance, vk::PhysicalDevice),
    set: SelectedImageSet<'_>,
) -> Result<(), DispatchError> {
    if !SELECTED_IMAGES.load(std::sync::atomic::Ordering::Relaxed) {
        return Err(DispatchError::Unsupported(
            "the device does not index an array of sampled images by a value each invocation computes, which an image selected at run time is read through (D983)".to_owned(),
        ));
    }
    if set.images.len() > orbistoun_spirv::MOST_SELECTED_IMAGES as usize {
        return Err(DispatchError::Unsupported(format!(
            "{} images selected at run time, more than the arrays hold (D983)",
            set.images.len()
        )));
    }
    // SAFETY: the physical device came from this instance.
    let limits = unsafe { instance.get_physical_device_properties(physical) }.limits;
    let wanted = 2 * orbistoun_spirv::MOST_SELECTED_IMAGES + orbistoun_spirv::MAX_TEXTURES as u32;
    let reach = limits
        .max_per_stage_descriptor_sampled_images
        .min(limits.max_descriptor_set_sampled_images);
    if reach < wanted {
        return Err(DispatchError::Unsupported(format!(
            "the device lets a stage reach {reach} sampled images, fewer than the {wanted} the arrays of images selected at run time and the texture slots take (D983)"
        )));
    }
    Ok(())
}

/// A host-visible storage buffer holding the directory's `words` (D983).
fn directory_buffer(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    words: &[u32],
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    let bytes = zerocopy::IntoBytes::as_bytes(words);
    let info = vk::BufferCreateInfo::default()
        .size(bytes.len().max(4) as vk::DeviceSize)
        .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer(selected directory)", e))?;
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
    let filled = unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .and_then(|()| fill_staging(device, memory, bytes));
    match filled {
        Ok(()) => Ok((buffer, memory)),
        Err(e) => {
            release_buffer(device, buffer, memory);
            Err(DispatchError::Vulkan("fill(selected directory)", e))
        }
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

/// The whole of a single-level image, every slice of a volume or layer of an array, tightly packed
/// in its buffer.
fn whole(
    width: u32,
    height: u32,
    (depth, layers): (Option<u32>, Option<u32>),
) -> vk::BufferImageCopy {
    vk::BufferImageCopy::default()
        .image_subresource(
            vk::ImageSubresourceLayers::default()
                .aspect_mask(vk::ImageAspectFlags::COLOR)
                .layer_count(layers.unwrap_or(1)),
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
                .layer_count(vk::REMAINING_ARRAY_LAYERS),
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

/// Creates one image and the host buffer holding its texels, for the roles `usage` names where it
/// is given - an image kept between dispatches is made for every role it may be bound in (D985) -
/// else for the one `stored` says.
fn create_image(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    texels: DispatchImage<'_>,
    stored: bool,
    usage: Option<vk::ImageUsageFlags>,
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
    let usage = usage.unwrap_or(if stored {
        vk::ImageUsageFlags::STORAGE
            | vk::ImageUsageFlags::TRANSFER_DST
            | vk::ImageUsageFlags::TRANSFER_SRC
    } else {
        vk::ImageUsageFlags::SAMPLED | vk::ImageUsageFlags::TRANSFER_DST
    });
    // A volume is a three-dimensional image of its slices, seen through a view of the same type,
    // which is what a module's three-dimensional storage image binds (D823); an array the program
    // addresses as one is seen through an array view of its layers (D897), and a cube through a
    // cube view of its six (D907).
    let (image_type, view_type) = if texels.depth.is_some() {
        (vk::ImageType::TYPE_3D, vk::ImageViewType::TYPE_3D)
    } else if texels.cube {
        (vk::ImageType::TYPE_2D, vk::ImageViewType::CUBE)
    } else if texels.layers.is_some() {
        (vk::ImageType::TYPE_2D, vk::ImageViewType::TYPE_2D_ARRAY)
    } else {
        (vk::ImageType::TYPE_2D, vk::ImageViewType::TYPE_2D)
    };
    let layers = texels.layers.unwrap_or(1);
    let flags = if texels.cube {
        vk::ImageCreateFlags::CUBE_COMPATIBLE
    } else {
        vk::ImageCreateFlags::empty()
    };
    let info = vk::ImageCreateInfo::default()
        .flags(flags)
        .image_type(image_type)
        .format(texels.format.vulkan())
        .extent(vk::Extent3D {
            width: texels.width,
            height: texels.height,
            depth: texels.depth.unwrap_or(1),
        })
        .mip_levels(1)
        .array_layers(layers)
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
                    .layer_count(layers),
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
    let _for = crate::device_memory::purpose("dispatch images");
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
    layers: Option<u32>,
    stored: bool,
}

impl SpareKey {
    /// Bytes it holds, on the device and in its staging buffer together.
    fn bytes(self) -> u64 {
        u64::from(self.width)
            * u64::from(self.height)
            * u64::from(self.depth.unwrap_or(1))
            * u64::from(self.layers.unwrap_or(1))
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

/// The sampler a fetched image is bound with: its guest sampler's, made once for each setting on a
/// device and kept (D904), or the kept one a fetch ignores.
fn image_sampler(
    device: &ash::Device,
    sampling: Option<orbistoun_gpu::TextureSampling>,
) -> Result<vk::Sampler, DispatchError> {
    type Kept = Vec<(u64, orbistoun_gpu::TextureSampling, vk::Sampler)>;
    static KEPT: std::sync::Mutex<Kept> = std::sync::Mutex::new(Vec::new());
    let Some(sampling) = sampling else {
        return kept_sampler(device);
    };
    let raw = vk::Handle::as_raw(device.handle());
    let mut kept = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some((_, _, sampler)) = kept
        .iter()
        .find(|(on, kept, _)| *on == raw && *kept == sampling)
    {
        return Ok(*sampler);
    }
    let mut custom = crate::framebuffer::border_colour_info(sampling.border);
    let mut info = crate::framebuffer::sampler_info(sampling, 1, false);
    if let Some(custom) = custom.as_mut() {
        info = info.push_next(custom);
    }
    // SAFETY: the device is live and the create info outlives the call.
    let sampler = unsafe { device.create_sampler(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_sampler(dispatch)", e))?;
    kept.push((raw, sampling, sampler));
    Ok(sampler)
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
            selected: None,
            more_fetched: &[],
            fetched: Some(DispatchImage {
                texels,
                format: DispatchFormat::Rgba8,
                width: WIDTH,
                height: HEIGHT,
                depth: None,
                layers: None,
                sampling: None,
                cube: false,
            }),
            stored: &[],
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
        let (image, sampler) = (first.images[0].image, first.images[0].sampler);
        first.destroy(device);
        let second_texels = vec![0x0203_0405; (width * height) as usize];
        let second = BoundImages::create(context, &images(&second_texels)).expect("created");
        let reused = (second.images[0].image, second.images[0].sampler);
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
