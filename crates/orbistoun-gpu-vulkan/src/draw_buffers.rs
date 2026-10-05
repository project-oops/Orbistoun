//! A draw's buffers (D733): the guest bytes a draw's shaders read through, uploaded once per content
//! and bound in a descriptor set of their own, beside the one a pipeline owns, so a cached pipeline
//! serves every draw whatever buffers it binds.
//!
//! Buffers and sets are kept by content, and all released together - after every recorded draw has
//! run - when they pass their limits.

use std::collections::HashMap;

use ash::vk;
use orbistoun_gpu::DrawBuffer;
use orbistoun_spirv::{DRAW_BUFFERS_PER_STAGE, GEOMETRY_BUFFERS_BINDING, PIXEL_BUFFERS_BINDING};

use crate::compute::{DispatchBuffer, DispatchError, Session};

/// One stage's buffers, as a set is found by them: each slot's content hash and length.
type StageKey = Vec<(u64, usize)>;

/// Bytes of uploaded buffers kept before all are released: a frame's constants and vertices, with
/// room for a guest streaming new ones every frame.
const BYTES_LIMIT: usize = 512 << 20;

/// Sets kept before all are released.
const SETS_LIMIT: usize = 4096;

/// What the draw buffers hold on the device.
struct Held {
    layout: vk::DescriptorSetLayout,
    /// One word, bound at every slot a draw leaves empty: a binding a module declares must hold a
    /// valid buffer at every element, and an empty slot is never read.
    placeholder: DispatchBuffer,
    buffers: HashMap<(u64, usize), DispatchBuffer>,
    bytes: usize,
    sets: HashMap<[StageKey; 2], (vk::DescriptorPool, vk::DescriptorSet)>,
}

/// The process's one device's draw buffers.
fn held() -> &'static std::sync::Mutex<Option<Held>> {
    static HELD: std::sync::Mutex<Option<Held>> = std::sync::Mutex::new(None);
    &HELD
}

/// The layout of the draw-buffer set: the primitive shader's buffers at one binding, the pixel
/// shader's at the other, each an array of [`DRAW_BUFFERS_PER_STAGE`] storage buffers.
///
/// # Errors
///
/// When the device cannot bind that many storage buffers per stage beside the pipeline's own, or a
/// Vulkan call fails.
pub(crate) fn set_layout(session: &Session) -> Result<vk::DescriptorSetLayout, DispatchError> {
    let mut held = held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    Ok(held_for(&mut held, session)?.layout)
}

/// The draw buffers, made on first use.
fn held_for<'a>(
    held: &'a mut Option<Held>,
    session: &Session,
) -> Result<&'a mut Held, DispatchError> {
    if held.is_none() {
        let layout = create_layout(session)?;
        let placeholder = crate::upload_host_buffer(session, &[0; 4])
            .map_err(|e| DispatchError::Unsupported(format!("the draw buffer placeholder: {e}")))?;
        *held = Some(Held {
            layout,
            placeholder,
            buffers: HashMap::new(),
            bytes: 0,
            sets: HashMap::new(),
        });
    }
    held.as_mut().ok_or_else(|| {
        DispatchError::Unsupported("the draw buffers were made and are not there".to_owned())
    })
}

/// Storage buffers the pipeline's own set gives each stage: the observation window, the
/// guest-memory window and the draw data.
const PIPELINE_STORAGE_BUFFERS: u32 = 3;

