//! Whether a compute program runs exactly as one invocation per lane.
//!
//! One invocation simulating a whole wave is exact for every program and costs the wave's width in
//! serial work. A lane run as its own invocation is exact only where nothing in the program treats
//! the wave as an object: no instruction reads or writes the wave's masks as a whole, reads another
//! lane, shares the local data share, waits at a barrier, or has a scalar side effect - each
//! invocation computes the scalar state, so a scalar store would be made once per lane.
//!
//! A per-lane bit is not the wave as an object. A comparison or a carry writes one bit per lane,
//! and a selection or a carry-in reads one; where no other instruction reads that register, each
//! invocation can hold only its own lane's bit. [`lane_needs`] counts those apart from the masks
//! read whole.
//!
//! A program that reads a mask whole and needs nothing else of its wave runs per lane too, each
//! invocation holding every lane's bits (D884): by the host subgroup's ballot where the subgroup is
//! the wave, and through workgroup memory where the group is one wave. [`lane_masks`] says which.

use std::collections::BTreeSet;

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};

use crate::model::{EXEC_LOW_HALF, lane_mask_high_name, lane_mask_name};

/// What in a program keeps one invocation per lane from running it exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum LaneNeed {
    /// It reads or writes the execution mask, or reads a lane mask a vector instruction built as a
    /// whole: a branch on it, a scalar operation on it, a lane count under it.
    WholeMask,
    /// It reads another lane's value: a lane read, a permute, a swizzle, a DPP modifier.
    CrossLane,
    /// It reads or writes the local data share, which the wave's lanes share.
    LocalShare,
    /// It waits at `s_barrier`.
    Barrier,
    /// It stores or makes an atomic through the scalar unit, which every lane's invocation would
    /// repeat.
    ScalarEffect,
}

impl core::fmt::Display for LaneNeed {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            Self::WholeMask => "a whole lane mask",
            Self::CrossLane => "another lane's value",
            Self::LocalShare => "the local data share",
            Self::Barrier => "a barrier",
            Self::ScalarEffect => "a scalar store or atomic",
        })
    }
}

/// Why a program with `need` is not run one invocation per lane (D880): as a refusal when asked for,
/// and as the reason the wavefront model was used when not.
#[must_use]
pub const fn refusal(need: LaneNeed) -> &'static str {
    match need {
        LaneNeed::WholeMask => concat!(
            "the program reads or writes a lane mask whole, and one invocation per lane holds only ",
            "its own lane's bits (D880)"
        ),
        LaneNeed::CrossLane => concat!(
            "the program reads another lane's value, and one invocation per lane holds only its own ",
            "lane's registers (D880)"
        ),
        LaneNeed::LocalShare => concat!(
            "the program uses the local data share, which one invocation per lane does not share ",
            "between a wave's lanes (D880)"
        ),
        LaneNeed::Barrier => concat!(
            "the program waits at a barrier, which one invocation per lane does not hold its wave ",
            "at (D880)"
        ),
        LaneNeed::ScalarEffect => concat!(
            "the program stores or makes an atomic through the scalar unit, which one invocation ",
            "per lane would make once for every lane (D880)"
        ),
    }
}

/// How a dispatch run one invocation per lane holds its lane masks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LaneMasks {
    /// Each invocation its own lane's bits: nothing reads a mask whole (D880).
    Own,
    /// Every lane's bits, by the host subgroup's ballot: the subgroup is the wave (D884).
    Ballot,
    /// Every lane's bits, through workgroup memory between barriers: the group is one wave (D884).
    Exchange,
}

/// Why a program that reads a mask whole is not run one invocation per lane where neither exchange
/// serves (D884).
pub const WHOLE_MASK_ACROSS_WAVES: &str = concat!(
    "the program reads or writes a lane mask whole, its group is several waves, and the host's ",
    "subgroup does not hold a wave: a mask made through workgroup memory would place barriers ",
    "in control flow the group's waves do not share (D884)"
);

/// How a dispatch of a program with `needs`, in waves of `lanes`, groups of `waves` of them, runs
/// one invocation per lane on a host whose subgroup is `subgroup` wide (`None` where it is not
/// known), or why it does not.
///
/// # Errors
///
/// Why the program keeps the whole wave in one invocation: any need but a whole mask (D880), or a
/// whole mask in a group of several waves the host's subgroup cannot ballot (D884).
pub fn lane_masks(
    needs: &LaneNeeds,
    lanes: u32,
    subgroup: Option<u32>,
    waves: u32,
) -> Result<LaneMasks, &'static str> {
    if let Some(&(need, _, _)) = needs
        .needs
        .iter()
        .find(|(need, _, _)| *need != LaneNeed::WholeMask)
    {
        return Err(refusal(need));
    }
    if !needs.has(LaneNeed::WholeMask) {
        Ok(LaneMasks::Own)
    } else if subgroup == Some(lanes) {
        Ok(LaneMasks::Ballot)
    } else if waves == 1 {
        Ok(LaneMasks::Exchange)
    } else {
        Err(WHOLE_MASK_ACROSS_WAVES)
    }
}

