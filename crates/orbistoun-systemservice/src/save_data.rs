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
    }

    #[test]
    fn save_data_memory_is_set_up_once_as_measured() {
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        super::INITIALISED.store(false, std::sync::atomic::Ordering::Release);
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
        orbistoun_fs::mount::mount_data(root.clone());
        orbistoun_fs::mount::allow_writes(orbistoun_fs::mount::DATA_MOUNT);
        let _alone = super::INITIALISING
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
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
