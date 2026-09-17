//! Running a compute shader and reading back what it wrote.
//!
//! # Why this exists
//!
//! `spirv-val` answers whether a module is well-formed. It cannot answer whether the
//! module computes the right thing, and a translator that emits valid SPIR-V which
//! renders the wrong output is the failure this project spends most of its effort
//! avoiding.
//!
//! This closes that gap. Dispatch a translated shader with known inputs, read the
//! buffer back, compare against what the source it came from was supposed to do.
//!
//! # A missing device is not a failure, and not a pass either
//!
//! [`probe`] reports whether a device exists, separately from anything going wrong.
//! That distinction is the whole design: a test that finds no device, returns early and
//! reports success would make the suite green on a machine where the most important
//! test never ran. Callers are expected to surface a skip *loudly* - the same rule
//! obSCEne's harness follows, for the same reason.
//!
//! # Software rendering is the better oracle here
//!
//! A software implementation is deterministic. Real drivers differ in floating-point
//! behaviour, denormal handling and optimisation, so a regression test that passes on
//! one machine and fails on another says nothing useful.
//!
//! The trade is worth naming: this verifies *the translator*, not compatibility with
//! any particular hardware. A green suite here is not hardware validation and must
//! never be read as it.
//!
//! # Test infrastructure, and it shows
//!
//! Resources are released on the successful path. An error abandons them, because the
//! alternative is a guard type per Vulkan object for code whose process exits moments
//! later. Stated rather than hidden: if this ever runs inside something long-lived,
//! that has to change first.

use ash::vk;

/// The Vulkan loader, loaded once for the life of the process.
///
/// # Why this is a static and not a local
///
/// [`ash::Entry`] owns the handle to the loader library. Dropping it unloads that
/// library **and every layer the loader pulled in** - overlays, capture tools, driver
/// shims. Creating one per call therefore made each dispatch a load/unload cycle of a
/// dozen DLLs, from whichever thread the test harness happened to be running on.
///
/// That is not merely wasteful, it faults. Layers register process-wide state and
/// thread-local storage that does not survive being unloaded underneath another thread
/// still inside it, and the symptom is an access violation partway through a long run
/// with no relation to what was being dispatched - intermittent, and it moved when
/// anything about the timing changed, which is what a threading fault looks like when
/// mistaken for a data bug.
///
/// The loader is documented as a once-per-process thing. So it is one.
///
/// Never unloaded. There is nowhere to do it from, and a process that has finished with
/// Vulkan is a process that is exiting.
fn entry() -> Result<&'static ash::Entry, DispatchError> {
    static ENTRY: std::sync::OnceLock<Option<ash::Entry>> = std::sync::OnceLock::new();
    // SAFETY: `Entry::load` requires that the loader is not concurrently unloaded, which
    // holds because nothing ever unloads it - the `OnceLock` both serialises the load and
    // keeps the result alive for the rest of the process.
    ENTRY
        .get_or_init(|| unsafe { ash::Entry::load() }.ok())
        .as_ref()
        .ok_or(DispatchError::Vulkan(
            "Entry::load",
            vk::Result::ERROR_INITIALIZATION_FAILED,
        ))
}

