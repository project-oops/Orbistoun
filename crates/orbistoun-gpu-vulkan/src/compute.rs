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
    /// How many invocations share a subgroup on this device.
    ///
    /// Reported rather than assumed: the guest's wavefront is 32 or 64 lanes, and the ratio to the
    /// host subgroup is what a lane-mapped translation is built around.
    pub subgroup_size: u32,
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
    let memory_type = (0..memory_properties.memory_type_count)
        .find(|i| {
            let usable = requirements.memory_type_bits & (1 << i) != 0;
            let suitable = memory_properties.memory_types[*i as usize]
                .property_flags
                .contains(wanted);
            usable && suitable
        })
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

/// A compute pipeline and the descriptor objects that feed it, created, used and released together.
struct BoundPipeline {
    shader: vk::ShaderModule,
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
    descriptor_pool: vk::DescriptorPool,
    set: vk::DescriptorSet,
}

/// Builds a compute pipeline bound to one storage buffer at set 0, binding 0.
fn build_pipeline(
    device: &ash::Device,
    module: &[u32],
    buffer: vk::Buffer,
    size: vk::DeviceSize,
    (memory_buffer, memory_offset, memory_size): (vk::Buffer, vk::DeviceSize, vk::DeviceSize),
) -> Result<BoundPipeline, DispatchError> {
    let shader_info = vk::ShaderModuleCreateInfo::default().code(module);
    // SAFETY: the module words outlive the call; malformed SPIR-V is reported as an error, which
    // makes this usable as a check.
    let shader = unsafe { device.create_shader_module(&shader_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module", e))?;

    let bindings = [
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
    let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    // SAFETY: the create info outlives the call.
    let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?;

    let set_layouts = [set_layout];
    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default().set_layouts(&set_layouts);
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
    let pipelines =
        unsafe { device.create_compute_pipelines(vk::PipelineCache::null(), &pipeline_info, None) }
            .map_err(|(_, e)| DispatchError::Vulkan("create_compute_pipelines", e))?;

    let pool_sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(2)];
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

    Ok(BoundPipeline {
        shader,
        set_layout,
        layout,
        pipeline: pipelines[0],
        descriptor_pool,
        set: sets[0],
    })
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
            .texture_compression_bc(available.texture_compression_bc == vk::TRUE);

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
        let mesh_names = [ash::ext::mesh_shader::NAME.as_ptr()];
        let enabled_extensions: &[*const core::ffi::c_char] =
            if mesh_offered { &mesh_names } else { &[] };
        let mut wanted_mesh =
            vk::PhysicalDeviceMeshShaderFeaturesEXT::default().mesh_shader(mesh_offered);

        let device_info = vk::DeviceCreateInfo::default()
            .queue_create_infos(&queue_info)
            .enabled_features(&wanted_features)
            .enabled_extension_names(enabled_extensions)
            .push_next(&mut wanted_float16)
            .push_next(&mut wanted_mesh);
        // SAFETY: the physical device is valid and the create info outlives the call.
        let device = unsafe { instance.create_device(physical, &device_info, None) }
            .map_err(|e| ("create_device", e))?;
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
            subgroup_size: subgroup.subgroup_size,
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
    groups: [u32; 3],
) -> Result<(Vec<u32>, Vec<u32>), DispatchError> {
    let bound = build_pipeline(
        device,
        module,
        binding0.buffer,
        binding0.size,
        (binding1.buffer, binding1.offset, binding1.size),
    )?;
    let pipeline = bound.pipeline;
    let pipeline_layout = bound.layout;
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
    // SAFETY: recording is open and a pipeline is bound.
    unsafe { device.cmd_dispatch(command, groups[0], groups[1], groups[2]) };
    // SAFETY: recording is open.
    unsafe { device.end_command_buffer(command) }
        .map_err(|e| DispatchError::Vulkan("end_command_buffer", e))?;

    let submits = [vk::SubmitInfo::default().command_buffers(&command_buffers)];
    // SAFETY: the command buffer has finished recording and the queue belongs to this device.
    unsafe { device.queue_submit(queue, &submits, vk::Fence::null()) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))?;
    // A device wait rather than a fence: one submission, and a fence would add an object to release
    // for no extra guarantee.
    //
    // SAFETY: the device is live and nothing else is using it.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;

    // Read back.
    let observed = read_back(device, binding0)?;
    let guest_memory = read_back(device, binding1)?;

    // Release everything but the buffers.
    //
    // SAFETY: every handle below was created here on this device, is not in use after the queue
    // wait, and is destroyed exactly once.
    unsafe { device.destroy_command_pool(command_pool, None) };
    // SAFETY: as above.
    unsafe { device.destroy_descriptor_pool(bound.descriptor_pool, None) };
    // SAFETY: as above.
    unsafe { device.destroy_pipeline(bound.pipeline, None) };
    // SAFETY: as above.
    unsafe { device.destroy_pipeline_layout(bound.layout, None) };
    // SAFETY: as above.
    unsafe { device.destroy_descriptor_set_layout(bound.set_layout, None) };
    // SAFETY: as above.
    unsafe { device.destroy_shader_module(bound.shader, None) };

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
    let (observed, guest_memory) =
        dispatch_core(device, queue, family, module, &binding0, &binding1, groups)?;

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
    dispatch_core(device, queue, family, module, observation, window, groups)
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

    let result = dispatch_core(device, queue, family, module, &observation, window, groups);

    // The throwaway observation is destroyed whether or not the dispatch succeeded.
    //
    // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
    unsafe { device.destroy_buffer(scratch, None) };
    // SAFETY: as above, and nothing is bound to the memory now.
    unsafe { device.free_memory(scratch_memory, None) };

    result
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
