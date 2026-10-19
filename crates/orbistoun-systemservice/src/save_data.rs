//! `libSceSaveData_native` - save data.
//!
//! `sceSaveDataInitialize3` answers as obSCEne measured it through a static import (`-d7a1`, sweep
//! 20260927-204316, check `130-layout/savedata-layout`), and `sceSaveDataSetupSaveDataMemory2` as
//! `-5d20` measured it (sweep 20260928-100713, check `130-layout/savedata-memory-5d20`), and
//! `sceSaveDataGetSaveDataMemory2` before set-up as the census measured it. `sceSaveDataMount3` and
//! `sceSaveDataUmount2` mount a save from the title's own storage (D792, assumed). The other names
//! are declared and unserved.
//!
//! The other arities are `6`, the trampoline's full capture, not a claim about how many arguments
//! a function takes: a wrong arity only degrades a trace, while a wrong name is unreachable.

use std::sync::atomic::{AtomicBool, Ordering};

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError};
use orbistoun_hle::guest_module;
use orbistoun_mem::guest;

guest_module! {
    "libSceSaveData_native" {
        "sceSaveDataGetSaveDataMemory2" => 6,
        "sceSaveDataInitialize3" => 1,
        // (param, result)
        "sceSaveDataMount3" => 2,
        "sceSaveDataSetSaveDataMemory2" => 6,
        "sceSaveDataSetupSaveDataMemory2" => 6,
        "sceSaveDataSyncSaveDataMemory" => 6,
        // (mode, mount point)
        "sceSaveDataUmount2" => 2,
        // (condition, result)
        "sceSaveDataDirNameSearch" => 2,
        // (parameter)
        "sceSaveDataDelete" => 1,
        // (memory size)
        "sceSaveDataCreateTransactionResource" => 1,
        // (mount point, parameter)
        "sceSaveDataPrepare" => 2,
        // (parameter)
        "sceSaveDataCommit" => 1,
        // (resource)
        "sceSaveDataDeleteTransactionResource" => 1,
        // (mount point, type, buffer, size)
        "sceSaveDataSetParam" => 4,
        // (mount point, path)
        "sceSaveDataSaveIconByPath" => 2,
    }
}

/// What `sceSaveDataInitialize3` answered a zeroed 64-byte parameter block, before the library was
/// initialised and after it was terminated alike (measured).
const ZEROED_PARAMETER: u64 = 0x809f_0000;

/// Bytes of the parameter block oops-sdk passes and the probe measured.
const PARAMETER_BYTES: u64 = 64;

/// Whether a null-parameter initialisation has succeeded.
static INITIALISED: AtomicBool = AtomicBool::new(false);