/// What a device says about itself, beyond existing.
///
/// # Why this is more than a name
///
/// It used to be a name and nothing else, which answers "can anything run here" and no
/// other question. Two separate pieces of work then wanted the same missing thing -
/// whether subnormals survive, and how wide a subgroup is - and whichever came second
/// would have retrofitted whatever the first invented. So it is asked once.
///
/// Everything here is a *property of the device*, reported verbatim. Nothing here is a
/// judgement about whether it is good enough; that belongs to whoever is asking.
#[derive(Debug, Clone, PartialEq, Eq)]
// Four of these are yes-or-no facts about one device, and each is asked about on its own by
// code that needs it. Grouping them into a sub-structure to satisfy a count would put a name
// between a caller and the one bit it wants, and the usual reason this lint fires - a boolean
// parameter list nobody can read at a call site - does not apply to a report nobody passes.
#[expect(
    clippy::struct_excessive_bools,
    reason = "a device report is a list of independent facts, not an argument list"
)]
pub struct Properties {
    /// What the driver calls itself, for the record.
    pub device: String,
    /// Whether 32-bit subnormal results survive rather than being flushed to zero.
    ///
    /// A module may *require* this, through `SPV_KHR_float_controls`, and a device that
    /// does not offer it cannot run one that does. The guest's division pre-scale
    /// depends on the difference: two of its branches ask whether a quotient is
    /// subnormal, and on a flushing device both answer false, which silently disables
    /// the scaling the instruction exists to perform.
    pub subnormals_preserved: bool,
    /// How many invocations share a subgroup on this device.
    ///
    /// Reported rather than assumed. The guest's wavefront is 32 or 64 lanes depending
    /// on how a shader was compiled, the host's subgroup is whatever the hardware says,
    /// and the ratio between them is what any lane-mapped translation is built around.
    pub subgroup_size: u32,
    /// Whether a **fragment** shader on this device may write a storage buffer.
    ///
    /// Reported as what was *enabled*, not as what the hardware could do. A Vulkan
    /// feature the device was not created with is unusable however capable the silicon
    /// is, so the physical device's answer would be the wrong one to hand a caller
    /// deciding whether a translated fragment module can store (D552).
    ///
    /// It decides the shape of the fragment path rather than merely permitting it. A
    /// translated module keeps its registers in `Private` storage already, so the only
    /// storage-buffer writes are the observation window the epilogue fills and a guest
    /// memory store - and a fragment variant that omits the first and refuses the second
    /// needs this feature not at all.
    pub fragment_stores: bool,
    /// Whether a mesh stage may be used.
    ///
    /// The stage a guest's NGG primitive shader translates to (D688). A device without it can
    /// run a frame's pixel shaders and not its geometry, which is worth saying in a report
    /// rather than discovering at pipeline creation.
    pub mesh_shading: bool,
    /// Whether a shader may write a storage image without declaring its format.
    ///
    /// What a translated `image_store` needs. The alternative is to name a format in the
    /// module, and the guest's format is in a descriptor nothing here decodes - so a device
    /// without this cannot run a shader that stores to an image, and saying so is better than
    /// picking a format and rendering something subtly wrong (D692).
    pub storage_image_write: bool,
    /// Whether this device can sample block-compressed images without them being decoded.
    ///
    /// **The half of surface layout that is not blocked on a capture** (G15). A guest's textures
    /// are block-compressed and tiled; the tiling swizzle is hardware nothing here has measured,
    /// so detiling cannot be written yet - but the compression is a different question, because
    /// Vulkan consumes BC data natively. If a device offers this, the eventual upload path
    /// undoes the tiling and hands the blocks over untouched, and a decoder is a fallback for
    /// devices without it rather than a requirement.
    ///
    /// Asked now, before there is anything to upload, because it decides whether a decoder is on
    /// the critical path - and that is a one-line question whose answer changes a plan.
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
    /// No device. **Not an error** - a machine may legitimately have none.
    ///
    /// Carries why, because "no Vulkan" and "a Vulkan that refused us" want different
    /// responses and look identical from the outside.
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
/// Distinct from [`Availability`]: this means a device existed and something went
/// wrong, which is a real failure rather than an absent environment.
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
}

/// Reports whether compute can be dispatched here.
pub fn probe() -> Availability {
    // Asks the shared session rather than building an instance of its own. It used to
    // build one, use it once and destroy it - the pattern that faulted the process from
    // `dispatch` (D142) - and a probe that opens a device is also the most direct way to
    // find out whether opening one works.
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

/// Creates a storage buffer in memory the host can read, and zeroes it.
///
/// Zeroing matters: without it, a value read back afterwards might be whatever
/// previously occupied that memory rather than something the shader wrote, and a
/// shader that does nothing would be indistinguishable from one that works.
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

    // Host-visible and coherent, so the result can be read without an explicit flush.
    // A device-local buffer would need a staging copy, which is more machinery for a
    // harness whose buffers are a few words.
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
    // SAFETY: buffer and memory both come from this device, and the memory is large
    // enough by construction of the allocation above.
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_buffer_memory", e))?;

    // SAFETY: the memory is host-visible, was just allocated, and is not mapped.
    let mapped = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    // SAFETY: the mapping covers `size` bytes, which is `words` whole `u32`s, and this
    // process has exclusive access to it while mapped.
    unsafe { std::ptr::write_bytes(mapped.cast::<u8>(), 0, words * 4) };
    // SAFETY: mapped immediately above and not used after unmapping.
    unsafe { device.unmap_memory(memory) };

    Ok((buffer, memory))
}

