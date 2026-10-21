//! A compute dispatch's traced buffers (D746): each bound writable at its slot of the
//! draw-buffer set's compute binding, filled from the guest range its descriptor names, and read
//! back after the dispatch so the caller can write what changed to guest memory.

use ash::vk;
use orbistoun_spirv::{COMPUTE_BUFFERS_BINDING, COMPUTE_WRITTEN_BINDING, DRAW_BUFFERS_PER_STAGE};

use crate::DispatchError;
use crate::compute::{DispatchBuffer, read_back};

/// Where the shader reads and writes a dispatch's buffer: on the device, never across the bus. A
/// dispatch can run millions of groups over one buffer.
const BOUND_IN: &[vk::MemoryPropertyFlags] = &[vk::MemoryPropertyFlags::DEVICE_LOCAL];

/// Where its words wait on either side of the dispatch: host memory the CPU reads quickly, cached
/// where the device offers it.
const STAGED_IN: &[vk::MemoryPropertyFlags] = &[
    vk::MemoryPropertyFlags::from_raw(
        vk::MemoryPropertyFlags::HOST_VISIBLE.as_raw()
            | vk::MemoryPropertyFlags::HOST_COHERENT.as_raw()
            | vk::MemoryPropertyFlags::HOST_CACHED.as_raw(),
    ),
    vk::MemoryPropertyFlags::from_raw(
        vk::MemoryPropertyFlags::HOST_VISIBLE.as_raw()
            | vk::MemoryPropertyFlags::HOST_COHERENT.as_raw(),
    ),
];

/// The use a slot's staging buffer is made for.
const STAGING_USAGE: vk::BufferUsageFlags = vk::BufferUsageFlags::from_raw(
    vk::BufferUsageFlags::TRANSFER_SRC.as_raw() | vk::BufferUsageFlags::TRANSFER_DST.as_raw(),
);
/// The use the buffer a slot binds is made for.
const BOUND_USAGE: vk::BufferUsageFlags = vk::BufferUsageFlags::from_raw(
    vk::BufferUsageFlags::STORAGE_BUFFER.as_raw()
        | vk::BufferUsageFlags::TRANSFER_SRC.as_raw()
        | vk::BufferUsageFlags::TRANSFER_DST.as_raw(),
);

/// One slot: the buffer the set binds, and the host buffer its words are copied through.
struct Slot {
    bound: (vk::Buffer, vk::DeviceMemory),
    /// The memory properties `bound` was allocated with.
    #[cfg_attr(not(test), allow(dead_code))]
    placed: vk::MemoryPropertyFlags,
    staging: DispatchBuffer,
    /// Whether the host did not read its words, so the device clears it rather than copying them
    /// in (D809).
    unread: bool,
    /// Its write mask (D809). Every slot of the dispatch's own has one, read or not: the module
    /// marks each slot its trace found only stored to, and a mark must land in a mask, never in a
    /// buffer.
    mask: Option<Mask>,
    /// For a large buffer handed over unread, the list its stored words are gathered into on the
    /// device, which is all that comes back unless it overflows.
    list: Option<StoredList>,
}

/// The words a dispatch stored into one of its buffers, gathered on the device by
/// [`orbistoun_spirv::stored_word_compaction_module`]: a count, then `(index, value)` pairs.
struct StoredList {
    bound: (vk::Buffer, vk::DeviceMemory),
    placed: vk::MemoryPropertyFlags,
    staging: DispatchBuffer,
    /// How many pairs it holds.
    capacity: u32,
    /// The set binding the mask, the buffer and the list to the gathering program.
    set: vk::DescriptorSet,
}

/// The fewest words a buffer handed over unread has for its stored words to be gathered on the
/// device. Below it, the buffer and its mask are as small as the list would be.
const LISTED_FROM_WORDS: usize = 4096;

/// A list's capacity, in pairs, as a share of its buffer's words: one in 32. A pair is two words,
/// so a list that fits is a sixteenth of the buffer, against the buffer and its mask whole.
const LIST_SHARE: usize = 32;

/// The groups in one row of a gathering dispatch, the least `maxComputeWorkGroupCount[0]` a device
/// may offer; a buffer needing more is gathered by as many rows as it takes (PPSA28061's 32 MiB
/// buffer takes two).
const LIST_ROW_GROUPS: u32 = 65_535;

/// How many dispatches a buffer of a size whose list overflowed is read back whole before a list
/// is tried for that size again. A buffer a program stores whole - a clear - overflows every time,
/// and each overflow costs the gathering and a second trip to the device on top of the whole copy.
const UNLISTED_AFTER_OVERFLOW: u32 = 16;

/// For each buffer size whose list last overflowed, how many more dispatches read it back whole.
fn unlisted_sizes() -> &'static std::sync::Mutex<std::collections::HashMap<usize, u32>> {
    static UNLISTED: std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<usize, u32>>> =
        std::sync::OnceLock::new();
    UNLISTED.get_or_init(Default::default)
}

/// Whether a buffer of `words` words handed over unread keeps a list this time: large enough, its
/// words countable in a push constant, and no recent overflow at its size - which counts that
/// dispatch down.
fn lists(words: usize) -> bool {
    if words < LISTED_FROM_WORDS || u32::try_from(words).is_err() {
        return false;
    }
    let Ok(mut unlisted) = unlisted_sizes().lock() else {
        return true;
    };
    match unlisted.get_mut(&words) {
        Some(left) if *left > 0 => {
            *left -= 1;
            false
        }
        Some(_) => {
            unlisted.remove(&words);
            true
        }
        None => true,
    }
}

/// Notes that a list for a buffer of `words` words overflowed.
fn note_overflow(words: usize) {
    if let Ok(mut unlisted) = unlisted_sizes().lock() {
        unlisted.insert(words, UNLISTED_AFTER_OVERFLOW);
    }
}

/// A write mask: a word for each of its buffer's words and one more (D809), on the device and
/// read back through a host buffer.
struct Mask {
    bound: (vk::Buffer, vk::DeviceMemory),
    /// The memory properties `bound` was allocated with.
    placed: vk::MemoryPropertyFlags,
    staging: DispatchBuffer,
}

/// A buffer a finished dispatch released, kept for the next that needs one of its size, use and
/// placement (D805): every reuse overwrites all of it - a staging buffer with the words copied in,
/// a bound one with the copy from its staging buffer - so nothing it held is ever read.
struct Spare {
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    /// Which placement list it was allocated from, by the list's address.
    preferred: usize,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    flags: vk::MemoryPropertyFlags,
}