/// Held by every test that initialises the library: [`INITIALISED`] is the process's, and tests in
/// one binary run at once, so a second initialisation answered as a first one would.
#[cfg(test)]
static INITIALISING: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// `sceSaveDataInitialize3(param)`: with a null parameter, initialises the library and answers
/// `0` - PPSA21564's call; with a zeroed 64-byte block, answers `0x809f0000` and writes nothing,
/// as measured twice before initialisation and once after termination. A second null call, and
/// any block with something in it, were not measured and are refused by name.
pub(crate) fn initialize3(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let param = args[0];
    if param == 0 {
        return if INITIALISED.swap(true, Ordering::AcqRel) {
            u64::from(GuestError::Unimplemented.as_raw())
        } else {
            0
        };
    }
    let zeroed = (0..PARAMETER_BYTES / 8).all(|word| {
        // SAFETY: the guest's parameter block, 64 bytes by the call's contract.
        (unsafe { guest::read_u64(param + word * 8) }) == Some(0)
    });
    if zeroed {
        ZEROED_PARAMETER
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// What a second `sceSaveDataSetupSaveDataMemory2` answered, with a null result pointer and with a
/// `0xcc`-filled one alike, writing nothing (measured).
const MEMORY_ALREADY_SET_UP: u64 = 0x809f_0000;

/// Whether save-data memory has been set up.
static MEMORY_SET_UP: AtomicBool = AtomicBool::new(false);

/// `sceSaveDataSetupSaveDataMemory2(param, result)`: sets up the title's save-data memory. Measured
/// after a null-parameter `sceSaveDataInitialize3` (`-5d20`): PPSA21564's parameter block - u32 0,
/// u32 the user, u64 `0x200000`, zeros to 64 bytes - with a null result answers `0` and leaves the
/// block unwritten; every call after that answers `0x809f0000` and writes nothing. A call before
/// initialisation, and a first call with a result pointer, whose writes were not measured, are
/// refused by name.
pub(crate) fn setup_save_data_memory2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (param, result) = (args[0], args[1]);
    if !INITIALISED.load(Ordering::Acquire) || param == 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if MEMORY_SET_UP.load(Ordering::Acquire) {
        return MEMORY_ALREADY_SET_UP;
    }
    if result != 0 {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    if MEMORY_SET_UP.swap(true, Ordering::AcqRel) {
        MEMORY_ALREADY_SET_UP
    } else {
        // SAFETY: the guest's parameter block; the size is its u64 at +8.
        let size = unsafe { guest::read_u64(param + 8) }
            .unwrap_or(0)
            .min(STORE_MOST);
        if let Ok(mut store) = STORE.lock() {
            *store = vec![0; usize::try_from(size).unwrap_or(0)];
            load_kept(&mut store);
        }
        0
    }
}

/// The span a request block names, as `(buffer, length, offset into the memory)`: the block holds
/// the user at +0 and the data block's address at +8; the data block the buffer, its length and the
/// offset, as obSCEne's `130-layout/savedata-memory-5d20` built them.
fn requested_span(request: u64) -> Option<(u64, usize, usize)> {
    // SAFETY: the guest's request block, by the call's contract.
    let data = unsafe { guest::read_u64(request + 8) }.filter(|&d| d != 0)?;
    // SAFETY: the data block the request names.
    let buffer = unsafe { guest::read_u64(data) }?;
    // SAFETY: as above.
    let length = unsafe { guest::read_u64(data + 8) }?;
    // SAFETY: as above.
    let offset = unsafe { guest::read_u64(data + 16) }?;
    Some((
        buffer,
        usize::try_from(length).ok()?,
        usize::try_from(offset).ok()?,
    ))
}

/// `sceSaveDataSetSaveDataMemory2(set)`: copies the request's buffer into save-data memory at its
/// offset and answers `0` for the signed-in user after set-up (D839, assumed); before set-up, for
/// another user, or for a span the memory does not hold, it answers as a read would or refuses by
/// name.
pub(crate) fn set_save_data_memory2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's request block, its first word the user by the call's contract.
    let user = unsafe { guest::read_u32(args[0]) }.unwrap_or(u32::MAX);
    let answer = get_answer(MEMORY_SET_UP.load(Ordering::Acquire), user);
    if answer != 0 {
        return answer;
    }
    let unserved = u64::from(GuestError::Unimplemented.as_raw());
    let Some((buffer, length, offset)) = requested_span(args[0]) else {
        return unserved;
    };
    let Ok(mut store) = STORE.lock() else {
        return unserved;
    };
    let Some(span) = offset
        .checked_add(length)
        .and_then(|end| store.get_mut(offset..end))
    else {
        return unserved;
    };
    // SAFETY: the guest's buffer, `length` bytes by the request's contract.
    if unsafe { guest::read_into(buffer, span) } {
        0
    } else {
        unserved
    }
}

/// What `sceSaveDataGetSaveDataMemory2` answered on a console with no save-data memory set up, with
/// every argument a buffer and with every argument zero, writing none of them (obSCEne census
/// `200-census/libSceSaveData_native/sceSaveDataGetSaveDataMemory2`, `20261009-151440-eboot.obs.log`).
const READ_BEFORE_SET_UP: u64 = 0x809f_0001;

/// Save-data memory: as many bytes as set-up asked for, zero until written. Reads copy out of it and
/// writes into it (D839, assumed); nothing persists it past the run.
static STORE: std::sync::Mutex<Vec<u8>> = std::sync::Mutex::new(Vec::new());

/// The most save-data memory a set-up is given, so a size read from a guest block cannot ask the
/// host for an unbounded allocation. PPSA21564 asks for 0x200000.
const STORE_MOST: u64 = 0x1000_0000;

/// Where save-data memory is kept between runs: in the title's own save storage (D792), in the
/// directory the console's own log names for it, `sce_sdmemory` (REQ-sd01's system log).
const MEMORY_FILE: &str = "/data/savedata/sce_sdmemory/memory.bin";

/// What a previous run kept, laid over the start of `store`.
fn load_kept(store: &mut [u8]) {
    let Some(host) = orbistoun_fs::mount::resolve_existing(MEMORY_FILE) else {
        return;
    };
    if let Ok(kept) = std::fs::read(host) {
        let n = kept.len().min(store.len());
        store[..n].copy_from_slice(&kept[..n]);
    }
}

/// `sceSaveDataSyncSaveDataMemory(sync)`: writes save-data memory to the title's own save storage,
/// where the next set-up reads it back, and answers `0` for the signed-in user after set-up (D839,
/// assumed); before set-up and for another user it answers as a read does, and storage that cannot
/// be written is refused by name.
pub(crate) fn sync_save_data_memory(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's request block, its first word the user by the call's contract.
    let user = unsafe { guest::read_u32(args[0]) }.unwrap_or(u32::MAX);
    let answer = get_answer(MEMORY_SET_UP.load(Ordering::Acquire), user);
    if answer != 0 {
        return answer;
    }
    let unserved = u64::from(GuestError::Unimplemented.as_raw());
    let Some(host) = orbistoun_fs::mount::resolve_for_create(MEMORY_FILE) else {
        return unserved;
    };
    if let Some(parent) = host.parent()
        && std::fs::create_dir_all(parent).is_err()
    {
        return unserved;
    }
    let Ok(store) = STORE.lock() else {
        return unserved;
    };
    if std::fs::write(host, &*store).is_ok() {
        0
    } else {
        unserved
    }
}

/// What a read after set-up answered for user 1, which is no user (line 6105).
const NOT_A_USER: u64 = 0x809f_0011;

/// What a save-data memory request answers for `user`: before set-up `0x809f0001` (measured);
/// after it `0` for the signed-in user, whose memory the request reads or writes (D839, assumed),
/// and `0x809f0011` for any other.
fn get_answer(set_up: bool, user: u32) -> u64 {
    if !set_up {
        READ_BEFORE_SET_UP
    } else if user == crate::signed_in_user() {
        0
    } else {
        NOT_A_USER
    }
}

/// `sceSaveDataGetSaveDataMemory2(get)`: copies save-data memory at the request's offset into its
/// buffer and answers `0` for the signed-in user after set-up (D839, assumed); otherwise as
/// [`get_answer`] decides, writing nothing, and a span the memory does not hold is refused by name.
pub(crate) fn get_save_data_memory2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's request block, its first word the user by the call's contract.
    let user = unsafe { guest::read_u32(args[0]) }.unwrap_or(u32::MAX);
    let answer = get_answer(MEMORY_SET_UP.load(Ordering::Acquire), user);
    if answer != 0 {
        return answer;
    }
    let unserved = u64::from(GuestError::Unimplemented.as_raw());
    let Some((buffer, length, offset)) = requested_span(args[0]) else {
        return unserved;
    };
    let Ok(store) = STORE.lock() else {
        return unserved;
    };
    let Some(span) = offset
        .checked_add(length)
        .and_then(|end| store.get(offset..end))
    else {
        return unserved;
    };
    // SAFETY: the guest's buffer, `length` bytes by the request's contract.
    if unsafe { guest::write_bytes(buffer, span) } {
        0
    } else {
        unserved
    }
}

#[cfg(test)]
mod tests {
    use super::{initialize3, setup_save_data_memory2};

    /// Before save-data memory is set up, a read answers `0x809f0001` (obSCEne census
    /// `200-census/libSceSaveData_native/sceSaveDataGetSaveDataMemory2`); after it, `0x809f0000` for
    /// the signed-in user and `0x809f0011` for anyone else (REQ-sd01, `20261009-215800-eboot.log`
    /// 6102-6107).
    #[test]
    fn a_read_answers_as_measured_before_and_after_set_up() {
        let me = crate::signed_in_user();
        let someone_else = me.wrapping_add(1);
        assert_eq!(super::get_answer(false, me), 0x809f_0001);
        assert_eq!(
            super::get_answer(true, me),
            0,
            "a read from the store (D839)"
        );
        assert_eq!(super::get_answer(true, someone_else), 0x809f_0011);
    }
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// The first set-up after initialisation answers `0` and writes nothing; every later one answers
    /// `0x809f0000` and writes nothing, result pointer or not (obSCEne `-5d20`).
    /// What is written to save-data memory is read back, and what was never written reads as
    /// zero: the memory is a store reads and writes copy through (D839).
    #[test]
    fn save_data_memory_reads_back_what_was_written() {
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let at = |bytes: &mut [u8]| bytes.as_mut_ptr().expose_provenance() as u64;
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        let _ = initialize3(&args);
        let mut param = [0u8; 64];
        param[8..16].copy_from_slice(&0x20_0000_u64.to_le_bytes());
        args[0] = at(&mut param);
        let _ = setup_save_data_memory2(&args);
        let me = crate::signed_in_user();
        // A request block: the user, then the data block's address; a data block: the buffer, its
        // size and the offset into the memory.
        let request = |data: &mut [u64; 4]| {
            let mut block = [0u8; 64];
            block[0..4].copy_from_slice(&me.to_le_bytes());
            block[8..16].copy_from_slice(&at(bytemuck_words(data)).to_le_bytes());
            block
        };
        let mut written = [0u8; 16];
        for (i, b) in written.iter_mut().enumerate() {
            *b = i as u8 + 1;
        }
        let mut data = [at(&mut written), 16, 0x30, 0];
        let mut block = request(&mut data);
        args[0] = at(&mut block);
        assert_eq!(super::set_save_data_memory2(&args), 0, "written");
        let mut read = [0xa5u8; 32];
        let mut data = [at(&mut read), 32, 0x28, 0];
        let mut block = request(&mut data);
        args[0] = at(&mut block);
        assert_eq!(super::get_save_data_memory2(&args), 0, "read");
        assert_eq!(&read[..8], &[0; 8], "never written reads as zero");
        assert_eq!(&read[8..24], &written, "and what was written reads back");
    }

