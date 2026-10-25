//! Guest memory the device addresses directly (D964).
//!
//! Guest addresses are host addresses: guest code runs natively, so a guest range is a range of
//! this process's own memory. A range is imported into the device whole (`VK_EXT_external_memory_host`)
//! and bound to a buffer the device addresses by pointer (`bufferDeviceAddress`), so a module reads
//! and writes the guest's bytes where they lie, with nothing uploaded or written back. A guest
//! address `a` in a range imported from `base` is the device address `device_address + (a - base)`.
//!
//! The import is host-visible and host-coherent memory over the guest's own pages. What the guest
//! writes before a submission is what the device reads, and what the device writes is in the guest's
//! pages once the submission's fence has signalled. A device write passes no host page protection
//! and sets no host write-watch bit, so whoever caches guest memory is told of it apart (D964).

use std::sync::OnceLock;

use ash::vk;

use crate::compute::DispatchError;

mod mirror;
pub(crate) mod reached;
pub use mirror::set_copying;

/// How many bytes of guest memory device-addressed dispatches now reach in device-local copies
/// (D974).
#[must_use]
pub fn copied_bytes() -> u64 {
    locked().mirrors.bytes()
}

/// What the device enabled for direct addressing, once it opened with it.
struct Enabled {
    /// The device the imports are made on.
    device: ash::Device,
    /// The extension's own functions on that device.
    host: ash::ext::external_memory_host::Device,
    /// The alignment an imported range's base and length keep (`minImportedHostPointerAlignment`).
    alignment: u64,
    /// The device's memory types, to choose an import's from those it may take.
    memory: vk::PhysicalDeviceMemoryProperties,
}

impl Enabled {
    /// The memory types the host memory at `pointer` may be imported as.
    ///
    /// # Safety
    ///
    /// `pointer` is committed memory of this process.
    unsafe fn host_types(&self, pointer: *mut core::ffi::c_void) -> Result<u32, DispatchError> {
        let mut host_pointer = vk::MemoryHostPointerPropertiesEXT::default();
        // SAFETY: the function was loaded from this device; the pointer is committed memory of
        // this process, by the contract, and the output structure is a live local.
        let asked = unsafe {
            (self.host.fp().get_memory_host_pointer_properties_ext)(
                self.device.handle(),
                vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT,
                pointer.cast_const(),
                &raw mut host_pointer,
            )
        };
        if asked != vk::Result::SUCCESS {
            return Err(DispatchError::Vulkan(
                "get_memory_host_pointer_properties",
                asked,
            ));
        }
        Ok(host_pointer.memory_type_bits)
    }

    /// The first host-visible, host-coherent memory type among `allowed`, a mask of type indices.
    fn coherent_type(&self, allowed: u32) -> Option<u32> {
        let coherent =
            vk::MemoryPropertyFlags::HOST_VISIBLE | vk::MemoryPropertyFlags::HOST_COHERENT;
        (0..self.memory.memory_type_count).find(|&i| {
            allowed & (1 << i) != 0
                && self.memory.memory_types[i as usize]
                    .property_flags
                    .contains(coherent)
        })
    }
}

static ENABLED: OnceLock<Enabled> = OnceLock::new();

/// Whether the device offers what direct addressing needs (D964): the host-pointer import, buffer
/// device addresses, and the 64-bit integers a pointer is formed from among the core features
/// `wanted`.
pub(crate) fn offered(
    extensions: &[vk::ExtensionProperties],
    features: &vk::PhysicalDeviceVulkan12Features<'_>,
    wanted: &vk::PhysicalDeviceFeatures,
) -> bool {
    crate::compute::names(extensions, ash::ext::external_memory_host::NAME)
        && features.buffer_device_address == vk::TRUE
        && wanted.shader_int64 == vk::TRUE
}

/// Records whether the session's `device` was created with direct addressing, and what an import
/// on it needs.
pub(crate) fn note_enabled(
    enabled: bool,
    (instance, physical, device): (&ash::Instance, vk::PhysicalDevice, &ash::Device),
) {
    if !enabled {
        return;
    }
    let mut host_properties = vk::PhysicalDeviceExternalMemoryHostPropertiesEXT::default();
    let mut properties = vk::PhysicalDeviceProperties2::default().push_next(&mut host_properties);
    // SAFETY: the physical device is valid and the chained structure outlives the call.
    unsafe { instance.get_physical_device_properties2(physical, &mut properties) };
    // SAFETY: as above.
    let memory = unsafe { instance.get_physical_device_memory_properties(physical) };
    let _ = ENABLED.set(Enabled {
        device: device.clone(),
        host: ash::ext::external_memory_host::Device::new(instance, device),
        alignment: host_properties.min_imported_host_pointer_alignment.max(1),
        memory,
    });
    orbistoun_gpu::device_addressed::offer();
}

/// The alignment an imported range keeps, its base and its length, where the device addresses guest
/// memory directly; `None` where it cannot, or no device has opened.
#[must_use]
pub fn alignment() -> Option<u64> {
    ENABLED.get().map(|enabled| enabled.alignment)
}

/// A guest range imported into the device, addressed by pointer from its device address.
///
/// Released when dropped. It holds the device's own handle, not the session's lock, so dropping one
/// inside a dispatch is sound; dropping it while a submission that reads it is pending is not, and
/// whoever keeps one keeps it past that submission's fence.
pub struct Imported {
    base: u64,
    len: u64,
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    address: u64,
    device: ash::Device,
}

