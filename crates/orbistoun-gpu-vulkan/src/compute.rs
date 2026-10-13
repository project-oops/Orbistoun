//! Runs a compute shader and reads back what it wrote.
//!
//! `spirv-val` checks that a module is well-formed; a dispatch with known inputs checks that it
//! computes the right thing. [`probe`] reports a missing device separately from a failure, so a
//! caller surfaces a skip rather than passing a test that never ran. A software implementation is
//! the preferred device because it is deterministic: this verifies the translator, not any
//! hardware.
//!
//! Resources are released on the successful path only. An error abandons them, because the process
//! exits shortly after; a long-lived caller needs guard types first.

use ash::vk;

/// The Vulkan loader, loaded once for the life of the process and never unloaded.
///
/// Dropping an [`ash::Entry`] unloads the loader library and every layer it pulled in. Layers keep
/// process-wide and thread-local state that faults when unloaded under another thread, so the
/// loader is a process-wide static.
fn entry() -> Result<&'static ash::Entry, DispatchError> {
    static ENTRY: std::sync::OnceLock<Option<ash::Entry>> = std::sync::OnceLock::new();
    // SAFETY: `Entry::load` requires that the loader is not concurrently unloaded. Nothing unloads
    // it: the `OnceLock` serialises the load and keeps the result alive for the process.
    ENTRY
        .get_or_init(|| unsafe { ash::Entry::load() }.ok())
        .as_ref()
        .ok_or(DispatchError::Vulkan(
            "Entry::load",
            vk::Result::ERROR_INITIALIZATION_FAILED,
        ))
}

/// What a device reports about itself, beyond existing.
///
/// Every field is a property of the device, reported verbatim; judging whether it is good enough
/// belongs to the caller.
#[derive(Debug, Clone, PartialEq, Eq)]
// Each boolean is an independent device fact read on its own by the code that needs it; this is a
// report, not a parameter list, so grouping them would only add indirection.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a device report is a list of independent facts, not an argument list"
)]
pub struct Properties {
    /// What the driver calls itself, for the record.
    pub device: String,
    /// Whether 32-bit subnormal results survive rather than being flushed to zero.
    ///
    /// A module may require this through `SPV_KHR_float_controls`. The guest's division pre-scale
    /// depends on it: on a flushing device its subnormal tests both answer false and the scaling is
    /// skipped.
    pub subnormals_preserved: bool,
    /// Whether 32-bit signed zeros, infinities and NaNs survive a module that declares they must.
    ///
    /// Every translated module declares it; on a device without it the declaration is removed
    /// before the module is created (`for_this_device`).
    pub inf_nan_preserved: bool,
    /// How many invocations share a subgroup on this device.
    ///
    /// Reported rather than assumed: the guest's wavefront is 32 or 64 lanes, and the ratio to the
    /// host subgroup is what a lane-mapped translation is built around.
    pub subgroup_size: u32,
    /// Whether the mesh stage may take a subgroup ballot and shuffle, which a primitive shader run
    /// one invocation per lane needs (D760).
    pub mesh_subgroup_ops: bool,
    /// Whether a fragment shader on this device may write a storage buffer.
    ///
    /// Reports what was enabled at device creation, not what the hardware offers, because a feature
    /// the device was not created with is unusable. A translated fragment module stores only to the
    /// observation window and to guest memory, so a fragment variant without either needs no
    /// feature.
    pub fragment_stores: bool,
    /// Whether a mesh stage may be used.
    ///
    /// A guest's primitive shader translates to a mesh stage (D688); without it a device runs a
    /// frame's pixel shaders but not its geometry.
    pub mesh_shading: bool,
    /// Whether a shader may write a storage image without declaring its format.
    ///
    /// A translated `image_store` needs this: the guest's format is in a descriptor nothing here
    /// decodes, so the module declares `Unknown` rather than guessing a format (D692).
    pub storage_image_write: bool,
    /// Whether this device can sample block-compressed images without decoding them.
    ///
    /// Vulkan consumes BC data natively, so on a device that offers this an upload path only undoes
    /// the tiling and a decoder is a fallback rather than a requirement.
    pub compressed_textures: bool,
}

/// Whether a device is available to run anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Availability {
    /// A device exists and compute can be dispatched.
    Available {
        /// What it reports about itself.
        properties: Properties,
    },
    /// No device. Not an error: a machine may legitimately have none.
    ///
    /// Carries the reason, because "no Vulkan" and "a Vulkan that refused us" want different
    /// responses.
    Unavailable {
        /// The reason, for a skip message worth reading.
        reason: String,
    },
}

impl Availability {
    /// Whether anything can be run.
    pub const fn is_available(&self) -> bool {
        matches!(self, Self::Available { .. })
    }
}

/// Why a dispatch failed.
///
/// Distinct from [`Availability`]: a device existed and something went wrong.
#[derive(Debug, thiserror::Error)]
pub enum DispatchError {
    /// A Vulkan call failed.
    #[error("vulkan: {0} failed ({1:?})")]
    Vulkan(&'static str, vk::Result),
    /// No memory type suits a buffer the host must read.
    #[error("no host-visible memory type; a buffer written by the device cannot be read back")]
    NoHostVisibleMemory,
    /// No queue family supports compute.
    #[error("no compute queue family on the selected device")]
    NoComputeQueue,
    /// The draw asks for state this cannot build exactly; named rather than approximated.
    #[error("unsupported: {0}")]
    Unsupported(String),
}

/// Reports whether compute can be dispatched here.
pub fn probe() -> Availability {
    // Asks the shared session rather than building its own instance, so the probe also tests that
    // opening the shared device works.
    match session() {
        Ok(session) => {
            let session = session
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            Availability::Available {
                properties: session.properties.clone(),
            }
        }
        Err(e) => Availability::Unavailable {
            reason: format!("no usable Vulkan device on this machine ({e})"),
        },
    }
}

/// Creates a zeroed storage buffer in memory the host can read.
///
/// Zeroing ensures a value read back was written by the shader rather than left in memory.
pub(crate) fn create_host_buffer(
    instance: &ash::Instance,
    physical: vk::PhysicalDevice,
    device: &ash::Device,
    size: vk::DeviceSize,
    words: usize,
) -> Result<(vk::Buffer, vk::DeviceMemory), DispatchError> {
    let buffer_info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(vk::BufferUsageFlags::STORAGE_BUFFER)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&buffer_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer", e))?;

    // SAFETY: the buffer was created on this device and not yet destroyed.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    // SAFETY: the physical device is valid.
    let memory_properties = unsafe { instance.get_physical_device_memory_properties(physical) };

    // Host-visible and coherent, so the result is read without an explicit flush or a staging copy.
    let wanted = vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
    let with = |flags: vk::MemoryPropertyFlags| {
        (0..memory_properties.memory_type_count).find(|i| {
            requirements.memory_type_bits & (1 << i) != 0
                && memory_properties.memory_types[*i as usize]
                    .property_flags
                    .contains(flags)
        })
    };
    // Cached where the device offers it, as `framebuffer::create_host_buffer` is: the window is
    // read back whole after every dispatch, and the first host-visible type is commonly
    // write-combined, which the host reads far more slowly.
    let memory_type = with(wanted | vk::MemoryPropertyFlags::HOST_CACHED)
        .or_else(|| with(wanted))
        .ok_or(DispatchError::NoHostVisibleMemory)?;

    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    let memory = unsafe { device.allocate_memory(&allocate, None) }
        .map_err(|e| DispatchError::Vulkan("allocate_memory", e))?;
    // SAFETY: buffer and memory both come from this device, and the allocation above is at least
    // the buffer's required size.
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_buffer_memory", e))?;

    // SAFETY: the memory is host-visible, was just allocated, and is not mapped.
    let mapped = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    // SAFETY: the mapping covers `size` bytes, which is `words` whole `u32`s, and this process has
    // exclusive access to it while mapped.
    unsafe { std::ptr::write_bytes(mapped.cast::<u8>(), 0, words * 4) };
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(memory) };