    /// A data block's words as bytes, so the request can hold its address.
    fn bytemuck_words(words: &mut [u64; 4]) -> &mut [u8] {
        // SAFETY: four u64s are 32 initialised bytes, with no padding and any bit pattern valid.
        unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), 32) }
    }

    /// A sync keeps save-data memory in the title's own save storage, where the next set-up finds it
    /// (D839).
    #[test]
    fn a_sync_keeps_save_data_memory_in_the_titles_storage() {
        let root = std::env::temp_dir().join(format!("orbistoun-save-{}", std::process::id()));
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orbistoun_fs::mount::mount_data(root.clone());
        orbistoun_fs::mount::allow_writes(orbistoun_fs::mount::DATA_MOUNT);
        let at = |bytes: &mut [u8]| bytes.as_mut_ptr().expose_provenance() as u64;
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        let _ = initialize3(&args);
        let mut param = [0u8; 64];
        param[8..16].copy_from_slice(&0x20_0000_u64.to_le_bytes());
        args[0] = at(&mut param);
        let _ = setup_save_data_memory2(&args);
        let me = crate::signed_in_user();
        let mut written = *b"kept between runs";
        let mut data = [at(&mut written), written.len() as u64, 0x40, 0];
        let mut block = [0u8; 64];
        block[0..4].copy_from_slice(&me.to_le_bytes());
        block[8..16].copy_from_slice(&at(bytemuck_words(&mut data)).to_le_bytes());
        args[0] = at(&mut block);
        assert_eq!(super::set_save_data_memory2(&args), 0, "written");
        let mut sync = [0u8; 64];
        sync[0..4].copy_from_slice(&me.to_le_bytes());
        args[0] = at(&mut sync);
        assert_eq!(super::sync_save_data_memory(&args), 0, "synced");
        let kept = std::fs::read(root.join("savedata/sce_sdmemory/memory.bin"))
            .expect("the memory is in the title's storage");
        assert_eq!(&kept[0x40..0x40 + written.len()], &written);
        let _ = std::fs::remove_dir_all(&root);
    }

    #[test]
    fn save_data_memory_is_set_up_once_as_measured() {
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        super::INITIALISED.store(false, std::sync::atomic::Ordering::Release);
        super::MEMORY_SET_UP.store(false, std::sync::atomic::Ordering::Release);
        let mut param = [0u8; 64];
        param[4..8].copy_from_slice(&0x1ea2_f4d9_u32.to_le_bytes());
        param[8..16].copy_from_slice(&0x20_0000_u64.to_le_bytes());
        let before = param;
        let mut result = [0xccu8; 64];
        let at = |bytes: &mut [u8; 64]| std::ptr::from_mut(bytes).expose_provenance() as u64;
        let mut args = [0u64; GUEST_ARG_REGISTERS];
        assert_eq!(initialize3(&args), 0, "initialised with a null parameter");
        args[0] = at(&mut param);
        assert_eq!(setup_save_data_memory2(&args), 0);
        assert_eq!(param, before, "the block is left unwritten");
        assert_eq!(setup_save_data_memory2(&args), 0x809f_0000, "a second call");
        args[1] = at(&mut result);
        assert_eq!(setup_save_data_memory2(&args), 0x809f_0000);
        assert!(
            result.iter().all(|&b| b == 0xcc),
            "nothing written through the result"
        );
    }
}

/// Where a mounted save appears to the guest (D792): the first of the platform's save mount points.
const SAVE_MOUNT_POINT: &str = "/savedata0";

/// Where a save's files live, from the guest's point of view: its directory name under the title's
/// writable `/data` (D792). The platform keeps them elsewhere; the guest only ever sees the mount.
const SAVE_STORE: &str = "/data/savedata";

/// The mount modes `sceSaveDataMount3` reads at `param+0x20` (D792, assumed): read-only, read-write
/// and create.
mod mode {
    pub(super) const READ_WRITE: u32 = 2;
    pub(super) const CREATE: u32 = 4;
}

/// What `sceSaveDataMount3` answers for a save that does not exist, mounted without
/// [`mode::CREATE`] (D792, assumed): not found.
const SAVE_NOT_FOUND: u64 = 0x809f_0008;

/// `SCE_SAVE_DATA_ERROR_INVALID_ARGUMENT`, measured for an icon path naming no file and for a
/// bad `sceSaveDataUmount2` mount point (REQ-gt19).
const SAVE_INVALID_ARGUMENT: u64 = 0x809f_0000;

/// What a mount reports in `result+0x1c` when it created the save (D792, assumed).
const MOUNT_CREATED: u32 = 1;

/// Whether a save is mounted at [`SAVE_MOUNT_POINT`].
static MOUNTED: AtomicBool = AtomicBool::new(false);

/// A save directory name from `name`, a NUL-terminated string of at most 32 bytes made of the
/// characters a directory name may hold, or `None`.
fn save_directory_name(name: u64) -> Option<String> {
    // SAFETY: the guest's directory name, by the call's contract; read through the checked
    // accessor.
    let bytes = unsafe { guest::read_cstr(name, 32) }?;
    let text = String::from_utf8(bytes).ok()?;
    let allowed = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    (!text.is_empty() && text.chars().all(allowed)).then_some(text)
}

