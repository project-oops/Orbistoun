//! A draw's buffers (D733): the guest bytes a draw's shaders read through, uploaded once per content
//! and bound in a descriptor set of their own, beside the one a pipeline owns, so a cached pipeline
//! serves every draw whatever buffers it binds.
//!
//! Buffers and sets are kept by content, and all released together - after every recorded draw has
//! run - when they pass their limits. A buffer is a range of a large mapped arena and a set comes
//! from a shared pool, so a frame of tens of thousands of draws makes no allocation per draw, and a
//! release rewinds the arenas and resets the pools rather than freeing them.

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

/// Sets kept before all are released: a GL frame binds a set of its own for most of its draws.
const SETS_LIMIT: usize = 32768;

/// Sets one descriptor pool holds; pools are added as sets are, up to [`SETS_LIMIT`].
const SETS_PER_POOL: u32 = 4096;

/// Bytes one arena holds; a buffer larger than that gets an arena of its own.
const ARENA_BYTES: u64 = 32 << 20;

/// The most arenas: the geometry binding holds them all, one element each (D747).
const ARENAS_MOST: usize = DRAW_BUFFERS_PER_STAGE as usize;

/// Where each of a draw's geometry buffers lies, by slot (D747): the arena at the geometry binding
/// it is a range of, its first word there, how many words it holds, and the low half of the guest
/// address it starts at (D758). Its draw's words carry them, so draws with different geometry
/// buffers share one set and batch.
pub(crate) type GeometryPlaces = [[u32; 4]; DRAW_BUFFERS_PER_STAGE as usize];

/// Where an uploaded buffer lies: its arena's buffer, and the range of it.
#[derive(Clone, Copy)]
struct Placed {
    buffer: vk::Buffer,
    offset: vk::DeviceSize,
    range: vk::DeviceSize,
}

/// A large host-visible buffer, mapped for its life, that uploaded buffers are ranges of.
struct Arena {
    held: DispatchBuffer,
    /// The mapping's address, kept as an integer so the cache can sit in a `static`.
    mapped: usize,
    used: vk::DeviceSize,
}

/// What the draw buffers hold on the device.
struct Held {
    layout: vk::DescriptorSetLayout,
    /// One word, bound at every slot a draw leaves empty: a binding a module declares must hold a
    /// valid buffer at every element, and an empty slot is never read.
    placeholder: DispatchBuffer,
    /// `minStorageBufferOffsetAlignment`, which every range's start keeps.
    alignment: vk::DeviceSize,
    arenas: Vec<Arena>,
    buffers: HashMap<(u64, usize), Placed>,
    bytes: usize,
    pools: Vec<vk::DescriptorPool>,
    /// Sets allocated from the last pool in use, and which pool that is.
    pool_sets: u32,
    pool_index: usize,
    sets: HashMap<[StageKey; 2], vk::DescriptorSet>,
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
        // SAFETY: the physical device belongs to the live instance.
        let alignment = unsafe {
            session
                .instance
                .get_physical_device_properties(session.physical)
        }
        .limits
        .min_storage_buffer_offset_alignment
        .max(4);
        *held = Some(Held {
            layout,
            placeholder,
            alignment,
            arenas: Vec::new(),
            buffers: HashMap::new(),
            bytes: 0,
            pools: Vec::new(),
            pool_sets: 0,
            pool_index: 0,
            sets: HashMap::new(),
        });
    }
    held.as_mut().ok_or_else(|| {
        DispatchError::Unsupported("the draw buffers were made and are not there".to_owned())
    })
}