    Ok((buffer, memory))
}

/// A compute pipeline and the layouts it binds through, which outlive one dispatch.
#[derive(Debug, Clone, Copy)]
struct KeptPipeline {
    shader: vk::ShaderModule,
    set_layout: vk::DescriptorSetLayout,
    /// Set 1's, where the pipeline binds a dispatch's buffers.
    buffers_layout: Option<vk::DescriptorSetLayout>,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

/// A pipeline as one dispatch binds it: the pipeline, whether it is kept for later dispatches or
/// released after this one, and the descriptor set this dispatch fills.
struct BoundPipeline {
    kept: KeptPipeline,
    held: bool,
    descriptor_pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
}

/// Whether the device took up the float-controls guarantee, recorded when its properties are read.
static INF_NAN_PRESERVED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(true);

/// A module as this device can take it: translated modules declare `SignedZeroInfNanPreserve`, and
/// a device that does not offer it gets the module without that declaration.
pub(crate) fn for_this_device(words: &[u32]) -> std::borrow::Cow<'_, [u32]> {
    if INF_NAN_PRESERVED.load(std::sync::atomic::Ordering::Relaxed) {
        std::borrow::Cow::Borrowed(words)
    } else {
        std::borrow::Cow::Owned(orbistoun_spirv::without_inf_nan_preserve(words))
    }
}

/// The process's global data share (D789): one zeroed buffer, created with the session and bound
/// to every compute module that declares [`orbistoun_spirv::GDS_BINDING`]. Its memory lives as
/// long as the process.
static GLOBAL_SHARE: std::sync::OnceLock<vk::Buffer> = std::sync::OnceLock::new();

/// The global data share as one compute pipeline binds it: the buffer, where its module declares
/// [`orbistoun_spirv::GDS_BINDING`] (D789).
struct GlobalShare(Option<vk::Buffer>);

impl GlobalShare {
    /// The share for `module`: none where it does not declare the binding, and an error where it
    /// does and the session could not create the share.
    fn for_module(module: &[u32]) -> Result<Self, DispatchError> {
        if !orbistoun_spirv::declares_binding(module, orbistoun_spirv::GDS_BINDING) {
            return Ok(Self(None));
        }
        GLOBAL_SHARE
            .get()
            .copied()
            .map(|buffer| Self(Some(buffer)))
            .ok_or(DispatchError::Vulkan(
                "the global data share",
                vk::Result::ERROR_OUT_OF_DEVICE_MEMORY,
            ))
    }

    /// The set-zero binding it adds to the pipeline's layout.
    fn layout_binding(&self) -> Option<vk::DescriptorSetLayoutBinding<'static>> {
        self.0.map(|_| {
            vk::DescriptorSetLayoutBinding::default()
                .binding(orbistoun_spirv::GDS_BINDING)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(1)
                .stage_flags(vk::ShaderStageFlags::COMPUTE)
        })
    }

    /// Points `set`'s binding at the share.
    fn write(&self, device: &ash::Device, set: vk::DescriptorSet) {
        let Some(share) = self.0 else {
            return;
        };
        let info = [vk::DescriptorBufferInfo::default()
            .buffer(share)
            .offset(0)
            .range(vk::WHOLE_SIZE)];
        let write = [vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(orbistoun_spirv::GDS_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&info)];
        // SAFETY: the set is live, the share lives as long as the process, and the slices outlive
        // the call.
        unsafe { device.update_descriptor_sets(&write, &[]) };
    }
}

/// Builds a compute pipeline bound to one storage buffer at set 0, binding 0, with `buffers` - a
/// dispatch's traced buffers (D746) - as set 1 where it has any.
fn build_pipeline(
    device: &ash::Device,
    module: &[u32],
    buffer: vk::Buffer,
    size: vk::DeviceSize,
    (memory_buffer, memory_offset, memory_size): (vk::Buffer, vk::DeviceSize, vk::DeviceSize),
    (push_bytes, images, buffers): (
        u32,
        &crate::compute_images::BoundImages,
        Option<vk::DescriptorSetLayout>,
    ),
) -> Result<BoundPipeline, DispatchError> {
    let module = for_this_device(module);
    // The global data share, where the module reaches it (D789).
    let global_share = GlobalShare::for_module(&module)?;
    let image_bindings = images.layout_bindings();
    let key = pipeline_key(
        device,
        &module,
        &image_bindings,
        buffers.is_some(),
        push_bytes,
    );
    let found = kept_pipelines()
        .lock()
        .ok()
        .and_then(|kept| kept.get(&key).copied());
    let (kept, held) = if let Some(kept) = found {
        (kept, true)
    } else {
        let made = make_pipeline(
            device,
            &module,
            (&image_bindings, &global_share),
            (buffers.is_some(), push_bytes),
        )?;
        let held = kept_pipelines()
            .lock()
            .is_ok_and(|mut kept| kept.len() < KEPT_PIPELINES && kept.insert(key, made).is_none());
        (made, held)
    };

    let mut pool_sizes = vec![
        vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(2 + u32::from(global_share.0.is_some())),
    ];
    pool_sizes.extend(images.pool_sizes());
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(1);
    // SAFETY: the create info outlives the call.
    let descriptor_pool = unsafe { device.create_descriptor_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_pool", e))?;

    let set_layouts = [kept.set_layout];
    let allocate_sets = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(descriptor_pool)
        .set_layouts(&set_layouts);
    // SAFETY: the pool has room for exactly this one set.
    let sets = unsafe { device.allocate_descriptor_sets(&allocate_sets) }
        .map_err(|e| DispatchError::Vulkan("allocate_descriptor_sets", e))?;

    let observation_info = [vk::DescriptorBufferInfo::default()
        .buffer(buffer)
        .offset(0)
        .range(size)];
    let memory_info = [vk::DescriptorBufferInfo::default()
        .buffer(memory_buffer)
        .offset(memory_offset)
        .range(memory_size)];
    let writes = [
        vk::WriteDescriptorSet::default()
            .dst_set(sets[0])
            .dst_binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&observation_info),
        vk::WriteDescriptorSet::default()
            .dst_set(sets[0])
            .dst_binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&memory_info),
    ];
    // SAFETY: the write refers to a live set and a live buffer, and the slices outlive the call.
    unsafe { device.update_descriptor_sets(&writes, &[]) };
    global_share.write(device, sets[0]);
    images.write(device, sets[0]);

    Ok(BoundPipeline {
        kept,
        held,
        descriptor_pool,
        set: sets[0],
    })
}

