//! Device-local copies of the guest chunks device-addressed dispatches reach most (D974).
//!
//! A device-addressed access reaches guest memory across the bus: the imported pages are host
//! memory (D964). A hierarchy walk over them is bound by the bus's throughput for scattered
//! accesses, not by the lookup (D969). So a chunk a dispatch was measured to reach gets a copy in
//! device memory, and the table points the chunk at the copy: the modules do not change.
//!
//! The imported pages stay every byte's home, and a copy is clean between dispatches:
//!
//! - Before a dispatch, each copied run is marked in the host's write-watch (D824), and every chunk
//!   of it written since its last mark is copied in again from its home pages. A copy is made
//!   before the dispatch reads it, and a CPU write after the mark is seen at the next.
//! - After the dispatch, every copied chunk its stores marked is copied back to its home pages, on
//!   the device, before the dispatch is reported done, and given up; the command processor then
//!   treats it as any device write.
//! - A chunk the CPU wrote while a dispatch read its copy is one the CPU and the device share
//!   while a dispatch runs - a poll, a hand-off - and is dropped and never copied again.
//!
//! What is copied is measured, not guessed: a program's first dispatch, and every
//! [`RECORD_EVERY`]th after, records the chunks its lookups found (the touched marks), and those
//! are copied while the copies' memory lasts, the least recently touched given up first.

use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use ash::vk;

use super::{ENABLED, Imported, Mapped, TableMemory};
use crate::compute::DispatchError;
use orbistoun_gpu::device_addressed::Table;

thread_local! {
    /// Whether the dispatches this thread makes use the copies.
    static COPYING: Cell<bool> = const { Cell::new(true) };
}

/// Turns the copies on or off for the dispatches the calling thread makes from now on (D974): off,
/// they reach every chunk in its home pages, through a table that points at no copy, and record
/// nothing. Other threads' dispatches, and the copies themselves, are untouched. For measurement.
pub fn set_copying(on: bool) {
    COPYING.with(|copying| copying.set(on));
}

/// Whether the dispatches the calling thread makes use the copies.
pub(super) fn copying() -> bool {
    COPYING.with(Cell::get)
}

/// The most device memory the copies take, and the least worth taking.
const MOST_BYTES: u64 = 1 << 30;
/// See [`MOST_BYTES`].
const LEAST_BYTES: u64 = 64 << 20;

/// A program records what it touches at its first dispatch and every this many after it.
pub(super) const RECORD_EVERY: u64 = 16;

/// The device memory the copies lie in: one buffer, a slot of one chunk each.
struct Pool {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    address: u64,
    slots: u32,
    slot_bytes: u64,
    device: ash::Device,
}

impl Drop for Pool {
    fn drop(&mut self) {
        // SAFETY: made on this device for the copies alone, and dropped only under the mappings'
        // lock, which every dispatch reading them holds until its fence.
        unsafe { self.device.destroy_buffer(self.buffer, None) };
        // SAFETY: as above, and the buffer bound to it is gone.
        unsafe { self.device.free_memory(self.memory, None) };
    }
}

impl Pool {
    /// A pool of slots of `slot_bytes` in device-local memory, as large as [`MOST_BYTES`] or the
    /// largest the device grants down to [`LEAST_BYTES`].
    fn make(slot_bytes: u64) -> Option<Self> {
        let enabled = ENABLED.get()?;
        let mut bytes = MOST_BYTES;
        while bytes >= LEAST_BYTES {
            if let Some(pool) = Self::try_make(enabled, bytes, slot_bytes) {
                tracing::info!(
                    "device-addressed dispatches copy the guest chunks they reach most into {} MiB of device memory (D974)",
                    bytes >> 20
                );
                return Some(pool);
            }
            bytes /= 2;
        }
        None
    }