/// Copies `bytes` into the first arena with room, at the device's alignment, padded with zeros to
/// whole words; an arena is added when none has room, sized for the buffer when it is the larger.
fn place(
    session: &Session,
    held: &mut Held,
    bytes: &[u8],
) -> Result<Option<Placed>, DispatchError> {
    let size = (bytes.len().div_ceil(4).max(1) * 4) as vk::DeviceSize;
    let alignment = held.alignment;
    let fits = |arena: &Arena| arena.used.next_multiple_of(alignment) + size <= arena.held.size;
    let index = if let Some(index) = held.arenas.iter().position(fits) {
        index
    } else if held.arenas.len() == ARENAS_MOST {
        return Ok(None);
    } else {
        {
            let capacity = ARENA_BYTES.max(size);
            let words = usize::try_from(capacity / 4).unwrap_or(usize::MAX);
            let (buffer, memory) = crate::compute::create_host_buffer(
                &session.instance,
                session.physical,
                &session.device,
                capacity,
                words,
            )?;
            // SAFETY: the memory is host-visible and coherent, just created and not mapped; it stays
            // mapped until the arena is freed.
            let mapped = unsafe {
                session
                    .device
                    .map_memory(memory, 0, capacity, vk::MemoryMapFlags::empty())
            }
            .map_err(|e| DispatchError::Vulkan("map_memory(draw buffer arena)", e))?;
            held.arenas.push(Arena {
                held: DispatchBuffer {
                    buffer,
                    memory,
                    size: capacity,
                    words,
                    offset: 0,
                },
                mapped: mapped as usize,
                used: 0,
            });
            held.arenas.len() - 1
        }
    };
    let arena = &mut held.arenas[index];
    let offset = arena.used.next_multiple_of(alignment);
    let at = usize::try_from(offset).unwrap_or(usize::MAX);
    let length = usize::try_from(size).unwrap_or(usize::MAX);
    // SAFETY: the arena is mapped for `held.size` bytes, and `offset` is within it.
    let start = unsafe { (arena.mapped as *mut u8).add(at) };
    // SAFETY: `offset + size` fits within the mapping; no recorded draw reads this range, which no
    // placed buffer covered since the last release.
    let destination = unsafe { std::slice::from_raw_parts_mut(start, length) };
    destination[..bytes.len()].copy_from_slice(bytes);
    destination[bytes.len()..].fill(0);
    arena.used = offset + size;
    Ok(Some(Placed {
        buffer: arena.held.buffer,
        offset,
        range: size,
    }))
}

/// A set from the pools: the current one's next, or a fresh pool's when it is full.
fn pooled_set(session: &Session, held: &mut Held) -> Result<vk::DescriptorSet, DispatchError> {
    let device = &session.device;
    if held.pool_sets == SETS_PER_POOL || held.pools.is_empty() {
        if held.pool_sets == SETS_PER_POOL {
            held.pool_index += 1;
            held.pool_sets = 0;
        }
        if held.pool_index == held.pools.len() {
            let sizes = [vk::DescriptorPoolSize::default()
                .ty(vk::DescriptorType::STORAGE_BUFFER)
                .descriptor_count(2 * DRAW_BUFFERS_PER_STAGE * SETS_PER_POOL)];
            let pool_info = vk::DescriptorPoolCreateInfo::default()
                .pool_sizes(&sizes)
                .max_sets(SETS_PER_POOL);
            // SAFETY: the create info outlives the call.
            let pool = unsafe { device.create_descriptor_pool(&pool_info, None) }
                .map_err(|e| DispatchError::Vulkan("create_descriptor_pool(draw buffers)", e))?;
            held.pools.push(pool);
        }
    }
    let layouts = [held.layout];
    let allocate = vk::DescriptorSetAllocateInfo::default()
        .descriptor_pool(held.pools[held.pool_index])
        .set_layouts(&layouts);
    // SAFETY: the pool has room for this set: fewer than its `SETS_PER_POOL` came from it.
    let set = unsafe { device.allocate_descriptor_sets(&allocate) }
        .map_err(|e| DispatchError::Vulkan("allocate_descriptor_sets(draw buffers)", e))?[0];
    held.pool_sets += 1;
    Ok(set)
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
) -> Result<(vk::DescriptorSet, GeometryPlaces), DispatchError> {
    if stages
        .iter()
        .any(|buffers| buffers.len() > DRAW_BUFFERS_PER_STAGE as usize)
    {
        return Err(DispatchError::Unsupported(
            "a stage binds more buffers than its binding holds (D733)".to_owned(),
        ));
    }
    let stage_key = |buffers: &[DrawBuffer]| {
        buffers
            .iter()
            .map(|buffer| (buffer.hash, buffer.bytes.len()))
            .collect::<StageKey>()
    };
    let mut guard = held()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let held = held_for(&mut guard, session)?;
    let incoming: usize = stages
        .iter()
        .flat_map(|b| b.iter())
        .map(|b| b.bytes.len())
        .sum();
    if held.bytes + incoming > BYTES_LIMIT || held.sets.len() >= SETS_LIMIT {
        crate::framebuffer::settle(session)?;
        release(&session.device, held);
    }
    // Every buffer into the arenas; when they are all taken, the frame so far runs and they are
    // rewound, once.
    if !place_all(session, held, stages)? {
        crate::framebuffer::settle(session)?;
        release(&session.device, held);
        if !place_all(session, held, stages)? {
            return Err(DispatchError::Unsupported(
                "a draw's buffers fill more arenas than the geometry binding holds (D747)"
                    .to_owned(),
            ));
        }
    }
    let mut places: GeometryPlaces = [[0; 4]; DRAW_BUFFERS_PER_STAGE as usize];
    for (place, buffer) in places.iter_mut().zip(stages[0]) {
        let Some(range) = held.buffers.get(&(buffer.hash, buffer.bytes.len())) else {
            continue;
        };
        let arena = held
            .arenas
            .iter()
            .position(|arena| arena.held.buffer == range.buffer)
            .unwrap_or(0);
        *place = [
            u32::try_from(arena).unwrap_or(0),
            u32::try_from(range.offset / 4).unwrap_or(u32::MAX),
            u32::try_from(range.range / 4).unwrap_or(u32::MAX),
            buffer.base as u32,
        ];
    }
    // The geometry binding is the arenas, so the set is the pixel stage's buffers' and how many
    // arenas there are (D747).
    let key = [
        vec![(held.arenas.len() as u64, usize::MAX)],
        stage_key(stages[1]),
    ];
    if let Some(&set) = held.sets.get(&key) {
        return Ok((set, places));
    }
    let set = allocate(session, held, &key)?;
    Ok((set, places))
}