impl std::fmt::Debug for Imported {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Imported")
            .field("base", &format_args!("{:#x}", self.base))
            .field("len", &format_args!("{:#x}", self.len))
            .field("address", &format_args!("{:#x}", self.address))
            .finish_non_exhaustive()
    }
}

impl Imported {
    /// Imports the guest range `[base, base + len)` into the device.
    ///
    /// Both ends must keep [`alignment`]. Refused where the device does not address guest memory
    /// directly, or the driver will not take the range. A range not committed is not refused: the
    /// host's driver touches the pages it imports from the calling thread, and a reserved page
    /// faults there, so the caller commits first.
    ///
    /// # Safety
    ///
    /// Every page of the range must be memory this process has committed, and stay so - neither
    /// released nor remapped - until the import is dropped and no submission that reads it is still
    /// pending. The device reads and writes the pages behind the host's page protection.
    ///
    /// # Errors
    ///
    /// No direct addressing, a misaligned range, or a step the driver refused.
    pub unsafe fn import(base: u64, len: u64) -> Result<Self, DispatchError> {
        let enabled = ENABLED.get().ok_or_else(|| {
            DispatchError::Unsupported(
                "the device does not address guest memory directly (D964)".to_owned(),
            )
        })?;
        if len == 0 || base % enabled.alignment != 0 || len % enabled.alignment != 0 {
            return Err(DispatchError::Unsupported(format!(
                "a guest range {base:#x}+{len:#x} does not keep the import alignment {:#x}",
                enabled.alignment
            )));
        }
        let pointer = usize::try_from(base).map_err(|_| {
            DispatchError::Unsupported(format!("{base:#x} does not fit a host pointer"))
        })?;
        let pointer = std::ptr::with_exposed_provenance_mut::<core::ffi::c_void>(pointer);
        let device = &enabled.device;
        let handle_type = vk::ExternalMemoryHandleTypeFlags::HOST_ALLOCATION_EXT;
        // SAFETY: the caller's contract: the range is committed memory of this process.
        let host_types = unsafe { enabled.host_types(pointer) }?;

        let mut external = vk::ExternalMemoryBufferCreateInfo::default().handle_types(handle_type);
        let buffer_info = vk::BufferCreateInfo::default()
            .size(len)
            .usage(
                vk::BufferUsageFlags::STORAGE_BUFFER
                    | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS
                    | vk::BufferUsageFlags::TRANSFER_SRC
                    | vk::BufferUsageFlags::TRANSFER_DST,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE)
            .push_next(&mut external);
        // SAFETY: the device is live and the create info and its chain outlive the call.
        let buffer = unsafe { device.create_buffer(&buffer_info, None) }
            .map_err(|e| DispatchError::Vulkan("create_buffer", e))?;
        let destroy_buffer = |error: DispatchError| {
            // SAFETY: created just above on this device and bound to nothing the device uses.
            unsafe { device.destroy_buffer(buffer, None) };
            error
        };

        // SAFETY: the buffer was created on this device and not yet destroyed.
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        if requirements.size > len {
            return Err(destroy_buffer(DispatchError::Unsupported(format!(
                "a buffer over {len:#x} imported bytes needs {:#x}",
                requirements.size
            ))));
        }
        let Some(memory_type) = enabled.coherent_type(requirements.memory_type_bits & host_types)
        else {
            return Err(destroy_buffer(DispatchError::Unsupported(
                "no host-coherent memory type takes an imported guest range".to_owned(),
            )));
        };

        let mut import = vk::ImportMemoryHostPointerInfoEXT::default()
            .handle_type(handle_type)
            .host_pointer(pointer);
        let mut flags =
            vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(len)
            .memory_type_index(memory_type)
            .push_next(&mut import)
            .push_next(&mut flags);
        let _for = crate::device_memory::purpose("guest memory addressed directly");
        // SAFETY: the allocation imports the caller's committed range, which stays committed while
        // the import lives; its size and base keep the device's import alignment.
        let memory = unsafe { device.allocate_memory(&allocate, None) }
            .map_err(|e| destroy_buffer(DispatchError::Vulkan("allocate_memory (import)", e)))?;
        // SAFETY: buffer and memory come from this device, and the memory holds the buffer's
        // required size.
        if let Err(e) = unsafe { device.bind_buffer_memory(buffer, memory, 0) } {
            // SAFETY: allocated just above and bound to nothing.
            unsafe { device.free_memory(memory, None) };
            return Err(destroy_buffer(DispatchError::Vulkan(
                "bind_buffer_memory",
                e,
            )));
        }
        let info = vk::BufferDeviceAddressInfo::default().buffer(buffer);
        // SAFETY: the buffer was made with `SHADER_DEVICE_ADDRESS` and is bound to memory allocated
        // with `DEVICE_ADDRESS`, on a device created with `bufferDeviceAddress`.
        let address = unsafe { device.get_buffer_device_address(&info) };
        // The device stores into these pages behind the host's protection, and a store into a page
        // a write-watch guard made read-only lost the device: none stands over them while they are
        // imported (D974).
        orbistoun_mem::watch::forbid_guards(base, len);
        Ok(Self {
            base,
            len,
            buffer,
            memory,
            address,
            device: device.clone(),
        })
    }

    /// The guest address the range starts at.
    #[must_use]
    pub const fn base(&self) -> u64 {
        self.base
    }