/// Compiled compute pipelines kept across dispatches, by [`pipeline_key`]. A frame runs the same
/// few programs every time, and building one took longer than running it.
fn kept_pipelines() -> &'static std::sync::Mutex<std::collections::HashMap<u64, KeptPipeline>> {
    static KEPT: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<u64, KeptPipeline>>,
    > = std::sync::OnceLock::new();
    KEPT.get_or_init(Default::default)
}

/// How many pipelines are kept. Past this one is built for its dispatch and released after it, as
/// every pipeline was before any was kept: a kept one is never released while a dispatch may hold
/// it.
const KEPT_PIPELINES: usize = 512;

/// What a compute pipeline is built from: the device, the module, the images' bindings, whether it
/// binds a buffer set and its push-constant size. The global data share's binding follows from
/// the module.
fn pipeline_key(
    device: &ash::Device,
    module: &[u32],
    images: &[vk::DescriptorSetLayoutBinding<'_>],
    buffers: bool,
    push_bytes: u32,
) -> u64 {
    let mut shape = vec![
        orbistoun_gpu::content_hash(module) as u32,
        (orbistoun_gpu::content_hash(module) >> 32) as u32,
        u32::from(buffers),
        push_bytes,
        vk::Handle::as_raw(device.handle()) as u32,
        (vk::Handle::as_raw(device.handle()) >> 32) as u32,
    ];
    for binding in images {
        shape.extend([binding.binding, binding.descriptor_type.as_raw() as u32]);
    }
    orbistoun_gpu::content_hash(&shape)
}

/// Builds the parts of a compute pipeline that outlive a dispatch.
fn make_pipeline(
    device: &ash::Device,
    module: &[u32],
    (images, global_share): (&[vk::DescriptorSetLayoutBinding<'static>], &GlobalShare),
    (buffers, push_bytes): (bool, u32),
) -> Result<KeptPipeline, DispatchError> {
    let shader_info = vk::ShaderModuleCreateInfo::default().code(module);
    // SAFETY: the module words outlive the call; malformed SPIR-V is reported as an error, which
    // makes this usable as a check.
    let shader = unsafe { device.create_shader_module(&shader_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module", e))?;

    let mut bindings = vec![
        vk::DescriptorSetLayoutBinding::default()
            .binding(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
        vk::DescriptorSetLayoutBinding::default()
            .binding(1)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::COMPUTE),
    ];
    // The images a guest dispatch reads and writes, at the bindings its module names them.
    bindings.extend_from_slice(images);
    bindings.extend(global_share.layout_binding());
    let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    // SAFETY: the create info outlives the call.
    let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?;
    // The dispatch's buffers are set 1, in a layout of its own defined as every dispatch's buffer
    // set is, so any dispatch's set binds to this pipeline.
    let buffers_layout = if buffers {
        let bindings = crate::compute_buffers::layout_bindings();
        let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        // SAFETY: the create info outlives the call.
        Some(
            unsafe { device.create_descriptor_set_layout(&info, None) }
                .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?,
        )
    } else {
        None
    };

    // The pipeline's layout has the dispatch's buffers as set 1; the set allocated for a dispatch
    // is set 0's.
    let pipeline_set_layouts: Vec<vk::DescriptorSetLayout> =
        std::iter::once(set_layout).chain(buffers_layout).collect();
    // A module reading user data declares the push-constant block; one that reads none declares
    // none and gets no range.
    let push_ranges = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
        .offset(0)
        .size(push_bytes)];
    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&pipeline_set_layouts)
        .push_constant_ranges(if push_bytes == 0 { &[] } else { &push_ranges });
    // SAFETY: the create info outlives the call.
    let layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_pipeline_layout", e))?;

    let stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader)
        .name(c"main");
    let pipeline_info = [vk::ComputePipelineCreateInfo::default()
        .stage(stage)
        .layout(layout)];
    // SAFETY: the create info outlives the call and names a shader module still alive.
    let pipelines = unsafe {
        device.create_compute_pipelines(crate::pipeline_cache::cache(), &pipeline_info, None)
    }
    .map_err(|(_, e)| DispatchError::Vulkan("create_compute_pipelines", e))?;

    Ok(KeptPipeline {
        shader,
        set_layout,
        buffers_layout,
        layout,
        pipeline: pipelines[0],
    })
}

impl KeptPipeline {
    /// Releases everything it holds.
    fn release(self, device: &ash::Device) {
        // SAFETY: every handle was created on this device for this pipeline, nothing recorded with
        // it is still running (the dispatch waited for its submission), and each is destroyed once.
        unsafe { device.destroy_pipeline(self.pipeline, None) };
        // SAFETY: as above.
        unsafe { device.destroy_pipeline_layout(self.layout, None) };
        // SAFETY: as above.
        unsafe { device.destroy_descriptor_set_layout(self.set_layout, None) };
        if let Some(buffers) = self.buffers_layout {
            // SAFETY: as above.
            unsafe { device.destroy_descriptor_set_layout(buffers, None) };
        }
        // SAFETY: as above.
        unsafe { device.destroy_shader_module(self.shader, None) };
    }
}

/// What a dispatch left behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The observation buffer at binding zero: registers, for inspection.
    pub observed: Vec<u32>,
    /// Guest memory at binding one.
    pub memory: Vec<u32>,
}

/// Copies a host-visible allocation back into a vector.
///
/// One function so the unsafe reasoning is written once for every buffer.
pub(crate) fn read_back(
    device: &ash::Device,
    buffer: &DispatchBuffer,
) -> Result<Vec<u32>, DispatchError> {
    let (memory, words) = (buffer.memory, buffer.words);
    // SAFETY: the memory is host-visible, holds `size` bytes from `offset` (the buffer's own, or
    // one slot of the window ring), and is not currently mapped.
    let mapped = unsafe {
        device.map_memory(
            memory,
            buffer.offset,
            buffer.size,
            vk::MemoryMapFlags::empty(),
        )
    }
    .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    let mut out = vec![0u32; words];
    // SAFETY: the mapping covers `words` whole `u32`s, the destination has exactly that capacity,
    // and the two cannot overlap: one is device memory, the other a fresh allocation.
    unsafe {
        std::ptr::copy_nonoverlapping(mapped.cast::<u32>(), out.as_mut_ptr(), words);
    }
    // SAFETY: mapped immediately above and not mapped anywhere else.
    unsafe { device.unmap_memory(memory) };
    Ok(out)
}

/// A Vulkan instance and device, created once and reused for every dispatch.
///
/// Creating and destroying an instance and device per dispatch faults intermittently in long runs,
/// for the same reason as [`entry`], and costs over a second each time. Queue submission and
/// command pools need external synchronisation and tests run on several threads, so one lock covers
/// a whole dispatch; the device is the bottleneck, so a finer scheme gains nothing.
pub(crate) struct Session {
    pub(crate) instance: ash::Instance,
    pub(crate) physical: vk::PhysicalDevice,
    pub(crate) device: ash::Device,
    pub(crate) queue: vk::Queue,
    pub(crate) family: u32,
    properties: Properties,
}

/// Whether the session's device clips z to `-w..w` on request (`VK_EXT_depth_clip_control`).
static DEPTH_CLIP_CONTROL: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether a pipeline may clip z to `-w..w`, as a GL guest does: the device enabled
/// `VK_EXT_depth_clip_control`.
pub(crate) fn depth_clip_control() -> bool {
    DEPTH_CLIP_CONTROL.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the session's device was created with `depth_bias_clamp`.
static DEPTH_BIAS_CLAMP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a pipeline may clamp its depth bias: the device enabled `depth_bias_clamp`.
pub(crate) fn depth_bias_clamp() -> bool {
    DEPTH_BIAS_CLAMP.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the session's device was created with `wide_lines`.
static WIDE_LINES: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// The widest line the session's device rasterizes, as `f32` bits: `lineWidthRange`'s upper end.
static MAX_LINE_WIDTH: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Whether a pipeline may rasterize lines wider than one pixel: the device enabled `wide_lines`.
pub(crate) fn wide_lines() -> bool {
    WIDE_LINES.load(std::sync::atomic::Ordering::Relaxed)
}

/// The widest line a pipeline may ask for, when [`wide_lines`] holds.
pub(crate) fn max_line_width() -> f32 {
    f32::from_bits(MAX_LINE_WIDTH.load(std::sync::atomic::Ordering::Relaxed)).max(1.0)
}

/// Whether the session's device was created with `logic_op`.
static LOGIC_OP: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether a pipeline may write through a logic op: the device enabled `logic_op`.
pub(crate) fn logic_op() -> bool {
    LOGIC_OP.load(std::sync::atomic::Ordering::Relaxed)
}

/// Whether the session's device was created with `sampler_anisotropy`.
static SAMPLER_ANISOTROPY: std::sync::atomic::AtomicBool =
    std::sync::atomic::AtomicBool::new(false);

/// Whether a sampler may filter anisotropically: the device enabled `sampler_anisotropy`, whose
/// limit is then at least sixteen, the most a guest sampler asks for (D775).
pub(crate) fn sampler_anisotropy() -> bool {
    SAMPLER_ANISOTROPY.load(std::sync::atomic::Ordering::Relaxed)
}

/// The shared session, created on first use and never destroyed.
///
/// A failure is cached with the stage that failed, so a machine with no device does not repeat the
/// setup on every call, and "no device" stays distinct from "a device that refused us".
pub(crate) fn session() -> Result<&'static std::sync::Mutex<Session>, DispatchError> {
    static SESSION: std::sync::OnceLock<
        Result<std::sync::Mutex<Session>, (&'static str, vk::Result)>,
    > = std::sync::OnceLock::new();
    SESSION
        .get_or_init(|| Session::new().map(std::sync::Mutex::new))
        .as_ref()
        .map_err(|(stage, result)| DispatchError::Vulkan(stage, *result))
}

impl Session {
    /// Opens the instance and device this process will use.
    fn new() -> Result<Self, (&'static str, vk::Result)> {
        let entry =
            entry().map_err(|_| ("Entry::load", vk::Result::ERROR_INITIALIZATION_FAILED))?;

        // Vulkan 1.2 is required: the properties below need 1.1 and the float controls 1.2. The
        // target hardware exceeds both, so there is no fallback.
        let application = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2);
        let instance_info = vk::InstanceCreateInfo::default().application_info(&application);
        // SAFETY: the create info outlives the call and is fully initialised.
        let instance = unsafe { entry.create_instance(&instance_info, None) }
            .map_err(|e| ("create_instance", e))?;

        let (physical, family) = Self::compute_family(&instance)?;
        let (device, wanted_features, mesh_enabled) =
            Self::create_device(&instance, physical, family)?;
        // SAFETY: the family index came from this device's own queue properties.
        let queue = unsafe { device.get_device_queue(family, 0) };
        let properties = Self::properties(&instance, physical, &wanted_features, mesh_enabled);
        crate::pipeline_cache::open(&instance, physical, &device);
        // The global data share, zeroed once and kept for the process (D789).
        let gds_words = orbistoun_spirv::GDS_WORDS as usize;
        if let Ok((buffer, _memory)) = create_host_buffer(
            &instance,
            physical,
            &device,
            (gds_words * 4) as vk::DeviceSize,
            gds_words,
        ) {
            let _ = GLOBAL_SHARE.set(buffer);
        }

        Ok(Self {
            instance,
            physical,
            device,
            queue,
            family,
            properties,
        })
    }

    /// Picks the first physical device and its first queue family that can compute.
    fn compute_family(
        instance: &ash::Instance,
    ) -> Result<(vk::PhysicalDevice, u32), (&'static str, vk::Result)> {
        // SAFETY: the instance is live.
        let physical = unsafe { instance.enumerate_physical_devices() }
            .map_err(|e| ("enumerate_physical_devices", e))?
            .into_iter()
            .next()
            .ok_or((
                "enumerate_physical_devices",
                vk::Result::ERROR_INITIALIZATION_FAILED,
            ))?;

        // SAFETY: the handle came from enumeration on this live instance.
        let families = unsafe { instance.get_physical_device_queue_family_properties(physical) };
        let family = families
            .iter()
            .position(|f| f.queue_flags.contains(vk::QueueFlags::COMPUTE))
            .ok_or(("compute queue", vk::Result::ERROR_FEATURE_NOT_PRESENT))?;
        let family =
            u32::try_from(family).map_err(|_| ("compute queue", vk::Result::ERROR_UNKNOWN))?;
        Ok((physical, family))
    }

    /// Creates the logical device with the features and extensions the emitted modules declare,
    /// returning the core features requested and whether mesh shading was enabled.
    fn create_device(
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
        family: u32,
    ) -> Result<(ash::Device, vk::PhysicalDeviceFeatures, vk::Bool32), (&'static str, vk::Result)>
    {
        let priorities = [1.0_f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        // Only what emitted modules declare, and each only where the device offers it, so a device
        // that lacks one is refused by the code that needs it rather than at creation.
        // `shader_int16` and `shader_float16` serve modules that read a half-format buffer channel;
        // `vertex_pipeline_stores_and_atomics` and `fragment_stores_and_atomics` serve pre-fragment
        // (including mesh) and fragment shaders that write a storage buffer, as guest shaders do.
        //
        // SAFETY: the handle came from enumeration on this live instance.
        let available = unsafe { instance.get_physical_device_features(physical) };
        // `shader_storage_image_write_without_format` lets a module store to an image whose format
        // is `Unknown`, because the guest's format is in a descriptor nothing here decodes (D692).
        let wanted_features = vk::PhysicalDeviceFeatures::default()
            .shader_int16(available.shader_int16 == vk::TRUE)
            .fragment_stores_and_atomics(available.fragment_stores_and_atomics == vk::TRUE)
            .shader_storage_image_write_without_format(
                available.shader_storage_image_write_without_format == vk::TRUE,
            )
            .vertex_pipeline_stores_and_atomics(
                available.vertex_pipeline_stores_and_atomics == vk::TRUE,
            )
            // `texture_compression_bc` lets the report say whether an upload path needs a decoder.
            .texture_compression_bc(available.texture_compression_bc == vk::TRUE)
            // `geometry_shader` serves a pixel shader that reads its render-target layer, which
            // SPIR-V spells as the `Geometry` capability.
            .geometry_shader(available.geometry_shader == vk::TRUE)
            // `sampler_anisotropy` serves a guest sampler that filters anisotropically (D775).
            .sampler_anisotropy(available.sampler_anisotropy == vk::TRUE)
            // `logic_op` serves a guest raster operation other than copy (`CB_COLOR_CONTROL`).
            .logic_op(available.logic_op == vk::TRUE)
            // `wide_lines` serves a guest line wider than one pixel (`PA_SU_LINE_CNTL`).
            .wide_lines(available.wide_lines == vk::TRUE)
            // `depth_bias_clamp` serves a guest polygon offset with a clamp.
            .depth_bias_clamp(available.depth_bias_clamp == vk::TRUE);

        // Float16 is a Vulkan 1.2 feature and lives in its own structure, chained on.
        let mut offered_float16 = vk::PhysicalDeviceVulkan12Features::default();
        let mut chained = vk::PhysicalDeviceFeatures2::default().push_next(&mut offered_float16);
        // SAFETY: the handle came from enumeration on this live instance, and the chained structure
        // outlives the call.
        unsafe { instance.get_physical_device_features2(physical, &mut chained) };
        let mut wanted_float16 = vk::PhysicalDeviceVulkan12Features::default()
            .shader_float16(offered_float16.shader_float16 == vk::TRUE);

        // `VK_EXT_mesh_shader` where the device has it: a guest's primitive shader is a mesh shader
        // (D688), and modules that do not use it are unaffected.
        //
        // SAFETY: the handle came from enumeration on this live instance.
        let extensions = unsafe { instance.enumerate_device_extension_properties(physical) }
            .map_err(|e| ("enumerate_device_extension_properties", e))?;
        let mesh_offered = extensions.iter().any(|extension| {
            extension
                .extension_name_as_c_str()
                .is_ok_and(|name| name == ash::ext::mesh_shader::NAME)
        });
        // `VK_EXT_depth_clip_control` where the device has it: a GL guest clips z to `-w..w`, and a
        // pipeline can clip so only with it.
        let clip_control_offered = extensions.iter().any(|extension| {
            extension
                .extension_name_as_c_str()
                .is_ok_and(|name| name == ash::ext::depth_clip_control::NAME)
        });
        let enabled_extensions: Vec<*const core::ffi::c_char> = [
            (mesh_offered, ash::ext::mesh_shader::NAME.as_ptr()),
            (
                clip_control_offered,
                ash::ext::depth_clip_control::NAME.as_ptr(),
            ),
        ]
        .into_iter()
        .filter_map(|(offered, name)| offered.then_some(name))
        .collect();
        let mut wanted_mesh =
            vk::PhysicalDeviceMeshShaderFeaturesEXT::default().mesh_shader(mesh_offered);
        let mut wanted_clip_control = vk::PhysicalDeviceDepthClipControlFeaturesEXT::default()
            .depth_clip_control(clip_control_offered);

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_features(&wanted_features)
            .enabled_extension_names(&enabled_extensions)
            .push_next(&mut wanted_float16)
            .push_next(&mut wanted_mesh)
            .push_next(&mut wanted_clip_control);
        // SAFETY: the physical device is valid and the create info outlives the call.
        let device = unsafe { instance.create_device(physical, &device_info, None) }
            .map_err(|e| ("create_device", e))?;
        DEPTH_CLIP_CONTROL.store(clip_control_offered, std::sync::atomic::Ordering::Relaxed);
        DEPTH_BIAS_CLAMP.store(
            wanted_features.depth_bias_clamp == vk::TRUE,
            std::sync::atomic::Ordering::Relaxed,
        );
        WIDE_LINES.store(
            wanted_features.wide_lines == vk::TRUE,
            std::sync::atomic::Ordering::Relaxed,
        );
        // SAFETY: the handle came from enumeration on this live instance.
        let limits = unsafe { instance.get_physical_device_properties(physical) }.limits;
        MAX_LINE_WIDTH.store(
            limits.line_width_range[1].to_bits(),
            std::sync::atomic::Ordering::Relaxed,
        );
        LOGIC_OP.store(
            wanted_features.logic_op == vk::TRUE,
            std::sync::atomic::Ordering::Relaxed,
        );
        SAMPLER_ANISOTROPY.store(
            wanted_features.sampler_anisotropy == vk::TRUE,
            std::sync::atomic::Ordering::Relaxed,
        );
        Ok((device, wanted_features, wanted_mesh.mesh_shader))
    }

    /// Reads what the device reports and what was enabled into [`Properties`].
    fn properties(
        instance: &ash::Instance,
        physical: vk::PhysicalDevice,
        wanted_features: &vk::PhysicalDeviceFeatures,
        mesh_enabled: vk::Bool32,
    ) -> Properties {
        let mut float_controls = vk::PhysicalDeviceFloatControlsProperties::default();
        let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
        let mut reported = vk::PhysicalDeviceProperties2::default()
            .push_next(&mut float_controls)
            .push_next(&mut subgroup);
        // SAFETY: the physical device is valid, and both chained structures are locals declared
        // immediately above that outlive the call.
        unsafe { instance.get_physical_device_properties2(physical, &mut reported) };

        Properties {
            device: reported.properties.device_name_as_c_str().map_or_else(
                |_| "unnamed device".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            ),
            subnormals_preserved: float_controls.shader_denorm_preserve_float32 == vk::TRUE,
            inf_nan_preserved: {
                let preserved =
                    float_controls.shader_signed_zero_inf_nan_preserve_float32 == vk::TRUE;
                INF_NAN_PRESERVED.store(preserved, std::sync::atomic::Ordering::Relaxed);
                if !preserved {
                    tracing::warn!(
                        "this device cannot preserve 32-bit infinities and NaNs on request; translated shaders run without the guarantee"
                    );
                }
                preserved
            },
            subgroup_size: subgroup.subgroup_size,
            mesh_subgroup_ops: mesh_enabled == vk::TRUE
                && subgroup
                    .supported_stages
                    .contains(vk::ShaderStageFlags::MESH_EXT)
                && subgroup.supported_operations.contains(
                    vk::SubgroupFeatureFlags::BASIC
                        | vk::SubgroupFeatureFlags::BALLOT
                        | vk::SubgroupFeatureFlags::SHUFFLE,
                ),
            // Derived from the creation request, so the report follows whatever was enabled.
            fragment_stores: wanted_features.fragment_stores_and_atomics == vk::TRUE,
            // Enabled, not merely offered, like every other field.
            mesh_shading: mesh_enabled == vk::TRUE,
            storage_image_write: wanted_features.shader_storage_image_write_without_format
                == vk::TRUE,
            compressed_textures: wanted_features.texture_compression_bc == vk::TRUE,
        }
    }
}

/// A host-visible storage buffer bound in a compute dispatch, with what read-back needs.
///
/// Lets a dispatch bind either a throwaway buffer ([`dispatch`]) or a resident one
/// ([`dispatch_bound`]). Copyable because it is plain handles and sizes; the backend keeps the
/// owning copy.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DispatchBuffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: vk::DeviceSize,
    pub words: usize,
    /// Where the `words` start within the buffer: one slot of the window ring, or zero for a buffer
    /// that holds only the words.
    pub offset: vk::DeviceSize,
}

/// Records and runs one compute dispatch against two caller-owned buffers, and reads both back.
///
/// Releases everything it created except the buffers, which belong to the caller. On an error
/// before read-back it releases nothing, per the module's successful-path-only release.
fn dispatch_core(
    device: &ash::Device,
    queue: vk::Queue,
    family: u32,
    module: &[u32],
    binding0: &DispatchBuffer,
    binding1: &DispatchBuffer,
    (groups, push, images, buffers): (
        [u32; 3],
        &[u32],
        &crate::compute_images::BoundImages,
        Option<&crate::compute_buffers::BoundBuffers>,
    ),
) -> Result<(Vec<u32>, Vec<u32>), DispatchError> {
    use orbistoun_gpu::perf::{Span, SpanGuard, span};
    let push_bytes = u32::try_from(push.len() * 4)
        .map_err(|_| DispatchError::Vulkan("push constants", vk::Result::ERROR_UNKNOWN))?;
    let bound = span(Span::DevicePipeline, || {
        build_pipeline(
            device,
            module,
            binding0.buffer,
            binding0.size,
            (binding1.buffer, binding1.offset, binding1.size),
            (push_bytes, images, buffers.map(|b| b.layout)),
        )
    })?;
    let running = SpanGuard::start(Span::DeviceRun);
    let pipeline = bound.kept.pipeline;
    let pipeline_layout = bound.kept.layout;
    let sets = [bound.set];

    // Record and submit.
    let pool_create = vk::CommandPoolCreateInfo::default().queue_family_index(family);
    // SAFETY: the family index is one this device was created with.
    let command_pool = unsafe { device.create_command_pool(&pool_create, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool", e))?;

    let command_allocate = vk::CommandBufferAllocateInfo::default()
        .command_pool(command_pool)
        .level(vk::CommandBufferLevel::PRIMARY)
        .command_buffer_count(1);
    // SAFETY: the pool is live and was just created.
    let command_buffers = unsafe { device.allocate_command_buffers(&command_allocate) }
        .map_err(|e| DispatchError::Vulkan("allocate_command_buffers", e))?;
    let command = command_buffers[0];

    let begin =
        vk::CommandBufferBeginInfo::default().flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
    // SAFETY: the command buffer is freshly allocated and not recording.
    unsafe { device.begin_command_buffer(command, &begin) }
        .map_err(|e| DispatchError::Vulkan("begin_command_buffer", e))?;
    // The images' texels reach the device before anything runs over them.
    images.record_upload(device, command);
    if let Some(buffers) = buffers {
        buffers.record_upload(device, command);
    }
    // SAFETY: recording is open and the pipeline is live.
    unsafe { device.cmd_bind_pipeline(command, vk::PipelineBindPoint::COMPUTE, pipeline) };
    // SAFETY: recording is open; the layout and set match the pipeline.
    unsafe {
        device.cmd_bind_descriptor_sets(
            command,
            vk::PipelineBindPoint::COMPUTE,
            pipeline_layout,
            0,
            &sets,
            &[],
        );
    }
    if let Some(buffers) = buffers {
        // SAFETY: recording is open; set 1 of the layout is this set's layout.
        unsafe {
            device.cmd_bind_descriptor_sets(
                command,
                vk::PipelineBindPoint::COMPUTE,
                pipeline_layout,
                1,
                &[buffers.set],
                &[],
            );
        }
    }
    if !push.is_empty() {
        // SAFETY: recording is open, and the range is the one the pipeline layout declares.
        unsafe {
            device.cmd_push_constants(
                command,
                pipeline_layout,
                vk::ShaderStageFlags::COMPUTE,
                0,
                zerocopy::IntoBytes::as_bytes(push),
            );
        }
    }
    // SAFETY: recording is open and a pipeline is bound.
    unsafe { device.cmd_dispatch(command, groups[0], groups[1], groups[2]) };
    images.record_readback(device, command);
    if let Some(buffers) = buffers {
        buffers.record_readback(device, command);
    }
    // SAFETY: recording is open.
    unsafe { device.end_command_buffer(command) }
        .map_err(|e| DispatchError::Vulkan("end_command_buffer", e))?;

    let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
    // A fence for this submission alone: a device wait also waited for whatever else the device
    // was doing, such as a frame being shown.
    //
    // SAFETY: the device is live and the create info outlives the call.
    let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
        .map_err(|e| DispatchError::Vulkan("create_fence", e))?;
    // SAFETY: the command buffer has finished recording, the queue belongs to this device, and the
    // fence is unsignalled and used by nothing else.
    let waited = unsafe { device.queue_submit(queue, &submits, fence) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))
        .and_then(|()| {
            // SAFETY: the fence was submitted with the work just above.
            unsafe { device.wait_for_fences(&[fence], true, u64::MAX) }
                .map_err(|e| DispatchError::Vulkan("wait_for_fences", e))
        });
    // SAFETY: the submission it fenced has completed, or was never made.
    unsafe { device.destroy_fence(fence, None) };
    waited?;
    drop(running);
    let _reading = SpanGuard::start(Span::DeviceReadBack);

    // Read back.
    let observed = read_back(device, binding0)?;
    let guest_memory = read_back(device, binding1)?;

    // Release what was this dispatch's alone: the buffers are the caller's, and a kept pipeline
    // serves the next dispatch of its program.
    //
    // SAFETY: every handle below was created here on this device, is not in use after the fence
    // wait, and is destroyed exactly once.
    unsafe { device.destroy_command_pool(command_pool, None) };
    // SAFETY: as above.
    unsafe { device.destroy_descriptor_pool(bound.descriptor_pool, None) };
    if !bound.held {
        bound.kept.release(device);
    }

    Ok((observed, guest_memory))
}

/// Runs a compute shader over two throwaway storage buffers and returns what it left in them.
///
/// Binding zero is the observation window a translated shader reports registers through; binding
/// one is guest memory. Both are created zeroed and destroyed after. They are separate bindings so
/// an out-of-range guest store cannot overwrite the observed registers. `dispatch_bound` binds
/// resident buffers instead.
pub fn dispatch(
    module: &[u32],
    words: usize,
    memory_words: usize,
    groups: [u32; 3],
) -> Result<Output, DispatchError> {
    // A poisoned lock means an earlier dispatch panicked. Every handle a dispatch creates is
    // released before it returns, so the session is still sound and later dispatches proceed.
    let session = session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Session {
        ref instance,
        physical,
        ref device,
        queue,
        family,
        ..
    } = *session;

    let size = (words * 4) as vk::DeviceSize;
    let memory_size = (memory_words * 4) as vk::DeviceSize;

    let (buffer, memory) = create_host_buffer(instance, physical, device, size, words)?;
    let (memory_buffer, memory_memory) =
        create_host_buffer(instance, physical, device, memory_size, memory_words)?;
    let binding0 = DispatchBuffer {
        buffer,
        memory,
        size,
        words,
        offset: 0,
    };
    let binding1 = DispatchBuffer {
        buffer: memory_buffer,
        memory: memory_memory,
        size: memory_size,
        words: memory_words,
        offset: 0,
    };

    // On an error the buffers leak with everything else, per the successful-path-only release.
    let (observed, guest_memory) = dispatch_core(
        device,
        queue,
        family,
        module,
        &binding0,
        &binding1,
        (
            groups,
            &[],
            &crate::compute_images::BoundImages::none(),
            None,
        ),
    )?;

    // SAFETY: both buffers were created here, are no longer in use, and are destroyed once.
    unsafe { device.destroy_buffer(buffer, None) };
    // SAFETY: as above.
    unsafe { device.destroy_buffer(memory_buffer, None) };
    // SAFETY: the memory is unmapped and nothing is bound to it any longer.
    unsafe { device.free_memory(memory, None) };
    // SAFETY: as above.
    unsafe { device.free_memory(memory_memory, None) };

    // The device and instance belong to the shared session and are not destroyed.

    Ok(Output {
        observed,
        memory: guest_memory,
    })
}

/// Runs a compute dispatch binding two caller-owned buffers, an observation at binding 0 and the
/// guest-memory window at binding 1, and reads both back, destroying neither.
///
/// This is how a guest dispatch is observed: a guest compute shader's result is in guest memory at
/// binding 1. Both buffers are resident and reused across dispatches.
pub(crate) fn dispatch_bound(
    observation: &DispatchBuffer,
    window: &DispatchBuffer,
    module: &[u32],
    groups: [u32; 3],
) -> Result<(Vec<u32>, Vec<u32>), DispatchError> {
    let session = session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Session {
        ref device,
        queue,
        family,
        ..
    } = *session;
    dispatch_core(
        device,
        queue,
        family,
        module,
        observation,
        window,
        (
            groups,
            &[],
            &crate::compute_images::BoundImages::none(),
            None,
        ),
    )
}

/// Runs a compute dispatch over a caller-owned guest-memory window at binding 1, with a throwaway
/// observation of `observation_words` at binding 0, and reads both back.
///
/// The observation exists because the module's binding 0 needs a buffer; the window is left for the
/// caller.
pub(crate) fn dispatch_reading_window(
    window: &DispatchBuffer,
    observation_words: usize,
    module: &[u32],
    groups: [u32; 3],
) -> Result<(Vec<u32>, Vec<u32>), DispatchError> {
    let session = session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Session {
        ref instance,
        physical,
        ref device,
        queue,
        family,
        ..
    } = *session;

    let size = (observation_words * 4) as vk::DeviceSize;
    let (scratch, scratch_memory) =
        create_host_buffer(instance, physical, device, size, observation_words)?;
    let observation = DispatchBuffer {
        buffer: scratch,
        memory: scratch_memory,
        size,
        words: observation_words,
        offset: 0,
    };

    let result = dispatch_core(
        device,
        queue,
        family,
        module,
        &observation,
        window,
        (
            groups,
            &[],
            &crate::compute_images::BoundImages::none(),
            None,
        ),
    );

    // The throwaway observation is destroyed whether or not the dispatch succeeded.
    //
    // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
    unsafe { device.destroy_buffer(scratch, None) };
    // SAFETY: as above, and nothing is bound to the memory now.
    unsafe { device.free_memory(scratch_memory, None) };

    result
}

/// What a guest dispatch left: guest memory as the window holds it after, and whether any lane
/// made an access the module could not place exactly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuestDispatch {
    /// The window's words after the dispatch.
    pub memory: Vec<u32>,
    /// Whether an access left the window or went through a descriptor whose addressing is not
    /// modelled; the result is then not the guest's and must not reach its memory.
    pub escaped: bool,
    /// The stored image's texels after the dispatch, when it bound one.
    pub stored: Option<Vec<u32>>,
    /// Each traced buffer's words after the dispatch and its write mask, by slot (D746, D809);
    /// empty when it bound none.
    pub buffers: Vec<crate::compute_buffers::BufferOut>,
}

/// Words of the observation a guest dispatch module declares: the register-observation layout,
/// whose word zero it uses for its escape flag.
const GUEST_OBSERVATION_WORDS: usize = 16;

/// Runs a guest compute dispatch's module: `memory` is the window's contents before, uploaded to
/// binding 1; `push` is the user-data block; the module runs `groups` workgroups, one wave each.
///
/// # Errors
///
/// No device, or a device that refused the module or a step of the dispatch.
pub fn dispatch_guest(
    module: &[u32],
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    images: &crate::compute_images::DispatchImages<'_>,
) -> Result<GuestDispatch, DispatchError> {
    dispatch_guest_with_buffers(module, memory, (push, groups), images, &[])
}

/// [`dispatch_guest`], with the dispatch's traced buffers (D746): `buffers` holds each slot's words
/// before, bound writable at the compute binding of the draw-buffer set, and the result holds them
/// after.
///
/// # Errors
///
/// No device, more buffers than a module declares, or a device that refused a step.
pub fn dispatch_guest_with_buffers(
    module: &[u32],
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    images: &crate::compute_images::DispatchImages<'_>,
    buffers: &[crate::compute_buffers::BufferIn<'_>],
) -> Result<GuestDispatch, DispatchError> {
    use orbistoun_gpu::perf::{Span, SpanGuard};
    let setting_up = SpanGuard::start(Span::DeviceSetup);
    // Draws already queued finish first: the window was read from guest memory they write.
    crate::framebuffer::settle_session()?;
    let session = session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Session {
        ref instance,
        physical,
        ref device,
        queue,
        family,
        ..
    } = *session;

    let observation_size = (GUEST_OBSERVATION_WORDS * 4) as vk::DeviceSize;
    let memory_size = (memory.len().max(1) * 4) as vk::DeviceSize;
    let (observation_buffer, observation_memory) = create_host_buffer(
        instance,
        physical,
        device,
        observation_size,
        GUEST_OBSERVATION_WORDS,
    )?;
    let (memory_buffer, memory_memory) =
        create_host_buffer(instance, physical, device, memory_size, memory.len())?;
    // SAFETY: the memory is host-visible, holds `memory_size` bytes and is not mapped.
    let mapped =
        unsafe { device.map_memory(memory_memory, 0, memory_size, vk::MemoryMapFlags::empty()) }
            .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    // SAFETY: the mapping covers `memory.len()` whole words, and the two ranges cannot overlap:
    // one is device memory, the other the caller's slice.
    unsafe {
        std::ptr::copy_nonoverlapping(memory.as_ptr(), mapped.cast::<u32>(), memory.len());
    }
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(memory_memory) };

    let observation = DispatchBuffer {
        buffer: observation_buffer,
        memory: observation_memory,
        size: observation_size,
        words: GUEST_OBSERVATION_WORDS,
        offset: 0,
    };
    let window = DispatchBuffer {
        buffer: memory_buffer,
        memory: memory_memory,
        size: memory_size,
        words: memory.len(),
        offset: 0,
    };
    let bound = crate::compute_images::BoundImages::create((instance, physical, device), images)?;
    let traced =
        crate::compute_buffers::BoundBuffers::create((instance, physical, device), buffers)?;
    drop(setting_up);
    let dispatched = dispatch_core(
        device,
        queue,
        family,
        module,
        &observation,
        &window,
        (groups, push, &bound, traced.as_ref()),
    );
    let stored = orbistoun_gpu::perf::span(Span::ReadBackImage, || {
        dispatched
            .as_ref()
            .ok()
            .map(|_| bound.read_stored(device))
            .transpose()
    });
    let buffers_after =
        orbistoun_gpu::perf::span(Span::ReadBackBuffers, || match (&dispatched, &traced) {
            (Ok(_), Some(traced)) => traced.read(device, (queue, family)),
            _ => Ok(Vec::new()),
        });
    let _releasing = SpanGuard::start(Span::DeviceRelease);
    bound.destroy(device);
    if let Some(traced) = traced {
        traced.destroy(device);
    }
    let (observed, after) = dispatched?;
    let stored = stored?.flatten();

    // SAFETY: both buffers were created here, are no longer in use, and are destroyed once.
    unsafe { device.destroy_buffer(observation_buffer, None) };
    // SAFETY: as above.
    unsafe { device.destroy_buffer(memory_buffer, None) };
    // SAFETY: the memory is unmapped and nothing is bound to it any longer.
    unsafe { device.free_memory(observation_memory, None) };
    // SAFETY: as above.
    unsafe { device.free_memory(memory_memory, None) };

    Ok(GuestDispatch {
        memory: after,
        escaped: observed.first().is_some_and(|&flag| flag != 0),
        stored,
        buffers: buffers_after?,
    })
}

#[cfg(test)]
mod tests {
    use super::{Availability, probe};

    /// A probe yields either device properties or a reason there is no device.
    #[test]
    fn probing_reports_a_reason_when_there_is_no_device() {
        // Either answer must be usable: a device carries its properties, an absence carries a
        // reason a skip message can report.
        match probe() {
            Availability::Available { properties } => {
                assert!(
                    !properties.device.is_empty(),
                    "an available device must name itself"
                );
                // A subgroup is at least one invocation wide, so zero means the property was never
                // filled in.
                assert!(
                    properties.subgroup_size >= 1,
                    "a device reporting a subgroup size of zero has not been asked"
                );
            }
            Availability::Unavailable { reason } => {
                assert!(!reason.is_empty(), "an absence must say why");
            }
        }
    }
}