/// `sceSaveDataMount3(param, result)`: mounts the save `param` names at `/savedata0` and answers 0
/// (D792, assumed). `param` is `{ user u32, _, dir_name *, blocks u64, _, mode u32 at +0x20 }` as
/// PPSA03416's `SaveData.prx` fills it; the save is the directory of that name under the title's
/// `/data/savedata`. A save that does not exist is created when `mode` has [`mode::CREATE`] and
/// otherwise answers [`SAVE_NOT_FOUND`]; it is writable only when `mode` has
/// [`mode::READ_WRITE`]. `result` takes the mount point at +0x00, zero required blocks at +0x10 and
/// [`MOUNT_CREATED`] at +0x1c when the mount created it. Before `sceSaveDataInitialize3`, for
/// another user, or while a save is mounted, it answers the placeholder.
pub(crate) fn mount3(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (param, result) = (args[0], args[1]);
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    if param == 0 || result == 0 || !INITIALISED.load(Ordering::Acquire) {
        return refuse;
    }
    // SAFETY: the guest's parameter block, by the call's contract; read through a checked accessor.
    let user = unsafe { guest::read_u32(param) };
    // SAFETY: as above, its directory-name pointer.
    let name = unsafe { guest::read_u64(param + 0x08) };
    // SAFETY: as above, its mode.
    let mode = unsafe { guest::read_u32(param + 0x20) };
    let (Some(user), Some(name), Some(mode)) = (user, name, mode) else {
        return refuse;
    };
    if user != crate::signed_in_user() {
        return refuse;
    }
    let Some(directory) = save_directory_name(name) else {
        return refuse;
    };
    let guest_path = format!("{SAVE_STORE}/{directory}");
    let existed = orbistoun_fs::mount::is_directory(&guest_path);
    if !existed && mode & mode::CREATE == 0 {
        return SAVE_NOT_FOUND;
    }
    let Some(host) = orbistoun_fs::mount::resolve_for_create(&guest_path) else {
        return refuse;
    };
    if std::fs::create_dir_all(&host).is_err() || MOUNTED.swap(true, Ordering::AcqRel) {
        return refuse;
    }
    orbistoun_fs::mount::mount(SAVE_MOUNT_POINT, host);
    if mode & mode::READ_WRITE != 0 {
        orbistoun_fs::mount::allow_writes(SAVE_MOUNT_POINT);
    }
    let mut point = [0_u8; 16];
    point[..SAVE_MOUNT_POINT.len()].copy_from_slice(SAVE_MOUNT_POINT.as_bytes());
    let status = if existed { 0 } else { MOUNT_CREATED };
    // SAFETY: the guest's result block, by the call's contract; written through checked accessors.
    let point_written = unsafe { guest::write_bytes(result, &point) };
    // SAFETY: as above, its required blocks.
    let blocks_written = unsafe { guest::write_u64(result + 0x10, 0) };
    // SAFETY: as above, its mount status.
    let status_written = unsafe { guest::write_u32(result + 0x1c, status) };
    let written = point_written && blocks_written && status_written;
    if written { 0 } else { refuse }
}

/// `sceSaveDataUmount2(mode, mount_point)`: releases the save mounted at `mount_point` and answers
/// 0 (D792, assumed); any other mount point, or none mounted, answers the placeholder.
pub(crate) fn umount2(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    // SAFETY: the guest's mount point, by the call's contract; read through the checked accessor.
    let point = unsafe { guest::read_cstr(args[1], 16) };
    if point.as_deref() != Some(SAVE_MOUNT_POINT.as_bytes()) || !MOUNTED.load(Ordering::Acquire) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    orbistoun_fs::mount::unmount(SAVE_MOUNT_POINT);
    MOUNTED.store(false, Ordering::Release);
    0
}

/// What `sceSaveDataDirNameSearch` answers for a condition or result it cannot read: the parameter
/// error `sceSaveDataInitialize3` was measured answering (assumed for this call).
const SEARCH_PARAMETER: u64 = 0x809f_0000;

/// Bytes of a save directory name in a search's result array.
const DIR_NAME_BYTES: usize = 32;

/// Bytes of `SceSaveDataParam`: title (128), subtitle (128), detail (1024), user parameter, padding
/// and modification time - the stride PPSA04263 allocates its search's parameter array at.
const SEARCH_PARAM_BYTES: usize = 0x530;

/// Where `SceSaveDataParam` holds the modification time, seconds since the Unix epoch.
const SEARCH_PARAM_MTIME: usize = 0x508;

/// Whether `name` matches `pattern`, as a search condition spells one: `%` any run of characters,
/// none included, `_` any one; letters match either case.
fn name_matches(name: &[u8], pattern: &[u8]) -> bool {
    match pattern.split_first() {
        None => name.is_empty(),
        Some((b'%', rest)) => (0..=name.len()).any(|skip| name_matches(&name[skip..], rest)),
        Some((&first, rest)) => name.split_first().is_some_and(|(&head, tail)| {
            (first == b'_' || head.eq_ignore_ascii_case(&first)) && name_matches(tail, rest)
        }),
    }
}

/// The saves under `store`, a host directory, whose names match `pattern` (every save when it is
/// empty), each with its modification time since the Unix epoch, ordered by `key` - 3 by
/// modification time, anything else by name - and reversed when `descending`. Directories the
/// platform keeps for itself, `sce_`-prefixed, are not saves.
fn matching_saves(
    store: &std::path::Path,
    pattern: &[u8],
    key: u32,
    descending: bool,
) -> Vec<(String, std::time::Duration)> {
    let mut saves: Vec<(String, std::time::Duration)> = std::fs::read_dir(store)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            let since = entry
                .metadata()
                .and_then(|meta| meta.modified())
                .ok()
                .and_then(|at| at.duration_since(std::time::UNIX_EPOCH).ok())
                .unwrap_or_default();
            Some((name, since))
        })
        .filter(|(name, _)| !name.starts_with("sce_") && save_name_is_valid(name))
        .filter(|(name, _)| pattern.is_empty() || name_matches(name.as_bytes(), pattern))
        .collect();
    saves.sort_by(|a, b| a.0.cmp(&b.0));
    if key == 3 {
        saves.sort_by_key(|save| save.1);
    }
    if descending {
        saves.reverse();
    }
    saves
}

/// Whether `name` is made of the characters a save directory name may hold, as
/// [`save_directory_name`] requires.
fn save_name_is_valid(name: &str) -> bool {
    let allowed = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    !name.is_empty() && name.len() < DIR_NAME_BYTES && name.chars().all(allowed)
}