    /// Its length in bytes.
    #[must_use]
    pub const fn len(&self) -> u64 {
        self.len
    }

    /// Whether it holds nothing; never, since an empty range is refused.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The device address its first byte has.
    #[must_use]
    pub const fn device_address(&self) -> u64 {
        self.address
    }

    /// The device address of guest address `at`, where the range holds it.
    #[must_use]
    pub fn device_address_of(&self, at: u64) -> Option<u64> {
        let offset = at.checked_sub(self.base).filter(|&o| o < self.len)?;
        Some(self.address + offset)
    }
}

impl Drop for Imported {
    fn drop(&mut self) {
        orbistoun_mem::watch::allow_guards(self.base, self.len);
        // SAFETY: both were made on this device for this import alone; the holder keeps the import
        // past every submission that reads it, so nothing pending uses them.
        unsafe { self.device.destroy_buffer(self.buffer, None) };
        // SAFETY: as above, and the buffer bound to it is gone.
        unsafe { self.device.free_memory(self.memory, None) };
    }
}

/// Whether `module` reaches guest memory by device address: it declares the capability to form
/// pointers from device addresses, which only a device-addressed dispatch's module does (D964).
#[must_use]
pub fn addresses_directly(module: &[u32]) -> bool {
    // The capabilities lead the module, after its five-word header, two words each.
    let mut at = 5;
    while let Some(&word) = module.get(at) {
        if word != (2 << 16 | u32::from(orbistoun_spirv::op::CAPABILITY)) {
            return false;
        }
        if module.get(at + 1)
            == Some(&orbistoun_spirv::capability::PHYSICAL_STORAGE_BUFFER_ADDRESSES)
        {
            return true;
        }
        at += 2;
    }
    false
}

/// Words in host memory the device addresses: a fresh allocation keeping the import alignment,
/// imported as any guest range is. The host reads and writes them at host-memory speed; the device
/// at the bus's.
struct HostWords {
    /// The import over the words; dropped before the words are freed.
    import: Option<Imported>,
    /// The words' allocation, by address, and its layout.
    words: (usize, std::alloc::Layout),
    /// How many words.
    len: usize,
}

impl HostWords {
    /// `words` placed in fresh memory keeping the import alignment, and imported.
    fn place(words: &[u32]) -> Result<Self, DispatchError> {
        let alignment = alignment().ok_or_else(|| {
            DispatchError::Unsupported("the device does not address guest memory directly".into())
        })?;
        let alignment = alignment.max(4096);
        let bytes = (words.len().max(1) as u64 * 4).next_multiple_of(alignment);
        let layout = usize::try_from(bytes)
            .ok()
            .zip(usize::try_from(alignment).ok())
            .and_then(|(size, align)| std::alloc::Layout::from_size_align(size, align).ok())
            .ok_or_else(|| DispatchError::Unsupported("a device-address table too large".into()))?;
        // SAFETY: the layout's size is not zero: it holds one word at the least.
        let pointer = unsafe { std::alloc::alloc_zeroed(layout) };
        if pointer.is_null() {
            return Err(DispatchError::Unsupported(
                "no memory for the device-address table".into(),
            ));
        }
        let address = pointer.expose_provenance();
        let mut placed = Self {
            import: None,
            words: (address, layout),
            len: words.len(),
        };
        placed.words_mut().copy_from_slice(words);
        // SAFETY: the allocation is committed memory of this process, kept until `self` drops, and
        // `Drop` releases the import first; its base and length keep the import alignment.
        placed.import = Some(unsafe { Imported::import(address as u64, bytes) }?);
        Ok(placed)
    }

    /// The words as they lie in their memory.
    fn words_mut(&mut self) -> &mut [u32] {
        let pointer = std::ptr::with_exposed_provenance_mut::<u32>(self.words.0);
        // SAFETY: the allocation holds at least `len` words, is aligned past four bytes, and is
        // this value's alone; the device writes it only while a dispatch the caller waits for
        // runs, never while this borrow lives.
        unsafe { std::slice::from_raw_parts_mut(pointer, self.len) }
    }

    /// The device address of the first word.
    fn device_address(&self) -> Option<u64> {
        self.import.as_ref().map(Imported::device_address)
    }
}

impl Drop for HostWords {
    fn drop(&mut self) {
        drop(self.import.take());
        let (address, layout) = self.words;
        // SAFETY: allocated in `place` with this layout, and nothing imports it any more.
        unsafe { std::alloc::dealloc(std::ptr::with_exposed_provenance_mut(address), layout) };
    }
}

/// Words in the device's own memory, written once through a host mapping and then only read: a
/// buffer in memory that is device-local and host-visible, which the device reads at its own
/// memory's speed and the host writes across the bus.
struct DeviceWords {
    buffer: vk::Buffer,
    memory: vk::DeviceMemory,
    address: u64,
    device: ash::Device,
}

