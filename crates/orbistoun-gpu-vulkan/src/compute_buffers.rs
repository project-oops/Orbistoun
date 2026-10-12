//! A compute dispatch's traced buffers (D746): each bound writable at its slot of the
//! draw-buffer set's compute binding, filled from the guest range its descriptor names, and read
//! back after the dispatch so the caller can write what changed to guest memory.

use ash::vk;
use orbistoun_spirv::{COMPUTE_BUFFERS_BINDING, DRAW_BUFFERS_PER_STAGE};

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
    new_buffer_in(context, size, usage, preferred)
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

/// A dispatch's buffers on the device, with the set that binds them.
pub(crate) struct BoundBuffers {
    /// One per slot the module declares; the slots past the dispatch's own hold one word each, so
    /// every element of the array is a valid buffer.
    slots: Vec<Slot>,
    /// How many of `slots` are the dispatch's.
    used: usize,
    /// The set's layout, set one of the pipeline.
    pub(crate) layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    /// The set to bind.
    pub(crate) set: vk::DescriptorSet,
}

impl BoundBuffers {
    /// Stages `contents`, one buffer per slot, and builds the set binding them; `None` for a
    /// dispatch that binds none. The words reach the bound buffers through
    /// [`Self::record_upload`].
    ///
    /// # Errors
    ///
    /// More buffers than a module declares, or a device that refused a step.
    pub(crate) fn create(
        (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
        contents: &[Vec<u32>],
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
        let mut slots = Vec::with_capacity(DRAW_BUFFERS_PER_STAGE as usize);
        for slot in 0..DRAW_BUFFERS_PER_STAGE as usize {
            let words = contents.get(slot).map_or(&[0][..], |w| w.as_slice());
            let words = if words.is_empty() { &[0][..] } else { words };
            let size = (words.len() * 4) as vk::DeviceSize;
            let (staging, staging_memory, _) = buffer_in(context, size, STAGING_USAGE, STAGED_IN)?;
            // SAFETY: the memory is host-visible, holds `size` bytes and is not mapped.
            let mapped =
                unsafe { device.map_memory(staging_memory, 0, size, vk::MemoryMapFlags::empty()) }
                    .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
            // SAFETY: the mapping covers `words.len()` whole words, and device memory and the
            // caller's slice cannot overlap.
            unsafe {
                std::ptr::copy_nonoverlapping(words.as_ptr(), mapped.cast::<u32>(), words.len());
            };
            // SAFETY: mapped immediately above and not used after unmapping.
            unsafe { device.unmap_memory(staging_memory) };
            let (bound, bound_memory, placed) = buffer_in(context, size, BOUND_USAGE, BOUND_IN)?;
            slots.push(Slot {
                bound: (bound, bound_memory),
                placed,
                staging: DispatchBuffer {
                    buffer: staging,
                    memory: staging_memory,
                    size,
                    words: words.len(),
                    offset: 0,
                },
            });
        }

        let bindings = [vk::DescriptorSetLayoutBinding::default()
            .binding(COMPUTE_BUFFERS_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(DRAW_BUFFERS_PER_STAGE)
            .stage_flags(vk::ShaderStageFlags::COMPUTE)];
        let layout_info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
        // SAFETY: the create info outlives the call.
        let layout = unsafe { device.create_descriptor_set_layout(&layout_info, None) }
            .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout", e))?;
        let pool_sizes = [vk::DescriptorPoolSize::default()
            .ty(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(DRAW_BUFFERS_PER_STAGE)];
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
        let infos: Vec<vk::DescriptorBufferInfo> = slots
            .iter()
            .map(|slot| {
                vk::DescriptorBufferInfo::default()
                    .buffer(slot.bound.0)
                    .offset(0)
                    .range(slot.staging.size)
            })
            .collect();
        let writes = [vk::WriteDescriptorSet::default()
            .dst_set(set)
            .dst_binding(COMPUTE_BUFFERS_BINDING)
            .dst_array_element(0)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .buffer_info(&infos)];
        // SAFETY: the write refers to a live set and live buffers, and the slices outlive the call.
        unsafe { device.update_descriptor_sets(&writes, &[]) };
        Ok(Some(Self {
            slots,
            used: contents.len(),
            layout,
            pool,
            set,
        }))
    }

    /// Records each slot's copy from its staging buffer to the buffer the set binds, ordered before
    /// the dispatch, into an open recording.
    pub(crate) fn record_upload(&self, device: &ash::Device, command: vk::CommandBuffer) {
        for slot in &self.slots {
            let region = [vk::BufferCopy::default().size(slot.staging.size)];
            // SAFETY: recording is open, and both buffers are live and at least `size` bytes.
            unsafe { device.cmd_copy_buffer(command, slot.staging.buffer, slot.bound.0, &region) };
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

    /// Records the dispatch's own buffers' copy back to their staging buffers, after its stores and
    /// before the host reads them, into an open recording.
    pub(crate) fn record_readback(&self, device: &ash::Device, command: vk::CommandBuffer) {
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
        for slot in &self.slots[..self.used] {
            let region = [vk::BufferCopy::default().size(slot.staging.size)];
            // SAFETY: recording is open, and both buffers are live and at least `size` bytes.
            unsafe { device.cmd_copy_buffer(command, slot.bound.0, slot.staging.buffer, &region) };
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

    /// The dispatch's buffers as the dispatch left them, by slot, once the recording that copied
    /// them back has completed.
    ///
    /// # Errors
    ///
    /// A device that refused a mapping.
    pub(crate) fn read(&self, device: &ash::Device) -> Result<Vec<Vec<u32>>, DispatchError> {
        self.slots[..self.used]
            .iter()
            .map(|slot| read_back(device, &slot.staging))
            .collect()
    }

    /// The memory properties each of the dispatch's own buffers was allocated with.
    #[cfg(test)]
    fn placed_in(&self) -> Vec<vk::MemoryPropertyFlags> {
        self.slots[..self.used]
            .iter()
            .map(|slot| slot.placed)
            .collect()
    }

    /// Releases the set, and the buffers to the spares (D805), once the dispatch that used them
    /// has finished.
    pub(crate) fn destroy(self, device: &ash::Device) {
        // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
        unsafe { device.destroy_descriptor_pool(self.pool, None) };
        // SAFETY: as above.
        unsafe { device.destroy_descriptor_set_layout(self.layout, None) };
        for slot in self.slots {
            let size = slot.staging.size;
            release(
                device,
                slot.bound,
                (size, BOUND_USAGE, BOUND_IN, slot.placed),
            );
            release(
                device,
                (slot.staging.buffer, slot.staging.memory),
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
        let first = BoundBuffers::create(context, &[vec![1; words]])
            .expect("created")
            .expect("one buffer");
        let bound = first.slots[0].bound.0;
        first.destroy(device);
        let second = BoundBuffers::create(context, &[vec![2; words]])
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
        let bound = BoundBuffers::create((instance, physical, device), &[vec![7; 64]])
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