/// `sceSaveDataDirNameSearch(condition, result)`: the signed-in user's saves of this title, as
/// [`mount3`] keeps them under `/data/savedata`, matching the condition's name pattern and in its
/// order (assumed, as the reference emulators answer it for local saves).
///
/// `condition` is `{ user u32, _, title id *, name pattern *, key u32, order u32 }` and `result`
/// `{ hits u32, _, names *, names room u32, set u32, params *, infos * }` - PPSA04263 fills the
/// condition so (no title id: its own; key 3, modification time; order 1, descending) and reads
/// `hits` and the arrays. Each name takes 32 bytes; each parameter 0x530, zero but for its
/// modification time; each info 48, the save's blocks unknown and so zero. Another title's saves,
/// a user other than the signed-in one, or a call before `sceSaveDataInitialize3` answer the
/// placeholder.
pub(crate) fn dir_name_search(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let (condition, result) = (args[0], args[1]);
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    if condition == 0 || result == 0 {
        return SEARCH_PARAMETER;
    }
    if !INITIALISED.load(Ordering::Acquire) {
        return refuse;
    }
    // SAFETY: the guest's condition block, by the call's contract; read through checked accessors.
    let user = unsafe { guest::read_u32(condition) };
    // SAFETY: as above, its title-id pointer.
    let title = unsafe { guest::read_u64(condition + 0x08) };
    // SAFETY: as above, its name-pattern pointer.
    let pattern_at = unsafe { guest::read_u64(condition + 0x10) };
    // SAFETY: as above, its sort key and order.
    let order = unsafe { guest::read_u64(condition + 0x18) };
    // SAFETY: the guest's result block, by the call's contract: its arrays and their room.
    let names_at = unsafe { guest::read_u64(result + 0x08) };
    // SAFETY: as above.
    let room = unsafe { guest::read_u32(result + 0x10) };
    // SAFETY: as above.
    let params_at = unsafe { guest::read_u64(result + 0x18) };
    // SAFETY: as above.
    let infos_at = unsafe { guest::read_u64(result + 0x20) };
    let (
        Some(user),
        Some(title),
        Some(pattern_at),
        Some(order),
        Some(names_at),
        Some(room),
        Some(params_at),
        Some(infos_at),
    ) = (
        user, title, pattern_at, order, names_at, room, params_at, infos_at,
    )
    else {
        return SEARCH_PARAMETER;
    };
    if user != crate::signed_in_user() || title != 0 {
        return refuse;
    }
    if room != 0 && names_at == 0 {
        return SEARCH_PARAMETER;
    }
    let pattern = if pattern_at == 0 {
        Vec::new()
    } else {
        // SAFETY: the guest's name pattern, a NUL-terminated string within a directory name's
        // bytes; read through the checked accessor.
        match unsafe { guest::read_cstr(pattern_at, DIR_NAME_BYTES) } {
            Some(pattern) => pattern,
            None => return SEARCH_PARAMETER,
        }
    };
    let saves = orbistoun_fs::mount::resolve_existing(SAVE_STORE).map_or_else(Vec::new, |store| {
        matching_saves(&store, &pattern, order as u32, (order >> 32) as u32 == 1)
    });
    let set = saves.len().min(room as usize);
    for (index, (name, since)) in saves.iter().take(set).enumerate() {
        let mut named = [0_u8; DIR_NAME_BYTES];
        named[..name.len()].copy_from_slice(name.as_bytes());
        let seconds = i64::try_from(since.as_secs()).unwrap_or(i64::MAX);
        let mut param = kept_param(&format!("{SAVE_STORE}/{name}"));
        param[SEARCH_PARAM_MTIME..SEARCH_PARAM_MTIME + 8].copy_from_slice(&seconds.to_le_bytes());
        let slot = index as u64;
        // SAFETY: the guest's name array, `room` names of 32 bytes by the call's contract.
        let named_ok =
            unsafe { guest::write_bytes(names_at + slot * DIR_NAME_BYTES as u64, &named) };
        // SAFETY: the guest's parameter array when it passed one, as many entries as names.
        let param_ok = params_at == 0
            || unsafe { guest::write_bytes(params_at + slot * SEARCH_PARAM_BYTES as u64, &param) };
        // SAFETY: the guest's info array when it passed one, as many entries as names.
        let info_ok =
            infos_at == 0 || unsafe { guest::write_bytes(infos_at + slot * 48, &[0; 48]) };
        if !(named_ok && param_ok && info_ok) {
            return SEARCH_PARAMETER;
        }
    }
    let hits = u32::try_from(saves.len()).unwrap_or(u32::MAX);
    // SAFETY: the guest's result block: its hit count.
    let hits_ok = unsafe { guest::write_u32(result, hits) };
    // SAFETY: as above, how many entries were set.
    let set_ok = unsafe { guest::write_u32(result + 0x14, set as u32) };
    if hits_ok && set_ok {
        0
    } else {
        SEARCH_PARAMETER
    }
}

/// `sceSaveDataDelete(param)`: removes the signed-in user's save `param` names from the title's
/// `/data/savedata` and answers 0 (assumed, as the reference emulators delete a local save).
/// `param` is `{ user u32, _, title id *, name * }` - PPSA04263 fills it so after searching for the
/// name, with no title id (its own), and raises its error flag on a negative answer. A save that
/// does not exist answers 0 - already gone (REQ-gt19, measured); the mounted save, another user's
/// or another title's, and a call before `sceSaveDataInitialize3` answer the placeholder.
pub(crate) fn delete(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let param = args[0];
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    if param == 0 || !INITIALISED.load(Ordering::Acquire) {
        return refuse;
    }
    // SAFETY: the guest's parameter block, by the call's contract; read through checked accessors.
    let user = unsafe { guest::read_u32(param) };
    // SAFETY: as above, its title-id pointer.
    let title = unsafe { guest::read_u64(param + 0x08) };
    // SAFETY: as above, its directory-name pointer.
    let name = unsafe { guest::read_u64(param + 0x10) };
    let (Some(user), Some(0), Some(name)) = (user, title, name) else {
        return refuse;
    };
    if user != crate::signed_in_user() {
        return refuse;
    }
    let Some(directory) = save_directory_name(name) else {
        return refuse;
    };
    let guest_path = format!("{SAVE_STORE}/{directory}");
    if !orbistoun_fs::mount::is_directory(&guest_path) {
        // A save that is not there is already gone: the console answers 0, not an error
        // (REQ-gt19, measured - delete of an absent directory returned 0x0).
        return 0;
    }
    let mounted = MOUNTED.load(Ordering::Acquire)
        && orbistoun_fs::mount::resolve_existing(SAVE_MOUNT_POINT)
            == orbistoun_fs::mount::resolve_existing(&guest_path);
    let Some(host) = orbistoun_fs::mount::resolve_for_removal(&guest_path) else {
        return refuse;
    };
    if mounted || std::fs::remove_dir_all(host).is_err() {
        return refuse;
    }
    0
}

/// Each transaction resource `sceSaveDataCreateTransactionResource` has made and not deleted.
static RESOURCES: std::sync::Mutex<std::collections::BTreeSet<u32>> =
    std::sync::Mutex::new(std::collections::BTreeSet::new());