/// What a program needs of the wave, each need with the first instruction that has it, and whether
/// it holds per-lane bits no instruction reads whole.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct LaneNeeds {
    /// Each need once, in the order first met, with that instruction's offset and name.
    pub needs: Vec<(LaneNeed, u32, String)>,
    /// Whether a comparison or a carry writes, or a selection or carry-in reads, a per-lane bit.
    pub lane_bits: bool,
}

impl LaneNeeds {
    /// Whether one invocation per lane runs the program exactly.
    #[must_use]
    pub fn exact(&self) -> bool {
        self.needs.is_empty()
    }

    /// Whether the program has `need`.
    #[must_use]
    pub fn has(&self, need: LaneNeed) -> bool {
        self.needs.iter().any(|(held, _, _)| *held == need)
    }

    fn add(&mut self, need: LaneNeed, instruction: &Instruction, name: &str) {
        if !self.has(need) {
            self.needs.push((need, instruction.offset, name.to_owned()));
        }
    }
}

/// Where a vector instruction holds a per-lane mask, by operand position: the one it writes, and the
/// one it reads.
pub(crate) fn mask_positions(name: &str, operands: usize) -> (Option<usize>, Option<usize>) {
    const CARRY_OUT: [&str; 7] = [
        "v_add_co_u32",
        "v_sub_co_u32",
        "v_subrev_co_u32",
        "v_div_scale_f32",
        "v_div_scale_f64",
        "v_mad_u64_u32",
        "v_mad_i64_i32",
    ];
    const CARRY_THROUGH: [&str; 3] = ["v_addc_co_u32", "v_subb_co_u32", "v_subbrev_co_u32"];
    let last = operands.checked_sub(1);
    if name.starts_with("v_cmp_") {
        (Some(0), None)
    } else if CARRY_THROUGH.iter().any(|base| name.starts_with(base)) {
        (Some(1), last)
    } else if CARRY_OUT.iter().any(|base| name.starts_with(base)) {
        (Some(1), None)
    } else if name.starts_with("v_cndmask_b32") {
        (None, last)
    } else {
        (None, None)
    }
}