impl DeviceWords {
    /// `words` placed in device-local, host-visible memory; `None` where the device has no such
    /// memory with room for them, or refused a step.
    fn place(words: &[u32]) -> Option<Self> {
        let enabled = ENABLED.get()?;
        let device = &enabled.device;
        let bytes = (words.len().max(1) as u64) * 4;
        let buffer_info = vk::BufferCreateInfo::default()
            .size(bytes)
            .usage(
                vk::BufferUsageFlags::STORAGE_BUFFER | vk::BufferUsageFlags::SHADER_DEVICE_ADDRESS,
            )
            .sharing_mode(vk::SharingMode::EXCLUSIVE);
        // SAFETY: the device is live and the create info outlives the call.
        let buffer = unsafe { device.create_buffer(&buffer_info, None) }.ok()?;
        let destroy_buffer = || {
            // SAFETY: created just above on this device and bound to nothing the device uses.
            unsafe { device.destroy_buffer(buffer, None) };
        };
        // SAFETY: the buffer was created on this device and not yet destroyed.
        let requirements = unsafe { device.get_buffer_memory_requirements(buffer) };
        let wanted = vk::MemoryPropertyFlags::DEVICE_LOCAL
            | vk::MemoryPropertyFlags::HOST_VISIBLE
            | vk::MemoryPropertyFlags::HOST_COHERENT;
        let memory = &enabled.memory;
        let Some(memory_type) = (0..memory.memory_type_count).find(|&i| {
            let kind = memory.memory_types[i as usize];
            requirements.memory_type_bits & (1 << i) != 0
                && kind.property_flags.contains(wanted)
                && memory.memory_heaps[kind.heap_index as usize].size >= requirements.size
        }) else {
            destroy_buffer();
            return None;
        };
        let mut flags =
            vk::MemoryAllocateFlagsInfo::default().flags(vk::MemoryAllocateFlags::DEVICE_ADDRESS);
        let allocate = vk::MemoryAllocateInfo::default()
            .allocation_size(requirements.size)
            .memory_type_index(memory_type)
            .push_next(&mut flags);
        let _for = crate::device_memory::purpose("the device-address table");
        // SAFETY: a plain allocation of a type the buffer may take, of the size it needs.
        let Ok(allocation) = (unsafe { device.allocate_memory(&allocate, None) }) else {
            destroy_buffer();
            return None;
        };
        let placed = Self {
            buffer,
            memory: allocation,
            address: 0,
            device: device.clone(),
        };
        // SAFETY: buffer and memory come from this device, and the memory holds the buffer's
        // required size.
        unsafe { device.bind_buffer_memory(buffer, allocation, 0) }.ok()?;
        // SAFETY: host-visible memory, mapped once here and unmapped before anything reads it.
        let mapped = unsafe {
            device.map_memory(allocation, 0, vk::WHOLE_SIZE, vk::MemoryMapFlags::empty())
        }
        .ok()?;
        // SAFETY: the mapping holds at least `bytes`, aligned past four bytes (a mapping keeps
        // `minMemoryMapAlignment`), and nothing else touches it while it is written.
        unsafe {
            std::ptr::copy_nonoverlapping(words.as_ptr(), mapped.cast::<u32>(), words.len());
        }
        // SAFETY: mapped just above; host-coherent, so the writes need no flush.
        unsafe { device.unmap_memory(allocation) };
        let mut placed = placed;
        let info = vk::BufferDeviceAddressInfo::default().buffer(buffer);
        // SAFETY: the buffer was made with `SHADER_DEVICE_ADDRESS` and is bound to memory allocated
        // with `DEVICE_ADDRESS`, on a device created with `bufferDeviceAddress`.
        placed.address = unsafe { device.get_buffer_device_address(&info) };
        Some(placed)
    }
}

impl Drop for DeviceWords {
    fn drop(&mut self) {
        // SAFETY: both were made on this device for this table alone, and the table is dropped
        // only under the mappings' lock, which every dispatch reading it holds until its fence.
        unsafe { self.device.destroy_buffer(self.buffer, None) };
        // SAFETY: as above, and the buffer bound to it is gone.
        unsafe { self.device.free_memory(self.memory, None) };
    }
}

/// Where a table's words lie: in the device's own memory where it has device-local memory the host
/// can write, else in host memory imported as a guest range is.
enum TableWords {
    Device(DeviceWords),
    Host(HostWords),
}

/// The device-address table in memory the device reads it from, and the written marks the device's
/// stores set beside it (D964). The table is only read on the device, a lookup's probes on every
/// access, so it lies in device memory where it can; the marks are only written on the device and
/// read on the host after the dispatch, so they lie in host memory, where the device's write is
/// one the bus posts and the host's scan reads its own memory.
struct TableMemory {
    table: orbistoun_gpu::device_addressed::Table,
    words: TableWords,
    marks: HostWords,
}

impl TableMemory {
    /// `table`'s words placed for the device, and its marks, cleared.
    fn place(table: orbistoun_gpu::device_addressed::Table) -> Result<Self, DispatchError> {
        let words = match DeviceWords::place(&table.words) {
            Some(words) => TableWords::Device(words),
            None => TableWords::Host(HostWords::place(&table.words)?),
        };
        let marks = HostWords::place(&vec![0; table.mark_words()])?;
        Ok(Self {
            table,
            words,
            marks,
        })
    }

    /// The push-constant words that name the table and its marks.
    fn push_words(
        &self,
    ) -> Option<[u32; orbistoun_gpu::device_addressed::DEVICE_TABLE_PUSH_WORDS as usize]> {
        let address = match &self.words {
            TableWords::Device(words) => words.address,
            TableWords::Host(words) => words.device_address()?,
        };
        Some(
            self.table
                .push_words(address, self.marks.device_address()?, false),
        )
    }

    /// The guest ranges the device's stores marked since the last take, the marks cleared.
    fn take_written(&mut self) -> Vec<(u64, u64)> {
        self.table.take_written(self.marks.words_mut())
    }
}