/// A compute pipeline and the descriptor plumbing that feeds it.
///
/// Grouped because they are created together, used together and released together;
/// passing six handles between functions instead would be six chances to release one
/// twice or not at all.
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
    memory_buffer: vk::Buffer,
    memory_size: vk::DeviceSize,
) -> Result<BoundPipeline, DispatchError> {
    let shader_info = vk::ShaderModuleCreateInfo::default().code(module);
    // SAFETY: the module words outlive the call; malformed SPIR-V is reported as an
    // error rather than accepted, which is what makes this usable as a check.
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
        .offset(0)
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
    // SAFETY: the write refers to a live set and a live buffer, and the slices outlive
    // the call.
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
/// Extracted because it is done once per buffer and the unsafe reasoning is identical
/// each time - repeating it invites the two copies to drift, and a `// SAFETY:` comment
/// that no longer describes its block is worse than none.
fn read_back(
    device: &ash::Device,
    memory: vk::DeviceMemory,
    size: vk::DeviceSize,
    words: usize,
) -> Result<Vec<u32>, DispatchError> {
    // SAFETY: the memory is host-visible, was allocated with exactly `size` bytes, and
    // is not currently mapped.
    let mapped = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
        .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
    let mut out = vec![0u32; words];
    // SAFETY: the mapping covers `words` whole `u32`s, the destination has exactly that
    // capacity, and the two cannot overlap - one is device memory and the other a fresh
    // allocation.
    unsafe {
        std::ptr::copy_nonoverlapping(mapped.cast::<u32>(), out.as_mut_ptr(), words);
    }
    // SAFETY: mapped immediately above and not mapped anywhere else.
    unsafe { device.unmap_memory(memory) };
    Ok(out)
}

/// A Vulkan instance and device, created once and reused for every dispatch.
///
/// # Why this is a static and not a local
///
/// The same reason as [`entry`], one layer up, and found the same way. `dispatch` used
/// to build an instance and a device, use them once, and tear them down - so a run that
/// dispatched ninety times built and destroyed ninety of each. That faulted the process
/// intermittently, about three runs in five, always deep into a long run and never in a
/// way that pointed at the shader being dispatched.
///
/// It is also where nearly all of the time went: an instance and a device cost over a
/// second to create, and dispatching a twenty-instruction shader does not.
///
/// A real emulator dispatches thousands of times a frame against one device. This is
/// what that looks like, and the harness should not have been shaped any other way.
///
/// # Locking
///
/// Queue submission and command pools need external synchronisation, and the harness
/// runs tests on several threads. One lock around the whole dispatch is coarse and
/// correct; the device is the bottleneck anyway, so a finer scheme would buy nothing.
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
/// Never destroyed for the same reason the loader is not: there is nowhere to do it
/// from, and a process that has finished with Vulkan is one that is exiting. A failure
/// is cached alongside, so a machine with no device does not repeat the whole setup for
/// every call - and the stage that failed is kept, because "no device" and "a device
/// that refused us" want different responses.
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

        // Vulkan 1.2, because the properties below are only reportable from 1.1 onward
        // and the float controls from 1.2. Stated as a requirement rather than probed
        // for and worked around: this is a harness for a target whose own hardware is
        // newer than either, and a fallback would be untested code guarding against a
        // machine nobody is using.
        let application = vk::ApplicationInfo::default().api_version(vk::API_VERSION_1_2);
        let instance_info = vk::InstanceCreateInfo::default().application_info(&application);
        // SAFETY: the create info outlives the call and is fully initialised.
        let instance = unsafe { entry.create_instance(&instance_info, None) }
            .map_err(|e| ("create_instance", e))?;

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

        let priorities = [1.0_f32];
        let queue_info = [vk::DeviceQueueCreateInfo::default()
            .queue_family_index(family)
            .queue_priorities(&priorities)];
        // **Only what the emitted modules actually declare**, and each of these was found by
        // a validator rather than chosen.
        //
        // Nothing was requested here until a module translated from a shader a console ran
        // was offered to a driver under the Khronos validation layer, which named three
        // things the modules had been relying on and the device had never been asked for
        // (worklog 554):
        //
        //   `shader_int16` and `shader_float16` - a module that reads a half-format buffer
        //     channel declares the Int16 and Float16 capabilities. Every module used to
        //     declare them, used or not, which is how this went unnoticed through every
        //     compute dispatch the suite has ever run; the models now declare the types on
        //     first use (worklog 556), so these two are needed by the few modules that read
        //     halves and by nothing else.
        //   `vertex_pipeline_stores_and_atomics` - the same permission for every stage before
        //     the fragment one, which includes the mesh stage a guest's primitive shader
        //     becomes. The GL cube's vertex program writes a canary word, so it needs this for
        //     the same reason its pixel shader needs the next one.
        //   `fragment_stores_and_atomics` - a fragment shader that writes a storage buffer
        //     needs it, and the guest's pixel shaders do exactly that: the GL cube's writes a
        //     canary word every frame. D552 recorded that the fragment path was *designed*
        //     not to need this, which was true of the hand-written modules and is not true of
        //     the console's.
        //
        // Each is requested only where the device offers it, so a device that cannot is
        // refused by the code that needs it rather than at creation.
        // SAFETY: the handle came from enumeration on this live instance.
        let available = unsafe { instance.get_physical_device_features(physical) };
        //   `shader_storage_image_write_without_format` - a module that writes a storage image
        //     whose format it does not declare. A guest's `image_store` names a format in a
        //     descriptor this project does not decode, so declaring one in the module would be
        //     inventing it; saying `Unknown` instead needs this (D692, worklog 575).
        let wanted_features = vk::PhysicalDeviceFeatures::default()
            .shader_int16(available.shader_int16 == vk::TRUE)
            .fragment_stores_and_atomics(available.fragment_stores_and_atomics == vk::TRUE)
            .shader_storage_image_write_without_format(
                available.shader_storage_image_write_without_format == vk::TRUE,
            )
            .vertex_pipeline_stores_and_atomics(
                available.vertex_pipeline_stores_and_atomics == vk::TRUE,
            )
            //   `texture_compression_bc` - sampling block-compressed images directly. Requested
            //     so the report can say whether an upload path would need a decoder (G15).
            .texture_compression_bc(available.texture_compression_bc == vk::TRUE);

        // Float16 is a Vulkan 1.2 feature and lives in its own structure, chained on.
        let mut offered_float16 = vk::PhysicalDeviceVulkan12Features::default();
        let mut chained = vk::PhysicalDeviceFeatures2::default().push_next(&mut offered_float16);
        // SAFETY: the handle came from enumeration on this live instance, and the chained
        // structure outlives the call.
        unsafe { instance.get_physical_device_features2(physical, &mut chained) };
        let mut wanted_float16 = vk::PhysicalDeviceVulkan12Features::default()
            .shader_float16(offered_float16.shader_float16 == vk::TRUE);

        // `VK_EXT_mesh_shader`, where the device has it. A guest's NGG primitive shader is a
        // mesh shader (D688), so without this the vertex half of a console frame has no stage
        // to run in at all - and with it, the modules that do not use it are unaffected.
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
        // SAFETY: the family index came from this device own queue properties.
        let queue = unsafe { device.get_device_queue(family, 0) };

        let mut float_controls = vk::PhysicalDeviceFloatControlsProperties::default();
        let mut subgroup = vk::PhysicalDeviceSubgroupProperties::default();
        let mut reported = vk::PhysicalDeviceProperties2::default()
            .push_next(&mut float_controls)
            .push_next(&mut subgroup);
        // SAFETY: the physical device is valid, and both chained structures outlive the
        // call - they are locals declared immediately above it.
        unsafe { instance.get_physical_device_properties2(physical, &mut reported) };

        let properties = Properties {
            device: reported.properties.device_name_as_c_str().map_or_else(
                |_| "unnamed device".to_owned(),
                |s| s.to_string_lossy().into_owned(),
            ),
            subnormals_preserved: float_controls.shader_denorm_preserve_float32 == vk::TRUE,
            subgroup_size: subgroup.subgroup_size,
            // What was asked for at device creation, which is nothing - see the field's
            // note. Written as a comparison against the request rather than as `false` so
            // that enabling it later updates this by construction.
            fragment_stores: wanted_features.fragment_stores_and_atomics == vk::TRUE,
            // Enabled, not merely offered - the same rule every other row here follows.
            mesh_shading: wanted_mesh.mesh_shader == vk::TRUE,
            storage_image_write: wanted_features.shader_storage_image_write_without_format
                == vk::TRUE,
            compressed_textures: wanted_features.texture_compression_bc == vk::TRUE,
        };

        Ok(Self {
            instance,
            physical,
            device,
            queue,
            family,
            properties,
        })
    }
}