    fn try_make(enabled: &super::Enabled, bytes: u64, slot_bytes: u64) -> Option<Self> {
        let device = &enabled.device;
        let info = vk::BufferCreateInfo::default()
            .size(bytes)
            .usage(
                vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: the device is live and the create info outlives the call.
        let buffer = unsafe { device.create_buffer(&info, None) }.ok()?;
        // SAFETY: the buffer was created on this device and not yet destroyed.
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let memory = &enabled.memory;
        let local = vk::MemoryPropertyFlags::DEVICE_LOCAL;
        let fits = |i: u32| {
            let kind = memory.memory_types[i as usize];
            requirements.memory_type_bits & (1 << i) != 0
                && kind.property_flags.contains(local)
                && memory.memory_heaps[kind.heap_index as usize].size > requirements.size
        };
        // Memory the host cannot see first: the host never maps the copies.
        let chosen = (0..memory.memory_type_count)
            .find(|&i| {
                fits(i)
                    && !memory.memory_types[i as usize]
                        .property_flags
                        .contains(vk::MemoryPropertyFlags::HOST_VISIBLE)
            })
            .or_else(|| (0..memory.memory_type_count).find(|&i| fits(i)));
        let destroy = || {
            // SAFETY: created above and bound to nothing the device uses.
            unsafe { device.destroy_buffer(buffer, None) };
        };
        let Some(memory_type) = chosen else {
            destroy();
            return None;
        };
        let mut flags =
            vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type)
            .push_next(&mut flags);
        let _for = crate::device_memory::purpose("copies of guest memory");
        // SAFETY: a plain allocation of a type the buffer may take, of the size it needs.
        let Ok(allocation) = (unsafe { device.allocate_memory(&allocate, None) }) else {
            destroy();
            return None;
        };
        let mut pool = Self {
            buffer,
            memory: allocation,
            address: 0,
            slots: u32::try_from(bytes / slot_bytes).unwrap_or(u32::MAX),
            slot_bytes,
            device: device.clone(),
        };
        // SAFETY: buffer and memory come from this device, and the memory holds the buffer's
        // required size.
        unsafe { device.bind_buffer_memory(buffer, allocation, 0) }.ok()?;
        let info = vk::BufferDeviceAddressInfo::default().buffer(buffer);
        // SAFETY: the buffer was made with `SHADER_DEVICE_ADDRESS` and is bound to memory allocated
        // with `DEVICE_ADDRESS`, on a device created with `bufferDeviceAddress`.
        pool.address = unsafe { device.get_buffer_device_address(&info) };
        Some(pool)
    }

    /// Where slot `slot` starts in the pool's buffer.
    fn offset(&self, slot: u32) -> u64 {
        u64::from(slot) * self.slot_bytes
    }
}

/// One chunk's copy: its slot, whether it holds what the home pages hold, and the recording that
/// last found it touched.
#[derive(Debug, Clone, Copy)]
struct Copy {
    slot: u32,
    fresh: bool,
    used: u64,
}

/// The copies, and what decides them.
#[derive(Default)]
pub(super) struct Mirrors {
    pool: Option<Pool>,
    /// The chunk size the copies were made at; a table at another drops them all.
    shift: u32,
    /// Each copied chunk, by its first guest address.
    copies: BTreeMap<u64, Copy>,
    /// The pool's slots no copy holds.
    free: Vec<u32>,
    /// Each run of copied chunks, by base, with its length and the write-watch mark it was last
    /// marked with.
    marks: BTreeMap<u64, (u64, u64)>,
    /// Chunks the CPU wrote while a dispatch read their copy: never copied again.
    refused: BTreeSet<u64>,
    /// How many dispatches each program has made, by its module's hash.
    dispatches: HashMap<u64, u64>,
    /// The table as the mappings make it, with no chunk pointed at a copy.
    plain: Option<Table>,
    /// That table placed for the device, for a dispatch that does not use the copies; placed when
    /// one first asks.
    placed_plain: Option<TableMemory>,
    /// Whether the copies changed since the table was placed.
    changed: bool,
    /// How many recordings there have been.
    clock: u64,
    /// Whether the dispatch running records what it touches.
    recording: bool,
}

