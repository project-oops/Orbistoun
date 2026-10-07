//! The register-default descriptors `sceAgcGetRegisterDefaults2` and `...Internal` answer.
//!
//! Each is a library-owned 64-byte descriptor: four table pointers at `+0x00..+0x18`, the tables'
//! entry counts at `+0x20..+0x2c`, a record array at `+0x30` and its count at `+0x38`. A record is
//! 12 bytes, `{id, loc, third}`; a title resolves one through table `loc & 3`, slot
//! `(loc & 0x3fc) / 4`, and reads the bytes at the entry that slot holds. obSCEne walked every
//! record of the six descriptors titles are seen to ask for (`data/register-defaults.toml`), and
//! this rebuilds each in memory it owns: the records as measured, each table at its measured
//! length, and the entries' bytes at their measured distances from one another, so a read that
//! runs from one entry into the next finds what it would on hardware. A slot no record names was
//! not dumped and is left null, which the title code seen reading slots checks for.

use std::collections::HashMap;
use std::sync::{Mutex, OnceLock, PoisonError};

use serde::Deserialize;

/// Bytes of a descriptor.
const DESCRIPTOR_BYTES: usize = 0x40;
/// Bytes of a record.
const RECORD_BYTES: usize = 12;

#[derive(Deserialize)]
struct File {
    descriptor: Vec<Measured>,
}

/// One descriptor as measured.
#[derive(Deserialize)]
struct Measured {
    function: String,
    version: u32,
    /// The console's table pointers, `0` for a null one.
    tables: [u64; 4],
    /// Each table's entry count.
    lengths: [u32; 4],
    /// `[id, loc, third, entry, the bytes at entry]`: the whole entry, to the next entry's address
    /// (capped at 0x400), where REQ-rd01 measured it, else the 16 bytes REQ-4e9b read.
    records: Vec<(u32, u32, u32, u64, String)>,
}

/// A descriptor built from a measurement: its memory, and where the descriptor starts in it.
struct Built {
    memory: Box<[u64]>,
}

impl Built {
    fn address(&self) -> u64 {
        self.memory.as_ptr().expose_provenance() as u64
    }
}

fn hex(text: &str) -> Option<Vec<u8>> {
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(text.get(i..i + 2)?, 16).ok())
        .collect()
}

/// Lays `measured` out in one allocation: descriptor, records, tables, then the entries' span.
/// `None` when the measurement contradicts itself - a record naming a null table or a slot past
/// its table, or two overlapping entry dumps disagreeing.
fn build(measured: &Measured) -> Option<Built> {
    let count = measured.records.len();
    let records_at = DESCRIPTOR_BYTES;
    let mut tables_at = [0_usize; 4];
    let mut end = (records_at + count * RECORD_BYTES).next_multiple_of(8);
    for ((at, &pointer), &length) in tables_at
        .iter_mut()
        .zip(&measured.tables)
        .zip(&measured.lengths)
    {
        *at = end;
        if pointer != 0 {
            end += length as usize * 8;
        }
    }
    let low = measured.records.iter().map(|r| r.3).min()?;
    let values_at = end;
    // Each entry's bytes run as far as it was measured, so the span ends at the furthest of them.
    let span = measured
        .records
        .iter()
        .map(|r| Some(usize::try_from(r.3 - low).ok()? + r.4.len() / 2))
        .collect::<Option<Vec<usize>>>()?
        .into_iter()
        .max()?;
    end = (values_at + span).next_multiple_of(8);

    let mut memory = vec![0_u64; end / 8].into_boxed_slice();
    let base = memory.as_ptr().expose_provenance() as u64;
    let mut bytes = vec![0_u8; end];
    let mut written = vec![false; end];
    let put = |bytes: &mut [u8], at: usize, value: &[u8]| {
        bytes[at..at + value.len()].copy_from_slice(value);
    };

    for (index, &(id, loc, third, entry, ref value)) in measured.records.iter().enumerate() {
        let record = records_at + index * RECORD_BYTES;
        put(&mut bytes, record, &id.to_le_bytes());
        put(&mut bytes, record + 4, &loc.to_le_bytes());
        put(&mut bytes, record + 8, &third.to_le_bytes());
        let table = (loc & 3) as usize;
        let slot = ((loc & 0x3fc) / 4) as usize;
        if measured.tables[table] == 0 || slot >= measured.lengths[table] as usize {
            return None;
        }
        let at = values_at + usize::try_from(entry - low).ok()?;
        let value = hex(value)?;
        for (offset, &byte) in value.iter().enumerate() {
            if written[at + offset] && bytes[at + offset] != byte {
                return None;
            }
            bytes[at + offset] = byte;
            written[at + offset] = true;
        }
        put(
            &mut bytes,
            tables_at[table] + slot * 8,
            &(base + at as u64).to_le_bytes(),
        );
    }
    for (table, (&at, (&console, &length))) in tables_at
        .iter()
        .zip(measured.tables.iter().zip(&measured.lengths))
        .enumerate()
    {
        let pointer = if console == 0 { 0 } else { base + at as u64 };
        put(&mut bytes, table * 8, &pointer.to_le_bytes());
        put(&mut bytes, 0x20 + table * 4, &length.to_le_bytes());
    }
    put(&mut bytes, 0x30, &(base + records_at as u64).to_le_bytes());
    put(&mut bytes, 0x38, &u32::try_from(count).ok()?.to_le_bytes());

    for (word, chunk) in memory.iter_mut().zip(bytes.chunks_exact(8)) {
        *word = u64::from_le_bytes(chunk.try_into().ok()?);
    }
    Some(Built { memory })
}

