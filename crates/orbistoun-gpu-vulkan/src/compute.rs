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
fn create_host_buffer(
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
struct Session {
    instance: ash::Instance,
    physical: vk::PhysicalDevice,
    device: ash::Device,
    queue: vk::Queue,
    family: u32,
    properties: Properties,
}

/// The shared session, created on first use and never destroyed.
///
/// Never destroyed for the same reason the loader is not: there is nowhere to do it
/// from, and a process that has finished with Vulkan is one that is exiting. A failure
/// is cached alongside, so a machine with no device does not repeat the whole setup for
/// every call - and the stage that failed is kept, because "no device" and "a device
/// that refused us" want different responses.
fn session() -> Result<&'static std::sync::Mutex<Session>, DispatchError> {
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
        let device_info = vk::DeviceCreateInfo::default().queue_create_infos(&queue_info);
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

/// Runs a compute shader over two storage buffers and returns what it left in them.
///
/// Binding zero is the observation window a translated shader reports registers
/// through; binding one is guest memory. Both are zeroed before the dispatch, so a
/// value read back afterwards was written by the shader rather than left over.
///
/// They are separate bindings rather than halves of one buffer, because a guest address
/// must not be able to reach the observation area: an out-of-range store would rewrite
/// the registers a test is about to assert on, and the failure would present as a
/// register bug rather than a memory one.
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

    let bound = build_pipeline(device, module, buffer, size, memory_buffer, memory_size)?;
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
    // SAFETY: the command buffer has finished recording and the queue belongs to this
    // device.
    unsafe { device.queue_submit(queue, &submits, vk::Fence::null()) }
        .map_err(|e| DispatchError::Vulkan("queue_submit", e))?;
    // Waiting on the device rather than a fence: one submission, and a fence would be
    // more objects to release for no extra guarantee.
    // SAFETY: the device is live and nothing else is using it.
    unsafe { device.device_wait_idle() }
        .map_err(|e| DispatchError::Vulkan("device_wait_idle", e))?;

    // ---- read back -----------------------------------------------------------
    let observed = read_back(device, memory, size, words)?;
    let guest_memory = read_back(device, memory_memory, memory_size, memory_words)?;

    // ---- release -------------------------------------------------------------
    // Only on the successful path. See the note at the top of the module.
    // SAFETY: every handle below was created on this device, is not in use - the queue
    // has been waited on - and is destroyed exactly once.
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
    // SAFETY: as above.
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