/// The identifier the next transaction resource takes.
static NEXT_RESOURCE: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(1);

/// The live transaction resources, held.
fn resources() -> std::sync::MutexGuard<'static, std::collections::BTreeSet<u32>> {
    RESOURCES
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// `sceSaveDataCreateTransactionResource(size)`: a resource for a write transaction, answered as
/// its non-negative identifier. Writes to a mounted save reach the title's storage as they are
/// made, so the resource holds nothing; `size` is the memory a title lends it (PPSA04263: 0xc0000)
/// and is not used (assumed, as the reference emulators answer it). Before
/// `sceSaveDataInitialize3` it answers the placeholder.
pub(crate) fn create_transaction_resource(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if !INITIALISED.load(Ordering::Acquire) {
        return u64::from(GuestError::Unimplemented.as_raw());
    }
    let resource = NEXT_RESOURCE.fetch_add(1, Ordering::Relaxed);
    resources().insert(resource);
    u64::from(resource)
}

/// Whether the parameter block at `param` names a live resource: its first word.
fn names_live_resource(param: u64) -> bool {
    // SAFETY: the guest's parameter block, by the call's contract; read through the checked
    // accessor.
    let resource = unsafe { guest::read_u32(param) };
    resource.is_some_and(|resource| resources().contains(&resource))
}

/// `sceSaveDataPrepare(mount point, param)`: prepares the save mounted at `mount point` for the
/// transaction `param` names - `{ resource u32, mode u32 }` - and answers 0: the save becomes
/// writable until it is unmounted, and a title then writes into it and commits. PPSA04263 mounts
/// its save read-only (mode 1, or 5 to create it, image+0x2b3b05c and +0x2b3b124) and writes only
/// through a transaction: it prepares the save, then opens `<mount point>/<file>` for writing
/// (image+0x2b3d678). A mount point no save is mounted at, or a
/// resource not made, answers the placeholder (assumed, as the reference emulators answer it).
pub(crate) fn prepare(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    // SAFETY: the guest's mount point, a NUL-terminated string by the call's contract; read through
    // the checked accessor.
    let point = unsafe { guest::read_cstr(args[0], 16) };
    let mounted =
        point.as_deref() == Some(SAVE_MOUNT_POINT.as_bytes()) && MOUNTED.load(Ordering::Acquire);
    if mounted && names_live_resource(args[1]) {
        orbistoun_fs::mount::allow_writes(SAVE_MOUNT_POINT);
        0
    } else {
        refuse
    }
}

