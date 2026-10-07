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

/// One slot: the buffer the set binds, and the host buffer its words are copied through.
struct Slot {
    bound: (vk::Buffer, vk::DeviceMemory),
    /// The memory properties `bound` was allocated with.
    #[cfg_attr(not(test), allow(dead_code))]
    placed: vk::MemoryPropertyFlags,
    staging: DispatchBuffer,
}

/// A buffer of `size` bytes for `usage`, in the first memory of `preferred` the device offers.
fn buffer_in(
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
            let (staging, staging_memory, _) = buffer_in(
                context,
                size,
                vk::BufferUsageFlags::TRANSFER_SRC | vk::BufferUsageFlags::TRANSFER_DST,
                STAGED_IN,
            )?;
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
            let (bound, bound_memory, placed) = buffer_in(
                context,
                size,
                vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST,
                BOUND_IN,
            )?;
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

    /// Releases the buffers and the set, once the dispatch that used them has finished.
    pub(crate) fn destroy(self, device: &ash::Device) {
        // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
        unsafe { device.destroy_descriptor_pool(self.pool, None) };
        // SAFETY: as above.
        unsafe { device.destroy_descriptor_set_layout(self.layout, None) };
        for slot in self.slots {
            for (buffer, memory) in [slot.bound, (slot.staging.buffer, slot.staging.memory)] {
                // SAFETY: as above.
                unsafe { device.destroy_buffer(buffer, None) };
                // SAFETY: as above, and nothing is bound to the memory now.
                unsafe { device.free_memory(memory, None) };
            }
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
