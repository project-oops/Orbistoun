//! A compute dispatch's traced buffers (D746): each bound writable at its slot of the
//! draw-buffer set's compute binding, filled from the guest range its descriptor names, and read
//! back after the dispatch so the caller can write what changed to guest memory.

use ash::vk;
use orbistoun_spirv::{COMPUTE_BUFFERS_BINDING, DRAW_BUFFERS_PER_STAGE};

use crate::DispatchError;
use crate::compute::{DispatchBuffer, create_host_buffer, read_back};

/// A dispatch's buffers on the device, with the set that binds them.
pub(crate) struct BoundBuffers {
    /// One per slot the module declares; the slots past the dispatch's own hold one word each, so
    /// every element of the array is a valid buffer.
    buffers: Vec<DispatchBuffer>,
    /// How many of `buffers` are the dispatch's.
    used: usize,
    /// The set's layout, set one of the pipeline.
    pub(crate) layout: vk::DescriptorSetLayout,
    pool: vk::DescriptorPool,
    /// The set to bind.
    pub(crate) set: vk::DescriptorSet,
}

impl BoundBuffers {
    /// Uploads `contents`, one buffer per slot, and builds the set binding them; `None` for a
    /// dispatch that binds none.
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
        let mut buffers = Vec::with_capacity(DRAW_BUFFERS_PER_STAGE as usize);
        for slot in 0..DRAW_BUFFERS_PER_STAGE as usize {
            let words = contents.get(slot).map_or(&[0][..], |w| w.as_slice());
            let words = if words.is_empty() { &[0][..] } else { words };
            let size = (words.len() * 4) as vk::DeviceSize;
            let (buffer, memory) =
                create_host_buffer(instance, physical, device, size, words.len())?;
            // SAFETY: the memory is host-visible, holds `size` bytes and is not mapped.
            let mapped = unsafe { device.map_memory(memory, 0, size, vk::MemoryMapFlags::empty()) }
                .map_err(|e| DispatchError::Vulkan("map_memory", e))?;
            // SAFETY: the mapping covers `words.len()` whole words, and device memory and the
            // caller's slice cannot overlap.
            unsafe {
                std::ptr::copy_nonoverlapping(words.as_ptr(), mapped.cast::<u32>(), words.len());
            };
            // SAFETY: mapped immediately above and not used after unmapping.
            unsafe { device.unmap_memory(memory) };
            buffers.push(DispatchBuffer {
                buffer,
                memory,
                size,
                words: words.len(),
                offset: 0,
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
        let infos: Vec<vk::DescriptorBufferInfo> = buffers
            .iter()
            .map(|b| {
                vk::DescriptorBufferInfo::default()
                    .buffer(b.buffer)
                    .offset(0)
                    .range(b.size)
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
            buffers,
            used: contents.len(),
            layout,
            pool,
            set,
        }))
    }

    /// The dispatch's buffers as the dispatch left them, by slot.
    ///
    /// # Errors
    ///
    /// A device that refused a mapping.
    pub(crate) fn read(&self, device: &ash::Device) -> Result<Vec<Vec<u32>>, DispatchError> {
        self.buffers[..self.used]
            .iter()
            .map(|buffer| read_back(device, buffer))
            .collect()
    }

    /// Releases the buffers and the set, once the dispatch that used them has finished.
    pub(crate) fn destroy(self, device: &ash::Device) {
        // SAFETY: created here on this device, no longer in use, and destroyed exactly once.
        unsafe { device.destroy_descriptor_pool(self.pool, None) };
        // SAFETY: as above.
        unsafe { device.destroy_descriptor_set_layout(self.layout, None) };
        for buffer in self.buffers {
            // SAFETY: as above.
            unsafe { device.destroy_buffer(buffer.buffer, None) };
            // SAFETY: as above, and nothing is bound to the memory now.
            unsafe { device.free_memory(buffer.memory, None) };
        }
    }
}