/// The guest runs taken up, by base, with their lengths; the pieces of them the device maps, by
/// guest base; the pieces of them it does not, by base, with their lengths; the table over those it
/// maps; the generation of the runs they were made from and the host pages' generation the pieces
/// left out were last looked at in (`orbistoun_mem::mappings`, D968).
#[derive(Default)]
struct Mapped {
    generation: Option<u64>,
    pages: Option<u64>,
    runs: std::collections::BTreeMap<u64, u64>,
    imports: std::collections::BTreeMap<u64, Imported>,
    left_out: std::collections::BTreeMap<u64, u64>,
    table: Option<TableMemory>,
    /// The device-local copies of the chunks dispatches reach most (D974).
    mirrors: mirror::Mirrors,
}

/// The span a piece left out is mapped in once any of its pages is committed (D968): its pages
/// are committed and mapped this many bytes around the page at a time, so the guest touching its
/// memory a commit at a time does not make an import of each.
const LEFT_OUT_CELL: u64 = 2 << 20;

/// The parts of the run `(base, len)` that none of `held` covers, as `(base, len)` in order:
/// `held` is `(start, end)` pairs inside the run, in order and apart.
fn gaps((base, len): (u64, u64), held: &[(u64, u64)]) -> Vec<(u64, u64)> {
    let end = base.saturating_add(len);
    let mut at = base;
    let mut gaps = Vec::new();
    for &(start, stop) in held {
        if start > at {
            gaps.push((at, start - at));
        }
        at = at.max(stop);
    }
    if end > at {
        gaps.push((at, end - at));
    }
    gaps
}

/// The `[start, end)` a committed piece `(from, len)` of the gap `(base, len)` is mapped in: the
/// [`LEFT_OUT_CELL`]s it touches, cut to the gap.
fn cell_window((base, len): (u64, u64), (from, piece): (u64, u64)) -> (u64, u64) {
    let start = (from / LEFT_OUT_CELL * LEFT_OUT_CELL).max(base);
    let end = from
        .saturating_add(piece)
        .next_multiple_of(LEFT_OUT_CELL)
        .min(base.saturating_add(len));
    (start, end)
}

/// The bytes of the guest pages an escape names and a capture keeps (D971).
const GUEST_PAGE: u64 = 1 << orbistoun_spirv::DISPATCH_ESCAPE_PAGE_SHIFT;

/// The window an escape at `page` commits so the next dispatch maps it (D971): the
/// [`LEFT_OUT_CELL`] around it, cut to the piece left out that holds it; `None` where none does,
/// and the page lies outside every run the device maps.
fn escape_window(left_out: &std::collections::BTreeMap<u64, u64>, page: u64) -> Option<(u64, u64)> {
    let (&base, &len) = left_out.range(..=page).next_back()?;
    (page < base.saturating_add(len)).then(|| cell_window((base, len), (page, GUEST_PAGE)))
}

/// The guest pages a device-addressed dispatch's user data names (D971): for each word and the
/// next, bits 47:0 of the address they form, its page and the page after it, where `holds` says a
/// mapping holds them, in order and once each. The words `skip` covers - the device table's - are
/// not user data. A capture keeps these pages, the memory a dispatch's descriptors and pointers
/// start from.
fn named_pages(
    push: &[u32],
    skip: std::ops::Range<usize>,
    holds: impl Fn(u64) -> bool,
) -> Vec<u64> {
    let mut pages = std::collections::BTreeSet::new();
    for (index, pair) in push.windows(2).enumerate() {
        if skip.contains(&index) || skip.contains(&(index + 1)) {
            continue;
        }
        let at = u64::from(pair[0]) | (u64::from(pair[1] & 0xFFFF) << 32);
        if at == 0 {
            continue;
        }
        let page = at & !(GUEST_PAGE - 1);
        for page in [page, page + GUEST_PAGE] {
            if holds(page) {
                pages.insert(page);
            }
        }
    }
    pages.into_iter().collect()
}

fn mapped() -> &'static std::sync::Mutex<Mapped> {
    static MAPPED: OnceLock<std::sync::Mutex<Mapped>> = OnceLock::new();
    MAPPED.get_or_init(|| std::sync::Mutex::new(Mapped::default()))
}

fn locked() -> std::sync::MutexGuard<'static, Mapped> {
    mapped()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// Drops the device's imports over `[base, base + len)` before its host memory is released:
/// called by `orbistoun_mem` with the release waiting, and waiting itself for any dispatch reading
/// them, which holds the lock.
fn release(base: u64, len: u64) {
    let end = base.saturating_add(len);
    let mut mapped = locked();
    let before = (mapped.imports.len(), mapped.runs.len());
    mapped
        .imports
        .retain(|&at, import| at.saturating_add(import.len()) <= base || end <= at);
    mapped
        .runs
        .retain(|&at, &mut len| at.saturating_add(len) <= base || end <= at);
    mapped
        .left_out
        .retain(|&at, &mut len| at.saturating_add(len) <= base || end <= at);
    if (mapped.imports.len(), mapped.runs.len()) != before {
        mapped.generation = None;
    }
}

