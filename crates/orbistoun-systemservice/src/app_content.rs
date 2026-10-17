//! `libSceAppContent` - downloadable content and additional-data mounts.
//!
//! The implemented names are the initialisation an IL2CPP title does at startup; the rest are
//! declared only. Arity is `6`, the trampoline's full capture, not a claim about how many
//! arguments each takes.

use orbistoun_core::{GUEST_ARG_REGISTERS, GuestError, GuestFn};
use orbistoun_hle::guest_module;

guest_module! {
    "libSceAppContent" {
        "sceAppContentAddcontMount" => 6,
        "sceAppContentAddcontUnmount" => 6,
        "sceAppContentAppParamGetInt" => 6,
        "sceAppContentDownloadDataGetAvailableSpaceKb" => 6,
        "sceAppContentInitialize" => 6,
        "sceAppContentTemporaryDataFormat" => 6,
        "sceAppContentTemporaryDataGetAvailableSpaceKb" => 6,
        "sceAppContentTemporaryDataMount2" => 6,
    }
}

/// Successful return, as the guest reads it.
const OK: u64 = 0;

/// `sceAppContentInitialize(init, boot)` - starts the app-content subsystem. Answers `0`.
///
/// Guest-observed: `libil2cpp` calls it at startup and traps on the placeholder return. The
/// library is not reachable from a measurement context, so the guest's behaviour is the evidence.
/// `boot` is left unwritten because nothing measured says what it holds.
fn initialize(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// `sceAppContentAppParamGetInt(param_id, out)` - an integer app parameter, into `*out`.
///
/// Writes `0` and answers success, as `sceSystemServiceParamGetInt` does: the identifiers are
/// unmeasured, and zero reads as a first index, an unset flag or an empty count. The
/// out-parameter is always written, because an unwritten one hands the guest stale stack.
fn app_param_get_int(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let out = args[1];
    // Zero is the documented placeholder. The checked helper refuses address zero.
    if !super::write_word(out, 0) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// `sceAppContentTemporaryDataMount2(option, mountPoint)` - mounts the title's temporary-data area.
/// Answers `0`.
///
/// Guest-observed on the same terms as [`initialize`]: a mount call fails on an error code and
/// proceeds on success. `mountPoint` is left unwritten because the `SceAppContentMountPoint`
/// layout and the path it would carry are unmeasured.
fn temporary_data_mount2(_args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    OK
}

/// Where a title's download data lives, from the guest's point of view.
const DOWNLOAD_MOUNT: &str = "/download0";

/// The title's parameters, which declare its download-data size.
const PARAM_JSON: &str = "/app0/sce_sys/param.json";

/// The download-data size `param.json` declares, in MiB, from its text.
fn declared_download_mib(param: &str) -> Option<u64> {
    serde_json::from_str::<serde_json::Value>(param)
        .ok()?
        .get("downloadDataSize")?
        .as_u64()
}

/// The bytes the files under `host` hold, every level down.
fn bytes_under(host: &std::path::Path) -> u64 {
    std::fs::read_dir(host)
        .into_iter()
        .flatten()
        .flatten()
        .map(|entry| match entry.file_type() {
            Ok(kind) if kind.is_dir() => bytes_under(&entry.path()),
            Ok(_) => entry.metadata().map_or(0, |meta| meta.len()),
            Err(_) => 0,
        })
        .sum()
}

/// The KiB left of `declared_mib` MiB once `used` bytes are taken.
const fn available_kib(declared_mib: u64, used: u64) -> u64 {
    (declared_mib * 1024).saturating_sub(used.div_ceil(1024))
}

/// `sceAppContentDownloadDataGetAvailableSpaceKb(mountPoint, kb)`: the KiB left in the title's
/// download data, written to `*kb`: the size its `param.json` declares (`downloadDataSize`, MiB)
/// less what `/download0` already holds, and answers 0 (assumed: the console sizes the area by the
/// title's declaration; the reference emulators answer free space). PPSA04263 asks for
/// `/download0` and keeps `kb << 10` as the bytes it may write. Another mount point, a title that
/// declares no download data, or a null destination answers the placeholder.
fn download_data_get_available_space_kb(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let refuse = u64::from(GuestError::Unimplemented.as_raw());
    // SAFETY: the guest's mount point, a NUL-terminated string within 16 bytes by the call's
    // contract; read through the checked accessor.
    let point = unsafe { orbistoun_mem::guest::read_cstr(args[0], 16) };
    if point.as_deref() != Some(DOWNLOAD_MOUNT.as_bytes()) {
        return refuse;
    }
    let declared = orbistoun_fs::mount::resolve_existing(PARAM_JSON)
        .and_then(|host| std::fs::read_to_string(host).ok())
        .and_then(|text| declared_download_mib(&text));
    let Some(declared) = declared else {
        return refuse;
    };
    let used =
        orbistoun_fs::mount::resolve_existing(DOWNLOAD_MOUNT).map_or(0, |host| bytes_under(&host));
    if super::write_word(args[1], available_kib(declared, used)) {
        OK
    } else {
        refuse
    }
}

/// Implementations this module provides for `libSceAppContent`.
pub fn implementations() -> &'static [(&'static str, GuestFn)] {
    &[
        ("sceAppContentInitialize", initialize),
        (
            "sceAppContentDownloadDataGetAvailableSpaceKb",
            download_data_get_available_space_kb,
        ),
        ("sceAppContentAppParamGetInt", app_param_get_int),
        ("sceAppContentTemporaryDataMount2", temporary_data_mount2),
    ]
}

#[cfg(test)]
mod tests {
    /// The area is the declared MiB, in KiB, less what it holds, rounded up to a whole KiB.
    #[test]
    fn download_space_is_the_declared_size_less_what_is_held() {
        assert_eq!(
            super::declared_download_mib(r#"{"downloadDataSize":5120}"#),
            Some(5120)
        );
        assert_eq!(super::declared_download_mib(r#"{"titleId":"X"}"#), None);
        assert_eq!(super::available_kib(5120, 0), 5_242_880);
        assert_eq!(super::available_kib(5120, 2306), 5_242_877);
        assert_eq!(super::available_kib(1, 2 << 20), 0);
    }

    /// The bytes under a directory count every file, every level down.
    #[test]
    fn the_bytes_held_count_every_level() {
        let root = std::env::temp_dir().join(format!("orbistoun-dl-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("saves/1")).unwrap();
        std::fs::write(root.join("saves/1/PROFILEB"), [0_u8; 2306]).unwrap();
        std::fs::write(root.join("top.bin"), [0_u8; 10]).unwrap();
        assert_eq!(super::bytes_under(&root), 2316);
        let _ = std::fs::remove_dir_all(&root);
    }
}