/// Descriptors by `(function, version)`; `None` for a measurement that did not build.
type Descriptors = HashMap<(String, u32), Option<Built>>;

/// The built descriptors, made once each: the library's are static, so every call answers the same
/// address.
fn built() -> &'static Mutex<Descriptors> {
    static BUILT: OnceLock<Mutex<Descriptors>> = OnceLock::new();
    BUILT.get_or_init(Mutex::default)
}

fn measured() -> &'static [Measured] {
    static MEASURED: OnceLock<Vec<Measured>> = OnceLock::new();
    MEASURED.get_or_init(|| {
        toml::from_str::<File>(include_str!("../data/register-defaults.toml"))
            .map(|file| file.descriptor)
            .unwrap_or_default()
    })
}

/// The address of `function`'s descriptor for `version`, or `None` for a version no probe walked.
#[must_use]
pub fn descriptor(function: &str, version: u32) -> Option<u64> {
    let mut built = built().lock().unwrap_or_else(PoisonError::into_inner);
    built
        .entry((function.to_owned(), version))
        .or_insert_with(|| {
            measured()
                .iter()
                .find(|m| m.function == function && m.version == version)
                .and_then(build)
        })
        .as_ref()
        .map(Built::address)
}

#[cfg(test)]
mod tests {
    use super::{build, measured};

    /// The colour-target entry is whole (REQ-rd01, `reports/hardware/20261007-102530-eboot.obs.log`
    /// 4793-7912): record `0x38e92c91` holds 0x400 bytes, colour targets 0 to 7 as
    /// `{offset, value}` pairs, `CB_COLOR0_INFO` (`0x31c`) the third. With only its first 16 bytes,
    /// PPSA03416's and PPSA02664's colour-target registers after `CB_COLOR0_VIEW` landed on
    /// context offset 0.
    #[test]
    fn the_colour_target_entry_is_whole() {
        let m = measured()
            .iter()
            .find(|m| m.function == "sceAgcGetRegisterDefaults2" && m.version == 0xd)
            .expect("version 0xd");
        let (.., value) = m
            .records
            .iter()
            .find(|r| r.0 == 0x38e9_2c91)
            .expect("the colour-target record");
        let bytes = super::hex(value).expect("hex");
        assert_eq!(bytes.len(), 0x400, "the whole entry");
        assert_eq!(
            bytes[16..20],
            0x31c_u32.to_le_bytes(),
            "CB_COLOR0_INFO third"
        );
        let built = build(m).expect("builds");
        assert_ne!(built.address(), 0);
    }

    /// Every measured descriptor builds: no record names a null table or a slot past its table's
    /// length, and wherever two entries' dumps overlap they agree byte for byte.
    #[test]
    fn every_measurement_is_self_consistent() {
        assert_eq!(measured().len(), 6, "three versions of each function");
        for m in measured() {
            assert!(build(m).is_some(), "{} {:#x}", m.function, m.version);
        }
    }
}