/// Creates the set layout, refusing a device whose per-stage storage-buffer limit it passes.
fn create_layout(session: &Session) -> Result<vk::DescriptorSetLayout, DispatchError> {
    // SAFETY: the physical device belongs to the live instance.
    let limits = unsafe {
        session
            .instance
            .get_physical_device_properties(session.physical)
    }
    .limits;
    // The user-data block is pushed whole to every draw: a device taking fewer bytes cannot run
    // a stage's thirty-two user registers.
    if limits.max_push_constants_size < crate::framebuffer::USER_DATA_BLOCK_BYTES {
        return Err(DispatchError::Unsupported(format!(
            "the device takes {} push-constant bytes, and a draw's user data is {}",
            limits.max_push_constants_size,
            crate::framebuffer::USER_DATA_BLOCK_BYTES
        )));
    }
    if limits.max_per_stage_descriptor_storage_buffers
        < DRAW_BUFFERS_PER_STAGE + PIPELINE_STORAGE_BUFFERS
    {
        return Err(DispatchError::Unsupported(format!(
            "the device binds {} storage buffers per stage, and a draw's buffers need {} (D733)",
            limits.max_per_stage_descriptor_storage_buffers,
            DRAW_BUFFERS_PER_STAGE + PIPELINE_STORAGE_BUFFERS
        )));
    }
    let bindings = [
        vk::DescriptorSetLayoutBinding::default()
            .binding(GEOMETRY_BUFFERS_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(DRAW_BUFFERS_PER_STAGE)
            .stage_flags(vk::ShaderStageFlags::MESH_EXT),
        vk::DescriptorSetLayoutBinding::default()
            .binding(PIXEL_BUFFERS_BINDING)
            .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
            .descriptor_count(DRAW_BUFFERS_PER_STAGE)
            .stage_flags(vk::ShaderStageFlags::FRAGMENT),
    ];
    let info = vk::DescriptorSetLayoutCreateInfo::default().bindings(&bindings);
    // SAFETY: the create info outlives the call and the device is live.
    unsafe { session.device.create_descriptor_set_layout(&info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_set_layout(draw buffers)", e))
}

/// The set binding `stages` - the primitive shader's buffers, then the pixel shader's - made the
/// first time a draw binds them. Each buffer is uploaded the first time its content is seen.
///
/// Passing the limits releases every set and buffer first, once every recorded draw has run, so a
/// set in use is never destroyed.
///
/// # Errors
///
/// When a stage binds more buffers than its binding holds, or a Vulkan call fails.
pub(crate) fn descriptor_set(
    session: &Session,
    stages: [&[DrawBuffer]; 2],
) -> Result<vk::DescriptorSet, DispatchError> {
    if stages
        .iter()
        .any(|buffers| buffers.len() > DRAW_BUFFERS_PER_STAGE as usize)
    {
        return Err(DispatchError::Unsupported(
            "a stage binds more buffers than its binding holds (D733)".to_owned(),
        ));
    }
    let key = stages.map(|buffers| {
        buffers
            .iter()
            .map(|buffer| (buffer.hash, buffer.bytes.len()))
            .collect::<StageKey>()
    });
    let mut guard = held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let held = held_for(&mut guard, session)?;
    if let Some(&(_, set)) = held.sets.get(&key) {
        return Ok(set);
    }
    let incoming: usize = stages
        .iter()
        .flat_map(|b| b.iter())
        .map(|b| b.bytes.len())
        .sum();
    if held.bytes + incoming > BYTES_LIMIT || held.sets.len() >= SETS_LIMIT {
        crate::framebuffer::settle(session)?;
        release(&session.device, held);
    }
    for buffer in stages.iter().flat_map(|b| b.iter()) {
        let id = (buffer.hash, buffer.bytes.len());
        if !held.buffers.contains_key(&id) {
            let uploaded = crate::upload_host_buffer(session, &buffer.bytes)
                .map_err(|e| DispatchError::Unsupported(format!("a draw buffer: {e}")))?;
            held.bytes += buffer.bytes.len();
            held.buffers.insert(id, uploaded);
        }
    }
    let set = allocate(session, held, &key)?;
    Ok(set)
}

/// Allocates and writes the set for `key`, whose buffers are all uploaded.
fn allocate(
    session: &Session,
    held: &mut Held,
    key: &[StageKey; 2],
) -> Result<vk::DescriptorSet, DispatchError> {
    let device = &session.device;
    let sizes = [vk::DescriptorPoolSize::default()
        .ty(vk::DescriptorType::STORAGE_BUFFER)
        .descriptor_count(2 * DRAW_BUFFERS_PER_STAGE)];
    let pool_info = vk::DescriptorPoolCreateInfo::default()
        .pool_sizes(&sizes)
        .max_sets(1);
    // SAFETY: the create info outlives the call.
    let pool = unsafe { device.create_descriptor_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_descriptor_pool(draw buffers)", e))?;
    let layouts = [held.layout];
    let allocate = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(pool)
        .set_layouts(&layouts);
    // SAFETY: the pool has room for exactly this one set.
    let set = match unsafe { device.allocate_descriptor_sets(&allocate) } {
        Ok(sets) => sets[0],
        Err(e) => {
            // SAFETY: the pool was created above and holds no set.
            unsafe { device.destroy_descriptor_pool(pool, None) };
            return Err(DispatchError::Vulkan(
                "allocate_descriptor_sets(draw buffers)",
                e,
            ));
        }
    };
    // Every element of both bindings: a bound buffer where a slot has one, the placeholder
    // elsewhere.
    let infos: Vec<[vk::DescriptorBufferInfo; DRAW_BUFFERS_PER_STAGE as usize]> = key
        .iter()
        .map(|stage| {
            std::array::from_fn(|slot| {
                let buffer = stage
                    .get(slot)
                    .and_then(|id| held.buffers.get(id))
                    .unwrap_or(&held.placeholder);
                vk::DescriptorBufferInfo::default()
                    .buffer(buffer.buffer)
                    .offset(0)
                    .range(vk::WHOLE_SIZE)
            })
        })
        .collect();
    let writes: Vec<vk::WriteDescriptorSet<'_>> = [GEOMETRY_BUFFERS_BINDING, PIXEL_BUFFERS_BINDING]
        .into_iter()
        .zip(&infos)
        .map(|(binding, info)| {
            vk::WriteDescriptorSet::default()
                .dst_set(set)
                .dst_binding(binding)
                .descriptor_type(vk::DescriptorType::STORAGE_BUFFER)
                .buffer_info(info)
        })
        .collect();
    // SAFETY: the set is new and unused, and every buffer outlives the call.
    unsafe { device.update_descriptor_sets(&writes, &[]) };
    held.sets.insert(key.clone(), (pool, set));
    Ok(set)
}

/// Releases every set and buffer; the layout and placeholder stay. The caller has waited for every
/// recorded draw.
fn release(device: &ash::Device, held: &mut Held) {
    for (_, (pool, _)) in held.sets.drain() {
        // SAFETY: no recorded draw uses the set any more, and the pool is destroyed once.
        unsafe { device.destroy_descriptor_pool(pool, None) };
    }
    for (_, buffer) in held.buffers.drain() {
        // SAFETY: nothing in flight reads the buffer, and it is destroyed once.
        unsafe { device.destroy_buffer(buffer.buffer, None) };
        // SAFETY: the buffer bound to the memory is destroyed above.
        unsafe { device.free_memory(buffer.memory, None) };
    }
    held.bytes = 0;
}