impl Mirrors {
    /// `table`, a fresh table over the mappings, with each copied chunk pointed at its copy. A copy
    /// whose chunk the table no longer holds, or every copy where the chunk size changed, is
    /// dropped: a copy is clean between dispatches, so dropping one loses nothing.
    pub(super) fn over(&mut self, mut table: Table) -> Table {
        if table.shift != self.shift {
            self.drop_all();
            self.shift = table.shift;
        }
        let gone: Vec<u64> = self
            .copies
            .keys()
            .copied()
            .filter(|&chunk| table.slot_of(chunk).is_none())
            .collect();
        for chunk in gone {
            self.give_up(chunk);
        }
        self.plain = Some(table.clone());
        self.placed_plain = None;
        self.redirect(&mut table);
        self.changed = false;
        table
    }

    /// Points each copied chunk of `table` at its copy.
    fn redirect(&self, table: &mut Table) {
        let Some(pool) = &self.pool else {
            return;
        };
        for (&chunk, copy) in &self.copies {
            table.redirect(chunk, pool.address + pool.offset(copy.slot));
        }
    }

    /// Drops every copy, after a dispatch that failed: what it left in them is not trusted.
    pub(super) fn forget(&mut self) {
        self.recording = false;
        self.drop_all();
    }

    fn drop_all(&mut self) {
        self.copies.clear();
        self.marks.clear();
        self.free = self
            .pool
            .as_ref()
            .map_or_else(Vec::new, |pool| (0..pool.slots).rev().collect());
        self.changed = true;
    }

    /// Drops `chunk`'s copy and frees its slot.
    fn give_up(&mut self, chunk: u64) {
        if let Some(copy) = self.copies.remove(&chunk) {
            self.free.push(copy.slot);
            self.changed = true;
        }
    }

    /// How many bytes the copies hold.
    pub(super) fn bytes(&self) -> u64 {
        self.copies.len() as u64 * (1 << self.shift)
    }

    /// The runs of consecutive copied chunks, as `(base, len)`.
    fn runs(&self) -> Vec<(u64, u64)> {
        let chunk_bytes = 1u64 << self.shift;
        let mut runs: Vec<(u64, u64)> = Vec::new();
        for &chunk in self.copies.keys() {
            match runs.last_mut() {
                Some((base, len)) if *base + *len == chunk => *len += chunk_bytes,
                _ => runs.push((chunk, chunk_bytes)),
            }
        }
        runs
    }
}

/// One copy between a chunk's home pages and its copy: the import's buffer and the offset in it,
/// and the pool slot's offset.
struct Transfer {
    import: vk::Buffer,
    at: u64,
    slot_at: u64,
}

impl Mapped {
    /// The table a dispatch runs over: the one pointing copied chunks at their copies where the
    /// calling thread uses them ([`set_copying`]), else the plain one, placed when first asked.
    ///
    /// # Errors
    ///
    /// No table, or a plain one the device refused.
    pub(super) fn dispatch_table(&mut self) -> Result<&mut TableMemory, DispatchError> {
        let none = || DispatchError::Unsupported("no device-address table".into());
        if copying() {
            return self.table.as_mut().ok_or_else(none);
        }
        if self.mirrors.placed_plain.is_none() {
            let plain = self.mirrors.plain.clone().ok_or_else(none)?;
            self.mirrors.placed_plain = Some(TableMemory::place(plain)?);
        }
        self.mirrors.placed_plain.as_mut().ok_or_else(none)
    }

    /// The import holding guest address `at`, and `at`'s offset in it.
    fn import_at(&self, at: u64) -> Option<(&Imported, u64)> {
        let (&base, import) = self.imports.range(..=at).next_back()?;
        (at < base.saturating_add(import.len())).then_some((import, at - base))
    }