/// Whether an instruction reads another lane, by name.
fn crosses_lanes(name: &str) -> bool {
    [
        "v_readlane",
        "v_readfirstlane",
        "v_writelane",
        "v_permlane",
        "ds_swizzle",
        "ds_permute",
        "ds_bpermute",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

/// Whether a scalar instruction stores or makes an atomic.
fn scalar_effect(name: &str) -> bool {
    [
        "s_store_",
        "s_buffer_store_",
        "s_atomic_",
        "s_buffer_atomic_",
    ]
    .iter()
    .any(|prefix| name.starts_with(prefix))
}

/// The scalar registers an operand names as a per-lane mask: one in a wave of thirty-two, a pair in
/// one of sixty-four.
fn mask_registers(operand: &Operand, wide: bool) -> Vec<u16> {
    match operand {
        Operand::Scalar(register) if wide => vec![*register, register.wrapping_add(1)],
        Operand::Scalar(register) => vec![*register],
        _ => Vec::new(),
    }
}

/// Each instruction of `decode` with a name, by the name its family gives its opcode.
fn named<'a>(
    decode: &'a Decode,
    encodings: &'a EncodingTable,
) -> impl Iterator<Item = (&'a Instruction, &'a str)> {
    let families = encodings.encodings();
    decode.instructions.iter().filter_map(move |instruction| {
        let family = families.get(usize::from(instruction.encoding?))?;
        let name = encodings.mnemonic_for(&family.name, instruction.opcode)?;
        Some((instruction, name))
    })
}

/// What `decode` needs of a wave of `lanes`.
#[must_use]
pub fn lane_needs(decode: &Decode, encodings: &EncodingTable, lanes: u32) -> LaneNeeds {
    let wide = lanes > 32;
    let families = encodings.encodings();

    // The scalar registers a vector instruction writes or reads as a per-lane mask. Gathered over
    // the whole program, so any other read of one, anywhere, counts as reading it whole.
    let mut lane_masks = BTreeSet::new();
    for (instruction, name) in named(decode, encodings) {
        let (writes, reads) = mask_positions(name, instruction.operands.len());
        for position in [writes, reads].into_iter().flatten() {
            if let Some(operand) = instruction.operands.get(position) {
                lane_masks.extend(mask_registers(operand, wide));
            }
        }
    }

    let mut needs = LaneNeeds::default();
    for (instruction, name) in named(decode, encodings) {
        let dpp = instruction
            .encoding
            .and_then(|e| families.get(usize::from(e)))
            .and_then(|family| family.modifier_selected(&[instruction.word]))
            .is_some_and(|marker| !crate::model::sdwa_translated(marker, instruction, encodings));
        if crosses_lanes(name) || dpp {
            needs.add(LaneNeed::CrossLane, instruction, name);
        } else if name.starts_with("ds_") {
            needs.add(LaneNeed::LocalShare, instruction, name);
        }
        if name == "s_barrier" {
            needs.add(LaneNeed::Barrier, instruction, name);
        }
        if scalar_effect(name) {
            needs.add(LaneNeed::ScalarEffect, instruction, name);
        }
        if whole_mask(instruction, name, &lane_masks) {
            needs.add(LaneNeed::WholeMask, instruction, name);
        }
        let (writes, reads) = mask_positions(name, instruction.operands.len());
        needs.lane_bits |= writes.is_some() || reads.is_some() || name == "v_div_fmas_f32";
    }
    needs
}

/// Whether an instruction reads or writes a lane mask as a whole.
fn whole_mask(instruction: &Instruction, name: &str, lane_masks: &BTreeSet<u16>) -> bool {
    let branches = matches!(
        name,
        "s_cbranch_vccz" | "s_cbranch_vccnz" | "s_cbranch_execz" | "s_cbranch_execnz"
    );
    let writes_exec = name.starts_with("v_cmpx_")
        || name.ends_with("_saveexec_b64")
        || name.ends_with("_saveexec_b32")
        || name.starts_with("s_wqm_");
    if branches || writes_exec {
        return true;
    }
    let (writes, reads) = mask_positions(name, instruction.operands.len());
    let pair = name.ends_with("_b64") || name.ends_with("_u64") || name.ends_with("_i64");
    instruction
        .operands
        .iter()
        .enumerate()
        .any(|(position, operand)| {
            let as_lane_bit = Some(position) == writes || Some(position) == reads;
            match operand {
                Operand::Named(named) => {
                    match lane_mask_name(named).or_else(|| lane_mask_high_name(named)) {
                        // Every vector instruction reads its own lane's execution bit and names the
                        // mask nowhere, so one that names it reads it whole.
                        Some(EXEC_LOW_HALF) => true,
                        // The condition mask as a per-lane bit is that lane's; anywhere else, all.
                        Some(_) => !as_lane_bit,
                        None => false,
                    }
                }
                Operand::Scalar(register) if !as_lane_bit => {
                    lane_masks.contains(register)
                        || (pair && lane_masks.contains(&register.wrapping_add(1)))
                }
                _ => false,
            }
        })
}

#[cfg(test)]
mod tests {
    use super::{LaneMasks, LaneNeed, lane_masks, lane_needs};
    use orbistoun_shader::{EncodingTable, OperandTable, decode};

    const END: u32 = 0xBF81_0000;

    fn needs(words: &[u32]) -> super::LaneNeeds {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = words.iter().flat_map(|w| w.to_le_bytes()).collect();
        lane_needs(&decode(&bytes, &encodings, &operands), &encodings, 64)
    }

    /// A program of per-lane arithmetic needs nothing of the wave.
    #[test]
    fn per_lane_arithmetic_is_exact() {
        // v_mov_b32 v1, v0
        let found = needs(&[0x7E02_0300, END]);
        assert!(found.exact(), "{found:?}");
        assert!(!found.lane_bits);
    }

    /// Writing the execution mask needs the whole mask.
    #[test]
    fn writing_exec_needs_the_whole_mask() {
        // s_mov_b64 exec, 0
        let encodings = EncodingTable::builtin().expect("encodings");
        let (family, opcode) = encodings.find_by_name("s_mov_b64").expect("s_mov_b64");
        let encoding = encodings
            .encodings()
            .iter()
            .find(|e| e.name == family)
            .expect("its family");
        let word = encoding.value | (opcode << encoding.opcode.shift) | (126 << 16) | 128;
        let found = needs(&[word, END]);
        assert!(found.has(LaneNeed::WholeMask), "{found:?}");
    }

    /// A whole mask is made by ballot where the subgroup is the wave, through workgroup memory in a
    /// group of one wave, and not at all in a group of several on a narrower subgroup (D884).
    #[test]
    fn a_whole_mask_is_exchanged_where_the_host_can() {
        let whole = super::LaneNeeds {
            needs: vec![(LaneNeed::WholeMask, 0, "s_cbranch_execz".to_owned())],
            lane_bits: true,
        };
        assert_eq!(lane_masks(&whole, 32, Some(32), 4), Ok(LaneMasks::Ballot));
        assert_eq!(lane_masks(&whole, 64, Some(64), 2), Ok(LaneMasks::Ballot));
        assert_eq!(lane_masks(&whole, 64, Some(32), 1), Ok(LaneMasks::Exchange));
        assert_eq!(lane_masks(&whole, 64, None, 1), Ok(LaneMasks::Exchange));
        assert!(lane_masks(&whole, 64, Some(32), 2).is_err());
        assert_eq!(
            lane_masks(&super::LaneNeeds::default(), 64, Some(32), 2),
            Ok(LaneMasks::Own)
        );
        let shared = super::LaneNeeds {
            needs: vec![
                (LaneNeed::WholeMask, 0, "s_cbranch_execz".to_owned()),
                (LaneNeed::LocalShare, 4, "ds_write_b32".to_owned()),
            ],
            lane_bits: false,
        };
        assert!(lane_masks(&shared, 32, Some(32), 1).is_err());
    }
}