/// A host-visible storage buffer bound in a compute dispatch, with what read-back needs.
///
/// Grouped so a dispatch can bind a buffer whether it created it (the throwaway one [`dispatch`]
/// makes) or was handed a resident one (the backend, through [`dispatch_into`]). Copyable because
/// it is plain handles and sizes; the backend keeps the owning copy and passes a copy to dispatch.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DispatchBuffer {
    pub buffer: vk::Buffer,
    pub memory: vk::DeviceMemory,
    pub size: vk::DeviceSize,
    pub words: usize,
}

/// Records and runs one compute dispatch against two caller-owned buffers, and reads both back.
///
/// Releases everything it created **except the buffers** - the caller owns those, and whether they
/// are throwaway or resident is the caller's business. On an error before read-back it releases
/// nothing, which is the successful-path-only release the whole module has always had: the note at
/// the top explains why the session survives a leak, and refusing every later dispatch would be
/// worse.
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
        binding1.buffer,
        binding1.size,
    )?;
    let pipeline = bound.pipeline;
    let pipeline_layout = bound.layout;
    let sets = [bound.set];

    // ---- record and submit ---------------------------------------------------
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
    // Waiting on the device rather than a fence: one submission, and a fence would be more objects
    // to release for no extra guarantee.
    // SAFETY: the device is live and nothing else is using it.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;

    // ---- read back -----------------------------------------------------------
    let observed = read_back(device, binding0.memory, binding0.size, binding0.words)?;
    let guest_memory = read_back(device, binding1.memory, binding1.size, binding1.words)?;

    // ---- release (everything but the buffers) --------------------------------
    // SAFETY: every handle below was created here on this device, is not in use - the queue has
    // been waited on - and is destroyed exactly once.
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
/// one is guest memory. Both are created here, zeroed, and destroyed after, so a value read back
/// was written by the shader rather than left over. They are separate bindings rather than halves
/// of one buffer, because a guest address must not be able to reach the observation area: an
/// out-of-range store would rewrite the registers a test is about to assert on, and the failure
/// would present as a register bug rather than a memory one.
///
/// [`dispatch_into`] is the variant that binds a *resident* buffer instead of a throwaway one.
pub fn dispatch(
    module: &[u32],
    words: usize,
    memory_words: usize,
    groups: [u32; 3],
) -> Result<Output, DispatchError> {
    // A poisoned lock means an earlier dispatch panicked. Every handle a dispatch
    // creates is released before it returns, so the session itself is still sound, and
    // refusing every later dispatch would turn one failed test into all of them.
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
    };
    let binding1 = DispatchBuffer {
        buffer: memory_buffer,
        memory: memory_memory,
        size: memory_size,
        words: memory_words,
    };

    // On an error the buffers leak with everything else - the successful-path-only release this
    // has always had.
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

    // The device and the instance are deliberately **not** destroyed. They belong to the
    // shared session and the next dispatch will use them.

    Ok(Output {
        observed,
        memory: guest_memory,
    })
}

