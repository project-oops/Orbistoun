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

/// `sceAppContentAppParamGetInt(param_id, out)` - an integer app parameter, into `*out`: the
/// title's own, as [`app_param`] reads them from its `param.json`. Answers success, and always
/// writes the out-parameter, because an unwritten one hands the guest stale stack.
fn app_param_get_int(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let param = orbistoun_fs::mount::resolve_existing(PARAM_JSON)
        .and_then(|host| std::fs::read_to_string(host).ok());
    let value = app_param(args[0], param.as_deref());
    // The checked helper refuses address zero.
    if !super::write_word(args[1], u64::from(value)) {
        return u64::from(GuestError::InvalidArgument.as_raw());
    }
    OK
}

/// The SKU flag's id, which reads [`SKU_FULL`].
const PARAM_SKU_FLAG: u64 = 0;
/// A full game's SKU flag, as against a trial's 1.
const SKU_FULL: u32 = 3;

/// App parameter `id` of a title whose `param.json` reads `param`: the SKU flag (0) a full game's,
/// and the four user-defined parameters (1-4) the title's `userDefinedParam1`-`4`, as the reference
/// emulators read them (assumed). A parameter the title does not declare, and any other id, reads
/// 0, which a flag reads as unset and a count as none.
fn app_param(id: u64, param: Option<&str>) -> u32 {
    if id == PARAM_SKU_FLAG {
        return SKU_FULL;
    }
    if !(1..=4).contains(&id) {
        return 0;
    }
    param
        .and_then(|text| serde_json::from_str::<serde_json::Value>(text).ok())
        .and_then(|json| json.get(format!("userDefinedParam{id}"))?.as_u64())
        .and_then(|value| u32::try_from(value).ok())
        .unwrap_or(0)
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
    /// The SKU flag reads as a full game, and the four user-defined parameters as `param.json`
    /// declares them: PPSA04263 reads its first, 9, and tests it bit by bit around its Story Mode
    /// entitlement. One the title does not declare, or any other id, reads 0.
    #[test]
    fn an_app_parameter_is_the_title_s_own() {
        let param = r#"{"titleId":"PPSA04263","userDefinedParam1":9,"userDefinedParam2":0}"#;
        assert_eq!(super::app_param(0, Some(param)), 3, "SKU flag: full");
        assert_eq!(super::app_param(1, Some(param)), 9);
        assert_eq!(super::app_param(2, Some(param)), 0);
        assert_eq!(super::app_param(4, Some(param)), 0, "not declared");
        assert_eq!(super::app_param(1, None), 0, "no param.json");
        assert_eq!(super::app_param(7, Some(param)), 0);
    }

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