/// Places every buffer of both stages not already placed; `false` when an arena was needed and the
/// geometry binding holds no more.
fn place_all(
    session: &Session,
    held: &mut Held,
    stages: [&[DrawBuffer]; 2],
) -> Result<bool, DispatchError> {
    for buffer in stages.iter().flat_map(|b| b.iter()) {
        let id = (buffer.hash, buffer.bytes.len());
        if !held.buffers.contains_key(&id) {
            let Some(placed) = place(session, held, &buffer.bytes)? else {
                return Ok(false);
            };
            held.bytes += buffer.bytes.len();
            held.buffers.insert(id, placed);
        }
    }
    Ok(true)
}

/// Allocates and writes the set for `key`, whose buffers are all uploaded.
fn allocate(
    session: &Session,
    held: &mut Held,
    key: &[StageKey; 2],
) -> Result<vk::DescriptorSet, DispatchError> {
    let device = &session.device;
    let set = pooled_set(session, held)?;
    // Every element of both bindings: the geometry binding's arenas (D747), and the pixel stage's
    // buffer where a slot has one; the placeholder elsewhere.
    let whole = |buffer: vk::Buffer| {
        vk::DescriptorBufferInfo::default()
            .buffer(buffer)
            .offset(0)
            .range(vk::WHOLE_SIZE)
    };
    let geometry: [vk::DescriptorBufferInfo; DRAW_BUFFERS_PER_STAGE as usize] =
        std::array::from_fn(|slot| {
            whole(
                held.arenas
                    .get(slot)
                    .map_or(held.placeholder.buffer, |arena| arena.held.buffer),
            )
        });
    let pixel: [vk::DescriptorBufferInfo; DRAW_BUFFERS_PER_STAGE as usize] = std::array::from_fn(
        |slot| match key[1].get(slot).and_then(|id| held.buffers.get(id)) {
            Some(placed) => vk::DescriptorBufferInfo::default()
                .buffer(placed.buffer)
                .offset(placed.offset)
                .range(placed.range),
            None => whole(held.placeholder.buffer),
        },
    );
    let infos = [geometry, pixel];
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
    held.sets.insert(key.clone(), set);
    Ok(set)
}

/// Releases every set and buffer: the pools are reset and the arenas rewound, kept for the next
/// frame's; the layout and placeholder stay. The caller has waited for every recorded draw.
fn release(device: &ash::Device, held: &mut Held) {
    for &pool in &held.pools {
        // SAFETY: no recorded draw uses a set from the pool any more.
        let _ =
            unsafe { device.reset_descriptor_pool(pool, vk::DescriptorPoolResetFlags::empty()) };
    }
    held.sets.clear();
    held.pool_sets = 0;
    held.pool_index = 0;
    held.buffers.clear();
    for arena in &mut held.arenas {
        arena.used = 0;
    }
    held.bytes = 0;
}