    /// Before a dispatch of the program whose module hashes to `program` (D974): every copied run
    /// marked in the write-watch, and every chunk of it written since its last mark, or never
    /// copied, copied in from its home pages; the table placed again where the copies changed.
    /// Whether this dispatch records what it touches.
    ///
    /// # Errors
    ///
    /// A copy or a table the device refused.
    pub(super) fn copies_before(&mut self, program: u64) -> Result<bool, DispatchError> {
        let count = self.mirrors.dispatches.entry(program).or_insert(0);
        let recording = *count % RECORD_EVERY == 0;
        *count += 1;
        self.mirrors.recording = recording;
        let chunk_bytes = 1u64 << self.mirrors.shift;
        let mut stale = Vec::new();
        let mut unwatched = Vec::new();
        let mut marks = BTreeMap::new();
        for (base, len) in self.mirrors.runs() {
            let before = self
                .mirrors
                .marks
                .get(&base)
                .filter(|&&(held, _)| held == len)
                .map(|&(_, since)| since);
            // Marked first, so a write after the mark is the next dispatch's to find, and one
            // before it is found below.
            let Some(now) = orbistoun_mem::watch::mark(base, len) else {
                unwatched.push((base, len));
                continue;
            };
            marks.insert(base, (len, now));
            for chunk in (base..base + len).step_by(chunk_bytes as usize) {
                let copy = self.mirrors.copies[&chunk];
                // The mark just harvested the whole run, so each chunk is answered from the record.
                let written = before.is_none_or(|since| {
                    orbistoun_mem::watch::recorded_since(chunk, chunk_bytes, since)
                });
                if !copy.fresh || written {
                    stale.push(chunk);
                }
            }
        }
        self.mirrors.marks = marks;
        // A copy the host cannot watch cannot be kept coherent.
        for (base, len) in unwatched {
            for chunk in (base..base + len).step_by(chunk_bytes as usize) {
                self.mirrors.give_up(chunk);
                self.mirrors.refused.insert(chunk);
            }
        }
        self.transfer(&stale, true)?;
        for chunk in stale {
            if let Some(copy) = self.mirrors.copies.get_mut(&chunk) {
                copy.fresh = true;
            }
        }
        if self.mirrors.changed
            && let Some(plain) = self.mirrors.plain.clone()
        {
            let mut table = plain;
            self.mirrors.redirect(&mut table);
            self.table = None;
            self.table = Some(TableMemory::place(table)?);
            self.mirrors.changed = false;
        }
        Ok(recording)
    }

    /// After a dispatch (D974): every copied chunk in `written` - the chunks its stores marked -
    /// copied back to its home pages; every copied chunk the CPU wrote while it ran dropped and
    /// refused; and, where it recorded, the chunks it touched copied from the next dispatch on -
    /// `taken`, where the caller already took them from the marks for a capture (D980).
    ///
    /// # Errors
    ///
    /// A copy the device refused.
    pub(super) fn copies_after(
        &mut self,
        written: &[(u64, u64)],
        taken: Option<Vec<u64>>,
    ) -> Result<(), DispatchError> {
        let chunk_bytes = 1u64 << self.mirrors.shift;
        let back: Vec<u64> = written
            .iter()
            .flat_map(|&(base, len)| (base..base + len).step_by(chunk_bytes as usize))
            .filter(|chunk| self.mirrors.copies.contains_key(chunk))
            .collect();
        let mut shared = Vec::new();
        for (&base, &(len, since)) in &self.mirrors.marks {
            if orbistoun_mem::watch::written_since(base, len, since) == Some(false) {
                continue;
            }
            // The question above harvested the whole run: each chunk is answered from the record.
            for chunk in (base..base + len).step_by(chunk_bytes as usize) {
                if orbistoun_mem::watch::recorded_since(chunk, chunk_bytes, since) {
                    shared.push(chunk);
                }
            }
        }
        for chunk in back.iter().filter(|chunk| shared.contains(chunk)) {
            tracing::warn!(
                "the CPU and a device-addressed dispatch both wrote the guest chunk {chunk:#x} while it ran; the device's copy lands last (D974)"
            );
        }
        self.transfer(&back, false)?;
        // A chunk a dispatch stores into is copied back whole after it: measured, that cost a
        // scattered store more than the copy saved its loads, so it is given up, and copied again
        // only when a later recording finds it read alone.
        for &chunk in &back {
            self.mirrors.give_up(chunk);
        }
        for chunk in shared {
            self.mirrors.give_up(chunk);
            self.mirrors.refused.insert(chunk);
        }
        if self.mirrors.recording {
            self.mirrors.recording = false;
            if let Some(table) = self.table.as_mut() {
                let mut touched =
                    taken.unwrap_or_else(|| table.table.take_touched(table.marks.words_mut()));
                // A chunk the dispatch stored into would be copied back whole after every dispatch
                // like it: measured, that cost a scattered store more than the copy saved its
                // loads. Only what it read alone is copied.
                touched.retain(|chunk| {
                    !written
                        .iter()
                        .any(|&(base, len)| (base..base + len).contains(chunk))
                });
                self.admit(&touched);
            }
        }
        Ok(())
    }

