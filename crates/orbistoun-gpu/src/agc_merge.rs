//! `libSceAgc` NIDs `0x0a4e469ba741de7d` and `0xe486fe61994e8976`: the pair PPSA04263 calls in
//! lockstep for every pipeline it builds (720 times each), and PPSA21564 33 times. Given a stage-5
//! shader A and a stage-7 shader B, `0x0a4e` writes one stage-3 shader record built from both, with
//! a merged register list, and `0xe486` answers the sizes that record needs.
//!
//! Measured on PPSA04263's own pair, rebuilt by obSCEne (`REQ-20261013T1110Z-sp01`,
//! `REQ-20261013T1800Z-sp02`, `200-census/libSceAgc/ppsa04263-shader-pair`,
//! `reports/hardware/report-20261013-184500.log`, FW 12.40):
//! - `0x0a4e(out, A, B, aux, n, 0)` answers 0 for `n` 4 and 1. It writes `out` with 0x60 bytes,
//!   B's own first 0x60 with `+0x08` cleared, `+0x20` pointing at `aux` and the stage byte at
//!   `+0x5a` 3. It writes `aux` with ten `{reg, value}` pairs, B's SH register list in B's order
//!   with:
//!   - PGM_LO/HI (`0x148`/`0x149`) taken from A's code address;
//!   - B's first two `0x100` entries carrying A's two `0x100` values;
//!   - `0x10a` (RSRC1) with A's VGPR field;
//!   - `0x10b` (RSRC2) with A's user-SGPR field.
//! - `0xe486(out, A, B, state, n, 0)` answers 0 and writes `{u64 0x50, u64 4}` into `out`, for `n`
//!   4 and 1, leaving `state` untouched. 0x50 is B's SH register count times eight, the bytes of the
//!   list `0x0a4e` writes, and 4 is B's CX register count, both from B's header.
//! - Either with A and B swapped answers `0x8a6c0008`.
//!
//! Three rules are generalised from that one pair: VGPRs and user SGPRs take the larger of the two
//! shaders', where A's happened to be larger; A's `0x100` values fill B's `0x100` entries in order;
//! and the sizes come from B. Any other pair of stages is refused `0x8a6c0008`, as the census
//! measured for a vertex and pixel shader `sceAgcCreateShader` had just made, and for the vertex
//! shader alone (`REQ-20261009T1130Z-cn11`). obSCEne `REQ-20261013T2000Z-sp03` asks for a second
//! stage-5/stage-7 pair.

use orbistoun_core::GUEST_ARG_REGISTERS;
use orbistoun_mem::guest;

/// The answer to a pair in the wrong order (measured).
const STAGES_DO_NOT_PAIR: u64 = 0x8a6c_0008;

/// A null shader or buffer, as the title's own wrapper refuses one (`sp01` evidence).
const BAD_ARGUMENT: u64 = 0x8a6c_000a;

/// The bytes of a shader record this copies, as `0x0a4e` wrote them.
const RECORD_BYTES: usize = 0x60;

/// Where a shader object keeps what this reads (`166-agc/create-shader`'s object model).
const CODE: u64 = 0x10;
const SH_LIST: u64 = 0x20;
const STAGE: u64 = 0x5a;
const CX_COUNT: u64 = 0x5b;
const SH_COUNT: u64 = 0x5c;

/// The stages of the one measured pair, and of the record `0x0a4e` made from it.
const FIRST_STAGE: u8 = 5;
const SECOND_STAGE: u8 = 7;
const MERGED_STAGE: u8 = 3;

/// SH registers the merge rewrites (`gfx103.json`).
const PGM_LO: u32 = 0x148;
const PGM_HI: u32 = 0x149;
const USER_DATA: u32 = 0x100;
const RSRC1: u32 = 0x10a;
const RSRC2: u32 = 0x10b;

/// `SPI_SHADER_PGM_RSRC1`'s VGPR field and `RSRC2`'s user-SGPR field.
const RSRC1_VGPRS: u32 = 0x3f;
const RSRC2_USER_SGPR: u32 = 0x3e;

fn byte(object: u64, at: u64) -> Option<u8> {
    // SAFETY: a guest shader object, read through the checked accessor.
    unsafe { guest::read_u32(object + at) }.map(|word| word as u8)
}

/// A shader object's SH register list, as `{reg, value}` pairs.
fn sh_list(object: u64) -> Option<Vec<(u32, u32)>> {
    let count = u64::from(byte(object, SH_COUNT)?);
    // SAFETY: as in `byte`.
    let list = unsafe { guest::read_u64(object + SH_LIST) }?;
    (0..count)
        .map(|i| {
            // SAFETY: the pair the object's own count and list pointer name.
            let reg = unsafe { guest::read_u32(list + i * 8) }?;
            // SAFETY: as above.
            let value = unsafe { guest::read_u32(list + i * 8 + 4) }?;
            Some((reg, value))
        })
        .collect()
}