impl Mapped {
    /// Brings the imports and the table up to the guest's readable runs, where they changed: each
    /// run committed first, then its pieces of committed, private, read-write host pages imported,
    /// and a run gone or changed dropped. A page outside those pieces - one a guard holds
    /// read-only, say - is left out, as is a piece the driver will not take, and what reaches
    /// either is an escape. A driver given any other page to import may fault inside the call.
    ///
    /// A piece left out is looked at again whenever the host pages' generation has moved: a page
    /// of it committed on touch since, or given another protection, is mapped then (D968).
    fn sync(
        &mut self,
    ) -> Result<
        [u32; orbistoun_gpu::device_addressed::DEVICE_TABLE_PUSH_WORDS as usize],
        DispatchError,
    > {
        let alignment = alignment().ok_or_else(|| {
            DispatchError::Unsupported("the device does not address guest memory directly".into())
        })?;
        orbistoun_mem::mappings::on_release(release);
        // Read before anything is committed below, so a commit made meanwhile is looked at next time.
        let pages = orbistoun_mem::mappings::pages_generation();
        let mut remapped = self.table.is_none();
        if self.generation != Some(orbistoun_mem::mappings::generation()) || self.table.is_none() {
            let (generation, runs) = orbistoun_mem::mappings::readable_runs();
            let wanted: std::collections::BTreeMap<u64, u64> = runs
                .into_iter()
                .filter(|&(base, len)| base % alignment == 0 && len % alignment == 0)
                .collect();
            self.runs.retain(|base, len| wanted.get(base) == Some(len));
            let runs = &self.runs;
            self.imports.retain(|&at, import| {
                runs.range(..=at).next_back().is_some_and(|(&base, &len)| {
                    at.saturating_add(import.len()) <= base.saturating_add(len)
                })
            });
            self.left_out.retain(|&at, &mut piece| {
                runs.range(..=at).next_back().is_some_and(|(&base, &len)| {
                    at.saturating_add(piece) <= base.saturating_add(len)
                })
            });
            let (mut taken, mut left_out) = (0u64, 0u64);
            for (&base, &len) in &wanted {
                if self.runs.contains_key(&base) {
                    continue;
                }
                // The driver touches every page it maps, so none may be left to commit on touch;
                // and it maps only writable pages, so the write-watch's own read-only guards are
                // released first, their pages noted written (D824). A guard set again later holds
                // only the host's writes: the device's pass it by, as they always do here.
                orbistoun_mem::watch::release_overlapping(base, len);
                let mut mapped_here = 0;
                let mut held = Vec::new();
                for (from, piece) in orbistoun_mem::lazy::writable_runs(base, len) {
                    let start = from.next_multiple_of(alignment);
                    let end = (from + piece) / alignment * alignment;
                    if end <= start {
                        continue;
                    }
                    // SAFETY: the piece lies in a reservation the guest holds, and every page of
                    // it is committed, private and read-write, as just asked of the host; the
                    // release hook drops the import, waiting for this lock, before the
                    // reservation's memory goes.
                    match unsafe { Imported::import(start, end - start) } {
                        Ok(import) => {
                            mapped_here += end - start;
                            held.push((start, end));
                            self.imports.insert(start, import);
                        }
                        Err(why) => {
                            tracing::debug!(
                                "guest memory {start:#x}+{:#x} is not mapped: {why:?}",
                                end - start
                            );
                        }
                    }
                }
                taken += mapped_here;
                left_out += len - mapped_here;
                self.left_out.extend(gaps((base, len), &held));
                self.runs.insert(base, len);
            }
            if left_out > 0 {
                tracing::info!(
                    "the device maps {} MiB more of guest memory and leaves out {} MiB whose host pages are not committed, private and read-write, or that the driver refused (D964)",
                    taken >> 20,
                    left_out >> 20
                );
            }
            self.generation = Some(generation);
            remapped = true;
        }
        if self.pages != Some(pages) {
            remapped |= self.map_left_out(alignment);
            self.pages = Some(pages);
        }
        if remapped {
            let runs: Vec<orbistoun_gpu::device_addressed::Mapped> = self
                .imports
                .values()
                .map(|import| orbistoun_gpu::device_addressed::Mapped {
                    guest: import.base(),
                    len: import.len(),
                    device: import.device_address(),
                })
                .collect();
            let table = orbistoun_gpu::device_addressed::Table::build(&runs)
                .map_err(|why| DispatchError::Unsupported(why.to_string()))?;
            self.table = None;
            self.table = Some(TableMemory::place(self.mirrors.over(table))?);
        }
        self.table
            .as_ref()
            .and_then(TableMemory::push_words)
            .ok_or_else(|| DispatchError::Unsupported("no device-address table".into()))
    }

    /// Whether an import holds guest address `at`.
    fn holds(&self, at: u64) -> bool {
        self.imports
            .range(..=at)
            .next_back()
            .is_some_and(|(&base, import)| at < base.saturating_add(import.len()))
    }

    /// After a dispatch an access of which escaped at the guest page `page` (D971): where a piece
    /// left out holds it, the cell around it is committed, which the next sync maps; this dispatch
    /// is not run again, since what it stored and combined has landed. Whether the page was in a
    /// piece left out.
    fn commit_escaped(&self, page: u64) -> bool {
        let Some((start, end)) = escape_window(&self.left_out, page) else {
            return false;
        };
        orbistoun_mem::lazy::commit(start, end - start);
        true
    }