/// Runs a compute dispatch binding two caller-owned buffers - an observation at binding 0 and the
/// guest-memory window at binding 1 - and reads **both** back, destroying neither (worklog 647).
///
/// This is how a guest's dispatch is observed. A guest compute shader's result is in guest memory
/// (binding 1), not in the observation window a *translated* shader reports registers through - so
/// the window is bound at binding 1 and returned, where the former `dispatch_into` bound a scratch
/// there and discarded it (worklog 635 named that gap). Both buffers are the caller's - a resident
/// observation and the resident guest-memory window - and are left for it, reused across dispatches.
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
/// observation of `observation_words` at binding 0, and reads both back (worklog 647).
///
/// The window is the caller's and is left for it; the observation is created and destroyed here,
/// because a dispatch with no bound observation buffer still needs one for the module's binding 0.
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
    };

    let result = dispatch_core(device, queue, family, module, &observation, window, groups);

    // The throwaway observation, destroyed whether or not the dispatch succeeded; the window is the
    // caller's and is left alone.
    // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
    unsafe { device.destroy_buffer(scratch, None) };
    // SAFETY: as above, and nothing is bound to the memory now.
    unsafe { device.free_memory(scratch_memory, None) };

    result
}

#[cfg(test)]
mod tests {
    use super::{Availability, probe};

    #[test]
    fn probing_reports_a_reason_when_there_is_no_device() {
        // Whichever way this machine answers, the answer must be usable: a device
        // carries what it can do, an absence carries why. An absence with no reason
        // produces a skip message nobody can act on.
        match probe() {
            Availability::Available { properties } => {
                assert!(
                    !properties.device.is_empty(),
                    "an available device must name itself"
                );
                // A subgroup is at least one invocation wide by definition, so zero
                // means the property was never filled in - which would read as "this
                // device has no subgroups" rather than as "nobody asked".
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