/// Whether `(a, b)` pair: `Ok(())` for stage 5 then 7, the measured refusal for any other.
fn check_stages(a: u64, b: u64) -> Result<(), u64> {
    match (byte(a, STAGE), byte(b, STAGE)) {
        (Some(FIRST_STAGE), Some(SECOND_STAGE)) => Ok(()),
        (Some(_), Some(_)) => Err(STAGES_DO_NOT_PAIR),
        _ => Err(BAD_ARGUMENT),
    }
}

/// B's list with A merged into it, as `0x0a4e` wrote it for the measured pair.
fn merged_list(a: u64, b: u64) -> Option<Vec<(u32, u32)>> {
    // SAFETY: as in `byte`.
    let code = unsafe { guest::read_u64(a + CODE) }?;
    let first = sh_list(a)?;
    let field = |reg: u32| first.iter().find(|(r, _)| *r == reg).map(|(_, v)| *v);
    let mut user_data = first
        .iter()
        .filter(|(r, _)| *r == USER_DATA)
        .map(|(_, v)| *v);
    let larger = |mine: u32, theirs: Option<u32>, mask: u32| {
        let theirs = theirs.unwrap_or(0) & mask;
        (mine & !mask) | (mine & mask).max(theirs)
    };
    Some(
        sh_list(b)?
            .into_iter()
            .map(|(reg, value)| {
                let value = match reg {
                    PGM_LO => (code >> 8) as u32,
                    PGM_HI => (code >> 40) as u32,
                    USER_DATA => user_data.next().unwrap_or(value),
                    RSRC1 => larger(value, field(RSRC1), RSRC1_VGPRS),
                    RSRC2 => larger(value, field(RSRC2), RSRC2_USER_SGPR),
                    _ => value,
                };
                (reg, value)
            })
            .collect(),
    )
}

/// `0x0a4e469ba741de7d(out, A, B, aux, n, 0)`: the merged stage-3 record into `out`, its register
/// list into `aux`; see the module documentation.
pub(crate) fn merge_shaders(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [out, a, b, aux, ..] = *args;
    if out == 0 || a == 0 || b == 0 || aux == 0 {
        return BAD_ARGUMENT;
    }
    if let Err(code) = check_stages(a, b) {
        return code;
    }
    let mut record = [0_u8; RECORD_BYTES];
    // SAFETY: B's object, at least the record's 0x60 bytes by the object model.
    let (Some(list), true) = (merged_list(a, b), unsafe {
        guest::read_into(b, &mut record)
    }) else {
        return BAD_ARGUMENT;
    };
    record[0x08..0x10].fill(0);
    record[0x20..0x28].copy_from_slice(&aux.to_le_bytes());
    record[STAGE as usize] = MERGED_STAGE;
    let pairs: Vec<u8> = list
        .iter()
        .flat_map(|(reg, value)| reg.to_le_bytes().into_iter().chain(value.to_le_bytes()))
        .collect();
    // SAFETY: the guest's register-list buffer, by the call's contract.
    let listed = unsafe { guest::write_bytes(aux, &pairs) };
    // SAFETY: the guest's record buffer, by the call's contract.
    let recorded = unsafe { guest::write_bytes(out, &record) };
    if listed && recorded { 0 } else { BAD_ARGUMENT }
}

/// `0xe486fe61994e8976(out, A, B, state, n, 0)`: `{B's SH count * 8, B's CX count}` into `out`,
/// `state` untouched; see the module documentation.
pub(crate) fn merged_sizes(args: &[u64; GUEST_ARG_REGISTERS]) -> u64 {
    let [out, a, b, ..] = *args;
    if out == 0 || a == 0 || b == 0 {
        return BAD_ARGUMENT;
    }
    if let Err(code) = check_stages(a, b) {
        return code;
    }
    let (Some(sh), Some(cx)) = (byte(b, SH_COUNT), byte(b, CX_COUNT)) else {
        return BAD_ARGUMENT;
    };
    // SAFETY: the guest's output, sixteen bytes by the measured write.
    let first = unsafe { guest::write_u64(out, u64::from(sh) * 8) };
    // SAFETY: as above, its second quadword.
    let second = unsafe { guest::write_u64(out + 8, u64::from(cx)) };
    if first && second { 0 } else { BAD_ARGUMENT }
}

#[cfg(test)]
mod tests {
    use orbistoun_core::GUEST_ARG_REGISTERS;

    /// PPSA04263's pair as obSCEne rebuilt it (`sp02`, shader objects at 0x60 and their lists),
    /// laid out in host memory with the same offsets.
    struct Pair {
        a: Vec<u8>,
        b: Vec<u8>,
    }

    fn put(object: &mut [u8], at: usize, value: u64) {
        object[at..at + 8].copy_from_slice(&value.to_le_bytes());
    }

    fn pairs(list: &[(u32, u32)]) -> Vec<u8> {
        list.iter()
            .flat_map(|(r, v)| r.to_le_bytes().into_iter().chain(v.to_le_bytes()))
            .collect()
    }