    /// Maps what has become mappable of the pieces left out (D968): each committed page of one,
    /// with the rest of its [`LEFT_OUT_CELL`] committed beside it and the write-watch's guards on
    /// them released as a new run's are, then its committed, private, read-write pages imported.
    /// Whether any was.
    fn map_left_out(&mut self, alignment: u64) -> bool {
        let mut windows: Vec<(u64, u64)> = Vec::new();
        for (&base, &len) in &self.left_out {
            for piece in orbistoun_mem::lazy::committed_runs(base, len) {
                let (start, end) = cell_window((base, len), piece);
                match windows.last_mut() {
                    Some((_, last_end)) if *last_end >= start => *last_end = (*last_end).max(end),
                    _ => windows.push((start, end)),
                }
            }
        }
        let mut mapped = 0u64;
        for (window_start, window_end) in windows {
            orbistoun_mem::watch::release_overlapping(window_start, window_end - window_start);
            let writable =
                orbistoun_mem::lazy::writable_runs(window_start, window_end - window_start);
            for (from, piece) in writable {
                let start = from.next_multiple_of(alignment);
                let end = (from + piece) / alignment * alignment;
                if end <= start {
                    continue;
                }
                // SAFETY: the piece lies in a run the guest holds, in a piece of it no import
                // holds, and every page of it is committed, private and read-write, as just asked
                // of the host; the release hook drops the import before the memory goes.
                match unsafe { Imported::import(start, end - start) } {
                    Ok(import) => {
                        mapped += end - start;
                        self.imports.insert(start, import);
                    }
                    Err(why) => {
                        tracing::debug!(
                            "guest memory {start:#x}+{:#x} is not mapped: {why:?}",
                            end - start
                        );
                    }
                }
            }
        }
        if mapped == 0 {
            return false;
        }
        let before = std::mem::take(&mut self.left_out);
        for (base, len) in before {
            let held: Vec<(u64, u64)> = self
                .imports
                .range(base..base.saturating_add(len))
                .map(|(&start, import)| (start, start.saturating_add(import.len())))
                .collect();
            self.left_out.extend(gaps((base, len), &held));
        }
        tracing::debug!(
            "the device maps {} KiB more of guest memory committed since its run was mapped (D968)",
            mapped >> 10
        );
        true
    }
}