/// The spares, on the session's one device.
fn spares() -> &'static std::sync::Mutex<Vec<Spare>> {
    static SPARES: std::sync::Mutex<Vec<Spare>> = std::sync::Mutex::new(Vec::new());
    &SPARES
}

/// The most bytes the spares hold together: two 32 MiB buffers - a display buffer's staging and
/// bound copies - and the slots' placeholders, with room to spare.
const SPARE_BYTES_MOST: vk::DeviceSize = 160 << 20;

/// Keeps a released buffer among the spares, or destroys it when they hold enough already.
fn release(
    device: &ash::Device,
    (buffer, memory): (vk::Buffer, vk::DeviceMemory),
    (size, usage, preferred, flags): (
        vk::DeviceSize,
        vk::BufferUsageFlags,
        &[vk::MemoryPropertyFlags],
        vk::MemoryPropertyFlags,
    ),
) {
    if let Ok(mut spares) = spares().lock() {
        let held: vk::DeviceSize = spares.iter().map(|spare| spare.size).sum();
        if held + size <= SPARE_BYTES_MOST {
            spares.push(Spare {
                size,
                usage,
                preferred: preferred.as_ptr() as usize,
                buffer,
                memory,
                flags,
            });
            return;
        }
    }
    // SAFETY: created on this device, no longer in use, and destroyed exactly once.
    unsafe { device.destroy_buffer(buffer, None) };
    // SAFETY: as above, and nothing is bound to the memory now.
    unsafe { device.free_memory(memory, None) };
}