    impl Pair {
        fn new() -> Self {
            let mut a = vec![0_u8; 0x140];
            let mut b = vec![0_u8; 0x180];
            put(&mut a, 0x00, 0x0000_0018_3433_3231);
            put(&mut a, 0x10, 0x01d9_b100);
            put(&mut a, 0x58, 0x0000_0005_0005_0030);
            let a_list = pairs(&[
                (0x10a, 0x0043_3a3a),
                (0x10b, 0x74),
                (0x14a, 0x0300_0005),
                (0x100, 0x1d00_03e0),
                (0x100, 0x5624_e9ee),
            ]);
            a[0xa8..0xa8 + a_list.len()].copy_from_slice(&a_list);
            let at = a.as_ptr() as u64 + 0xa8;
            put(&mut a, 0x20, at);

            put(&mut b, 0x00, 0x0000_0018_3433_3231);
            put(&mut b, 0x08, 0x1111);
            put(&mut b, 0x10, 0x01d9_b600);
            put(&mut b, 0x18, 0x2222);
            put(&mut b, 0x28, 0x3333);
            put(&mut b, 0x40, 0x0000_0e20_0000_0158);
            put(&mut b, 0x48, 0x0000_000b_0000_0000);
            put(&mut b, 0x58, 0x0000_000a_0407_0030);
            let b_list = pairs(&[
                (0x148, 0x005b_7320),
                (0x149, 0x74),
                (0x100, 0),
                (0x100, 0),
                (0x10a, 0x312c_01d1),
                (0x10b, 0x006c_0094),
                (0x108, 0),
                (0x109, 0),
                (0x100, 0x4700_0df0),
                (0x100, 0x8f0f_cc11),
            ]);
            b[0x90..0x90 + b_list.len()].copy_from_slice(&b_list);
            let at = b.as_ptr() as u64 + 0x90;
            put(&mut b, 0x20, at);
            Self { a, b }
        }

        fn args(out: u64, first: &[u8], second: &[u8], aux: u64) -> [u64; GUEST_ARG_REGISTERS] {
            [
                out,
                first.as_ptr() as u64,
                second.as_ptr() as u64,
                aux,
                4,
                0,
            ]
        }
    }

    /// The bytes the console wrote for the pair, but for the code pointers and list, which are
    /// this test's own addresses.
    #[test]
    fn the_measured_pair_merges_as_the_console_wrote_it() {
        let pair = Pair::new();
        let mut out = [0xcc_u8; 0x200];
        let mut aux = [0xcc_u8; 0x200];
        let args = Pair::args(
            out.as_mut_ptr() as u64,
            &pair.a,
            &pair.b,
            aux.as_mut_ptr() as u64,
        );
        assert_eq!(super::merge_shaders(&args), 0);
        let expected = pairs(&[
            (0x148, 0x0001_d9b1),
            (0x149, 0),
            (0x100, 0x1d00_03e0),
            (0x100, 0x5624_e9ee),
            (0x10a, 0x312c_01fa),
            (0x10b, 0x006c_00b4),
            (0x108, 0),
            (0x109, 0),
            (0x100, 0x4700_0df0),
            (0x100, 0x8f0f_cc11),
        ]);
        assert_eq!(&aux[..0x50], &expected[..]);
        assert!(aux[0x50..].iter().all(|&byte| byte == 0xcc));
        let mut record = pair.b[..0x60].to_vec();
        record[0x08..0x10].fill(0);
        record[0x20..0x28].copy_from_slice(&(aux.as_ptr() as u64).to_le_bytes());
        record[0x5a] = 3;
        assert_eq!(&out[..0x60], &record[..]);
        assert!(out[0x60..].iter().all(|&byte| byte == 0xcc));

        let swapped = Pair::args(
            out.as_mut_ptr() as u64,
            &pair.b,
            &pair.a,
            aux.as_mut_ptr() as u64,
        );
        assert_eq!(super::merge_shaders(&swapped), 0x8a6c_0008);
    }

    #[test]
    fn the_sizes_are_b_s_register_counts() {
        let pair = Pair::new();
        let mut out = [0xcc_u8; 0x20];
        let state = [0xcc_u8; 0x20];
        let args = Pair::args(
            out.as_mut_ptr() as u64,
            &pair.a,
            &pair.b,
            state.as_ptr() as u64,
        );
        assert_eq!(super::merged_sizes(&args), 0);
        assert_eq!(&out[..8], &0x50_u64.to_le_bytes());
        assert_eq!(&out[8..16], &4_u64.to_le_bytes());
        assert_eq!(out[16..], [0xcc; 16]);
        assert_eq!(state, [0xcc; 0x20]);
        let swapped = Pair::args(
            out.as_mut_ptr() as u64,
            &pair.b,
            &pair.a,
            state.as_ptr() as u64,
        );
        assert_eq!(super::merged_sizes(&swapped), 0x8a6c_0008);
    }
}