/// Runs a device-addressed dispatch (D964): guest memory's readable runs mapped into the device and
/// the table over them named in the push block, the dispatch run with the mappings held so none is
/// released under it, and the chunks its stores marked handed to the command processor
/// (`orbistoun_gpu::device_addressed::note_written`).
///
/// # Errors
///
/// No direct addressing, a table that cannot be built, or the dispatch's own.
pub(crate) fn dispatch(
    module: (&[u32], u64),
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    images: &crate::compute_images::DispatchImages<'_>,
    buffers: &[crate::compute_buffers::BufferIn<'_>],
) -> Result<crate::compute::GuestDispatch, DispatchError> {
    run_mapped((push, module.1), true, |push, pages| {
        crate::observed::dispatch(module, memory, (push, groups), images, buffers, pages)
    })
}

/// [`dispatch`] without the run's diagnostics: what a replay of a captured device-addressed
/// dispatch runs (D971), over the guest pages it placed where they were.
pub(crate) fn replay(
    module: (&[u32], u64),
    memory: &[u32],
    (push, groups): (&[u32], [u32; 3]),
    images: &crate::compute_images::DispatchImages<'_>,
    buffers: &[crate::compute_buffers::BufferIn<'_>],
) -> Result<crate::compute::GuestDispatch, DispatchError> {
    run_mapped((push, module.1), false, |push, _| {
        crate::compute::run_guest_dispatch(module, memory, (push, groups), images, buffers)
    })
}

/// Runs `run` with the mappings synced and held, handing it the push block with the table named
/// and the guest pages the user data names; then hands the chunks its stores marked to the command
/// processor, and commits the page an escape names where a piece left out holds it (D971).
///
/// The device-local copies of guest chunks are brought up to date before it, and the chunks it
/// stored into copied back after it, before anything is told of them (D974). `program` names the
/// dispatch's module, by which what it touches is recorded.
///
/// Where `observing` - a run's dispatch, not a replay's - and its program's capture is asked for, it
/// records what it touches too, and the capture is written after it with every chunk it reached
/// ([`reached`], D980).
fn run_mapped(
    (push, program): (&[u32], u64),
    observing: bool,
    run: impl FnOnce(&[u32], &[u64]) -> Result<crate::compute::GuestDispatch, DispatchError>,
) -> Result<crate::compute::GuestDispatch, DispatchError> {
    use orbistoun_gpu::perf::{Span, span};
    let mut mapped = locked();
    span(Span::DirectSync, || mapped.sync())?;
    let copying = mirror::copying();
    let recording = if copying {
        span(Span::DirectCopies, || mapped.copies_before(program))?
    } else {
        false
    };
    let capturing = observing && reached::begin(crate::device_lost::noted_compute());
    let words = mapped
        .dispatch_table()?
        .push_words()
        .ok_or_else(|| DispatchError::Unsupported("no device-address table".into()))?;
    let at = orbistoun_gpu::device_addressed::DEVICE_TABLE_PUSH_WORD as usize;
    let mut push = push.to_vec();
    if push.len() < at + words.len() {
        push.resize(at + words.len(), 0);
    }
    push[at..at + words.len()].copy_from_slice(&words);
    // The last of the table's words: whether this dispatch records what it touches.
    push[at + words.len() - 1] = u32::from(recording || capturing);
    let pages = named_pages(&push, at..at + words.len(), |page| mapped.holds(page));
    let done = run(&push, &pages);
    let (written, touched, chunk_bytes) = match mapped.dispatch_table() {
        Ok(table) => {
            let written = table.take_written();
            let touched = capturing.then(|| table.table.take_touched(table.marks.words_mut()));
            (written, touched, 1u64 << table.table.shift)
        }
        Err(_) => (Vec::new(), None, GUEST_PAGE),
    };
    // The capture is written before any copy goes back, while each copied chunk's home pages
    // still hold what they held before the dispatch.
    let wait = observing
        .then(|| {
            reached::after(
                (
                    &written,
                    touched.as_deref().unwrap_or_default(),
                    chunk_bytes,
                ),
                |chunk| copying && mapped.copied(chunk),
                copying && done.is_ok(),
            )
        })
        .flatten();
    let copied_back = if !copying {
        Ok(())
    } else if done.is_ok() {
        span(Span::DirectCopies, || {
            mapped.copies_after(&written, touched)
        })
        .map(|()| {
            // A capture waiting for the next dispatch has every chunk this one reached copied
            // for it, the chunks it stored into among them.
            if let Some(chunks) = &wait {
                mapped.admit(chunks);
            }
        })
    } else {
        // What a dispatch that failed left in the copies is not copied back; neither is it kept.
        mapped.mirrors.forget();
        Ok(())
    };
    orbistoun_gpu::device_addressed::note_written(&written);
    copied_back?;
    if let Ok(after) = &done
        && let Some(page) = after.escaped_page
    {
        if mapped.commit_escaped(page) {
            tracing::info!(
                "a device-addressed access escaped at the guest page {page:#x}, which no mapping held; committed, the next dispatch maps it (D971)"
            );
        } else {
            tracing::debug!(
                "a device-addressed access escaped at the guest page {page:#x}, outside every run the device maps (D971)"
            );
        }
    }
    done
}

#[cfg(test)]
mod tests {
    use super::{GUEST_PAGE, LEFT_OUT_CELL, cell_window, escape_window, gaps, named_pages};

    /// An escape in a piece left out commits the cell around it, cut to the piece; one outside every
    /// piece commits nothing.
    #[test]
    fn an_escape_commits_the_cell_around_it_in_its_piece() {
        let left_out =
            std::collections::BTreeMap::from([(LEFT_OUT_CELL + GUEST_PAGE, 4 * GUEST_PAGE)]);
        assert_eq!(
            escape_window(&left_out, LEFT_OUT_CELL + 2 * GUEST_PAGE),
            Some((LEFT_OUT_CELL + GUEST_PAGE, LEFT_OUT_CELL + 5 * GUEST_PAGE))
        );
        assert_eq!(escape_window(&left_out, LEFT_OUT_CELL), None, "before it");
        assert_eq!(
            escape_window(&left_out, LEFT_OUT_CELL + 5 * GUEST_PAGE),
            None,
            "past it"
        );
    }

    /// The pages user data names are those its word pairs address that a mapping holds, with the
    /// page after each, once each; the device table's words are not user data.
    #[test]
    fn user_data_names_the_pages_its_addresses_lie_in() {
        let held = 0x7400_9ce7_0000_u64;
        let holds = |page: u64| (held..held + 2 * GUEST_PAGE).contains(&page);
        let push = [
            0x9ce7_a424,
            0x0014_7400,
            0x9ce7_6040,
            0x0004_7400,
            0,
            0,
            0x9ce7_0000,
            0x7400,
        ];
        let both = vec![held, held + GUEST_PAGE];
        assert_eq!(named_pages(&push, 6..8, holds), both);
        assert_eq!(named_pages(&push, 0..0, holds), both);
        assert_eq!(named_pages(&push[4..], 0..0, |_| false), Vec::<u64>::new());
    }

    /// A run's gaps are what its imports leave, at either end and between them.
    #[test]
    fn a_run_s_gaps_are_what_its_imports_leave() {
        assert_eq!(
            gaps((0x1_0000, 0x4_0000), &[(0x2_0000, 0x3_0000)]),
            vec![(0x1_0000, 0x1_0000), (0x3_0000, 0x2_0000)]
        );
        assert_eq!(gaps((0x1_0000, 0x1_0000), &[(0x1_0000, 0x2_0000)]), vec![]);
        assert_eq!(gaps((0x1_0000, 0x1_0000), &[]), vec![(0x1_0000, 0x1_0000)]);
    }

    /// A committed piece is mapped in the cells it touches, never past its gap.
    #[test]
    fn a_committed_piece_is_mapped_in_its_cells_within_its_gap() {
        let gap = (LEFT_OUT_CELL - 0x1_0000, 3 * LEFT_OUT_CELL);
        assert_eq!(
            cell_window(gap, (LEFT_OUT_CELL + 0x8_0000, 0x1_0000)),
            (LEFT_OUT_CELL, 2 * LEFT_OUT_CELL),
            "the one cell it lies in"
        );
        assert_eq!(
            cell_window(gap, (LEFT_OUT_CELL - 0x1_0000, 0x1_0000)),
            (LEFT_OUT_CELL - 0x1_0000, LEFT_OUT_CELL),
            "cut to the gap's start"
        );
        assert_eq!(
            cell_window(gap, (3 * LEFT_OUT_CELL, 0x1_0000)),
            (3 * LEFT_OUT_CELL, 4 * LEFT_OUT_CELL - 0x1_0000),
            "cut to the gap's end"
        );
    }
}