    /// Whether the chunk starting at `chunk` is copied: a dispatch's stores into it land in its
    /// copy, and its home pages keep what they held until the copy-back.
    pub(super) fn copied(&self, chunk: u64) -> bool {
        self.mirrors.copies.contains_key(&chunk)
    }

    /// Copies the chunks of `touched` worth copying from the next dispatch on: each a mapping
    /// holds, not refused, while slots last, the least recently touched copy given up for one.
    pub(super) fn admit(&mut self, touched: &[u64]) {
        self.mirrors.clock += 1;
        let clock = self.mirrors.clock;
        if self.mirrors.pool.is_none() && !touched.is_empty() {
            self.mirrors.pool = Pool::make(1 << self.mirrors.shift);
            self.mirrors.free = self
                .mirrors
                .pool
                .as_ref()
                .map_or_else(Vec::new, |pool| (0..pool.slots).rev().collect());
        }
        if self.mirrors.pool.is_none() {
            return;
        }
        for &chunk in touched {
            if let Some(copy) = self.mirrors.copies.get_mut(&chunk) {
                copy.used = clock;
            }
        }
        for &chunk in touched {
            if self.mirrors.copies.contains_key(&chunk)
                || self.mirrors.refused.contains(&chunk)
                || self.import_at(chunk).is_none()
            {
                continue;
            }
            if self.mirrors.free.is_empty() {
                let oldest = self
                    .mirrors
                    .copies
                    .iter()
                    .filter(|(_, copy)| copy.used < clock)
                    .min_by_key(|(_, copy)| copy.used)
                    .map(|(&chunk, _)| chunk);
                let Some(oldest) = oldest else {
                    break;
                };
                self.mirrors.give_up(oldest);
            }
            let Some(slot) = self.mirrors.free.pop() else {
                break;
            };
            self.mirrors.copies.insert(
                chunk,
                Copy {
                    slot,
                    fresh: false,
                    used: clock,
                },
            );
            self.mirrors.changed = true;
        }
    }

    /// Copies each chunk of `chunks` between its home pages and its copy, on the device: into the
    /// copy where `inward`, back to the home pages otherwise. Waits for the device.
    fn transfer(&self, chunks: &[u64], inward: bool) -> Result<(), DispatchError> {
        let Some(pool) = &self.mirrors.pool else {
            return Ok(());
        };
        let mut transfers: Vec<Transfer> = Vec::new();
        for chunk in chunks {
            let (Some(copy), Some((import, at))) =
                (self.mirrors.copies.get(chunk), self.import_at(*chunk))
            else {
                continue;
            };
            transfers.push(Transfer {
                import: import.buffer,
                at,
                slot_at: pool.offset(copy.slot),
            });
        }
        if transfers.is_empty() {
            return Ok(());
        }
        copy_on_device(pool, &transfers, inward)
    }
}