/// A buffer of `size` bytes for `usage`, in the first memory of `preferred` the device offers: a
/// spare of exactly that where one is kept (D805), else a new one.
fn buffer_in(
    context: (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    preferred: &[vk::MemoryPropertyFlags],
) -> Result<(vk::Buffer, vk::DeviceMemory, vk::MemoryPropertyFlags), DispatchError> {
    if let Ok(mut spares) = spares().lock()
        && let Some(at) = spares.iter().position(|spare| {
            spare.size == size
                && spare.usage == usage
                && spare.preferred == preferred.as_ptr() as usize
        })
    {
        let spare = spares.swap_remove(at);
        return Ok((spare.buffer, spare.memory, spare.flags));
    }
    orbistoun_gpu::perf::span(orbistoun_gpu::perf::Span::DispatchNewBuffer, || {
        new_buffer_in(context, size, usage, preferred)
    })
}

/// [`buffer_in`]'s new buffer.
fn new_buffer_in(
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
    size: vk::DeviceSize,
    usage: vk::BufferUsageFlags,
    preferred: &[vk::MemoryPropertyFlags],
) -> Result<(vk::Buffer, vk::DeviceMemory, vk::MemoryPropertyFlags), DispatchError> {
    let info = vk::BufferCreateInfo::default()
        .size(size)
        .usage(usage)
        .sharing_mode(vk::SharingMode::EXCLUSIVE);
    // SAFETY: the device is live and the create info outlives the call.
    let buffer = unsafe { device.create_buffer(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_buffer(dispatch buffer)", e))?;
    // SAFETY: the buffer was created on this device.
    let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
    // SAFETY: the physical device is valid.
    let properties = unsafe { instance.get_physical_device_memory_properties(physical) };
    let chosen = preferred.iter().find_map(|&wanted| {
        (0..properties.memory_type_count).find_map(|i| {
            let flags = properties.memory_types[i as usize].property_flags;
            (requirements.memory_type_bits & (1 << i) != 0 && flags.contains(wanted))
                .then_some((i, flags))
        })
    });
    let Some((memory_type, flags)) = chosen else {
        // SAFETY: created above and never used.
        unsafe { device.destroy_buffer(buffer, None) };
        return Err(DispatchError::NoHostVisibleMemory);
    };
    let allocate = vk::MemoryAllocateInfo::default()
        .allocation_size(requirements.size)
        .memory_type_index(memory_type);
    // SAFETY: the allocation info is fully initialised and the device is live.
    let memory = match unsafe { device.allocate_memory(&allocate, None) } {
        Ok(memory) => memory,
        Err(e) => {
            // SAFETY: created above and never used.
            unsafe { device.destroy_buffer(buffer, None) };
            return Err(DispatchError::Vulkan("allocate_memory(dispatch buffer)", e));
        }
    };
    // SAFETY: buffer and memory come from this device and the allocation is large enough.
    unsafe { device.bind_buffer_memory(buffer, memory, 0) }
        .map_err(|e| DispatchError::Vulkan("bind_buffer_memory(dispatch buffer)", e))?;
    Ok((buffer, memory, flags))
}

/// The bindings of a dispatch's buffer set, set one of its pipeline: the same for every dispatch, so
/// a pipeline kept across dispatches binds any dispatch's set.
pub(crate) fn layout_bindings() -> [vk::DescriptorSetLayoutBinding<'static>; 2] {
    [COMPUTE_BUFFERS_BINDING, COMPUTE_WRITTEN_BINDING].map(|binding| {
        vk::DescriptorSetLayoutBinding::default()
            .binding(binding)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(DRAW_BUFFERS_PER_STAGE)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
    })
}

/// A dispatch's buffers on the device, with the set that binds them.
pub(crate) struct BoundBuffers {
    /// One per slot of the dispatch's own; every other element of the bound array binds one of
    /// these, which the module never reaches.
    slots: Vec<Slot>,
    /// For each buffer handed over, in order, the slot that holds it: its own, or the earlier one
    /// it aliases (D937).
    held_at: Vec<usize>,
    /// The set's layout, set one of the pipeline.
    pub(crate) layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    /// The set to bind.
    pub(crate) set: vk::DescriptorSet,
    /// The gathering program and the pool its sets come from, where a slot keeps a list.
    lists: Option<(Gathering, vk::DescriptorPool)>,
}

/// The program that gathers a dispatch's stored words into a list, built once per device.
#[derive(Debug, Clone, Copy)]
struct Gathering {
    set_layout: vk::DescriptorSetLayout,
    layout: vk::PipelineLayout,
    pipeline: vk::Pipeline,
}

/// The gathering program for `device`, built on first use and kept: it is the same for every
/// dispatch.
fn gathering(device: &ash::Device) -> Result<Gathering, DispatchError> {
    static KEPT: std::sync::Mutex<Option<(u64, Gathering)>> = std::sync::Mutex::new(None);
    let mut kept = KEPT
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let raw = vk::Handle::as_raw(device.handle());
    if let Some((on, made)) = *kept
        && on == raw
    {
        return Ok(made);
    }
    let words = orbistoun_spirv::stored_word_compaction_module();
    let info = vk::ShaderModuleCreateInfo::default().code(&words);
    // SAFETY: the words outlive the call.
    let shader = unsafe { device.create_shader_module(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_shader_module(gathering)", e))?;
    let bindings = [0, 1, 2].map(|binding| {
        vk::DescriptorSetLayoutBinding::default()
            .binding(binding)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(1)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)
    });
    let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    // SAFETY: the create info outlives the call.
    let set_layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout(gathering)", e))?;
    // The buffer's word count and the list's capacity.
    let push = [vk::PushConstantRange::default()
        .stage_flags(vk::ShaderStageFlags::COMPUTE)
        .offset(0)
        .size(12)];
    let set_layouts = [set_layout];
    let pipeline_layout_info = vk::PipelineLayoutCreateInfo::default()
        .set_layouts(&set_layouts)
        .push_constant_ranges(&push);
    // SAFETY: the create info outlives the call.
    let layout = unsafe { device.create_pipeline_layout(&pipeline_layout_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_pipeline_layout(gathering)", e))?;
    let stage = vk::PipelineShaderStageCreateInfo::default()
        .stage(vk::ShaderStageFlags::COMPUTE)
        .module(shader)
        .name(c"main");
    let pipeline_info = [vk::ComputePipelineCreateInfo::default()
        .stage(stage)
        .layout(layout)];
    // SAFETY: the create info outlives the call and names a live shader module.
    let pipelines = unsafe {
        device.create_compute_pipelines(crate::pipeline_cache::cache(), &pipeline_info, None)
    };
    // SAFETY: the pipeline, made or not, no longer needs the module.
    unsafe { device.destroy_shader_module(shader, None) };
    let pipeline = pipelines
        .map_err(|(_, e)| DispatchError::Vulkan("create_compute_pipelines(gathering)", e))?[0];
    let made = Gathering {
        set_layout,
        layout,
        pipeline,
    };
    *kept = Some((raw, made));
    Ok(made)
}

/// One of a dispatch's buffers as the host hands it over (D809).
#[derive(Debug, Clone, Copy)]
pub enum BufferIn<'a> {
    /// Its words before the dispatch, which the program may read.
    Words(&'a [u32]),
    /// A buffer of this many words the program only stores to: nothing is read or uploaded, and
    /// the device starts it at zero.
    Unread(usize),
    /// The buffer handed at this earlier index, bound here too (D937).
    Alias(usize),
}

impl BufferIn<'_> {
    /// How many words it holds; at least one, so every element of the bound array is a buffer.
    fn words(self) -> usize {
        match self {
            Self::Words(words) => words.len(),
            Self::Unread(words) => words,
            Self::Alias(_) => 1,
        }
        .max(1)
    }
}

/// A dispatch's buffer after it ran: its words, and - for one handed over unread - its write
/// mask, a word for each word, non-zero where the dispatch stored (D809).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BufferOut {
    /// The words after.
    pub words: Vec<u32>,
    /// Word `i` non-zero where word `i` was stored to; empty for a buffer the host read, whose
    /// stores are found by comparing its words.
    pub written: Vec<u32>,
    /// For a buffer handed over unread whose stores were gathered on the device: the indices of the
    /// words stored, ascending, and their values in the same order. [`Self::words`] and
    /// [`Self::written`] are then empty: the words not listed are the zeros the device cleared them
    /// to.
    pub stored: Option<(Vec<u32>, Vec<u32>)>,
    /// For a buffer handed over unread whose every word the device found stored with one value - a
    /// clear - that value, and nothing else: [`Self::words`], [`Self::written`] and
    /// [`Self::stored`] are then empty.
    pub whole: Option<u32>,
}

impl BoundBuffers {
    /// Stages `contents`, one buffer per slot, each with its write mask, and
    /// builds the set binding them; `None` for a dispatch that binds none. The words reach the
    /// bound buffers, and the masks and unread buffers are cleared, through
    /// [`Self::record_upload`].
    ///
    /// # Errors
    ///
    /// More buffers than a module declares, or a device that refused a step.
    pub(crate) fn create(
        (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
        contents: &[BufferIn<'_>],
    ) -> Result<Option<Self>, DispatchError> {
        if contents.is_empty() {
            return Ok(None);
        }
        if contents.len() > DRAW_BUFFERS_PER_STAGE as usize {
            return Err(DispatchError::Unsupported(
                "a dispatch binds more buffers than a module declares (D746)".to_owned(),
            ));
        }
        let context = (instance, physical, device);
        // Buffers for the dispatch's own slots only: the module reaches no other, so the rest of
        // the array binds one of these.
        // A buffer handed as an alias binds the slot it names; every other its own (D937).
        let mut slots = Vec::with_capacity(contents.len());
        let mut held_at = Vec::with_capacity(contents.len());
        for &handed in contents {
            if let BufferIn::Alias(earlier) = handed {
                let held = *held_at.get(earlier).ok_or_else(|| {
                    DispatchError::Unsupported(
                        "a buffer aliases one handed over after it (D937)".to_owned(),
                    )
                })?;
                held_at.push(held);
                continue;
            }
            held_at.push(slots.len());
            slots.push(Slot::create(context, handed)?);
        }
        let lists = bind_lists(device, &mut slots)?;
        let bound_at = |index: usize| {
            held_at
                .get(index)
                .and_then(|&held| slots.get(held))
                .unwrap_or(&slots[0])
        };

        let bindings = layout_bindings();
        let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        // SAFETY: the create info outlives the call.
        let layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
            .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?;
        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(DRAW_BUFFERS_PER_STAGE * 2)];
        let pool_info = vk::DescriptorPoolCreateInfo::default()
            .pool_sizes(&pool_sizes)
            .max_sets(1);
        // SAFETY: the create info outlives the call.
        let pool = unsafe { device.create_descriptor_pool(&pool_info, None) }
            .map_err(|e| DispatchError::Vulkan("create_descriptor_pool", e))?;
        let layouts = [layout];
        let allocate = vk::DescriptorSetAllocateInfo::default()
            .descriptor_pool(pool)
            .set_layouts(&layouts);
        // SAFETY: the pool has room for exactly this one set.
        let set = unsafe { device.allocate_descriptor_sets(&allocate) }
            .map_err(|e| DispatchError::Vulkan("allocate_descriptor_sets", e))?[0];
        let infos: Vec<vk::DescriptorBufferInfo> = (0..DRAW_BUFFERS_PER_STAGE as usize)
            .map(|index| {
                let slot = bound_at(index);
                vk::DescriptorBufferInfo::default()
                    .buffer(slot.bound.0)
                    .offset(0)
                    .range(slot.staging.size)
            })
            .collect();
        // Every slot of the dispatch's own has a mask; the rest of the array binds slot zero's.
        let masks: Vec<vk::DescriptorBufferInfo> = (0..DRAW_BUFFERS_PER_STAGE as usize)
            .map(|index| {
                let slot = bound_at(index);
                let (buffer, size) = slot
                    .mask
                    .as_ref()
                    .map_or((slot.bound.0, slot.staging.size), |mask| {
                        (mask.bound.0, mask.staging.size)
                    });
                vk::DescriptorBufferInfo::default()
                    .buffer(buffer)
                    .offset(0)
                    .range(size)
            })
            .collect();
        let writes = [
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(COMPUTE_BUFFERS_BINDING)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&infos),
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(COMPUTE_WRITTEN_BINDING)
                .dst_array_element(0)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(&masks),
        ];
        // SAFETY: the writes refer to a live set and live buffers, and the slices outlive the call.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
        Ok(Some(Self {
            slots,
            held_at,
            layout,
            pool,
            set,
            lists,
        }))
    }

    /// Records each slot's copy from its staging buffer to the buffer the set binds - or, for a
    /// buffer the host did not read, a clear to zero - and every write mask's clear, ordered before
    /// the dispatch, into an open recording.
    pub(crate) fn record_upload(&self, device: &ash::Device, command: vk::CommandBuffer) {
        for slot in &self.slots {
            if let Some(mask) = &slot.mask {
                // SAFETY: recording is open, and the buffer is live, made for transfers into it.
                unsafe { device.cmd_fill_buffer(command, mask.bound.0, 0, vk::WHOLE_SIZE, 0) };
            }
            if let Some(list) = &slot.list {
                // Its count and its flag, from zero; the first word is written by the gathering,
                // and the pairs past the count are never read.
                //
                // SAFETY: as above, and the list holds at least those two words.
                unsafe { device.cmd_fill_buffer(command, list.bound.0, 0, 8, 0) };
            }
            if slot.unread {
                // SAFETY: as above.
                unsafe { device.cmd_fill_buffer(command, slot.bound.0, 0, vk::WHOLE_SIZE, 0) };
            } else {
                let region = [vk::BufferCopy::default().size(slot.staging.size)];
                // SAFETY: recording is open, and both buffers are live and at least `size` bytes.
                unsafe {
                    device.cmd_copy_buffer(command, slot.staging.buffer, slot.bound.0, &region);
                };
            }
        }
        memory_barrier(
            device,
            command,
            (
                vk::AccessFlags::TRANSFER_WRITE,
                vk::AccessFlags::SHADER_READ | vk::AccessFlags::SHADER_WRITE,
            ),
            (
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::COMPUTE_SHADER,
            ),
        );
    }

    /// Records the dispatch's own buffers' and write masks' copies back to their staging buffers,
    /// after its stores and before the host reads them, into an open recording. A slot that keeps a
    /// list has its stored words gathered into it on the device first, and only the list is copied
    /// back.
    pub(crate) fn record_readback(&self, device: &ash::Device, command: vk::CommandBuffer) {
        memory_barrier(
            device,
            command,
            (
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::SHADER_READ,
            ),
            (
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER | vk::PipelineStageFlags::COMPUTE_SHADER,
            ),
        );
        if let Some((gathering, _)) = &self.lists {
            self.record_gathering(device, command, *gathering);
        }
        for slot in &self.slots {
            let copies: Vec<(vk::Buffer, &DispatchBuffer)> = match &slot.list {
                Some(list) => vec![(list.bound.0, &list.staging)],
                None => std::iter::once((slot.bound.0, &slot.staging))
                    .chain(slot.mask.as_ref().map(|mask| (mask.bound.0, &mask.staging)))
                    .collect(),
            };
            for (from, to) in copies {
                let region = [vk::BufferCopy::default().size(to.size)];
                // SAFETY: recording is open, and both buffers are live and at least `size` bytes.
                unsafe { device.cmd_copy_buffer(command, from, to.buffer, &region) };
            }
        }
        memory_barrier(
            device,
            command,
            (vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::HOST_READ),
            (
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
            ),
        );
    }

    /// Records the gathering of each listed slot's stored words into its list, and orders it before
    /// the copies back.
    fn record_gathering(
        &self,
        device: &ash::Device,
        command: vk::CommandBuffer,
        gathering: Gathering,
    ) {
        // SAFETY: recording is open and the pipeline is live.
        unsafe {
            device.cmd_bind_pipeline(command, vk::PipelineBindPoint::COMPUTE, gathering.pipeline);
        };
        for slot in &self.slots {
            let Some(list) = &slot.list else {
                continue;
            };
            let words = u32::try_from(slot.staging.words).unwrap_or(u32::MAX);
            // SAFETY: recording is open; the set was allocated from this pipeline's set layout.
            unsafe {
                device.cmd_bind_descriptor_sets(
                    command,
                    vk::PipelineBindPoint::COMPUTE,
                    gathering.layout,
                    0,
                    &[list.set],
                    &[],
                );
            }
            // Rows of groups, as many as the buffer takes.
            let groups = words.div_ceil(orbistoun_spirv::COMPACTION_GROUP);
            let (across, down) = (
                groups.min(LIST_ROW_GROUPS),
                groups.div_ceil(LIST_ROW_GROUPS),
            );
            let row_words = across * orbistoun_spirv::COMPACTION_GROUP;
            // SAFETY: recording is open, and the range is the one the layout declares.
            unsafe {
                device.cmd_push_constants(
                    command,
                    gathering.layout,
                    vk::ShaderStageFlags::COMPUTE,
                    0,
                    zerocopy::IntoBytes::as_bytes(&[words, list.capacity, row_words]),
                );
            }
            // SAFETY: recording is open and a pipeline is bound; each dimension's group count is
            // within the least any device offers (`LIST_ROW_GROUPS`).
            unsafe { device.cmd_dispatch(command, across, down, 1) };
        }
        memory_barrier(
            device,
            command,
            (
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::TRANSFER_READ,
            ),
            (
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER,
            ),
        );
    }

    /// The dispatch's buffers as the dispatch left them, with their write masks, by slot, once the
    /// recording that copied them back has completed. A slot that kept a list answers its stored
    /// words alone, or - where they overflowed it - its buffer and mask, copied back now on
    /// `queue`.
    ///
    /// # Errors
    ///
    /// A buffer that could not be mapped, a copy back the device refused, or a list naming a word
    /// past its buffer.
    pub(crate) fn read(
        &self,
        device: &ash::Device,
        queue: (vk::Queue, u32),
    ) -> Result<Vec<BufferOut>, DispatchError> {
        let held: Vec<BufferOut> = self
            .slots
            .iter()
            .map(|slot| {
                use orbistoun_gpu::perf::{Span, SpanGuard};
                if let Some(list) = &slot.list {
                    let listing = SpanGuard::start(Span::BuffersListed);
                    let held = read_back(device, &list.staging)?;
                    let answered = |stored, whole| BufferOut {
                        words: Vec::new(),
                        written: Vec::new(),
                        stored,
                        whole,
                    };
                    match orbistoun_gpu::perf::span(Span::BuffersGathered, || {
                        gathered(&held, slot.staging.words)
                    })? {
                        Gathered::Listed(stored) => return Ok(answered(Some(stored), None)),
                        Gathered::Whole(value) => return Ok(answered(None, Some(value))),
                        Gathered::Overflowed => {}
                    }
                    // More stores than the list holds: the buffer and its mask after all, and the
                    // next few of this size are read back whole from the start.
                    drop(listing);
                    let _overflowing = SpanGuard::start(Span::BuffersOverflowed);
                    note_overflow(slot.staging.words);
                    let mut copies = vec![(slot.bound.0, &slot.staging)];
                    copies.extend(slot.mask.as_ref().map(|mask| (mask.bound.0, &mask.staging)));
                    copy_back(device, queue, &copies)?;
                }
                let _whole = SpanGuard::start(Span::BuffersWhole);
                let written = match &slot.mask {
                    Some(mask) => {
                        let mut written = read_back(device, &mask.staging)?;
                        // The word past the buffer's marks the stores that did not land.
                        written.truncate(slot.staging.words);
                        written
                    }
                    None => Vec::new(),
                };
                Ok(BufferOut {
                    words: read_back(device, &slot.staging)?,
                    written,
                    stored: None,
                    whole: None,
                })
            })
            .collect::<Result<_, DispatchError>>()?;
        // Each buffer handed over answers its slot's words; an alias, the slot it shares (D937).
        Ok(self
            .held_at
            .iter()
            .filter_map(|&slot| held.get(slot).cloned())
            .collect())
    }

    /// The memory properties each of the dispatch's own buffers was allocated with.
    #[cfg(test)]
    fn placed_in(&self) -> Vec<vk::MemoryPropertyFlags> {
        self.slots.iter().map(|slot| slot.placed).collect()
    }

    /// Releases the set, and the buffers to the spares (D805), once the dispatch that used them
    /// has finished.
    pub(crate) fn destroy(self, device: &ash::Device) {
        // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
        unsafe { device.destroy_descriptor_pool(self.pool, None) };
        // SAFETY: as above.
        unsafe { device.destroy_descriptor_set_layout(self.layout, None) };
        if let Some((_, pool)) = self.lists {
            // SAFETY: as above; the gathering program itself is kept.
            unsafe { device.destroy_descriptor_pool(pool, None) };
        }
        for slot in self.slots {
            let mask = slot
                .mask
                .map(|mask| (mask.bound, mask.staging, mask.placed));
            let list = slot
                .list
                .map(|list| (list.bound, list.staging, list.placed));
            for (bound, staging, placed) in std::iter::once((slot.bound, slot.staging, slot.placed))
                .chain(mask)
                .chain(list)
            {
                let size = staging.size;
                release(device, bound, (size, BOUND_USAGE, BOUND_IN, placed));
                release(
                    device,
                    (staging.buffer, staging.memory),
                    (
                        size,
                        STAGING_USAGE,
                        STAGED_IN,
                        vk::MemoryPropertyFlags::empty(),
                    ),
                );
            }
        }
    }
}

impl Slot {
    /// One slot's buffer and write mask, each a bound buffer and its staging buffer, with the
    /// words staged where the host read them.
    fn create(
        context: (&ash::Instance, vk::PhysicalDevice, &ash::Device),
        handed: BufferIn<'_>,
    ) -> Result<Self, DispatchError> {
        let device = context.2;
        let words = handed.words();
        let size = (words * 4) as vk::DeviceSize;
        let (staging, staging_memory, _) = buffer_in(context, size, STAGING_USAGE, STAGED_IN)?;
        if let BufferIn::Words(contents) = handed
            && !contents.is_empty()
        {
            // SAFETY: the memory is host-visible, holds `size` bytes and is not mapped.
            let mapped =
                unsafe { device.map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty()) }
                    .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
            // SAFETY: the mapping covers `contents.len()` whole words, and device memory and the
            // caller's slice cannot overlap.
            unsafe {
                std::ptr::copy_nonoverlapping(
                    contents.as_ptr(),
                    mapped.cast::<u32>(),
                    contents.len(),
                );
            };
            // SAFETY: mapped immediately above and not used after unmapping.
            unsafe { device.unmap_memory(staging_memory) };
        }
        let (bound, bound_memory, placed) = buffer_in(context, size, BOUND_USAGE, BOUND_IN)?;
        let unread = !matches!(handed, BufferIn::Words(contents) if !contents.is_empty());
        // A mask word for each of the buffer's words, and one past them that a store which did not
        // land marks.
        let mask = {
            let mask_words = words + 1;
            let mask_size = (mask_words * 4) as vk::DeviceSize;
            let (staging, staging_memory, _) =
                buffer_in(context, mask_size, STAGING_USAGE, STAGED_IN)?;
            let (bound, bound_memory, placed) =
                buffer_in(context, mask_size, BOUND_USAGE, BOUND_IN)?;
            Some(Mask {
                bound: (bound, bound_memory),
                placed,
                staging: DispatchBuffer {
                    buffer: staging,
                    memory: staging_memory,
                    size: mask_size,
                    words: mask_words,
                    offset: 0,
                },
            })
        };
        // A large buffer handed over unread gathers its stored words into a list on the device.
        let list = if unread && lists(words) {
            let capacity = words / LIST_SHARE;
            // The count, the flag and the first word, then the pairs.
            let list_words = 3 + 2 * capacity;
            let list_size = (list_words * 4) as vk::DeviceSize;
            let (staging, staging_memory, _) =
                buffer_in(context, list_size, STAGING_USAGE, STAGED_IN)?;
            let (bound, bound_memory, placed) =
                buffer_in(context, list_size, BOUND_USAGE, BOUND_IN)?;
            Some(StoredList {
                bound: (bound, bound_memory),
                placed,
                staging: DispatchBuffer {
                    buffer: staging,
                    memory: staging_memory,
                    size: list_size,
                    words: list_words,
                    offset: 0,
                },
                capacity: u32::try_from(capacity).unwrap_or(u32::MAX),
                set: vk::DescriptorSet::null(),
            })
        } else {
            None
        };
        Ok(Self {
            bound: (bound, bound_memory),
            placed,
            staging: DispatchBuffer {
                buffer: staging,
                memory: staging_memory,
                size,
                words,
                offset: 0,
            },
            unread,
            mask,
            list,
        })
    }
}

/// Allocates and fills the gathering set of each slot that keeps a list, binding its mask, its
/// buffer and its list; `None` where no slot keeps one.
fn bind_lists(
    device: &ash::Device,
    slots: &mut [Slot],
) -> Result<Option<(Gathering, vk::DescriptorPool)>, DispatchError> {
    let listed = slots.iter().filter(|slot| slot.list.is_some()).count();
    let Ok(sets) = u32::try_from(listed) else {
        return Err(DispatchError::Unsupported("too many lists".to_owned()));
    };
    if sets == 0 {
        return Ok(None);
    }
    let gathering = gathering(device)?;
    let pool_sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(3 * sets)];
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&pool_sizes)
        .max_sets(sets);
    // SAFETY: the create info outlives the call.
    let pool = unsafe { device.create_descriptor_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_pool(gathering)", e))?;
    let layouts = vec![gathering.set_layout; listed];
    let allocate = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(&layouts);
    // SAFETY: the pool has room for exactly these sets.
    let allocated = match unsafe { device.allocate_descriptor_sets(&allocate) } {
        Ok(allocated) => allocated,
        Err(e) => {
            // SAFETY: created above and holding nothing in use.
            unsafe { device.destroy_descriptor_pool(pool, None) };
            return Err(DispatchError::Vulkan(
                "allocate_descriptor_sets(gathering)",
                e,
            ));
        }
    };
    for (slot, set) in slots
        .iter_mut()
        .filter(|slot| slot.list.is_some())
        .zip(allocated)
    {
        let (Some(mask), Some(list)) = (&slot.mask, &mut slot.list) else {
            continue;
        };
        list.set = set;
        let infos = [
            (mask.bound.0, mask.staging.size),
            (slot.bound.0, slot.staging.size),
            (list.bound.0, list.staging.size),
        ]
        .map(|(buffer, size)| {
            [vk::DescriptorBufferInfo::default()
                .buffer(buffer)
                .offset(0)
                .range(size)]
        });
        let writes: Vec<vk::WriteDescriptorSet<'_>> = infos
            .iter()
            .zip(0..)
            .map(|(info, binding)| {
                vk::WriteDescriptorSet::default()
                    .dst_set(set)
                    .dst_binding(binding)
                    .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                    .buffer_info(info)
            })
            .collect();
        // SAFETY: the writes name a live set and live buffers, and the slices outlive the call.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
    }
    Ok(Some((gathering, pool)))
}

/// Copies each `(from, to)` back to its host buffer in a submission of its own, and waits for it:
/// a slot whose list overflowed, read back whole after all.
fn copy_back(
    device: &ash::Device,
    (queue, family): (vk::Queue, u32),
    copies: &[(vk::Buffer, &DispatchBuffer)],
) -> Result<(), DispatchError> {
    let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(family);
    // SAFETY: the family index is one this device was created with.
    let pool = unsafe { device.create_command_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool(copy back)", e))?;
    let recorded = (|| {
        let allocate = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool is live.
        let command = unsafe { device.allocate_command_buffers(&allocate) }
            .map_err(|e| DispatchError::Vulkan("allocate_command_buffers(copy back)", e))?[0];
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: the command buffer is fresh and not recording.
        unsafe { device.begin_command_buffer(command, &begin) }
            .map_err(|e| DispatchError::Vulkan("begin_command_buffer(copy back)", e))?;
        memory_barrier(
            device,
            command,
            (
                vk::AccessFlags::SHADER_WRITE,
                vk::AccessFlags::TRANSFER_READ,
            ),
            (
                vk::PipelineStageFlags::COMPUTE_SHADER,
                vk::PipelineStageFlags::TRANSFER,
            ),
        );
        for &(from, to) in copies {
            let region = [vk::BufferCopy::default().size(to.size)];
            // SAFETY: recording is open, and both buffers are live and at least `size` bytes.
            unsafe { device.cmd_copy_buffer(command, from, to.buffer, &region) };
        }
        memory_barrier(
            device,
            command,
            (vk::AccessFlags::TRANSFER_WRITE, vk::AccessFlags::HOST_READ),
            (
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::HOST,
            ),
        );
        // SAFETY: recording is open.
        unsafe { device.end_command_buffer(command) }
            .map_err(|e| DispatchError::Vulkan("end_command_buffer(copy back)", e))?;
        // SAFETY: the device is live and the create info outlives the call.
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(|e| DispatchError::Vulkan("create_fence(copy back)", e))?;
        let commands = [command];
        let submits = [vk::SubmitInfo::default().command_buffers(&commands)];
        // SAFETY: the recording is complete, the queue is this device's, and the fence is fresh.
        let waited = unsafe { device.queue_submit(queue, &submits, fence) }
            .map_err(|e| DispatchError::Vulkan("queue_submit(copy back)", e))
            .and_then(|()| {
                // SAFETY: the fence was submitted with the work just above.
                unsafe { device.wait_for_fences(&[fence], true, u64::MAX) }
                    .map_err(|e| DispatchError::Vulkan("wait_for_fences(copy back)", e))
            });
        // SAFETY: the submission it fenced has completed, or was never made.
        unsafe { device.destroy_fence(fence, None) };
        waited
    })();
    // SAFETY: nothing recorded from the pool is still running: the wait above returned, or nothing
    // was submitted.
    unsafe { device.destroy_command_pool(pool, None) };
    recorded
}

/// `keyed` - each pair as one integer, its index above its value - in ascending order of index: a
/// radix sort on the index's bits, eleven at a time and only as many as the largest index has,
/// where a list is long enough for that to beat comparing. A list's indices are distinct, so the
/// order is the one a plain sort gives; sorting thousands of pairs by comparison was a fifth of
/// PPSA25872's run.
fn by_index(mut keyed: Vec<u64>) -> Vec<u64> {
    const DIGIT: u32 = 11;
    const BUCKETS: usize = 1 << DIGIT;
    if keyed.len() < BUCKETS {
        keyed.sort_unstable();
        return keyed;
    }
    let largest = keyed.iter().map(|&key| key >> 32).max().unwrap_or(0);
    let bits = u64::BITS - largest.leading_zeros();
    let mut spare = vec![0u64; keyed.len()];
    let mut shift = 32;
    while shift < 32 + bits {
        let digit = |key: u64| ((key >> shift) as usize) & (BUCKETS - 1);
        let mut starts = [0usize; BUCKETS];
        for &key in &keyed {
            starts[digit(key)] += 1;
        }
        let mut at = 0;
        for start in &mut starts {
            let count = *start;
            *start = at;
            at += count;
        }
        for &key in &keyed {
            let bucket = &mut starts[digit(key)];
            spare[*bucket] = key;
            *bucket += 1;
        }
        std::mem::swap(&mut keyed, &mut spare);
        shift += DIGIT;
    }
    keyed
}

/// What the gathering found in a buffer handed over unread.
#[derive(Debug, PartialEq, Eq)]
enum Gathered {
    /// The words stored, which the list held.
    Listed(Listed),
    /// Every word stored, all with this value: a clear.
    Whole(u32),
    /// More words stored than the list held, not all of them with one value.
    Overflowed,
}

/// What a list holds, read from its host copy - its count, its flag, the buffer's first word, then
/// the pairs - for a buffer of `words` words. Listed words come as the indices stored, ascending,
/// and their values in the same order; a list naming an index past the buffer is refused.
fn gathered(list: &[u32], words: usize) -> Result<Gathered, DispatchError> {
    let [count, differs, first, ref pairs @ ..] = *list else {
        return Err(DispatchError::Unsupported(
            "a list without its header".to_owned(),
        ));
    };
    let count = count as usize;
    // Stored whole with one value, whatever room the list had.
    if words > 0 && count == words && differs == 0 {
        return Ok(Gathered::Whole(first));
    }
    if count > pairs.len() / 2 {
        return Ok(Gathered::Overflowed);
    }
    // Each pair as one integer, its index above its value, so a plain sort orders them by index.
    let keyed = by_index(
        pairs[..count * 2]
            .chunks_exact(2)
            .map(|pair| u64::from(pair[0]) << 32 | u64::from(pair[1]))
            .collect(),
    );
    if keyed
        .last()
        .is_some_and(|&key| (key >> 32) as usize >= words)
    {
        return Err(DispatchError::Unsupported(
            "the device listed a word past its buffer".to_owned(),
        ));
    }
    Ok(Gathered::Listed(
        keyed
            .into_iter()
            .map(|key| ((key >> 32) as u32, key as u32))
            .unzip(),
    ))
}

/// The words a list held: their indices, ascending, and their values in the same order.
type Listed = (Vec<u32>, Vec<u32>);

/// One global memory barrier: `access` made available and visible across `stages`.
fn memory_barrier(
    device: &ash::Device,
    command: vk::CommandBuffer,
    (from, to): (vk::AccessFlags, vk::AccessFlags),
    (after, before): (vk::PipelineStageFlags, vk::PipelineStageFlags),
) {
    let barrier = [vk::MemoryBarrier::default()
        .src_access_mask(from)
        .dst_access_mask(to)];
    // SAFETY: recording is open and the barrier outlives the call.
    unsafe {
        device.cmd_pipeline_barrier(
            command,
            after,
            before,
            vk::DependencyFlags::empty(),
            &barrier,
            &[],
            &[],
        );
    }
}

#[cfg(test)]
mod tests {
    /// The radix order is a plain sort's, short lists and long, indices high and low.
    #[test]
    fn pairs_order_by_index_as_a_plain_sort_does() {
        for count in [0_u64, 5, 3000, 70_000] {
            // Distinct indices, scattered: a multiplier coprime to the count's range.
            let keyed: Vec<u64> = (0..count)
                .map(|i| ((i * 2_654_435_761) % (count * 7 + 1)) << 32 | (i ^ 0x5a5a))
                .collect();
            let mut sorted = keyed.clone();
            sorted.sort_unstable();
            assert_eq!(super::by_index(keyed), sorted, "{count} pairs");
        }
    }

    use ash::vk;

    use super::BoundBuffers;
    use crate::compute::{Availability, Session, probe, session};

    /// A finished dispatch's buffers are kept for the next of the same sizes (D805): the second
    /// dispatch binds the very buffers the first released, and its words are its own.
    #[test]
    fn a_dispatch_s_buffers_are_reused_by_the_next() {
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
        // A size no other test uses, so the spares hold none of it before.
        let words = 4099;
        let first = BoundBuffers::create(context, &[super::BufferIn::Words(&vec![1; words])])
            .expect("created")
            .expect("one buffer");
        let bound = first.slots[0].bound.0;
        first.destroy(device);
        let second = BoundBuffers::create(context, &[super::BufferIn::Words(&vec![2; words])])
            .expect("created")
            .expect("one buffer");
        let reused = second.slots[0].bound.0;
        let staged = crate::compute::read_back(device, &second.slots[0].staging);
        second.destroy(device);
        assert_eq!(reused, bound, "the bound buffer is the one released");
        assert_eq!(
            staged.expect("read").first(),
            Some(&2),
            "the second's own words"
        );
    }

    /// A large buffer handed over unread has its stored words gathered into a list on the device:
    /// stores that fit come back as the list alone, in order, and stores that overflow it come back
    /// as the buffer and its mask, whole. The stores are made by filling a stretch of the buffer
    /// and its mask, as a dispatch storing there would leave them.
    #[test]
    fn stored_words_come_back_listed_or_whole() {
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
            queue,
            family,
            ..
        } = *session;
        // Listed, with room for 8192 / 32 = 256 pairs: within it, past it; and a buffer gathered by
        // two rows of groups, its stores in the second row.
        let words = 8192;
        let row = (super::LIST_ROW_GROUPS * orbistoun_spirv::COMPACTION_GROUP) as usize;
        // And a clear: every word of a buffer of a size no other case uses stored with one value.
        let cleared = 3 * 4096;
        for (words, first, stretch) in [
            (cleared, 0_u32, cleared as u32),
            (words, 16_u32, 100_u32),
            (words, 16, 1000),
            (row + 512, row as u32 + 100, 200),
        ] {
            let bound = BoundBuffers::create(
                (instance, physical, device),
                &[super::BufferIn::Unread(words)],
            )
            .expect("created")
            .expect("one buffer");
            let slot = &bound.slots[0];
            let mask = slot.mask.as_ref().expect("a mask").bound.0;
            let buffer = slot.bound.0;
            run(device, (queue, family), |command| {
                bound.record_upload(device, command);
                let (offset, size) = (u64::from(first) * 4, u64::from(stretch) * 4);
                super::memory_barrier(
                    device,
                    command,
                    (
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::TRANSFER_WRITE,
                    ),
                    (
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::TRANSFER,
                    ),
                );
                // SAFETY: recording is open, and the mask is live and holds the stretch.
                unsafe { device.cmd_fill_buffer(command, mask, offset, size, 1) };
                // SAFETY: as above, for the buffer.
                unsafe { device.cmd_fill_buffer(command, buffer, offset, size, 0xABCD) };
                super::memory_barrier(
                    device,
                    command,
                    (
                        vk::AccessFlags::TRANSFER_WRITE,
                        vk::AccessFlags::SHADER_READ | vk::AccessFlags::TRANSFER_READ,
                    ),
                    (
                        vk::PipelineStageFlags::TRANSFER,
                        vk::PipelineStageFlags::COMPUTE_SHADER | vk::PipelineStageFlags::TRANSFER,
                    ),
                );
                bound.record_readback(device, command);
            });
            let out = bound.read(device, (queue, family)).expect("read");
            bound.destroy(device);
            let stored: (Vec<u32>, Vec<u32>) =
                (first..first + stretch).map(|i| (i, 0xABCD)).unzip();
            if stretch as usize == words {
                assert_eq!(out[0].whole, Some(0xABCD), "a clear: its value alone");
                assert_eq!(out[0].stored, None);
                assert!(out[0].words.is_empty() && out[0].written.is_empty());
            } else if stretch as usize <= words / super::LIST_SHARE {
                assert_eq!(
                    out[0].stored.as_ref(),
                    Some(&stored),
                    "listed: {words} words"
                );
                assert!(out[0].words.is_empty(), "and nothing else");
            } else {
                assert_eq!(out[0].stored, None, "overflowed");
                let range = first as usize..(first + stretch) as usize;
                assert!(out[0].words[range.clone()].iter().all(|&w| w == 0xABCD));
                assert!(out[0].written[range].iter().all(|&w| w == 1));
                assert_eq!(out[0].written[0], 0, "the mask whole, as before");
            }
        }
        // Having overflowed, a buffer of the size is read back whole for a while.
        let next = BoundBuffers::create(
            (instance, physical, device),
            &[super::BufferIn::Unread(words)],
        )
        .expect("created")
        .expect("one buffer");
        let listed = next.slots[0].list.is_some();
        next.destroy(device);
        assert!(!listed, "no list just after an overflow");
    }

    /// Records with `record`, submits, and waits.
    fn run(
        device: &ash::Device,
        (queue, family): (vk::Queue, u32),
        record: impl FnOnce(vk::CommandBuffer),
    ) {
        let pool_info = vk::CommandPoolCreateInfo::default().queue_family_index(family);
        // SAFETY: the family index is the session's.
        let pool = unsafe { device.create_command_pool(&pool_info, None) }.expect("pool");
        let allocate = vk::CommandBufferAllocateInfo::default()
            .command_pool(pool)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool is live.
        let command = unsafe { device.allocate_command_buffers(&allocate) }.expect("command")[0];
        // SAFETY: the command buffer is fresh.
        unsafe { device.begin_command_buffer(command, &vk::CommandBufferBeginInfo::default()) }
            .expect("begin");
        record(command);
        // SAFETY: recording is open.
        unsafe { device.end_command_buffer(command) }.expect("end");
        // SAFETY: the device is live.
        let fence =
            unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }.expect("fence");
        let commands = [command];
        let submits = [vk::SubmitInfo::default().command_buffers(&commands)];
        // SAFETY: the recording is complete and the fence fresh.
        unsafe { device.queue_submit(queue, &submits, fence) }.expect("submit");
        // SAFETY: the fence was submitted with the work above.
        unsafe { device.wait_for_fences(&[fence], true, u64::MAX) }.expect("wait");
        // SAFETY: the work has completed.
        unsafe { device.destroy_fence(fence, None) };
        // SAFETY: as above.
        unsafe { device.destroy_command_pool(pool, None) };
    }

    /// A dispatch's buffers live where the device reads them fastest. PPSA28061 runs 0x1fe000
    /// groups over one 32 MiB buffer each frame; held in host memory, every access crossed the bus
    /// and the dispatch took 400 ms.
    #[test]
    fn a_dispatch_s_buffers_are_device_local() {
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
        let bound = BoundBuffers::create(
            (instance, physical, device),
            &[super::BufferIn::Words(&[7; 64])],
        )
        .expect("created")
        .expect("one buffer");
        let placed = bound.placed_in();
        bound.destroy(device);
        assert!(
            placed
                .iter()
                .all(|flags| flags.contains(vk::MemoryPropertyFlags::DEVICE_LOCAL)),
            "memory properties {:x?}",
            placed
                .iter()
                .map(|flags| flags.as_raw())
                .collect::<Vec<_>>()
        );
    }
}