/// `sceSaveDataCommit(param)`: commits the transaction `param` names, `{ resource u32, mode u32 }`,
/// and answers 0. What was written is already in the title's storage, so nothing is left to
/// move (assumed, as the reference emulators answer it). A resource not made answers the
/// placeholder.
pub(crate) fn commit(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if names_live_resource(args[0]) {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// `sceSaveDataDeleteTransactionResource(resource)`: releases a resource and answers 0; one not
/// made, or released already, answers the placeholder.
pub(crate) fn delete_transaction_resource(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    if resources().remove(&(args[0] as u32)) {
        0
    } else {
        u64::from(GuestError::Unimplemented.as_raw())
    }
}

/// Where a save keeps the parameter `sceSaveDataSetParam` set, inside the save: the platform's
/// own `sce_sys` directory, which a search does not list as a save.
const PARAM_FILE: &str = "sce_sys/param.bin";

/// Where a save keeps the icon `sceSaveDataSaveIconByPath` saved.
const ICON_FILE: &str = "sce_sys/icon0.png";

/// The parameter kept in the save at `save` (a guest path), or a zeroed one when none was set.
fn kept_param(save: &str) -> [u8; SEARCH_PARAM_BYTES] {
    let mut param = [0_u8; SEARCH_PARAM_BYTES];
    if let Some(kept) = orbistoun_fs::mount::resolve_existing(&format!("{save}/{PARAM_FILE}"))
        .and_then(|host| std::fs::read(host).ok())
    {
        let n = kept.len().min(SEARCH_PARAM_BYTES);
        param[..n].copy_from_slice(&kept[..n]);
    }
    param
}

/// Where `sceSaveDataSetParam`'s `type` puts its value in `SceSaveDataParam`, and how long it is:
/// 0 the whole block, 1 the title, 2 the subtitle, 3 the detail, 4 the user parameter.
const fn param_field(kind: u32) -> Option<(usize, usize)> {
    match kind {
        0 => Some((0, SEARCH_PARAM_BYTES)),
        1 => Some((0, 0x80)),
        2 => Some((0x80, 0x80)),
        3 => Some((0x100, 0x400)),
        4 => Some((0x500, 4)),
        _ => None,
    }
}

/// The host path of `file` inside the save mounted at `point`, made ready for writing, when
/// `point` names the mounted save.
fn mounted_file_for_write(point: u64, file: &str) -> Option<std::path::PathBuf> {
    // SAFETY: the guest's mount point, a NUL-terminated string by the call's contract; read through
    // the checked accessor.
    let named = unsafe { guest::read_cstr(point, 16) };
    if named.as_deref() != Some(SAVE_MOUNT_POINT.as_bytes()) || !MOUNTED.load(Ordering::Acquire) {
        return None;
    }
    let host = orbistoun_fs::mount::resolve_existing(SAVE_MOUNT_POINT)?.join(file);
    std::fs::create_dir_all(host.parent()?).ok()?;
    Some(host)
}

/// `sceSaveDataSetParam(mount point, type, buffer, size)`: sets the mounted save's parameter -
/// its title, subtitle, detail and user parameter, by [`param_field`] - from `size` bytes at
/// `buffer`, keeping it in the save, where a search reads it back, and answers 0 (assumed, as the
/// reference emulators keep it). PPSA04263 sets the whole 0x530-byte block (type 0) after writing
/// its profile, and abandons the save on a negative answer (image+0x2b3dd3c). A mount point with
/// no save, an unknown type, or a size other than the field's answers the placeholder.
pub(crate) fn set_param(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    let (kind, buffer, size) = (args[1] as u32, args[2], args[3]);
    let Some((at, len)) = param_field(kind) else {
        return refuse;
    };
    let fits = if kind == 0 {
        size as usize == len
    } else {
        size as usize <= len
    };
    if !fits || buffer == 0 {
        return refuse;
    }
    let Some(host) = mounted_file_for_write(args[0], PARAM_FILE) else {
        return refuse;
    };
    let mut param = kept_param(SAVE_MOUNT_POINT);
    let mut value = vec![0_u8; len];
    // SAFETY: the guest's buffer, `size` bytes by the call's contract; read through the checked
    // accessor.
    if !unsafe { guest::read_into(buffer, &mut value[..size as usize]) } {
        return refuse;
    }
    param[at..at + len].copy_from_slice(&value);
    if std::fs::write(host, param).is_ok() {
        0
    } else {
        refuse
    }
}

/// `sceSaveDataSaveIconByPath(mount point, path)`: copies the PNG at `path` into the mounted save as
/// its icon and answers 0 (assumed, as the reference emulators save it). PPSA04263 saves
/// `/app0/sce_sys/save_data.png` into a save it has just created. A mount point with no save, or
/// a path that names no file, answers the placeholder.
pub(crate) fn save_icon_by_path(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    // SAFETY: the guest's icon path, a NUL-terminated string by the call's contract; read through
    // the checked accessor.
    let Some(path) = (unsafe { guest::read_path(args[1]) }) else {
        return refuse;
    };
    let Some(source) = orbistoun_fs::mount::resolve_existing(&path) else {
        // A path naming no file is an invalid argument, as the console answered for an icon path
        // that did not exist (REQ-gt19, measured 0x809f0000).
        return SAVE_INVALID_ARGUMENT;
    };
    let Some(host) = mounted_file_for_write(args[0], ICON_FILE) else {
        return refuse;
    };
    if std::fs::copy(source, host).is_ok() {
        0
    } else {
        refuse
    }
}

#[cfg(test)]
mod transaction_tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A transaction makes a resource, prepares the mounted save, commits and releases it; a mount
    /// point with nothing mounted, or a resource released, is refused.
    #[test]
    fn a_transaction_prepares_the_mounted_save_and_commits() {
        let root = std::env::temp_dir().join(format!("orbistoun-txn-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orbistoun_fs::mount::mount_data(root.clone());
        orbistoun_fs::mount::allow_writes(orbistoun_fs::mount::DATA_MOUNT);
        let _ = super::initialize3(&[0; GUEST_ARG_REGISTERS]);

        let resource = super::create_transaction_resource(&[0xc_0000, 0, 0, 0, 0, 0]);
        assert!(
            resource < 0x8000_0000,
            "an identifier, not an error: {resource:#x}"
        );
        let mut param = [0_u8; 0x28];
        param[..4].copy_from_slice(&(resource as u32).to_le_bytes());
        let point = b"/savedata0\0";
        let prepare = [point.as_ptr() as u64, param.as_ptr() as u64, 0, 0, 0, 0];
        assert_ne!(super::prepare(&prepare), 0, "nothing is mounted");

        let name = b"SAVE0001\0";
        let mut mount = [0_u8; 0x40];
        mount[..4].copy_from_slice(&crate::signed_in_user().to_le_bytes());
        mount[8..16].copy_from_slice(&(name.as_ptr() as u64).to_le_bytes());
        // Read-only and created, as PPSA04263 mounts it: only the transaction makes it writable.
        mount[0x20..0x24].copy_from_slice(&(1_u32 | 4).to_le_bytes());
        let mut result = [0_u8; 0x40];
        let mount_args = [
            mount.as_ptr() as u64,
            result.as_mut_ptr() as u64,
            0,
            0,
            0,
            0,
        ];
        assert_eq!(super::mount3(&mount_args), 0);
        assert!(
            !orbistoun_fs::mount::is_writable("/savedata0/profile"),
            "read-only"
        );
        assert_eq!(super::prepare(&prepare), 0);
        assert!(
            orbistoun_fs::mount::is_writable("/savedata0/profile"),
            "prepared"
        );
        let mut save_param = vec![0_u8; 0x530];
        save_param[..7].copy_from_slice(b"Profile");
        save_param[0x500..0x504].copy_from_slice(&9_u32.to_le_bytes());
        let set = [
            point.as_ptr() as u64,
            0,
            save_param.as_ptr() as u64,
            0x530,
            0,
            0,
        ];
        assert_eq!(super::set_param(&set), 0);
        let detail = b"Chapter 1\0";
        let set_detail = [
            point.as_ptr() as u64,
            3,
            detail.as_ptr() as u64,
            detail.len() as u64,
            0,
            0,
        ];
        assert_eq!(super::set_param(&set_detail), 0);
        let kept = super::kept_param("/data/savedata/SAVE0001");
        assert_eq!(&kept[..8], b"Profile\0");
        assert_eq!(&kept[0x100..0x10a], b"Chapter 1\0");
        assert_eq!(kept[0x500..0x504], 9_u32.to_le_bytes());
        assert_ne!(
            super::set_param(&[point.as_ptr() as u64, 9, 0x1000, 4, 0, 0]),
            0,
            "no such type"
        );

        let icon = root.join("icon.png");
        std::fs::write(&icon, b"\x89PNG").unwrap();
        orbistoun_fs::mount::mount("/icons", root.clone());
        let icon_path = b"/icons/icon.png\0";
        let save_icon = [point.as_ptr() as u64, icon_path.as_ptr() as u64, 0, 0, 0, 0];
        assert_eq!(super::save_icon_by_path(&save_icon), 0);
        assert!(root.join("savedata/SAVE0001/sce_sys/icon0.png").exists());
        orbistoun_fs::mount::unmount("/icons");

        assert_eq!(super::commit(&[param.as_ptr() as u64, 0, 0, 0, 0, 0]), 0);
        assert_eq!(
            super::delete_transaction_resource(&[resource, 0, 0, 0, 0, 0]),
            0
        );
        assert_ne!(
            super::delete_transaction_resource(&[resource, 0, 0, 0, 0, 0]),
            0
        );
        assert_ne!(super::commit(&[param.as_ptr() as u64, 0, 0, 0, 0, 0]), 0);

        let mut umount = [0_u64; GUEST_ARG_REGISTERS];
        umount[1] = point.as_ptr() as u64;
        assert_eq!(super::umount2(&umount), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod delete_tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A save is deleted from the title's own storage; deleting it again finds nothing, and another
    /// user's save is refused.
    #[test]
    fn a_save_is_deleted_once() {
        let root = std::env::temp_dir().join(format!("orbistoun-delete-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("savedata/SAVE0001/sce_sys")).expect("a save");
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orbistoun_fs::mount::mount_data(root.clone());
        orbistoun_fs::mount::allow_writes(orbistoun_fs::mount::DATA_MOUNT);
        let _ = super::initialize3(&[0; GUEST_ARG_REGISTERS]);

        let name = b"SAVE0001\0";
        let mut param = [0_u8; 0x40];
        param[..4].copy_from_slice(&crate::signed_in_user().to_le_bytes());
        param[0x10..0x18].copy_from_slice(&(name.as_ptr() as u64).to_le_bytes());
        let args = [param.as_ptr() as u64, 0, 0, 0, 0, 0];
        assert_eq!(super::delete(&args), 0);
        assert!(!root.join("savedata/SAVE0001").exists(), "deleted");
        assert_eq!(
            super::delete(&args),
            0,
            "an absent save is already gone (REQ-gt19)"
        );

        std::fs::create_dir_all(root.join("savedata/SAVE0001")).expect("a save");
        param[..4].copy_from_slice(&(crate::signed_in_user() + 1).to_le_bytes());
        assert_ne!(super::delete(&args), 0, "another user's");
        assert!(root.join("savedata/SAVE0001").exists(), "kept");
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod search_tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A pattern's `%` matches any run, none included, and `_` any one character, either case.
    #[test]
    fn a_name_pattern_matches_runs_and_single_characters() {
        let matches =
            |name: &str, pattern: &str| super::name_matches(name.as_bytes(), pattern.as_bytes());
        assert!(matches("SAVE0001", "SAVE%"));
        assert!(matches("SAVE", "SAVE%"));
        assert!(matches("save0001", "SAVE____"));
        assert!(!matches("SAVE01", "SAVE____"));
        assert!(matches("AUTO_SAVE_12", "%SAVE%"));
        assert!(!matches("PROFILE", "SAVE%"));
    }

    /// The signed-in user's saves are found by pattern and listed newest first, each name with its
    /// parameter's modification time; the platform's own directories are not saves, and a result
    /// with room for fewer than were found is filled as far as it goes.
    #[test]
    fn saves_are_found_by_pattern_newest_first() {
        let root = std::env::temp_dir().join(format!("orbistoun-search-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Made oldest first, a moment apart, so each is newer than the last.
        for name in ["SAVE0001", "PROFILE", "SAVE0002", "sce_sdmemory"] {
            std::fs::create_dir_all(root.join("savedata").join(name)).expect("a save directory");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orbistoun_fs::mount::mount_data(root.clone());
        let _ = super::initialize3(&[0; GUEST_ARG_REGISTERS]);

        let pattern = b"SAVE%\0";
        let mut condition = [0_u8; 0x40];
        condition[..4].copy_from_slice(&crate::signed_in_user().to_le_bytes());
        condition[0x10..0x18].copy_from_slice(&(pattern.as_ptr() as u64).to_le_bytes());
        condition[0x18..0x20].copy_from_slice(&0x1_0000_0003_u64.to_le_bytes());
        let mut names = [0xcc_u8; 32 * 4];
        let mut params = vec![0xcc_u8; 0x530 * 4];
        let mut result = [0_u8; 0x40];
        result[0x08..0x10].copy_from_slice(&(names.as_mut_ptr() as u64).to_le_bytes());
        result[0x10..0x14].copy_from_slice(&4_u32.to_le_bytes());
        result[0x18..0x20].copy_from_slice(&(params.as_mut_ptr() as u64).to_le_bytes());
        let args = [
            condition.as_ptr() as u64,
            result.as_mut_ptr() as u64,
            0,
            0,
            0,
            0,
        ];

        assert_eq!(super::dir_name_search(&args), 0);
        assert_eq!(result[..4], 2_u32.to_le_bytes(), "two saves match");
        assert_eq!(result[0x14..0x18], 2_u32.to_le_bytes(), "both set");
        assert_eq!(&names[..9], b"SAVE0002\0", "the newest first");
        assert_eq!(&names[32..41], b"SAVE0001\0");
        let newest = i64::from_le_bytes(params[0x508..0x510].try_into().unwrap());
        let older = i64::from_le_bytes(params[0x530 + 0x508..0x530 + 0x510].try_into().unwrap());
        assert!(newest >= older, "{newest} after {older}");
        assert_eq!(
            params[..0x508],
            [0; 0x508],
            "nothing else is known of a save"
        );

        result[0x10..0x14].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(super::dir_name_search(&args), 0);
        assert_eq!(result[..4], 2_u32.to_le_bytes(), "two found");
        assert_eq!(result[0x14..0x18], 1_u32.to_le_bytes(), "one set");

        assert_eq!(
            super::dir_name_search(&[0; GUEST_ARG_REGISTERS]),
            super::SEARCH_PARAMETER
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}

#[cfg(test)]
mod mount_tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// A save is mounted from the title's own `/data` (D792): absent and opened read-only, it is
    /// not found; with the create bit it is made, mounted at `/savedata0` and reported created;
    /// after unmounting, the same name mounts the existing save.
    #[test]
    fn a_save_mounts_from_the_titles_own_storage() {
        let root = std::env::temp_dir().join(format!("orbistoun-save-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        // Held before `/data` is pointed anywhere: every test that mounts it holds this.
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        orbistoun_fs::mount::mount_data(root.clone());
        orbistoun_fs::mount::allow_writes(orbistoun_fs::mount::DATA_MOUNT);
        let _ = super::initialize3(&[0; GUEST_ARG_REGISTERS]);

        let name = b"SaveData0\0";
        let mut param = [0_u8; 0x40];
        param[..4].copy_from_slice(&crate::signed_in_user().to_le_bytes());
        param[8..16].copy_from_slice(&(name.as_ptr() as u64).to_le_bytes());
        param[0x10..0x18].copy_from_slice(&0x200_u64.to_le_bytes());
        let mut result = [0xa5_u8; 0x40];
        let mut args = [0_u64; GUEST_ARG_REGISTERS];
        args[0] = param.as_ptr() as u64;
        args[1] = result.as_mut_ptr() as u64;

        param[0x20..0x24].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(
            super::mount3(&args),
            super::SAVE_NOT_FOUND,
            "absent, read-only"
        );

        param[0x20..0x24].copy_from_slice(&(1_u32 | 4).to_le_bytes());
        assert_eq!(super::mount3(&args), 0, "created");
        assert_eq!(&result[..11], b"/savedata0\0");
        assert_eq!(result[0x10..0x18], [0; 8], "no blocks required");
        assert_eq!(result[0x1c..0x20], 1_u32.to_le_bytes(), "created");
        assert!(orbistoun_fs::mount::is_directory("/savedata0"));

        let point = b"/savedata0\0";
        let mut umount = [0_u64; GUEST_ARG_REGISTERS];
        umount[1] = point.as_ptr() as u64;
        assert_eq!(super::umount2(&umount), 0);
        assert!(
            !orbistoun_fs::mount::is_directory("/savedata0"),
            "unmounted"
        );

        param[0x20..0x24].copy_from_slice(&1_u32.to_le_bytes());
        assert_eq!(super::mount3(&args), 0, "the save exists now");
        assert_eq!(result[0x1c..0x20], [0; 4], "not created this time");
        assert_eq!(super::umount2(&umount), 0);
        let _ = std::fs::remove_dir_all(&root);
    }
}