/// Records and submits the copies `transfers` between imports and `pool`, inward or back, and
/// waits for them: barriers before and after, so the dispatch before them has finished writing
/// what they read, and what they write is what the next dispatch, or the host, reads.
fn copy_on_device(pool: &Pool, transfers: &[Transfer], inward: bool) -> Result<(), DispatchError> {
    let session = crate::compute::session()?;
    let session = session
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let device = &session.device;
    let pool_info = vk::CommandPoolCreateInfo::default()
        .flags(vk::CommandPoolCreateFlags::TRANSIENT)
        .queue_family_index(session.family);
    // SAFETY: the device is live and the create info outlives the call.
    let commands = unsafe { device.create_command_pool(&pool_info, None) }
        .map_err(|e| DispatchError::Vulkan("create_command_pool (copies)", e))?;
    let result = (|| {
        let allocate = vk::CommandBufferAllocateInfo::default()
            .command_pool(commands)
            .level(vk::CommandBufferLevel::PRIMARY)
            .command_buffer_count(1);
        // SAFETY: the pool was just created on this device.
        let command = unsafe { device.allocate_command_buffers(&allocate) }
            .map_err(|e| DispatchError::Vulkan("allocate_command_buffers (copies)", e))?[0];
        let begin = vk::CommandBufferBeginInfo::default()
            .flags(vk::CommandBufferUsageFlags::ONE_TIME_SUBMIT);
        // SAFETY: the command buffer was just allocated and is not recording.
        unsafe { device.begin_command_buffer(command, &begin) }
            .map_err(|e| DispatchError::Vulkan("begin_command_buffer (copies)", e))?;
        let before = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::MEMORY_WRITE)
            .dst_access_mask(vk::AccessFlags::TRANSFER_READ | vk::AccessFlags::TRANSFER_WRITE)];
        // SAFETY: recording is open; the barrier names no resource.
        unsafe {
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
                vk::PipelineStageFlags::TRANSFER,
                vk::DependencyFlags::empty(),
                &before,
                &[],
                &[],
            );
        }
        let bytes = pool.slot_bytes;
        let mut by_import: BTreeMap<u64, (vk::Buffer, Vec<vk::BufferCopy>)> = BTreeMap::new();
        for transfer in transfers {
            let region = if inward {
                vk::BufferCopy {
                    src_offset: transfer.at,
                    dst_offset: transfer.slot_at,
                    size: bytes,
                }
            } else {
                vk::BufferCopy {
                    src_offset: transfer.slot_at,
                    dst_offset: transfer.at,
                    size: bytes,
                }
            };
            by_import
                .entry(vk::Handle::as_raw(transfer.import))
                .or_insert_with(|| (transfer.import, Vec::new()))
                .1
                .push(region);
        }
        for (import, regions) in by_import.values() {
            let (from, to) = if inward {
                (*import, pool.buffer)
            } else {
                (pool.buffer, *import)
            };
            // SAFETY: recording is open; both buffers are live, every region lies inside both - a
            // whole chunk of an import that holds it, a whole slot of the pool - and none overlaps
            // another's destination.
            unsafe { device.cmd_copy_buffer(command, from, to, regions) };
        }
        let after = [vk::MemoryBarrier::default()
            .src_access_mask(vk::AccessFlags::TRANSFER_WRITE)
            .dst_access_mask(vk::AccessFlags::MEMORY_READ | vk::AccessFlags::HOST_READ)];
        // SAFETY: recording is open; the barrier names no resource.
        unsafe {
            device.cmd_pipeline_barrier(
                command,
                vk::PipelineStageFlags::TRANSFER,
                vk::PipelineStageFlags::ALL_COMMANDS | vk::PipelineStageFlags::HOST,
                vk::DependencyFlags::empty(),
                &after,
                &[],
                &[],
            );
        }
        // SAFETY: recording is open.
        unsafe { device.end_command_buffer(command) }
            .map_err(|e| DispatchError::Vulkan("end_command_buffer (copies)", e))?;
        // SAFETY: the device is live.
        let fence = unsafe { device.create_fence(&vk::FenceCreateInfo::default(), None) }
            .map_err(|e| DispatchError::Vulkan("create_fence (copies)", e))?;
        let buffers = [command];
        let submits = [vk::SubmitInfo::default().command_buffers(&buffers)];
        crate::device_lost::submit(crate::device_lost::Kind::Copy);
        // SAFETY: recording has ended, the queue belongs to this device, and the fence is unsignalled.
        let submitted = unsafe { device.queue_submit(session.queue, &submits, fence) }
            .map_err(crate::device_lost::vulkan("queue_submit (copies)"));
        let waited = submitted.and_then(|()| {
            // SAFETY: the fence was submitted just above.
            unsafe { device.wait_for_fences(&[fence], true, u64::MAX) }
                .map_err(crate::device_lost::vulkan("wait_for_fences (copies)"))
        });
        // SAFETY: the fence is signalled, or was never submitted.
        unsafe { device.destroy_fence(fence, None) };
        waited
    })();
    // SAFETY: the copies are finished, or never submitted, so nothing from the pool is in use.
    unsafe { device.destroy_command_pool(commands, None) };
    result
}
