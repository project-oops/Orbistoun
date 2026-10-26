//! What every wavefront model provides, and the instruction dispatch they share.
//!
//! An instruction is translated once, here, against the [`Model`] trait, so the fidelity levels
//! cannot disagree about what an instruction means. The models differ only in how many lanes an
//! invocation emits, where a register lives, and whether a write is masked.

use orbistoun_shader::{EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Builder, Id, image_operands, op};

use crate::TranslateError;
use crate::modifiers::Modifiers;

pub(crate) mod bvh;
mod device;
mod running_format;
pub(crate) mod sdwa;

/// Every instruction the translator understands, by name (D139).
///
/// Opcode numbers belong to one architecture generation and move between generations, so a list of
/// numbers retargeted elsewhere binds silently to whatever occupies them. A name the loaded target
/// lacks is reported by [`unresolved`] instead. The names are the ones the reference assembler
/// prints, recorded by the probe solver alongside each opcode's operand layout.
pub const SUPPORTED: &[&str] = &[
    "exp",
    "v_interp_mov_f32_e32",
    "v_interp_p1_f32_e32",
    "v_interp_p2_f32_e32",
    "buffer_load_dword",
    "buffer_load_dwordx2",
    "buffer_load_dwordx3",
    "buffer_load_dwordx4",
    "buffer_store_dword",
    "buffer_store_dwordx2",
    "buffer_store_dwordx3",
    "buffer_store_dwordx4",
    "buffer_load_format_x",
    "buffer_load_format_xy",
    "buffer_load_format_xyz",
    "buffer_load_format_xyzw",
    "buffer_store_format_x",
    "buffer_store_format_xy",
    "buffer_store_format_xyz",
    "buffer_store_format_xyzw",
    "tbuffer_load_format_x",
    "tbuffer_load_format_xy",
    "tbuffer_load_format_xyz",
    "tbuffer_load_format_xyzw",
    "tbuffer_store_format_x",
    "tbuffer_store_format_xy",
    "tbuffer_store_format_xyz",
    "tbuffer_store_format_xyzw",
    "ds_read_b32",
    "ds_write_b32",
    "ds_read_addtid_b32",
    "ds_write_addtid_b32",
    "ds_write2_b32",
    "ds_read2_b32",
    "ds_write2_b64",
    "ds_read2_b64",
    "ds_read_b96",
    "ds_write_b96",
    "ds_min_u32",
    "ds_min_f32",
    "ds_max_f32",
    "ds_write_b64",
    "ds_read_b64",
    "ds_read_b128",
    "s_min_i32",
    "s_max_i32",
    "s_min_u32",
    "s_max_u32",
    "s_sub_u32",
    "s_not_b32",
    "s_bitset1_b32",
    "v_ffbh_u32_e32",
    "v_movrels_b32_e32",
    "v_bfe_i32",
    "v_alignbyte_b32",
    "v_alignbit_b32",
    "s_orn2_saveexec_b64",
    "s_cmp_lg_u64",
    "s_cmp_eq_u64",
    "v_movreld_b32_e32",
    "v_permlanex16_b32",
    "v_writelane_b32",
    "v_add_f16_e32",
    "v_sub_f16_e32",
    "v_subrev_f16_e32",
    "v_mul_f16_e32",
    "v_max_f16_e32",
    "v_min_f16_e32",
    "v_fmac_f16_e32",
    "buffer_atomic_add",
    "buffer_atomic_swap_x2",
    "buffer_atomic_or_x2",
    "buffer_atomic_fmin",
    "buffer_atomic_fmax",
    "buffer_load_ushort",
    "ds_max_u32",
    "ds_add_rtn_u32",
    "ds_write_b128",
    "ds_write_b8",
    "ds_read_u8",
    "ds_or_b32",
    "image_load",
    "image_load_mip",
    "image_sample",
    "image_sample_l",
    "image_sample_d",
    "image_sample_lz",
    "image_gather4_lz",
    "image_sample_lz_o",
    "image_sample_c_lz",
    "image_sample_c",
    "image_store",
    "image_store_mip",
    "image_get_resinfo",
    "image_bvh_intersect_ray",
    "image_bvh64_intersect_ray",
    "global_load_ubyte",
    "global_load_sbyte",
    "global_load_ushort",
    "global_load_sshort",
    "global_load_dword",
    "global_load_dwordx2",
    "global_load_dwordx3",
    "global_load_dwordx4",
    "global_store_dword",
    "global_store_dwordx2",
    "global_store_dwordx3",
    "global_store_dwordx4",
    "global_store_byte",
    "global_store_short",
    "buffer_store_byte",
    "buffer_store_short",
    "s_add_i32",
    "s_addk_i32",
    "s_and_b32",
    "s_brev_b32",
    "s_getpc_b64",
    "s_andn2_b32",
    "s_orn2_b32",
    "s_and_b64",
    "s_lshl_b64",
    "s_lshr_b64",
    "s_lshl1_add_u32",
    "s_lshl2_add_u32",
    "s_lshl3_add_u32",
    "s_lshl4_add_u32",
    "s_andn2_b64",
    "s_barrier",
    "s_bfe_u32",
    "s_bfe_u64",
    "s_xor_b64",
    "s_nor_b64",
    "s_orn2_b64",
    "s_nand_b64",
    "s_not_b64",
    "s_bitreplicate_b64_b32",
    "s_lshr_b32",
    "s_add_u32",
    "s_addc_u32",
    "s_subb_u32",
    "s_cselect_b32",
    "s_bitcmp1_b32",
    "s_bitcmp0_b32",
    "s_nor_b32",
    "s_nand_b32",
    "s_trap",
    "v_not_b32_e32",
    "v_not_b32_e64",
    "ds_bpermute_b32",
    "v_cmpx_class_f32_e32",
    "v_cmp_eq_u64_e32",
    "v_cmp_ne_u64_e32",
    "v_cmp_lt_u64_e32",
    "v_cmp_le_u64_e32",
    "v_cmp_gt_u64_e32",
    "v_cmp_ge_u64_e32",
    "v_cmp_eq_i64_e32",
    "v_cmp_ne_i64_e32",
    "v_cmp_lt_i64_e32",
    "v_cmp_le_i64_e32",
    "v_cmp_gt_i64_e32",
    "v_cmp_ge_i64_e32",
    "v_cmpx_eq_u64_e32",
    "v_cmpx_ne_u64_e32",
    "v_cmpx_lt_u64_e32",
    "v_cmpx_le_u64_e32",
    "v_cmpx_gt_u64_e32",
    "v_cmpx_ge_u64_e32",
    "v_cmpx_eq_i64_e32",
    "v_cmpx_ne_i64_e32",
    "v_cmpx_lt_i64_e32",
    "v_cmpx_le_i64_e32",
    "v_cmpx_gt_i64_e32",
    "v_cmpx_ge_i64_e32",
    "buffer_atomic_or",
    "ds_add_u32",
    "s_andn1_saveexec_b32",
    "s_orn2_saveexec_b32",
    "v_ffbl_b32_e32",
    "buffer_gl0_inv",
    "buffer_gl1_inv",
    "buffer_atomic_and",
    "buffer_atomic_xor",
    "buffer_atomic_sub",
    "buffer_atomic_umin",
    "buffer_atomic_umax",
    "buffer_atomic_smin",
    "buffer_atomic_smax",
    "s_and_saveexec_b64",
    "s_andn1_saveexec_b64",
    "s_and_saveexec_b32",
    "s_bcnt1_i32_b64",
    "s_ff1_i32_b32",
    "s_ff1_i32_b64",
    "v_rndne_f32_e32",
    "v_floor_f32_e32",
    "v_ceil_f32_e32",
    "v_fract_f32_e32",
    "v_trunc_f32_e32",
    "v_bfrev_b32_e32",
    "v_cmpx_gt_f32_e32",
    "v_cmpx_lt_f32_e32",
    "v_cmpx_eq_f32_e32",
    "v_cmpx_nlt_f32_e32",
    "v_cmpx_eq_i32_e32",
    "v_cmpx_gt_i32_e32",
    "v_cmpx_gt_u32_e32",
    "v_cmpx_lt_u32_e32",
    "v_cmpx_eq_u32_e32",
    "v_cmpx_ne_u32_e32",
    "v_cmpx_le_f32_e32",
    "v_cmpx_lg_f32_e32",
    "v_cmpx_ge_f32_e32",
    "v_cmpx_nge_f32_e32",
    "v_cmpx_nlg_f32_e32",
    "v_cmpx_ngt_f32_e32",
    "v_cmpx_nle_f32_e32",
    "v_cmpx_lt_i32_e32",
    "v_cmpx_ne_i32_e32",
    "v_cmpx_ge_i32_e32",
    "v_cmpx_le_u32_e32",
    "v_cmpx_ge_u32_e32",
    "v_cmp_gt_u32_e32",
    "v_cmp_gt_u32_e64",
    "v_cmp_eq_u64_e64",
    "v_cmp_ne_u64_e64",
    "v_cvt_f32_ubyte0_e32",
    "v_cvt_f32_ubyte1_e32",
    "v_cvt_f32_ubyte2_e32",
    "v_cvt_f32_ubyte3_e32",
    "v_bfe_u32",
    "v_nop",
    "v_cvt_pkrtz_f16_f32_e64",
    "v_cvt_pknorm_u16_f32",
    "v_cvt_pknorm_i16_f32",
    "v_cvt_pk_u16_u32",
    "v_cvt_pk_i16_i32",
    "s_bfm_b32",
    "s_bfm_b64",
    "s_buffer_load_dword",
    "s_buffer_load_dwordx2",
    "s_buffer_load_dwordx4",
    "s_buffer_load_dwordx8",
    "s_buffer_load_dwordx16",
    "s_branch",
    "s_cbranch_execnz",
    "s_cbranch_execz",
    "s_cbranch_scc0",
    "s_cbranch_scc1",
    "s_cbranch_vccnz",
    "s_cbranch_vccz",
    "s_clause",
    "s_cmp_eq_i32",
    "s_cmp_ge_i32",
    "s_cmp_gt_i32",
    "s_cmp_le_i32",
    "s_cmp_lg_i32",
    "s_cmp_lg_u32",
    "s_cmp_eq_u32",
    "s_cmp_lt_i32",
    "s_cmp_gt_u32",
    "s_cmp_ge_u32",
    "s_cmp_lt_u32",
    "s_cmp_le_u32",
    "s_cmpk_eq_i32",
    "s_cmpk_lg_i32",
    "s_cmpk_gt_i32",
    "s_cmpk_ge_i32",
    "s_cmpk_lt_i32",
    "s_cmpk_le_i32",
    "s_cselect_b64",
    "s_endpgm",
    "s_load_dword",
    "s_load_dwordx2",
    "s_load_dwordx4",
    "s_load_dwordx8",
    "s_load_dwordx16",
    "s_lshl_b32",
    "s_mov_b32",
    "s_nop",
    "s_sendmsg",
    "s_mov_b64",
    "s_movk_i32",
    "s_setreg_b32",
    "s_setreg_imm32_b32",
    "s_inst_prefetch",
    "s_mulk_i32",
    "s_mul_i32",
    "s_mul_hi_u32",
    "s_or_b32",
    "s_or_b64",
    "s_pack_hh_b32_b16",
    "s_pack_ll_b32_b16",
    "s_setprio",
    "s_sub_i32",
    "s_waitcnt",
    "s_waitcnt_vscnt",
    "s_waitcnt_depctr",
    "s_wqm_b32",
    "s_wqm_b64",
    "s_quadmask_b64",
    "s_xor_b32",
    "v_add_co_u32",
    "v_add_f32_e32",
    "v_add_f32_e64",
    "v_add_nc_u32_e32",
    "v_sub_nc_u32_e32",
    "v_subrev_nc_u32_e32",
    "v_add_co_ci_u32_e32",
    "v_add_co_ci_u32_e64",
    "v_cmp_lt_f32_e32",
    "v_cmp_eq_f32_e32",
    "v_cmp_le_f32_e32",
    "v_cmp_gt_f32_e32",
    "v_cmp_ge_f32_e32",
    "v_cmp_neq_f32_e32",
    "v_cmp_ngt_f32_e32",
    "v_cmp_nlt_f32_e32",
    "v_cmp_u_f32_e32",
    "v_cmp_lg_f32_e32",
    "v_cmp_nge_f32_e32",
    "v_cmp_nlg_f32_e32",
    "v_cmp_nle_f32_e32",
    "v_cmp_class_f32_e32",
    "v_cmp_eq_i32_e32",
    "v_cmp_ne_i32_e32",
    "v_cmp_lt_i32_e32",
    "v_cmp_le_i32_e32",
    "v_cmp_gt_i32_e32",
    "v_cmp_ge_i32_e32",
    "v_cmp_lt_u32_e32",
    "v_cmp_le_u32_e32",
    "v_cmp_eq_u32_e32",
    "v_cmp_ne_u32_e32",
    "v_cmp_ge_u32_e32",
    "v_cmp_lt_f32_e64",
    "v_cmp_eq_f32_e64",
    "v_cmp_le_f32_e64",
    "v_cmp_gt_f32_e64",
    "v_cmp_ge_f32_e64",
    "v_cmp_neq_f32_e64",
    "v_cmp_nlt_f32_e64",
    "v_cmp_lg_f32_e64",
    "v_cmp_nge_f32_e64",
    "v_cmp_nlg_f32_e64",
    "v_cmp_nle_f32_e64",
    "v_cmp_ngt_f32_e64",
    "v_cmp_ne_i32_e64",
    "v_cmp_lt_i32_e64",
    "v_cmp_eq_i32_e64",
    "v_cmp_le_i32_e64",
    "v_cmp_gt_i32_e64",
    "v_cmp_ge_i32_e64",
    "v_bfi_b32",
    "v_cmpx_le_i16_e32",
    "v_cmpx_lt_i16_e32",
    "v_cmpx_le_i32_e32",
    "v_cmpx_neq_f32_e32",
    "v_cndmask_b32_e32",
    "v_cndmask_b32_e64",
    "v_cos_f32_e32",
    "v_cvt_pkrtz_f16_f32_e32",
    "v_cvt_f32_u32_e32",
    "v_cvt_f32_i32_e32",
    "v_cvt_f32_u32_e64",
    "v_cvt_f32_i32_e64",
    "v_cvt_u32_f32_e64",
    "v_cvt_i32_f32_e64",
    "v_cvt_i32_f32_e32",
    "v_cvt_u32_f32_e32",
    "v_div_fixup_f32",
    "v_fmac_f32_e32",
    "v_div_scale_f32",
    "v_div_fmas_f32",
    "v_exp_f32_e32",
    "v_fma_f32",
    "v_mad_f32",
    "v_mac_f32_e32",
    "v_madak_f32",
    "v_madmk_f32",
    "v_fmaak_f32",
    "v_fmamk_f32",
    "v_mul_u32_u24_e32",
    "v_min_u32_e32",
    "v_max_u32_e32",
    "v_min_i32_e32",
    "v_max_i32_e32",
    "v_cvt_rpi_i32_f32_e32",
    "v_cvt_flr_i32_f32_e32",
    "v_cvt_f16_f32_e32",
    "v_mad_u32_u24",
    "v_mad_u32_u16",
    "v_mad_i32_i24",
    "v_mul_lo_u32",
    "v_mul_hi_u32",
    "v_mul_hi_i32",
    "v_bfm_b32",
    "v_bcnt_u32_b32",
    "v_ldexp_f32",
    "v_and_or_b32",
    "v_or3_b32",
    "v_max3_f32",
    "v_min3_f32",
    "v_med3_f32",
    "v_med3_u32",
    "v_cubeid_f32",
    "v_cubesc_f32",
    "v_cubetc_f32",
    "v_cubema_f32",
    "v_msad_u8",
    "v_readlane_b32",
    "v_readfirstlane_b32",
    "v_permlane16_b32",
    "v_log_f32_e32",
    "v_lshlrev_b32_e32",
    "v_lshrrev_b32_e32",
    "v_ashrrev_i32_e32",
    "v_lshl_add_u32",
    "v_add3_u32",
    "v_add_lshl_u32",
    "v_lshl_or_b32",
    "v_perm_b32",
    "v_add_nc_u16",
    "v_lshlrev_b16",
    "v_pack_b32_f16",
    "v_pk_mad_u16",
    "v_fma_mix_f32",
    "v_fma_mixlo_f16",
    "v_fma_mixhi_f16",
    "v_pk_lshlrev_b16",
    "v_pk_sub_u16",
    "v_pk_add_u16",
    "v_pk_sub_i16",
    "v_pk_add_i16",
    "v_pk_add_f16",
    "v_pk_mul_f16",
    "v_pk_fma_f16",
    "v_pk_min_f16",
    "v_pk_max_f16",
    "v_fma_f16",
    "v_min3_f16",
    "v_max3_f16",
    "v_med3_f16",
    "v_lshrrev_b16",
    "v_sub_nc_i16",
    "v_rcp_f16_e32",
    "v_pk_fmac_f16",
    "v_cmp_lt_f16_e32",
    "v_cmp_eq_f16_e32",
    "v_cmp_le_f16_e32",
    "v_cmp_gt_f16_e32",
    "v_cmp_ge_f16_e32",
    "v_cmp_neq_f16_e32",
    "v_min3_i16",
    "v_lshrrev_b64",
    "v_lshlrev_b64",
    "v_max_f32_e32",
    "v_mbcnt_hi_u32_b32",
    "v_mbcnt_lo_u32_b32",
    "v_min_f32_e32",
    "v_mov_b32_e32",
    "v_mul_f32_e32",
    "v_mul_f32_e64",
    "v_mac_f32_e64",
    "v_or_b32_e32",
    "v_and_b32_e32",
    "v_xor_b32_e32",
    "v_rcp_f32_e32",
    "v_rcp_iflag_f32_e32",
    "v_rcp_f32_e64",
    "v_rsq_f32_e32",
    "v_sin_f32_e32",
    "v_sqrt_f32_e32",
    "v_sub_co_u32",
    "v_sub_f32_e32",
    "v_sub_f32_e64",
    "v_subrev_f32_e32",
    "v_subrev_f32_e64",
];

/// Names this translator understands that the loaded generation does not have.
///
/// Empty on the target the tables were built for; after a retarget it names exactly what needs
/// attention.
pub fn unresolved(encodings: &EncodingTable) -> Vec<&'static str> {
    SUPPORTED
        .iter()
        .copied()
        .filter(|name| encodings.find_by_name(name).is_none())
        .collect()
}

/// Whether the translator understands the instruction at a family and opcode.
///
/// Resolved through the loaded table's names, so the answer follows the generation the tables were
/// generated for. An opcode the table cannot name is not supported: nothing has observed it.
pub fn supports_named(encodings: &EncodingTable, family: &str, opcode: u32) -> bool {
    encodings
        .mnemonic_for(family, opcode)
        .is_some_and(|name| SUPPORTED.contains(&name))
}

/// The operand code a flat access uses to say it has no scalar base register.
///
/// The reference assembler encodes `global_store_dword v[8:9], v10, off offset:4` as `0xdc708004
/// 0x007d0a08`, so the marker is 0x7d, which the operand table names `null`; the same bytes are in
/// `primitive.gcn`. Matched by name because the same code means different things in different
/// fields.
pub const FLAT_NO_BASE: &str = NO_DESTINATION;

/// The byte offset a flat-family access carries: its `OFFSET` field (11:0 of the first word),
/// which a global or scratch access (`SEG`, 15:14, two or one) reads as a signed twelve-bit number
/// and a flat one as unsigned (AMD's published RDNA 2 instruction set, `FLAT`; the reference
/// assembler takes `offset:-2048` for a global access and encodes it `0x800`). The decoder hands
/// the field as it lies; zero where the instruction carries none.
pub fn flat_offset(instruction: &Instruction) -> i64 {
    const SIGN: i64 = 0x800;
    let Some(Operand::Immediate(field)) = instruction.operands.get(3) else {
        return 0;
    };
    let segment = instruction.word >> 14 & 3;
    if segment != 0 && field & SIGN != 0 {
        (field & 0xfff) - 2 * SIGN
    } else {
        *field
    }
}

/// The operand that names no register at all.
///
/// In an address field ([`FLAT_NO_BASE`]) the access has no base register; in a destination field
/// the result is not wanted and is written nowhere. The division pre-scale, for example, may
/// discard its flag this way, and translating that as a register write would invent a destination.
pub const NO_DESTINATION: &str = "null";

/// Whether a destination operand says the result is not wanted.
fn discards(operand: &Operand) -> bool {
    matches!(operand, Operand::Named(name) if name == NO_DESTINATION)
}

/// Scalar registers the guest has: s0 to s105 on GFX10, where Mesa allocates 108 counting the
/// condition mask as s106-s107 (`ac_gpu_info.c:258-261`).
///
/// The operand numbering continues past this into specials and inline constants, so a scalar
/// destination at or above it is not a register. A wide load such as `s_load_dwordx8` at s100 would
/// otherwise write into the condition mask.
pub const SCALAR_REGISTERS: u32 = 106;
/// `vcc_lo`'s operand code, the first past the scalar registers.
const VCC_LOW_CODE: u32 = 106;
/// `vcc_hi`'s operand code.
const VCC_HIGH_CODE: u32 = 107;

/// Scalar registers an image descriptor occupies.
///
/// Eight, as the instruction's resource field names. This translation reads none of the fields (see
/// [`Model::sampled_image`]), but a write anywhere in the group means the descriptor is no longer
/// the one the last sample used.
pub const IMAGE_DESCRIPTOR_REGISTERS: u32 = 8;

/// Scalar registers a sampler descriptor occupies.
pub const SAMPLER_DESCRIPTOR_REGISTERS: u32 = 4;

/// Components a sampling instruction can return, one per bit of its mask.
const IMAGE_COMPONENTS: u32 = 4;

/// Where an image instruction says how many dimensions its coordinate has.
///
/// The field is in the instruction, not the descriptor. Assembling one instruction at each of the
/// eight dimensionalities and differencing the encodings puts it at these bits and nowhere else.
/// The disassembler prints it symbolically (`dim:SQ_RSRC_IMG_2D`), which the operand solver skips,
/// so it is read here directly.
const IMAGE_DIMENSION: (u32, u32) = (3, 0b111);

/// The dimensionality code for a two-dimensional image.
///
/// Codes run upward in the order a disassembler prints them: 1D, 2D, 3D, cube, 1D array, 2D array,
/// 2D multi-sampled, 2D multi-sampled array. Measured from the same eight encodings.
const IMAGE_DIMENSION_2D: u32 = 1;

/// The dimensionality code for a three-dimensional image, third in the same order
/// (`dim:SQ_RSRC_IMG_3D`).
const IMAGE_DIMENSION_3D: u32 = 2;

/// The dimensionality code for a cube, fourth in the same order (`dim:SQ_RSRC_IMG_CUBE`).
const IMAGE_DIMENSION_CUBE: u32 = 3;

/// The dimensionality code for a 2D array, sixth in the same order (`dim:SQ_RSRC_IMG_2D_ARRAY`).
const IMAGE_DIMENSION_2D_ARRAY: u32 = 5;

/// Each dimensionality code's name, in the same order.
const IMAGE_DIMENSION_NAMES: [&str; 8] = [
    "1D",
    "2D",
    "3D",
    "cube",
    "1D array",
    "2D array",
    "2D multi-sampled",
    "2D multi-sampled array",
];

/// What a translated texture sample needs from the model.
///
/// Four identifiers declared together on first use: the variable, what loading it produces, the
/// coordinate type, and the result type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Texture {
    /// The variable holding the image and its sampler together.
    pub variable: Id,
    /// The type a load of [`variable`](Self::variable) produces.
    pub sampled: Id,
    /// The image type inside that, which a fetch takes and a sample does not.
    ///
    /// A guest's `image_load` reads a texel by integer coordinate with no sampler. On the host that
    /// is a fetch, which takes an image, so the bound sampled image is unwrapped to this and
    /// reading a texel needs no binding of its own.
    pub image: Id,
    /// The two-component float vector a two-dimensional sampling coordinate is.
    pub coordinate: Id,
    /// The two-component unsigned vector a two-dimensional texel coordinate is.
    ///
    /// A different type from [`coordinate`](Self::coordinate): a sample takes a normalised
    /// position, a fetch the texel's own index.
    pub texel: Id,
    /// The four-component vector a sample or a fetch answers with: float, or unsigned for an
    /// integer image.
    pub result: Id,
    /// Whether the image has an unsigned integer format, whose texels are raw words.
    pub integer: bool,
    /// Which coordinates, across then down, a sample saturates first: those its sampler clamps to
    /// half a border (D743).
    pub saturate: [bool; 2],
    /// Which coordinates, across then down, a sample mirrors once first: those its sampler mirrors
    /// once, taken as their absolute value - the reflection about zero a mirror-once mode makes -
    /// then clamped as the mode's clamp says.
    pub mirror: [bool; 2],
    /// Where the shader compares this texture against a reference itself: the sampler's
    /// `DEPTH_COMPARE_FUNC` and whether it filters bilinearly. The image is then declared as it is,
    /// not as depth.
    pub shader_compare: Option<(u8, bool)>,
    /// The shape the binding is declared as: a cube, sampled with a three-component direction that
    /// [`coordinate`](Self::coordinate) is the type of (D811); a three-dimensional image, sampled at
    /// three coordinates (D823); or a depth texture sampled against a reference (D816).
    pub shape: ImageShape,
    /// The image and sampler this access reads, already paired, where the module chose the image
    /// as it runs: one a dispatch selected from a table, out of the array every image the table
    /// names is bound in (D983). `None` for a slot's own binding, loaded where it is read.
    pub loaded: Option<Id>,
    /// Whether the array holds no image for the descriptor this access selected, where it chose
    /// one as it runs (D983): an escape, for each lane that runs the access.
    pub missing: Option<Id>,
}

/// The image and sampler `texture` reads, paired: the one its access chose as it ran (D983), or
/// its slot's binding, loaded here.
fn bound_texture(builder: &mut Builder, texture: &Texture) -> Id {
    if let Some(loaded) = texture.loaded {
        return loaded;
    }
    let bound = builder.id();
    builder.function(op::LOAD, &[texture.sampled.0, bound.0, texture.variable.0]);
    bound
}

/// The image `bound`, `texture`'s image and sampler, holds: decorated non-uniform where the access
/// chose it as it ran, as every value an image reached by such an index is read through must be
/// (D983).
fn image_of(builder: &mut Builder, texture: &Texture, bound: Id) -> Id {
    let image = builder.id();
    builder.function(op::IMAGE, &[texture.image.0, image.0, bound.0]);
    if texture.loaded.is_some() {
        builder.annotate(
            op::DECORATE,
            &[image.0, orbistoun_spirv::decoration::NON_UNIFORM],
        );
    }
    image
}

/// Notes the access `texture` serves as an escape for each running lane, where it selected an image
/// the array does not hold (D983).
fn note_unselected<M: Model + ?Sized>(model: &mut M, texture: &Texture) {
    let Some(missing) = texture.missing else {
        return;
    };
    for lane in running_lanes(model) {
        model.note_escape(missing, Some(lane));
    }
}

/// What a translated texture store needs from the model.
///
/// Separate from [`Texture`] because a storage image is a separate binding, and declaring it
/// declares a capability the device may lack (D692).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stored {
    /// The variable holding the storage image.
    pub variable: Id,
    /// The image type a load of [`variable`](Self::variable) produces.
    pub image: Id,
    /// The unsigned vector a texel coordinate is: two components, or three for a volume or an array.
    pub texel: Id,
    /// The four-component vector a texel's value is: float, or unsigned for an integer image.
    pub value: Id,
    /// Whether the image has an unsigned integer format, whose texels are raw words.
    pub integer: bool,
    /// Whether the image is three-dimensional, addressed by three coordinates.
    pub volume: bool,
    /// Whether the image is a 2D array, addressed by two coordinates and a layer (D897).
    pub layered: bool,
}

/// How the execution mask's low half arrives from the decoder.
///
/// A sixty-four-bit operand names its pair by the low register, which the operand table names as a
/// thirty-two-bit register, so `exec` decodes as `exec_lo` and the width comes from the opcode.
pub const EXEC_LOW_HALF: &str = "exec_lo";

/// How the condition mask's low half arrives from the decoder.
///
/// Where a comparison puts its answer, one bit per lane, which a shader then ands into the
/// execution mask.
pub const VCC_LOW_HALF: &str = "vcc_lo";

/// The `m0` register, as the operand table names it.
///
/// One word of scalar state outside the register file: a pixel shader passes the interpolator its
/// primitive mask through it, and a primitive shader its allocation counts. Held apart from the
/// scalar file because its operand code (124) is past the last scalar register.
pub const M0: &str = "m0";

/// The operand table's name for the scalar condition code read as a source, operand code 253.
pub const SRC_SCC: &str = "src_scc";

/// The operand table's name for `1 / (2 * pi)`, operand code 248.
const INVERSE_TWO_PI: &str = "inv_2pi";

/// The bits of `1 / (2 * pi)` as an inline constant holds them: the single-precision value LLVM
/// encodes as operand 248 on `gfx1013` (`v_mov_b32 v0, 0x3e22f983` assembles to `0xf8`).
const INVERSE_TWO_PI_F32: u32 = 0x3e22_f983;

/// The bits an inline float operand named `name` holds, or `None` for a name that is not one: the
/// decimal the operand table names, or `inv_2pi`.
#[must_use]
pub fn inline_float_bits(name: &str) -> Option<u32> {
    if name == INVERSE_TWO_PI {
        return Some(INVERSE_TWO_PI_F32);
    }
    name.parse::<f32>().ok().map(f32::to_bits)
}

/// A lane mask, by whichever spelling reached the translator.
///
/// A source field holding code 106 decodes through the operand table as `vcc_lo`; a comparison's
/// implicit destination comes from the operand layout as the reference printed it, `vcc`. Neither
/// carries the width, which comes from the opcode. Normalising here keeps both tables as observed.
pub fn lane_mask_name(name: &str) -> Option<&'static str> {
    match name {
        "exec" | EXEC_LOW_HALF => Some(EXEC_LOW_HALF),
        "vcc" | VCC_LOW_HALF => Some(VCC_LOW_HALF),
        _ => None,
    }
}

/// The lane mask whose high half a name is: `vcc_hi` is scalar register 107, the condition mask's
/// bits 63:32, and `exec_hi` 127 the execution mask's. A shader may use either as an ordinary
/// scalar, as PPSA03416's primitive shader does `vcc_hi`.
pub fn lane_mask_high_name(name: &str) -> Option<&'static str> {
    match name {
        "exec_hi" => Some(EXEC_LOW_HALF),
        "vcc_hi" => Some(VCC_LOW_HALF),
        _ => None,
    }
}

/// Whether an instruction reads or writes a lane mask.
///
/// Mostly a property of the operands: `s_mov_b64` needs a mask when its destination is `exec` and
/// not for an ordinary register pair, so keying on the opcode would push every 64-bit move onto the
/// slow model. Branches such as `s_cbranch_execz` name no mask in their operands, so for those the
/// opcode decides; missing them would make [`Fidelity::Auto`](crate::Fidelity::Auto) refuse a
/// shader it can translate. The family is required because an opcode number means nothing without
/// it.
pub fn touches_mask(instruction: &Instruction, name: &str) -> bool {
    let branches_on_a_mask = matches!(
        name,
        "s_cbranch_vccz" | "s_cbranch_vccnz" | "s_cbranch_execz" | "s_cbranch_execnz"
    );

    // `v_cndmask_b32` reads one bit of a mask per lane, and whole quad mode operates on a lane mask
    // whose operands may be ordinary register pairs; neither is visible to the operand check below.
    let whole_quad = matches!(name, "s_wqm_b64" | "s_wqm_b32" | "s_quadmask_b64");

    // The local data share is shared between the lanes of a wavefront. A model with one lane per
    // invocation would give each lane its own copy.
    let shares_between_lanes = name.starts_with("ds_");

    // `v_div_fmas_f32` reads the condition mask implicitly, so it is listed rather than detected.
    let selects_per_lane = name == CNDMASK || name == "v_div_fmas_f32";

    // A `v_cmpx` and a saving `s_and_saveexec` write the execution mask and name it nowhere in
    // their operands.
    let writes_exec = name.starts_with("v_cmpx_")
        || name.ends_with("_saveexec_b64")
        || name.ends_with("_saveexec_b32");

    branches_on_a_mask
        || whole_quad
        || shares_between_lanes
        || selects_per_lane
        || writes_exec
        || instruction.operands.iter().any(
            |operand| matches!(operand, Operand::Named(name) if lane_mask_name(name).is_some()),
        )
}

/// Reads a sixty-four-bit source as two thirty-two-bit halves.
///
/// A register pair reads both registers. A constant is sign-extended, not repeated: -1 fills both
/// halves, and 1 sets the low half to one and the high half to zero. `s_mov_b64 exec, -1` is the
/// common case, which repeating would also get right.
fn sixty_four_bit_source<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    source: &Operand,
) -> Result<(Id, Id), TranslateError> {
    match source {
        Operand::Scalar(from) => {
            let from = u32::from(*from);
            if from + 2 > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a 64-bit source reads past the end of the register file",
                });
            }
            Ok((model.read_scalar(from), model.read_scalar(from + 1)))
        }
        Operand::Integer(value) => {
            let value = i32::try_from(*value).map_err(|_| TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "inline constant does not fit in a register",
            })?;
            let high = if value < 0 { u32::MAX } else { 0 };
            Ok((model.constant(value as u32), model.constant(high)))
        }
        Operand::Named(name) if lane_mask_name(name).is_some() => {
            let name = lane_mask_name(name).unwrap_or(EXEC_LOW_HALF);
            model.read_lane_mask(name)
        }
        // A 32-bit literal with its top bit clear: sign and zero extension agree, so the high half
        // is zero whichever the hardware applies. ACO keeps a 64-bit literal as its low 32 bits and
        // an extension flag (`aco_ir.h` 589-595); with the top bit set the two readings differ, and
        // which one a 64-bit operand takes is not established here, so that stays refused.
        Operand::Literal(value) if value & 0x8000_0000 == 0 => {
            Ok((model.constant(*value), model.constant(0)))
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a 64-bit source is neither a register pair, an inline constant, a literal with ",
                "its top bit clear, nor the execution mask"
            ),
        }),
    }
}

/// A per-lane mask a vector instruction reads - `v_cndmask_b32`'s selector, a carry-in - as two
/// halves. In a thirty-two-lane wave a register mask is that one register and the high half is
/// zero: the 32-bit scalar forms ACO uses there, as [`compare_long`] writes it. A named mask, an
/// inline constant, and every mask of a sixty-four-lane wave read as [`sixty_four_bit_source`].
fn lane_mask_source<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    source: &Operand,
) -> Result<(Id, Id), TranslateError> {
    match source {
        Operand::Scalar(from) if model.lanes() <= 32 => {
            let low = model.read_scalar(u32::from(*from));
            Ok((low, model.constant(0)))
        }
        // A wave of 32's mask is one word, which a mask's high half holds as well (`vcc_hi`).
        Operand::Named(name) if model.lanes() <= 32 && lane_mask_high_name(name).is_some() => {
            let mask = lane_mask_high_name(name).unwrap_or(VCC_LOW_HALF);
            let (_, high) = model.read_lane_mask(mask)?;
            Ok((high, model.constant(0)))
        }
        _ => sixty_four_bit_source(model, instruction, source),
    }
}

/// Whether an instruction writes the scalar condition code.
///
/// A side effect on hidden state is invisible in the encoding and operand layout, so this list is
/// taken from the published instruction set. `the_corpus_agrees_about_hidden_side_effects` checks
/// it against compiled fixtures: a compiler places only non-writing instructions between a
/// condition code write and the branch that reads it.
pub fn writes_condition_code(name: &str) -> bool {
    matches!(
        name,
        // The comparisons exist to write it.
        "s_cmp_eq_i32"
            | "s_cmp_lg_i32"
            | "s_cmp_lg_u32"
            | "s_cmp_lg_u64"
            | "s_cmp_eq_u64"
            | "s_cmp_eq_u32"
            | "s_cmp_gt_i32"
            | "s_cmp_ge_i32"
            | "s_cmp_lt_i32"
            | "s_cmp_le_i32"
            | "s_cmp_gt_u32"
            | "s_cmp_ge_u32"
            | "s_cmp_lt_u32"
            | "s_cmp_le_u32"
            | "s_cmpk_eq_i32"
            | "s_cmpk_lg_i32"
            | "s_cmpk_gt_i32"
            | "s_cmpk_ge_i32"
            | "s_cmpk_lt_i32"
            | "s_cmpk_le_i32"
            // The arithmetic sets it on signed overflow.
            | "s_add_i32"
            | "s_sub_i32"
            | "s_addk_i32"
            // The logic sets it to whether the result is non-zero, at either width.
            | "s_and_b32"
            | "s_andn2_b32"
            | "s_orn2_b32"
            | "s_or_b32"
            | "s_xor_b32"
            // The shift and the bit-field extract set it to whether the result is non-zero.
            | "s_lshl_b32"
            | "s_lshr_b32"
            | "s_bfe_u32"
            | "s_bfe_u64"
            | "s_and_b64"
            | "s_or_b64"
            | "s_xor_b64"
            | "s_nor_b64"
            | "s_orn2_b64"
            | "s_andn2_b64"
            | "s_nand_b64"
            | "s_not_b64"
            | "s_not_b32"
            | "s_sub_u32"
            | "s_quadmask_b64"
            // The unsigned add sets it to the carry out; the bit test to the bit; the saving and
            // the count to whether the new mask or the count is non-zero.
            | "s_add_u32"
            | "s_addc_u32"
            | "s_subb_u32"
            | "s_lshl1_add_u32"
            | "s_lshl2_add_u32"
            | "s_lshl3_add_u32"
            | "s_lshl4_add_u32"
            | "s_bitcmp1_b32"
            | "s_bitcmp0_b32"
            | "s_nor_b32"
            | "s_nand_b32"
            | "s_and_saveexec_b64"
            | "s_andn1_saveexec_b64"
            | "s_orn2_saveexec_b64"
            | "s_and_saveexec_b32"
            | "s_andn1_saveexec_b32"
            | "s_orn2_saveexec_b32"
            | "s_bcnt1_i32_b64"
    )
}

/// Instructions that read the condition code: the branches, and the selects.
pub fn reads_condition_code(name: &str) -> bool {
    matches!(
        name,
        "s_cbranch_scc0" | "s_cbranch_scc1" | "s_cselect_b32" | "s_cselect_b64"
    )
}

/// Whether an instruction only invalidates a cache: `buffer_gl0_inv` and `buffer_gl1_inv`, which
/// name no descriptor and move no data (AMD's published RDNA 2 instruction set, MUBUF; GFX10
/// opcodes 0x71 and 0x72 in Mesa's `src/amd/compiler/aco_opcodes.py:1771-1772`). LLVM emits them
/// for an acquire at agent scope on this generation (`SIGfx10CacheControl::insertAcquire`,
/// `llvm/lib/Target/AMDGPU/SIMemoryLegalizer.cpp`). A module's guest memory and its dispatch's
/// buffers are coherent and volatile, so every access already reaches memory, and there is nothing
/// to invalidate.
#[must_use]
pub fn invalidates_a_cache(name: &str) -> bool {
    matches!(name, "buffer_gl0_inv" | "buffer_gl1_inv")
}

/// The refusal an instruction gets for not being in [`SUPPORTED`].
///
/// A named constant so a test can tell it from the other refusals a supported instruction can get,
/// for operands or for a fidelity with no lane mask.
pub const NO_TRANSLATION: &str = "no translation for this instruction";

/// Whether the translator understands an instruction, by name.
///
/// Callers holding an instruction rather than a name want [`supports_named`], which resolves
/// through the loaded table.
pub fn supports(mnemonic: &str) -> bool {
    SUPPORTED.contains(&mnemonic)
}

/// Instructions understood well enough to say what they are waiting on (D104).
///
/// Distinguishes "nobody has looked at this" from "this waits on something that is not encoding
/// work", so a worklist does not send effort at a refusal that cannot move. Empty is the goal.
pub const BLOCKED: &[(&str, &str)] = &[];

/// Why an instruction is blocked, if it is one this translator recognises. Keyed by name (D139).
pub fn blocked(name: &str) -> Option<&'static str> {
    BLOCKED
        .iter()
        .find(|(blocked, _)| *blocked == name)
        .map(|(_, reason)| *reason)
}

/// The parts of emission that differ between wavefront models.
pub trait Model {
    /// The encoding table, for naming an instruction's family.
    fn encodings(&self) -> &EncodingTable;

    /// The shader's `DX10_CLAMP` mode (`SPI_SHADER_PGM_RSRC1` bit 21): whether an output clamp
    /// turns a NaN into zero. `None`, the default, refuses a clamped instruction rather than
    /// choosing.
    fn dx10_clamp(&self) -> Option<bool> {
        None
    }

    /// The guest address the program was loaded at, which `s_getpc_b64` reads (D842). `None`, the
    /// default, refuses that instruction rather than inventing a place.
    fn program_address(&self) -> Option<u64> {
        None
    }

    /// Whether the program is known to run with no trap handler installed (`TRAP_PRESENT` clear in
    /// its `PGM_RSRC2`), so `s_trap` changes nothing it computes. `false`, the default, refuses
    /// `s_trap`.
    fn no_trap_handler(&self) -> bool {
        false
    }

    /// How many lanes this model emits code for: one where an invocation is a lane, the full
    /// wavefront where one invocation simulates all of them.
    fn lanes(&self) -> u32;

    /// Whether `lane` is known not to run, where the model knows its execution mask (a lane run,
    /// D758): a compare writes zero for such a lane, so what the lane holds is not read. Answers
    /// `false` wherever that is not known.
    fn lane_known_idle(&self, _lane: u32) -> bool {
        false
    }

    /// How many of [`Self::lanes`] the per-lane loops run, from lane zero: all of them, but in a
    /// model that knows no lane past these is read (a lane run, D758), which leaves each skipped
    /// lane's registers and mask bits as they were.
    fn modelled_lanes(&self) -> u32 {
        self.lanes()
    }

    /// Whether `lane` may be active here. `false` only where the execution mask is known at
    /// translation time with the lane's bit clear, so a masked-write-only instruction can emit
    /// nothing for that lane.
    ///
    /// The default knows nothing, so every lane may run.
    fn lane_may_run(&self, _lane: u32) -> bool {
        true
    }

    /// Opens a loop over the modelled lanes for the next instruction, one of [`LANE_LOOPED`], and
    /// answers whether it did: its per-lane work is then emitted once, for a lane known only at run
    /// time, rather than once for each lane, so a module grows with its program and not with the
    /// program times the wave's width. The lanes run in order, one after another, as the emitted
    /// lanes did. The default opens none, and its lanes are emitted one by one.
    fn open_lane_loop(&mut self) -> bool {
        false
    }

    /// Closes the loop [`Self::open_lane_loop`] opened.
    fn close_lane_loop(&mut self) {}
    /// Whether a loop over the lanes is open, so the per-lane loops run once, for its lane.
    fn in_lane_loop(&self) -> bool {
        self.looped_lane().is_some()
    }

    /// The lane of the loop over the lanes that is open, which an instruction that names its lane
    /// by number names by this instead.
    fn looped_lane(&self) -> Option<Id> {
        None
    }

    /// As [`Self::open_lane_loop`], for an instruction that builds a mask a bit a lane over every
    /// modelled lane: the loop carries the mask's halves, `carried` on entry, and answers them as
    /// its lane finds them. `None` where it opens none.
    fn open_carrying_lane_loop(&mut self, _carried: (Id, Id)) -> Option<(Id, Id)> {
        None
    }

    /// Closes the loop [`Self::open_carrying_lane_loop`] opened, its lane's halves `carried`, and
    /// answers the halves every lane made.
    fn close_carrying_lane_loop(&mut self, carried: (Id, Id)) -> (Id, Id) {
        carried
    }

    /// Whether buffer stores are dropped: a legacy vertex shader's draw, whose stores are its
    /// streamout, carried out by the streamout pass (D861, D881). The default stores.
    fn drops_streamout_stores(&self) -> bool {
        false
    }

    /// Whether position and parameter exports are dropped rather than written: a legacy vertex
    /// shader's streamout pass, whose rasterizer is killed (D861). The default writes them.
    fn discards_vertex_exports(&self) -> bool {
        false
    }

    /// Control has reached the start of a guest block, which more than one place may branch to, so
    /// anything inferred from the preceding instructions is dropped.
    fn enter_block(&mut self) {}

    /// The translation begins the guest instruction at byte `offset`: what follows belongs to it,
    /// for a model that records instruction by instruction what it did (D970).
    fn begin_instruction(&mut self, offset: u32) {
        let _ = offset;
    }

    /// A constant of the given value, declared once however often it is used.
    fn constant(&mut self, value: u32) -> Id;

    /// The colour output export `target` (`mrt0` to `mrt7`) writes, and its vector type, if the
    /// module has one: a fragment module declares `mrt0`'s always and another target's when one
    /// of its exports names it, at the location of the same number.
    ///
    /// [`None`] for every module that is not a fragment shader, which makes an export into a
    /// compute dispatch a refusal (D553).
    fn colour_output(&self, _target: u32) -> Option<(Id, Id)> {
        None
    }

    /// Whether `mrtK`'s colour target holds integers, so its output is a `uvec4` that takes the
    /// exported bits as they are (D851); `false` for every module without such a target.
    fn colour_is_integer(&self, _target: u32) -> bool {
        false
    }

    /// The `float` depth output of a fragment module that exports depth, and its type; [`None`]
    /// for any other module.
    fn depth_output(&self) -> Option<(Id, Id)> {
        None
    }

    /// The sample mask output of a fragment module that exports one: the `Output` pointer type to
    /// its one `uint`, and the array variable. [`None`] for any other module.
    fn sample_mask_output(&self) -> Option<(Id, Id)> {
        None
    }

    /// Declares how many vertices and primitives this workgroup will emit.
    ///
    /// [`None`] at any stage but mesh; the caller turns that into a refusal naming the
    /// instruction's offset. Both counts are values rather than literals, because the guest writes
    /// them into `m0` and may compute them.
    fn set_mesh_outputs(&mut self, _vertices: Id, _primitives: Id) -> Option<()> {
        None
    }

    /// Writes one vertex's clip-space position, for the lane that is that vertex.
    fn write_mesh_position(&mut self, _lane: u32, _components: [Id; 4]) -> Option<()> {
        None
    }

    /// Writes one vertex's parameter at a location, for the lane that is that vertex.
    fn write_mesh_parameter(
        &mut self,
        _location: u32,
        _lane: u32,
        _components: [Id; 4],
    ) -> Option<()> {
        None
    }

    /// The draw's position-vector routing, `PA_CL_VS_OUT_CNTL` (D867); `None` where it is not known.
    fn position_vectors(&self) -> Option<u32> {
        None
    }

    /// Writes one vertex's exported distance `index`, as the bits of a float, for the lane that is
    /// that vertex (D867); [`None`] for a module that declares no exported distances.
    fn write_mesh_clip_distance(&mut self, _lane: u32, _index: u32, _bits: Id) -> Option<()> {
        None
    }

    /// Writes one vertex's point size, as the bits of a float, for the lane that is that vertex
    /// (D867); [`None`] for a module that declares no point sizes.
    fn write_mesh_point_size(&mut self, _lane: u32, _bits: Id) -> Option<()> {
        None
    }

    /// Writes one vertex's render-target layer, for the lane that is that vertex (D859); [`None`]
    /// for a module that declares no layers.
    fn write_mesh_layer(&mut self, _lane: u32, _layer: Id) -> Option<()> {
        None
    }

    /// Writes one primitive's vertex indices, for the lane that is that primitive: as many as
    /// [`Self::mesh_primitive`] carries, one for a point, two for a line, three for a triangle.
    fn write_mesh_indices(&mut self, _lane: u32, _indices: &[Id]) -> Option<()> {
        None
    }

    /// The primitive this module assembles: meaningful only for a mesh module, and the default
    /// ([`crate::wavefront::MeshPrimitive::Triangles`]) for every other.
    fn mesh_primitive(&self) -> crate::wavefront::MeshPrimitive {
        crate::wavefront::MeshPrimitive::default()
    }

    /// The fragment input carrying an attribute, and its vector type, if this module has one.
    ///
    /// [`None`] for a compute module, and for a fragment module not told that the shader
    /// interpolates that attribute: inputs are declared in the header, before any instruction is
    /// seen.
    /// Vector register `register` at a lane known only when the program runs, or `None` in a model
    /// that does not hold every lane.
    fn read_vector_at(&mut self, register: u32, lane: Id) -> Option<Id> {
        let _ = (register, lane);
        None
    }

    /// This invocation's place in its quad, 0 to 3, where an invocation is one pixel whose quad
    /// neighbours are other invocations: a fragment module that permutes a quad's lanes. `None`
    /// for every other model, whose lanes are its own.
    fn quad_position(&mut self) -> Option<Id> {
        None
    }

    /// `value` as the invocation at place `from` in this invocation's quad holds it, where
    /// [`Self::quad_position`] answers.
    fn quad_broadcast(&mut self, value: Id, from: u32) -> Option<Id> {
        let _ = (value, from);
        None
    }

    /// A flat read takes the attribute's flat twin where the draw gave it one (D742).
    fn attribute_input(&self, _attribute: u32, _flat: bool) -> Option<(Id, Id)> {
        None
    }

    /// The texture this module samples, declaring it on first use.
    ///
    /// `descriptor` and `sampler` are the first scalar register of the two groups a sampling
    /// instruction names: eight registers of image descriptor and four of sampler. No host image
    /// stands behind that guest surface, so a sample reads the texture the pipeline bound, and a
    /// module that names a different descriptor, or reuses registers written since the last sample,
    /// is refused (D690). The sampler is [`None`] for a fetch, which names only an image
    /// descriptor; a fetch and a sample of the same descriptor are one texture.
    ///
    /// The error is a reason rather than a [`TranslateError`], because only the caller knows the
    /// offset.
    ///
    /// `at` is the sampling instruction's offset, which says which load reached its descriptor
    /// registers where the shader reuses them for another texture.
    ///
    /// `shape` is the shape of image the instruction samples, which the binding is then declared
    /// as: a cube, a volume, or compared against a reference (D811, D816, D823).
    fn sampled_image(
        &mut self,
        _descriptor: u32,
        _sampler: Option<u32>,
        (_at, _shape): (u32, ImageShape),
    ) -> Result<Texture, &'static str> {
        Err(concat!(
            "this model has no texture to sample - a sampled image is bound by a graphics ",
            "pipeline, and a compute dispatch has nowhere to put one"
        ))
    }

    /// The storage image this module writes, declaring it on first use.
    ///
    /// A different binding from [`sampled_image`](Self::sampled_image)'s: one is read through a
    /// sampler and cannot be written, the other is written and has no sampler. The module declares
    /// no format, because the guest's format is in a descriptor that is not decoded (D692). A
    /// `volume` is three-dimensional.
    fn storage_image(
        &mut self,
        _descriptor: u32,
        (_shape, _level, _at): (ImageShape, Option<u32>, u32),
    ) -> Result<Stored, &'static str> {
        Err(concat!(
            "this model has no storage image to write - one is bound by a graphics pipeline, ",
            "and a compute dispatch has nowhere to put one"
        ))
    }

    /// The value of a source operand, for one lane.
    fn read_source(
        &mut self,
        instruction: &Instruction,
        operand: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError>;

    /// Writes one lane of a vector register, honouring the execution mask.
    ///
    /// Where an invocation is a lane there is nothing to mask; where one invocation holds the
    /// wavefront every write is a select against the mask.
    fn write_vector_lane(&mut self, register: u32, lane: u32, value: Id);

    /// Writes one lane of a vector register whether the lane runs or not, as `v_writelane_b32`
    /// does (D978). The caller selects which lanes take a new value; this write ignores the
    /// execution mask.
    fn write_vector_lane_unmasked(&mut self, register: u32, lane: u32, value: Id);

    /// Writes a scalar register. Never masked: the scalar unit runs regardless of which lanes are
    /// active.
    fn write_scalar(&mut self, register: u32, value: Id);

    /// Counts an instruction as translated.
    fn count(&mut self);

    /// The module under construction.
    fn builder(&mut self) -> &mut Builder;

    /// The unsigned 32-bit type, which every register is.
    fn u32_type(&self) -> Id;

    /// The 32-bit float type, for arithmetic.
    fn f32_type(&self) -> Id;

    /// The id of the imported `GLSL.std.450` extended instruction set, importing it on first use.
    ///
    /// Lazy so a shader using no extended instruction emits no import; cached because a second
    /// import would be a second set.
    fn glsl_set(&mut self) -> Id;

    /// The 16-bit float type, the intermediate a packed half is read as before the driver's own
    /// conversion widens it to [`f32_type`](Self::f32_type).
    ///
    /// Declared on first use with its capability, so modules without sixteen-bit values ask the
    /// device for no extra features. Only a typed buffer load of a half-format channel reaches it.
    fn f16_type(&mut self) -> Id;

    /// The 16-bit unsigned type a packed half's field is narrowed to before it is read as a half: a
    /// bitcast needs both sides the same width. Declared on first use, like
    /// [`f16_type`](Self::f16_type).
    fn u16_type(&mut self) -> Id;

    /// Reads one word of the local data share.
    ///
    /// Storage shared between a wavefront's lanes; a model with one lane per invocation cannot
    /// represent it, so this and its write are fallible.
    fn read_local(&mut self, word_index: Id) -> Result<Id, TranslateError>;

    /// Reads the global data share's word at `byte_address`, or zero where `in_range` is false
    /// (D789). Refused by a model with no global data share.
    fn read_global_share(
        &mut self,
        instruction: &Instruction,
        (byte_address, in_range): (Id, Id),
    ) -> Result<Id, TranslateError> {
        let _ = (byte_address, in_range);
        Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a global data share (GDS) access in a model with no global data share",
        })
    }

    /// Writes `value` to the global data share's word at `byte_address` for `lane`, only where the
    /// lane is active and `in_range` holds (D789). Refused by a model with no global data share.
    fn write_global_share(
        &mut self,
        instruction: &Instruction,
        (byte_address, in_range): (Id, Id),
        value: Id,
        lane: u32,
    ) -> Result<(), TranslateError> {
        let _ = (byte_address, in_range, value, lane);
        Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a global data share (GDS) access in a model with no global data share",
        })
    }

    /// Writes one word of the local data share, honouring the execution mask.
    fn write_local(&mut self, word_index: Id, value: Id, lane: u32) -> Result<(), TranslateError>;

    /// The private memory (scratch) each wave of this module holds, in bytes, and the scalar
    /// register its wave's scratch offset arrives in, where its dispatch gives it scratch (D954);
    /// `None` for every other module, where a scratch access is refused.
    fn scratch(&self) -> Option<(u32, u32)> {
        None
    }

    /// Where word `word` of the wave's scratch - a byte address from the wave's scratch base, over
    /// four - lives for `lane` (D954): the index into the module's scratch array, and whether the
    /// array holds it there. `None` in a model with no scratch.
    fn scratch_place(&mut self, word: Id, lane: u32) -> Option<(Id, Id)> {
        let _ = (word, lane);
        None
    }

    /// Reads the module's scratch array at an `index` [`Self::scratch_place`] answered.
    fn read_scratch(&mut self, index: Id) -> Option<Id> {
        let _ = index;
        None
    }

    /// Writes `value` into the module's scratch array at `index` for `lane`, where the lane runs.
    fn write_scratch(&mut self, index: Id, value: Id, lane: u32) -> Option<()> {
        let _ = (index, value, lane);
        None
    }

    /// The draw buffer the access at `instruction` reads through (D733), or `None` for one that
    /// reaches guest memory through the window. `None` in a model that binds no draw buffers.
    fn draw_buffer_slot(&self, instruction: &Instruction) -> Option<u32> {
        let _ = instruction;
        None
    }

    /// The offset and name of the store that kept this program's buffers from being traced;
    /// `None` when they were, or in a model that traces none.
    fn untraced_by(&self) -> Option<&(u32, String)> {
        None
    }

    /// The low half of the guest address the draw buffer at `slot` starts at, which a global load's
    /// address is read against (D758); `None` in a model whose draw data carries no bases.
    fn draw_buffer_base(&mut self, slot: u32) -> Option<Id> {
        let _ = slot;
        None
    }

    /// How many words the draw buffer at `slot` holds, as the bound buffer's length reads, so a
    /// global load can tell an address the buffer does not hold (D883, D950); `None` in a model
    /// whose draw buffers are not each a bound buffer of their own.
    fn draw_buffer_words(&mut self, slot: u32) -> Option<Id> {
        let _ = slot;
        None
    }

    /// Whether a global load's draw buffer holds only the guest pages its waves read, behind a
    /// table of where each page starts ([`crate::wavefront::GLOBAL_PAGE_BYTES`]), rather than one
    /// range from its base: a dispatch's does. `false` in a model whose global loads read a range.
    fn global_pages(&self) -> bool {
        false
    }

    /// Whether the draw buffer at `slot` was placed from a sample of a dispatch's waves (D951): it
    /// starts at a guest base of its own, not at the descriptor's, and an access through it the
    /// buffer does not hold is an escape. `false` in a model that places none so.
    fn sampled_buffer(&self, slot: u32) -> bool {
        let _ = slot;
        false
    }

    /// The guest base of a draw buffer placed from a sample (D951), as its low half and its bits
    /// 47:32; `None` for one that starts at its descriptor's base, or where the model carries no
    /// such bases.
    fn sampled_buffer_base(&mut self, slot: u32) -> Option<(Id, Id)> {
        let _ = slot;
        None
    }

    /// The slot of the buffer of pages holding every record of the table the draw buffer at `slot`
    /// selects its descriptor from at run time (D966): an access through it reads, at the guest
    /// address the selected record's descriptor names, through
    /// [`crate::wavefront::GLOBAL_PAGE_BYTES`]' table. `None` for a slot bound any other way, and
    /// in a model that binds no table.
    fn table_pages(&self, slot: u32) -> Option<u32> {
        let _ = slot;
        None
    }

    /// The fourth word of the descriptor of the draw buffer at `slot`, at the draw this module is
    /// translated for (D738); `None` when no draw's descriptors were given.
    fn buffer_descriptor_format(&self, slot: u32) -> Option<u32> {
        let _ = slot;
        None
    }

    /// Whether a draw's descriptors' formats were given at all (D738), so a slot without one is a
    /// descriptor the draw did not resolve rather than a translation for no draw.
    fn buffer_formats_given(&self) -> bool {
        false
    }

    /// Reads word `word_index` of the draw buffer at `slot`, which the bound buffer's first word
    /// is word zero of. An index past the bound buffer's end reads word zero instead, so the read
    /// is always defined; the caller's bounds check decides what the access answers.
    ///
    /// # Errors
    ///
    /// Stores `value` at word `word_index` of the draw buffer at `slot`, in `lane` (D746).
    /// `outside` says where the store fell outside the buffer and `value` is the word kept, which
    /// a dispatch's write mask must not mark (D809).
    ///
    /// # Errors
    ///
    /// In a model that binds no draw buffers, where [`Self::draw_buffer_slot`] never names one.
    fn write_draw_buffer(
        &mut self,
        slot: u32,
        (word_index, value): (Id, Id),
        outside: Id,
        lane: u32,
    ) -> Result<(), TranslateError> {
        let _ = (slot, word_index, value, outside, lane);
        Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a store through a bound buffer in a model that binds none (D746)",
        })
    }

    /// Combines `value` into word `word_index` of the draw buffer at `slot` by the atomic `opcode`,
    /// as one indivisible step, in `lane` where `landed` holds - elsewhere the word is left alone -
    /// and answers what the word held (D919).
    ///
    /// # Errors
    ///
    /// In a model that binds no draw buffers.
    fn atomic_draw_buffer(
        &mut self,
        slot: u32,
        (word_index, opcode, value): (Id, u16, Id),
        landed: Id,
        lane: u32,
    ) -> Result<Id, TranslateError> {
        let _ = (slot, word_index, opcode, value, landed, lane);
        Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a buffer atomic in a model that binds no buffers (D919)",
        })
    }

    /// A pointer to word `word_index` of the draw buffer at `slot`, and whether the buffer holds it:
    /// what a part store merges into itself, a word at a time (D977).
    ///
    /// # Errors
    ///
    /// In a model that binds no draw buffers.
    fn draw_buffer_word(&mut self, slot: u32, word_index: Id) -> Result<(Id, Id), TranslateError> {
        let _ = (slot, word_index);
        Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a byte or half store through a buffer in a model that binds none (D977)",
        })
    }

    /// [`Self::atomic_draw_buffer`] on the 64-bit value whose low word is at `word_index` and high
    /// word the next, `value` low word first: one indivisible step over both, by the 64-bit atomic
    /// `opcode` (`OpAtomicExchange` or `OpAtomicOr`), answering what the pair held, low word first.
    /// A pair at an odd word is not one the hardware's 64-bit atomics address; it is not landed.
    ///
    /// # Errors
    ///
    /// In a model that binds no draw buffers, or has no 64-bit atomics.
    fn atomic_draw_buffer_pair(
        &mut self,
        slot: u32,
        (word_index, opcode, value): (Id, u16, [Id; 2]),
        landed: Id,
        lane: u32,
    ) -> Result<[Id; 2], TranslateError> {
        let _ = (slot, word_index, opcode, value, landed, lane);
        Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a 64-bit buffer atomic in a model with no 64-bit atomics on its buffers",
        })
    }

    /// In a model that binds no draw buffers, where [`Self::draw_buffer_slot`] never names one.
    fn read_draw_buffer(&mut self, slot: u32, word_index: Id) -> Result<Id, TranslateError> {
        let _ = (slot, word_index);
        Err(TranslateError::Unsupported {
            offset: 0,
            detail: "this model binds no draw buffers (D733)",
        })
    }

    /// The guest-memory buffer.
    fn memory_buffer(&self) -> Id;

    /// Pointer type for one word of guest memory.
    fn memory_element_ptr(&self) -> Id;

    /// Reads a scalar register, for an address held in one.
    fn read_scalar(&mut self, register: u32) -> Id;

    /// Reads a sixty-four-bit lane mask, by the name of its low half: [`EXEC_LOW_HALF`] or
    /// [`VCC_LOW_HALF`]. The two masks differ only in which registers they occupy.
    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError>;

    /// Writes a sixty-four-bit lane mask.
    ///
    /// Errors when the model has no such mask, because a model that cannot represent disabled lanes
    /// would produce a plausible wrong answer. The per-lane model refuses; the wavefront model
    /// writes it. This is what keeps [`Fidelity::Lane`](crate::Fidelity::Lane) safe.
    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError>;

    /// Sets bit `lane` of a pair of half-masks from a boolean: the low half for lanes 0-31, the
    /// high half for the rest.
    fn set_lane_bit(&mut self, halves: (Id, Id), lane: u32, condition: Id) -> (Id, Id) {
        constant_lane_bit(self, halves, lane, condition)
    }

    /// The lane this invocation is, when each invocation is one lane (D760); `None` where the lane
    /// a per-lane loop names is the lane.
    fn lane_index(&mut self) -> Option<Id> {
        None
    }

    /// The wave's lane `lane` is at run time, `lane` a lane the per-lane loops name, where an
    /// invocation's lanes are known only at run time: its one lane where each invocation is one
    /// (D760), either of its two where it holds two (D973). `None` where the loops name the lane
    /// itself.
    fn lane_number(&mut self, lane: u32) -> Option<Id> {
        let _ = lane;
        self.lane_index()
    }

    /// After a local-data-share access: where each invocation is one lane (D760), what one lane
    /// wrote is made visible to every other before any goes on, as the wave's own program order
    /// does. Nothing where one invocation is the whole wave.
    fn after_local_access(&mut self) {}

    /// Merges into word `word_index` of the local data share where each invocation is one lane
    /// (D760): keeps the bits `keep` has set, then sets `set`'s, each as one indivisible step, so
    /// lanes merging into one word at once each keep the others' bytes. Only where `lane` runs.
    /// `false` where one invocation is the whole wave and merges in lane order instead.
    fn merge_local(&mut self, word_index: Id, (keep, set): (Id, Id), lane: u32) -> bool {
        let _ = (word_index, keep, set, lane);
        false
    }

    /// Whether a sample here has derivatives to choose its level from: a pixel shader's quads. A
    /// compute dispatch has none, so only a sample naming its level or gradients is translated
    /// there (D904).
    fn has_derivatives(&self) -> bool {
        true
    }

    /// Applies the SPIR-V atomic `opcode` (`OpAtomicUMin`, `OpAtomicUMax`, `OpAtomicIAdd`) to one
    /// local-data-share word with `value`, as one indivisible step, where `lane` runs, answering what
    /// the word held before. `None` where the model runs the wave's lanes in order in one
    /// invocation of its own, and the step is a read and a write.
    fn atomic_local(&mut self, opcode: u16, word_index: Id, value: Id, lane: u32) -> Option<Id> {
        let _ = (opcode, word_index, value, lane);
        None
    }

    /// Writes one word of guest memory, honouring the execution mask.
    ///
    /// Required, because whether an inactive lane's store is suppressed is what distinguishes the
    /// models.
    fn write_memory(&mut self, word_index: Id, value: Id, lane: u32);

    /// Merges into word `word_index` of guest memory where `lane` runs and `landed` holds (D977):
    /// keeps the bits `keep` has set, then sets `set`'s, each one indivisible step, so another
    /// lane's or another wave's store to the word's other bytes is kept, as the hardware's
    /// byte-granular write keeps it.
    fn merge_memory(&mut self, word_index: Id, (keep, set): (Id, Id), landed: Id, lane: u32) {
        let (element_ptr, buffer) = (self.memory_element_ptr(), self.memory_buffer());
        let member = self.constant(0);
        let b = self.builder();
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[element_ptr.0, pointer.0, buffer.0, member.0, word_index.0],
        );
        let merge = open_if_active(self, lane, Some(landed));
        let gave_up = merge_bits(self, pointer, (keep, set));
        self.note_escape(gave_up, Some(lane));
        close_if_active(self, merge);
    }

    /// Turns a byte address into a word index, inside the window.
    ///
    /// The index is masked, so it is always a legal index: an out-of-range storage-buffer access is
    /// undefined in SPIR-V. Whether the address was in range is a separate question, answered by
    /// [`Model::address_within_window`], which callers act on.
    fn word_index(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        // Relative to where the window starts: an address below the base wraps to a large number,
        // which the check beside this refuses.
        let base = self.constant(self.memory_base());
        // The window is a power of two words, so masking keeps the index legal.
        let limit = self.constant(self.memory_words() - 1);
        let u32_type = self.u32_type();
        let b = self.builder();
        let offset = b.id();
        b.function(op::ISUB, &[u32_type.0, offset.0, address.0, base.0]);
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, offset.0, two.0],
        );
        let index = b.id();
        b.function(op::BITWISE_AND, &[u32_type.0, index.0, shifted.0, limit.0]);
        index
    }

    /// Whether a byte address falls inside the guest-memory window.
    ///
    /// [`Model::word_index`] masks, so an address past the window wraps onto its start. Callers ask
    /// this first: an access outside the window reads zero and writes nothing, as the hardware
    /// answers an out-of-range buffer access (D101).
    fn address_within_window(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        let base = self.constant(self.memory_base());
        let words = self.constant(self.memory_words());
        let u32_type = self.u32_type();
        let bool_type = self.bool_type();
        let b = self.builder();
        // Unsigned arithmetic makes one comparison do the work of two: an address below the base
        // wraps to a large offset and fails the same test as one past the end.
        let offset = b.id();
        b.function(op::ISUB, &[u32_type.0, offset.0, address.0, base.0]);
        let shifted = b.id();
        b.function(
            op::SHIFT_RIGHT_LOGICAL,
            &[u32_type.0, shifted.0, offset.0, two.0],
        );
        let inside = b.id();
        b.function(op::ULESS_THAN, &[bool_type.0, inside.0, shifted.0, words.0]);
        inside
    }

    /// The guest address the memory window starts at.
    ///
    /// Zero for a module whose addresses are the test's own; a guest's buffers are elsewhere, and a
    /// window anchored at zero refuses every access to them. Thirty-two bits, because a flat
    /// access's address pair is read by its low half here.
    fn memory_base(&self) -> u32 {
        0
    }

    /// How many words of guest memory this module addresses. A property of the module, so a test
    /// can widen the window.
    fn memory_words(&self) -> u32;

    /// How many words of local data share this module declares: a power of two, so masking keeps an
    /// index legal. One for a model with no share, whose accesses refuse.
    fn local_words(&self) -> u32 {
        1
    }

    /// Turns a local-data-share byte address into a word index.
    ///
    /// The share is the workgroup's own storage from address zero, not guest memory, so the
    /// guest-memory window ([`Self::word_index`]) has no part in it. Masked to the declared words,
    /// so the index is always legal: an out-of-range workgroup access is undefined in SPIR-V.
    fn local_word_index(&mut self, address: Id) -> Id {
        let two = self.constant(2);
        let limit = self.constant(self.local_words() - 1);
        let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, address, two);
        self.binary(op::BITWISE_AND, shifted, limit)
    }

    /// The high thirty-two bits of the window's guest address, which an exact access's address
    /// must share.
    fn memory_high(&self) -> u32 {
        0
    }
    /// Takes an `s_load_dwordx8` of an image descriptor from a traced table as the image's name,
    /// for a dispatch that binds that image itself: the load reads no memory, and its registers
    /// must not then be read as values. `false` where that is not how this model binds images.
    fn load_descriptor_opaquely(&mut self, first: u32, registers: u32) -> bool {
        let _ = (first, registers);
        false
    }

    /// Whether every guest-memory access must be placed exactly: a module whose writes land in
    /// guest memory for real, where an access dropped or folded into the window would be a wrong
    /// answer. Such a module records each access it cannot place ([`Self::note_escape`]), and an
    /// access that cannot even be checked is refused at translation.
    fn exact_memory(&self) -> bool {
        false
    }

    /// Records that an access left the memory this module can reach exactly, when `escaped` holds:
    /// for `lane` when it is active, or for the scalar unit (`None`). A model that is not exact
    /// records nothing.
    fn note_escape(&mut self, _escaped: Id, _lane: Option<u32>) {}

    /// [`Self::note_escape`] for an access at a guest address, naming its page: `page` is bits
    /// 47:16 of the address (D971). A model that names no page records the escape alone.
    fn note_escape_at(&mut self, escaped: Id, lane: Option<u32>, _page: Id) {
        self.note_escape(escaped, lane);
    }

    /// Whether this module reaches guest memory by device address (D964): its global and flat
    /// accesses, and the buffer and scalar accesses no bound buffer serves, through
    /// [`Self::device_table`]. `false` in a model that maps no guest memory into the device.
    fn device_addressed(&self) -> bool {
        false
    }

    /// The table a device-addressed module translates guest addresses through, loaded where it is
    /// asked for (D964); `None` in a model that is not device-addressed.
    fn device_table(&mut self) -> Option<crate::device_address::DeviceTable> {
        None
    }

    /// The helper function a device-addressed module looks guest addresses up through (D969), and
    /// whether its body is already written: the first asker is answered `false` and writes it. A
    /// model with none answers `None`, and each lookup is written where it is made.
    fn device_lookup_function(&mut self) -> Option<(Id, bool)> {
        None
    }

    /// A private word counting the backward jumps this invocation has taken, where the model cuts a
    /// loop that runs past [`crate::control::LOOP_CAP`] and reports the cut (D949); `None` where it
    /// has no way to report one, and then no loop is cut.
    fn loop_budget(&mut self) -> Option<Id> {
        None
    }

    /// The backward jumps past which [`Self::loop_budget`] cuts a loop (D949).
    fn loop_cap(&self) -> u32 {
        crate::control::LOOP_CAP
    }

    /// Records that the loop closed by the backward branch at byte offset `branch` was cut, when
    /// `cut` holds (D949). Called only where [`Self::loop_budget`] answered a word.
    fn note_loop_cut(&mut self, _cut: Id, _branch: u32) {}

    /// Reinterprets a register's bits as a float.
    fn as_float(&mut self, value: Id) -> Id {
        let f32_type = self.f32_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::BITCAST, &[f32_type.0, result.0, value.0]);
        result
    }

    /// A comparison of two values, producing a boolean. The opcode decides whether they are
    /// read as floats or as integers, so this serves both.
    ///
    /// `OpUnordered` and `OpOrdered` need the `Kernel` capability, which a Vulkan shader cannot
    /// declare: either operand a NaN, or neither, is formed from `OpIsNan` instead.
    fn compare(&mut self, opcode: u16, lhs: Id, rhs: Id) -> Id {
        let bool_type = self.bool_type();
        if matches!(opcode, op::UNORDERED | op::ORDERED) {
            let b = self.builder();
            let [left, right, either] = [b.id(), b.id(), b.id()];
            b.function(op::IS_NAN, &[bool_type.0, left.0, lhs.0]);
            b.function(op::IS_NAN, &[bool_type.0, right.0, rhs.0]);
            b.function(op::LOGICAL_OR, &[bool_type.0, either.0, left.0, right.0]);
            if opcode == op::UNORDERED {
                return either;
            }
            let neither = b.id();
            b.function(op::LOGICAL_NOT, &[bool_type.0, neither.0, either.0]);
            return neither;
        }
        let b = self.builder();
        let result = b.id();
        b.function(opcode, &[bool_type.0, result.0, lhs.0, rhs.0]);
        result
    }

    /// The boolean type.
    fn bool_type(&mut self) -> Id;

    /// The scalar condition code, as a value.
    ///
    /// One bit of hidden state written by the scalar compares and read by the `scc` branches. A
    /// private variable, because it is written in one arm of the dispatch switch and read in
    /// another. It is wavefront-wide rather than per lane, so both models represent it.
    fn condition_code(&mut self) -> Id;

    /// The `m0` register: a private word, like the condition code.
    ///
    /// A shader that copies it back into a register must read what it wrote. What the hardware does
    /// with the value (address the parameter cache, size an allocation) has no host counterpart, so
    /// the models hold it and do not act on it.
    fn read_m0(&mut self) -> Id;

    /// Writes the `m0` register. Never masked, like every scalar write.
    fn write_m0(&mut self, value: Id);

    /// Says that scalar register `destination` was just written with a plain copy of scalar
    /// register `source`, so a descriptor the program moved out of its user data is still found
    /// there. Called after the write; a model with no use for it ignores it.
    fn note_scalar_copy(&mut self, _destination: u32, _source: u32) {}

    /// Says that vector register `destination` was just written, in every running lane, with a
    /// plain copy of scalar register `source`. Called after the write; a model with no use for it
    /// ignores it.
    fn note_vector_copy(&mut self, _destination: u32, _source: u32) {}

    /// The user-data word vector register `register` holds in every lane, when a plain copy put it
    /// there and nothing has written the register since: a dispatch's level, moved from its user
    /// data before the store that names it.
    fn vector_user_data(&self, _register: u32) -> Option<u32> {
        None
    }

    /// The program counter: a private variable holding the index of the block to run. Private
    /// because it is written in one arm of the dispatch switch and read in the header.
    fn program_counter(&mut self) -> Id;

    /// Whether the instruction at `offset` only rewrites the type of an image descriptor loaded from
    /// the descriptor table, which keeps the image it names and translates to nothing (D895).
    fn rewrites_only_an_image_type(&self, _offset: u32) -> bool {
        false
    }

    /// How many waves of a compute thread group run as invocations of one workgroup, meeting at
    /// its `s_barrier`s (D858, D892); one for every other module.
    fn group_waves(&self) -> u32 {
        1
    }

    /// How many guest instructions have been translated so far.
    fn instructions(&self) -> usize;

    /// The scalar condition code as a source reads it: the word it is held as, zero or one.
    fn read_condition_word(&mut self) -> Id {
        let (u32_type, pointer) = (self.u32_type(), self.condition_code());
        let b = self.builder();
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        value
    }

    /// Stores a boolean into the scalar condition code, widened to a word because the code lives in
    /// a private variable and a boolean has no defined size in a storage class.
    fn set_condition_code(&mut self, condition: Id) {
        let one = self.constant(1);
        let zero = self.constant(0);
        let (u32_type, pointer) = (self.u32_type(), self.condition_code());
        let b = self.builder();
        let value = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, value.0, condition.0, one.0, zero.0],
        );
        b.function(op::STORE, &[pointer.0, value.0]);
    }

    /// Whether both of two booleans hold.
    fn both(&mut self, left: Id, right: Id) -> Id {
        let bool_type = self.bool_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::LOGICAL_AND, &[bool_type.0, result.0, left.0, right.0]);
        result
    }

    /// Whether either of two booleans holds.
    fn either(&mut self, left: Id, right: Id) -> Id {
        let bool_type = self.bool_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::LOGICAL_OR, &[bool_type.0, result.0, left.0, right.0]);
        result
    }

    /// Whether a value is not zero.
    fn is_not_zero(&mut self, value: Id) -> Id {
        let zero = self.constant(0);
        let bool_type = self.bool_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::INOT_EQUAL, &[bool_type.0, result.0, value.0, zero.0]);
        result
    }

    /// Whether bit `lane` of a sixty-four-bit mask, held as two halves, is set. `v_cndmask_b32`
    /// takes an arbitrary register pair rather than a named mask.
    fn lane_bit(&mut self, low: Id, high: Id, lane: u32) -> Id {
        constant_lane_test(self, (low, high), lane)
    }

    /// Counts the set bits of a value.
    fn bit_count(&mut self, value: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::BIT_COUNT, &[u32_type.0, result.0, value.0]);
        result
    }

    /// Bitwise complement.
    fn not(&mut self, value: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(op::NOT, &[u32_type.0, result.0, value.0]);
        result
    }

    /// A binary integer operation on two values.
    fn binary(&mut self, opcode: u16, lhs: Id, rhs: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(opcode, &[u32_type.0, result.0, lhs.0, rhs.0]);
        result
    }

    /// Adds two values.
    fn add(&mut self, lhs: Id, rhs: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let sum = b.id();
        b.function(op::IADD, &[u32_type.0, sum.0, lhs.0, rhs.0]);
        sum
    }

    /// Reads one word of guest memory.
    fn read_memory(&mut self, word_index: Id) -> Id {
        let (u32_type, element_ptr, buffer) = (
            self.u32_type(),
            self.memory_element_ptr(),
            self.memory_buffer(),
        );
        let member = self.constant(0);
        let b = self.builder();
        let pointer = b.id();
        b.function(
            op::ACCESS_CHAIN,
            &[element_ptr.0, pointer.0, buffer.0, member.0, word_index.0],
        );
        let value = b.id();
        b.function(op::LOAD, &[u32_type.0, value.0, pointer.0]);
        value
    }

    /// The address a flat access refers to, for one lane.
    ///
    /// A base of [`FLAT_NO_BASE`] means none and the vector address stands alone. Any other named
    /// operand is refused: an unknown base would put the access at the wrong address.
    fn flat_address(
        &mut self,
        instruction: &Instruction,
        vaddr: &Operand,
        base: &Operand,
        lane: u32,
    ) -> Result<Id, TranslateError> {
        let mut address = self.read_source(instruction, vaddr, lane)?;
        // The byte offset the instruction carries. The reference prints it only when it is not
        // zero, so the operand is absent from an access at offset zero.
        // Signed for a global access ([`flat_offset`]): a 32-bit wrapping add of its two's
        // complement subtracts.
        let byte_offset = i32::try_from(flat_offset(instruction)).unwrap_or(0);
        if byte_offset != 0 {
            let constant = self.constant(u32::from_ne_bytes(byte_offset.to_ne_bytes()));
            address = self.add(address, constant);
        }
        let offset = address;
        match base {
            Operand::Named(name) if name == FLAT_NO_BASE => Ok(offset),
            Operand::Scalar(register) => {
                let base = self.read_scalar(u32::from(*register));
                Ok(self.add(base, offset))
            }
            _ => Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "flat access has a base this translator does not understand",
            }),
        }
    }

    /// Applies a floating-point operation to two register values.
    ///
    /// A register holds thirty-two untyped bits and the instruction decides how to read them, so
    /// the operands are reinterpreted with `OpBitcast`. `OpConvertUToF` would turn the bits of 1.0
    /// into 1065353216.0.
    fn f32_binary(&mut self, operation: u16, lhs: Id, rhs: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();

        let lhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, lhs_f.0, lhs.0]);
        let rhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, rhs_f.0, rhs.0]);

        let result_f = b.id();
        b.function(operation, &[f32_type.0, result_f.0, lhs_f.0, rhs_f.0]);

        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// [`Self::f32_binary`] with the float operation decorated `NoContraction`, so a driver may
    /// not fuse it with another into one rounding.
    fn f32_binary_uncontracted(&mut self, operation: u16, lhs: Id, rhs: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let lhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, lhs_f.0, lhs.0]);
        let rhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, rhs_f.0, rhs.0]);
        let result_f = b.id();
        b.function(operation, &[f32_type.0, result_f.0, lhs_f.0, rhs_f.0]);
        b.annotate(
            op::DECORATE,
            &[result_f.0, orbistoun_spirv::decoration::NO_CONTRACTION],
        );
        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// Applies a `GLSL.std.450` extended float operation (by instruction number) to two register
    /// values, returning the result's bits.
    ///
    /// Bitcast in and out as in [`f32_binary`](Self::f32_binary); the operation is an `OpExtInst`,
    /// how min, max and the transcendentals are spelled.
    fn f32_ext_binary(&mut self, instruction: u32, lhs: Id, rhs: Id) -> Id {
        let (u32_type, f32_type, set) = (self.u32_type(), self.f32_type(), self.glsl_set());
        let b = self.builder();

        let lhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, lhs_f.0, lhs.0]);
        let rhs_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, rhs_f.0, rhs.0]);

        let result_f = b.ext_inst(f32_type, set, instruction, &[lhs_f, rhs_f]);

        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// Applies a `GLSL.std.450` extended float operation (by instruction number) to one register
    /// value, returning the result's bits, bitcast in and out as
    /// [`f32_ext_binary`](Self::f32_ext_binary) does.
    fn f32_ext_unary(&mut self, instruction: u32, operand: Id) -> Id {
        let (u32_type, f32_type, set) = (self.u32_type(), self.f32_type(), self.glsl_set());
        let b = self.builder();

        let operand_f = b.id();
        b.function(op::BITCAST, &[f32_type.0, operand_f.0, operand.0]);

        let result_f = b.ext_inst(f32_type, set, instruction, &[operand_f]);

        let result = b.id();
        b.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// The fused multiply-add `a * b + c` of three register values, rounded once: `GLSL.std.450`
    /// `Fma`, bitcast in and out as [`f32_ext_binary`](Self::f32_ext_binary) does.
    ///
    /// Which instructions are fused follows the instruction set's opcode tables (AMD's published
    /// RDNA 2 instruction set, the `VOP2`, `VOP3` and `VOP3P` sections):
    ///
    /// - fused, rounded once: `V_FMA_F32`, `V_FMAC_F32`, `V_FMAAK_F32`, `V_FMAMK_F32`,
    ///   `V_DIV_FMAS_F32` and `V_FMA_MIX_F32` here; `V_FMA_F16`, `V_FMAC_F16`, `V_PK_FMA_F16`,
    ///   `V_PK_FMAC_F16`, `V_FMA_MIXLO_F16` and `V_FMA_MIXHI_F16` once to a half
    ///   (`fused_to_half`);
    /// - unfused, the product rounded before the add: `V_MAD_F32`, `V_MAC_F32`, `V_MADAK_F32` and
    ///   `V_MADMK_F32`, their denormals flushed (`legacy_multiply_add`). ACO selects these for
    ///   an `fmul` and an `fadd` kept apart, and the fused opcodes for `ffma`
    ///   (`aco_instruction_selection.cpp`).
    ///
    /// `V_FMA_F64` is not translated.
    fn f32_fma(&mut self, a: Id, b: Id, c: Id) -> Id {
        let (u32_type, f32_type, set) = (self.u32_type(), self.f32_type(), self.glsl_set());
        let builder = self.builder();
        let floats = [a, b, c].map(|value| {
            let float = builder.id();
            builder.function(op::BITCAST, &[f32_type.0, float.0, value.0]);
            float
        });
        let result_f = builder.ext_inst(f32_type, set, GLSL_FMA, &floats);
        let result = builder.id();
        builder.function(op::BITCAST, &[u32_type.0, result.0, result_f.0]);
        result
    }

    /// Converts a register's value, read as an unsigned integer, to the equal float, and returns
    /// that float's bits.
    ///
    /// The counterpart to [`f32_binary`](Self::f32_binary)'s bitcast: here the value becomes the
    /// float, so 255 becomes 255.0. Used to lift a packed normalised component and its range into
    /// floats before the division that normalises it.
    fn unsigned_to_float_bits(&mut self, value: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let as_float = b.id();
        b.function(op::CONVERT_U_TO_F, &[f32_type.0, as_float.0, value.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, as_float.0]);
        bits
    }

    /// As [`unsigned_to_float_bits`](Self::unsigned_to_float_bits), reading the value as signed:
    /// `-128` becomes `-128.0`, so a sign-extended packed field keeps its sign.
    fn signed_to_float_bits(&mut self, value: Id) -> Id {
        let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
        let b = self.builder();
        let as_float = b.id();
        b.function(op::CONVERT_S_TO_F, &[f32_type.0, as_float.0, value.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, as_float.0]);
        bits
    }

    /// Widens a packed float narrower than a half (the 11- and 10-bit channels of formats like
    /// `10_11_11`) to a single-precision float, and returns that float's bits.
    ///
    /// These are not IEEE types, so there is no conversion instruction: each channel is unsigned,
    /// with a five-bit exponent biased by 15 and `width - 5` bits of mantissa. The rules are pinned
    /// by hardware measurements in `tests/execute.rs` (`MEASURED_10_11_11`):
    ///
    /// - Exponent 31 is Inf/NaN, with the mantissa at the top of the single's mantissa field: the
    ///   all-ones word gives `0x7ffe0000` for an 11-bit channel and `0x7ffc0000` for a 10-bit one.
    /// - Exponent 0 is subnormal: the mantissa as an integer times `2^-(14 + mantissa)` gives the
    ///   measured bits exactly, with no leading-zero count needed.
    ///
    /// The caller must have masked the field to its own bits.
    fn narrow_float_to_float_bits(&mut self, field: Id, width: u32) -> Id {
        let mantissa_bits = width - 5;

        let exponent_shift = self.constant(mantissa_bits);
        let exponent = self.binary(op::SHIFT_RIGHT_LOGICAL, field, exponent_shift);
        let mantissa_mask = self.constant((1u32 << mantissa_bits) - 1);
        let mantissa = self.binary(op::BITWISE_AND, field, mantissa_mask);

        // The mantissa sits at the top of the single's 23-bit field in every case, so it is placed
        // once.
        let place = self.constant(23 - mantissa_bits);
        let placed = self.binary(op::SHIFT_LEFT_LOGICAL, mantissa, place);

        // Normal: rebias 15 to 127 and drop the exponent into place.
        let rebias = self.constant(127 - 15);
        let biased = self.binary(op::IADD, exponent, rebias);
        let to_exponent = self.constant(23);
        let exponent_field = self.binary(op::SHIFT_LEFT_LOGICAL, biased, to_exponent);
        let normal = self.binary(op::BITWISE_OR, exponent_field, placed);

        // Infinity or NaN: the single's all-ones exponent over the same placed mantissa.
        let infinity = self.constant(0x7f80_0000);
        let inf_or_nan = self.binary(op::BITWISE_OR, infinity, placed);

        // Subnormal: the mantissa read as an integer, scaled by 2^-(14 + mantissa_bits). The
        // scale is exact in single precision, so this is a rounding-free multiply.
        let scale_bits = self.constant(0x3f80_0000 - ((14 + mantissa_bits) << 23));
        let subnormal = {
            let (u32_type, f32_type) = (self.u32_type(), self.f32_type());
            let b = self.builder();
            let as_float = b.id();
            b.function(op::CONVERT_U_TO_F, &[f32_type.0, as_float.0, mantissa.0]);
            let scale = b.id();
            b.function(op::BITCAST, &[f32_type.0, scale.0, scale_bits.0]);
            let scaled = b.id();
            b.function(op::FMUL, &[f32_type.0, scaled.0, as_float.0, scale.0]);
            let bits = b.id();
            b.function(op::BITCAST, &[u32_type.0, bits.0, scaled.0]);
            bits
        };

        // In this order so exponent 0 wins over the normal arithmetic, which would give 2^-15 times
        // the mantissa rather than 2^-14.
        let all_ones = self.constant(31);
        let is_inf_or_nan = self.compare(op::IEQUAL, exponent, all_ones);
        let finite = self.select(is_inf_or_nan, inf_or_nan, normal);
        let zero = self.constant(0);
        let is_subnormal = self.compare(op::IEQUAL, exponent, zero);
        self.select(is_subnormal, subnormal, finite)
    }

    /// Packs a single-precision float into an unsigned N-bit packed float, the inverse of
    /// [`Self::narrow_float_to_float_bits`], for storing a channel of a format like `10_11_11`.
    ///
    /// The rule is clamp to `[0, max finite]`, then truncate, pinned by `MEASURED_10_11_11_STORE`
    /// in `tests/execute.rs`: `1.009375` stores as mantissa 0 (rounding toward zero), and `100000`
    /// stores as the largest finite value, not infinity. A negative or underflowing value packs to
    /// zero, and infinity or NaN to the largest finite, as the same clamp gives. `value_bits` is
    /// the register's bits.
    fn float_to_narrow_float_bits(&mut self, value_bits: Id, width: u32) -> Id {
        let mantissa_bits = width - 5;

        // Decompose the single into its sign, eight-bit exponent and mantissa.
        let sign_shift = self.constant(31);
        let sign = self.binary(op::SHIFT_RIGHT_LOGICAL, value_bits, sign_shift);
        let exponent = {
            let exponent_shift = self.constant(23);
            let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, value_bits, exponent_shift);
            let mask = self.constant(0xFF);
            self.binary(op::BITWISE_AND, shifted, mask)
        };
        let mantissa = {
            let mask = self.constant(0x7F_FFFF);
            self.binary(op::BITWISE_AND, value_bits, mask)
        };

        // The truncated normal: drop the low mantissa bits, rebias the exponent 127 -> 15, assemble.
        let drop = self.constant(23 - mantissa_bits);
        let mantissa_field = self.binary(op::SHIFT_RIGHT_LOGICAL, mantissa, drop);
        let rebias = self.constant(127 - 15);
        let exponent15 = self.binary(op::ISUB, exponent, rebias);
        let place = self.constant(mantissa_bits);
        let exponent_field = self.binary(op::SHIFT_LEFT_LOGICAL, exponent15, place);
        let normal = self.binary(op::BITWISE_OR, exponent_field, mantissa_field);

        // The clamp: below the smallest normal (`exp <= 112`) or negative packs to zero; above the
        // largest finite (`exp >= 143`, where Inf and NaN sit) to the largest finite; otherwise the
        // normal.
        let zero = self.constant(0);
        let max_finite = self.constant((30u32 << mantissa_bits) | ((1u32 << mantissa_bits) - 1));
        let over_edge = self.constant(142);
        let is_over = self.compare(op::UGREATER_THAN, exponent, over_edge);
        let clamped_high = self.select(is_over, max_finite, normal);
        let under_edge = self.constant(113);
        let is_under = self.compare(op::ULESS_THAN, exponent, under_edge);
        let non_negative = self.select(is_under, zero, clamped_high);
        let one = self.constant(1);
        let is_negative = self.compare(op::IEQUAL, sign, one);
        self.select(is_negative, zero, non_negative)
    }

    /// Narrows a register's float to a half rounding toward zero, as `v_cvt_pkrtz_f16_f32` does, and
    /// returns the half's sixteen bits zero-extended. Done in integers, because `OpFConvert`
    /// rounds to nearest: the mantissa is truncated; a finite value past the largest half packs
    /// to the largest finite (`0x7bff`), as rounding toward zero gives, infinity to infinity, and a
    /// NaN to a quiet NaN keeping its top mantissa bits; a value below the smallest normal half
    /// becomes the subnormal it truncates to, and below the smallest subnormal a signed zero. A
    /// single-precision subnormal is below every half, so it packs to zero too.
    fn float_bits_to_half_toward_zero(&mut self, bits: Id) -> Id {
        let sign = {
            let shift = self.constant(16);
            let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, bits, shift);
            let mask = self.constant(0x8000);
            self.binary(op::BITWISE_AND, shifted, mask)
        };
        let exponent = {
            let shift = self.constant(23);
            let shifted = self.binary(op::SHIFT_RIGHT_LOGICAL, bits, shift);
            let mask = self.constant(0xFF);
            self.binary(op::BITWISE_AND, shifted, mask)
        };
        let mantissa = {
            let mask = self.constant(0x7F_FFFF);
            self.binary(op::BITWISE_AND, bits, mask)
        };
        let thirteen = self.constant(13);
        let top_mantissa = self.binary(op::SHIFT_RIGHT_LOGICAL, mantissa, thirteen);

        // Normal halves: single exponents 113..=142 rebias by 112 into 1..=30.
        let normal = {
            let rebias = self.constant(112);
            let half_exponent = self.binary(op::ISUB, exponent, rebias);
            let ten = self.constant(10);
            let field = self.binary(op::SHIFT_LEFT_LOGICAL, half_exponent, ten);
            self.binary(op::BITWISE_OR, field, top_mantissa)
        };
        // Subnormal halves: single exponents 103..=112, the significand with its leading one
        // shifted right by 126 - exponent (14..=23), the exponent clamped so the shift stays in
        // range where the result is discarded.
        let subnormal = {
            let leading = self.constant(0x80_0000);
            let significand = self.binary(op::BITWISE_OR, mantissa, leading);
            let base = self.constant(126);
            let lowest = self.constant(103);
            let highest = self.constant(112);
            let below = self.compare(op::ULESS_THAN, exponent, lowest);
            let clamped = self.select(below, lowest, exponent);
            let above = self.compare(op::UGREATER_THAN, clamped, highest);
            let clamped = self.select(above, highest, clamped);
            let shift = self.binary(op::ISUB, base, clamped);
            self.binary(op::SHIFT_RIGHT_LOGICAL, significand, shift)
        };
        let zero = self.constant(0);
        let largest_finite = self.constant(0x7BFF);
        let infinity = self.constant(0x7C00);
        let nan = {
            let quiet = self.constant(0x7E00);
            self.binary(op::BITWISE_OR, quiet, top_mantissa)
        };

        let all_ones = self.constant(0xFF);
        let is_special = self.compare(op::IEQUAL, exponent, all_ones);
        let has_payload = self.is_not_zero(mantissa);
        let special = self.select(has_payload, nan, infinity);
        let over_edge = self.constant(142);
        let is_over = self.compare(op::UGREATER_THAN, exponent, over_edge);
        let normal_edge = self.constant(113);
        let is_below_normal = self.compare(op::ULESS_THAN, exponent, normal_edge);
        let subnormal_edge = self.constant(103);
        let is_below_subnormal = self.compare(op::ULESS_THAN, exponent, subnormal_edge);

        let magnitude = self.select(is_below_normal, subnormal, normal);
        let magnitude = self.select(is_below_subnormal, zero, magnitude);
        let magnitude = self.select(is_over, largest_finite, magnitude);
        let magnitude = self.select(is_special, special, magnitude);
        self.binary(op::BITWISE_OR, sign, magnitude)
    }

    /// `condition ? when_true : when_false`, on `u32` values.
    fn select(&mut self, condition: Id, when_true: Id, when_false: Id) -> Id {
        let u32_type = self.u32_type();
        let b = self.builder();
        let result = b.id();
        b.function(
            op::SELECT,
            &[u32_type.0, result.0, condition.0, when_true.0, when_false.0],
        );
        result
    }

    /// Widens a packed half, a sixteen-bit IEEE float in the low bits of the register, to a
    /// single-precision float, and returns that float's bits.
    ///
    /// The field is narrowed to sixteen bits (`UConvert`), read as a half (`Bitcast`, which needs
    /// equal widths), and widened by `FConvert`, so subnormals, infinities and NaNs take the
    /// device's IEEE path. The caller must have masked the field to its sixteen bits.
    fn half_to_float_bits(&mut self, field: Id) -> Id {
        // Fetched one at a time because each may declare a type and its capability, so each needs
        // the builder to itself.
        let u32_type = self.u32_type();
        let f32_type = self.f32_type();
        let f16_type = self.f16_type();
        let u16_type = self.u16_type();
        let b = self.builder();
        let narrowed = b.id();
        b.function(op::UCONVERT, &[u16_type.0, narrowed.0, field.0]);
        let half = b.id();
        b.function(op::BITCAST, &[f16_type.0, half.0, narrowed.0]);
        let widened = b.id();
        b.function(op::FCONVERT, &[f32_type.0, widened.0, half.0]);
        let bits = b.id();
        b.function(op::BITCAST, &[u32_type.0, bits.0, widened.0]);
        bits
    }

    /// Narrows a register's float to a half and returns the half's sixteen bits, zero-extended.
    ///
    /// The inverse of [`Self::half_to_float_bits`], through `OpFConvert`, so overflow, denormals
    /// and NaNs are the device's. See `pack_halves` for the rounding this does not promise.
    fn float_bits_to_half(&mut self, bits: Id) -> Id {
        let u32_type = self.u32_type();
        let f32_type = self.f32_type();
        let f16_type = self.f16_type();
        let u16_type = self.u16_type();
        let b = self.builder();
        let float = b.id();
        b.function(op::BITCAST, &[f32_type.0, float.0, bits.0]);
        let half = b.id();
        b.function(op::FCONVERT, &[f16_type.0, half.0, float.0]);
        let narrowed = b.id();
        b.function(op::BITCAST, &[u16_type.0, narrowed.0, half.0]);
        let widened = b.id();
        b.function(op::UCONVERT, &[u32_type.0, widened.0, narrowed.0]);
        widened
    }
}

/// A destination and two sources, in the order the specification prints them.
fn three_operands(
    instruction: &Instruction,
) -> Result<(&Operand, &Operand, &Operand), TranslateError> {
    match instruction.operands.as_slice() {
        [destination, first, second] => Ok((destination, first, second)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "expected exactly three operands",
        }),
    }
}

/// The first three operands, of an instruction that may carry a fourth.
///
/// A flat access decodes a fourth operand only when the reference printed a non-zero offset, so the
/// flat paths take the first three and read the offset themselves.
fn first_three_operands(
    instruction: &Instruction,
) -> Result<(&Operand, &Operand, &Operand), TranslateError> {
    match instruction.operands.as_slice() {
        [first, second, third, ..] => Ok((first, second, third)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "expected at least three operands",
        }),
    }
}

/// A destination and a source, in that order.
fn two_operands(instruction: &Instruction) -> Result<(&Operand, &Operand), TranslateError> {
    match instruction.operands.as_slice() {
        [destination, source] => Ok((destination, source)),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "expected exactly two operands",
        }),
    }
}

/// What this target calls an instruction, or a refusal saying why it cannot be named.
///
/// Everything downstream dispatches on the name, because an opcode number belongs to one
/// architecture generation (D139).
fn resolve<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<String, TranslateError> {
    let family = instruction
        .encoding
        .and_then(|i| model.encodings().encodings().get(usize::from(i)))
        .map(|e| e.name.clone())
        .ok_or(TranslateError::Unrecognised {
            offset: instruction.offset,
        })?;

    model
        .encodings()
        .mnemonic_for(&family, instruction.opcode)
        .map(str::to_owned)
        // Nothing has observed this opcode on this target, so there is no name to dispatch on, and
        // a bare number is not acted on. The refusal says which, so it can be recorded.
        .ok_or_else(|| TranslateError::NotTranslated {
            offset: instruction.offset,
            mnemonic: format!(
                "{family} opcode {}, first word {:#010x}",
                instruction.opcode, instruction.word
            ),
            detail: concat!(
                "this target has no recorded name for that opcode, so there is ",
                "nothing to translate it as"
            ),
        })
}
/// `v_interp_p1_f32` / `v_interp_p2_f32`: an interpolated fragment attribute.
///
/// On the guest `p1` computes `P10 * I + P0` and `p2` adds `P20 * J`, from a parameter cache the
/// hardware fills. A fragment `Input` variable is already the interpolated attribute, so both
/// halves read the whole value (D555). That holds even when the two do not share a destination
/// (`unreached.s` does this), because the answer depends on no register history.
///
/// Assumed: a shader using `p1`'s intermediate for anything but `p2` gets the finished value here
/// instead of a partial sum. Both `vsrc` operands (the barycentric I and J) are ignored; the host
/// interpolates with its own.
fn interpolate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    flat: bool,
) -> Result<(), TranslateError> {
    let [
        Operand::Vector(destination),
        _source,
        Operand::Immediate(attribute),
        Operand::Immediate(channel),
    ] = instruction.operands.as_slice()
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "an interpolation decodes to a destination, a source, an attribute and a ",
                "channel; this one did not"
            ),
        });
    };
    let attribute = u32::try_from(*attribute).unwrap_or(u32::MAX);
    let Some((vec4, input)) = model.attribute_input(attribute, flat) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "an interpolation needs a fragment input to read, and this module has none ",
                "for that attribute - translate at the fragment stage"
            ),
        });
    };
    let Ok(channel) = u32::try_from(*channel) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the channel is not a component index",
        });
    };

    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let b = model.builder();
    let whole = b.id();
    b.function(op::LOAD, &[vec4.0, whole.0, input.0]);
    let component = b.id();
    b.function(
        op::COMPOSITE_EXTRACT,
        &[f32_type.0, component.0, whole.0, channel],
    );
    // Registers hold bits, not floats, as the export reads them back.
    let bits = b.id();
    b.function(op::BITCAST, &[u32_type.0, bits.0, component.0]);

    // Lane zero only: at the fragment stage one invocation is one pixel, and the export reads lane
    // zero too.
    model.write_vector_lane(u32::from(*destination), 0, bits);
    Ok(())
}

/// The parameter a `v_interp_mov_f32` reads, as the operand field encodes it.
///
/// `orbistoun-gen`'s symbolic-code solver assembles the instruction with each spelling and reads
/// the bits that moved: `p10` is 0, `p20` is 1, `p0` is 2. From the interpolation formulas, `P0` is
/// the value at the primitive's first vertex and the other two are deltas, so `P0` is flat shading
/// with a first-vertex provoking convention, the host default.
const PARAMETER_CONSTANT_TERM: i64 = 2;

/// `v_interp_mov_f32`: a fragment attribute read without interpolating.
///
/// The host has no parameter cache: an input variable is the attribute, and whether reading it
/// interpolates is the variable's `Flat` decoration, set by the declaration pass. What remains here
/// is the same component read the interpolating pair does. `P10` and `P20` are deltas between
/// vertices, which the host never exposes, so they are refused.
fn parameter_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let Some(Operand::Immediate(parameter)) = instruction.operands.get(1) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a parameter move decodes to a destination, a parameter, an attribute and 
                     a channel; this one did not",
        });
    };
    if *parameter != PARAMETER_CONSTANT_TERM {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "this moves one of the interpolation deltas, P10 or P20, rather than the 
                     constant term. The host interpolates with its own barycentrics and offers 
                     no way to read the gradient it used",
        });
    }
    interpolate(model, instruction, true)
}

/// The export targets, as the field encodes them.
///
/// `orbistoun-gen`'s symbolic-code solver assembles an export with each spelling and reads the bits
/// that moved: colour attachments from 0, `pos0` at 12, `param0` at 32, the primitive export at 20.
const EXPORT_POSITION: i64 = 12;
/// `pos1`, the misc vector: point size, edge flag, the layer and viewport index, and the shading
/// rate (Mesa `ac_nir_prerast_utils.c` 355-395).
pub(crate) const EXPORT_MISC: i64 = EXPORT_POSITION + 1;
/// The misc vector's third channel, in an export's `EN` field: the render-target layer in bits
/// 10:0 and the viewport index in 19:16 on GFX9 and later (`ac_nir_prerast_utils.c` 381-392, D859).
pub(crate) const EXPORT_LAYER_CHANNEL: u32 = 1 << 2;
/// The misc vector's first channel, in an export's `EN` field: the point size, a float
/// (`ac_nir_prerast_utils.c` 355-395, D867).
pub(crate) const EXPORT_POINT_SIZE_CHANNEL: u32 = 1;
/// The misc vector's second channel: the vertex's edge flag, which selects the edges a polygon
/// drawn as lines draws. Every polygon here is filled, where it reaches nothing - as the edge flags
/// in a primitive export do not ([`mesh_primitive_export`]).
const EXPORT_EDGE_FLAG_CHANNEL: u32 = 1 << 1;
/// The layer's bits in the misc vector's third channel.
const LAYER_BITS: u32 = 0x7FF;
/// The first parameter target; `param<n>` is this plus n.
const EXPORT_PARAMETER: i64 = 32;
/// How many parameter targets there are, so a code past them is not read as one.
const EXPORT_PARAMETERS: i64 = 32;
/// The primitive export: the triangle's vertex indices, packed into one register.
pub(crate) const EXPORT_PRIMITIVE: i64 = 20;
/// `SQ_EXP_NULL` (`gfx10-rsrc.json`): an export that writes nothing, which ACO's early exit from a
/// pixel shader makes with no channels, `done` and `vm` (`aco_lower_to_hw_instr.cpp:2471`).
const EXPORT_NULL: i64 = 9;
/// The depth export target, `MRTZ`: Z in its first channel, then stencil, sample mask and the
/// `mrt0` alpha (Mesa `ac_get_spi_shader_z_format`).
pub(crate) const EXPORT_MRTZ: i64 = 8;

/// A geometry stage's export - a vertex's position or parameters, or the primitive that joins
/// them, one lane a vertex (D688) - carried out; `None` for any other target.
fn geometry_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    target: i64,
    sources: [&Operand; 4],
) -> Option<Result<(), TranslateError>> {
    // A streamout pass keeps only what the program stores: with the rasterizer killed, a vertex's
    // position and parameters reach nothing on the hardware either (D861).
    if ((EXPORT_POSITION..EXPORT_POSITION + 4).contains(&target)
        || export_parameter_location(target).is_some())
        && model.discards_vertex_exports()
    {
        model.count();
        return Some(Ok(()));
    }
    if target == EXPORT_POSITION {
        return Some(mesh_vertex_export(model, instruction, sources, None));
    }
    if let Some(location) = export_parameter_location(target) {
        return Some(mesh_vertex_export(
            model,
            instruction,
            sources,
            Some(location),
        ));
    }
    (target == EXPORT_PRIMITIVE).then(|| mesh_primitive_export(model, instruction, sources[0]))
}

/// Which parameter location an export target names, if it names one.
///
/// Public because the declaration pass needs it before any instruction is translated.
#[must_use]
pub fn export_parameter_location(target: i64) -> Option<u32> {
    let location = target.checked_sub(EXPORT_PARAMETER)?;
    (0..EXPORT_PARAMETERS)
        .contains(&location)
        .then(|| u32::try_from(location).unwrap_or(0))
}

/// The message that asks the geometry engine for room to export: the reference assembler encodes
/// `s_sendmsg sendmsg(MSG_GS_ALLOC_REQ)` as `0xbf900009`.
const MSG_GS_ALLOC_REQ: i64 = 9;

/// Where the vertex and primitive counts sit in `m0`, for the allocation request.
///
/// The low field is the vertex count and the high field the primitive count (D688). Hardware runs
/// of a one-triangle body distinguish the order: `0x1003` and `0x1004` draw, and `0x3001`, one
/// vertex for a body that writes three, never completes. Those runs fit any split between bits 3
/// and 12; twelve reads the SDK's `0x1003` as one primitive of three vertices, and real counts stay
/// far below 2^12.
const MESH_COUNT_BITS: u32 = 12;

/// `s_sendmsg`: a message to a fixed-function unit outside the shader core.
///
/// Only the allocation request is translated, and only at the mesh stage, where the host also
/// declares its output before emitting. Every other message is refused by name.
fn send_message<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let Some(Operand::Immediate(message)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a message's payload did not decode, so which message it is is unknown",
        });
    };
    if *message != MSG_GS_ALLOC_REQ {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "only the geometry allocation request is translated; every other message 
                     asks a fixed-function unit for something this translator has no host 
                     counterpart for, and dropping one silently is not an answer",
        });
    }

    // The counts the guest wrote into `m0`, read back as values, since the host's declaration takes
    // ids and a shader may compute its counts.
    let counts = model.read_m0();
    let width = model.constant((1 << MESH_COUNT_BITS) - 1);
    let vertices = model.binary(op::BITWISE_AND, counts, width);
    let shift = model.constant(MESH_COUNT_BITS);
    let high = model.binary(op::SHIFT_RIGHT_LOGICAL, counts, shift);
    let primitives = model.binary(op::BITWISE_AND, high, width);

    if model.set_mesh_outputs(vertices, primitives).is_none() {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the geometry allocation request declares how much a workgroup will emit, 
                     and only a mesh module has somewhere to declare it - translate at the mesh 
                     stage, which is what a primitive shader is (D688)",
        });
    }
    Ok(())
}

/// `exp pos0` and `exp param<n>`: one vertex's position or one of its parameters.
///
/// The guest narrows its execution mask to the vertex lanes and exports once; each active lane is a
/// vertex. A mesh shader writes an array indexed the same way.
fn mesh_vertex_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    sources: [&Operand; 4],
    location: Option<u32>,
) -> Result<(), TranslateError> {
    for lane in running_lanes(model) {
        let mut components = [Id(0); 4];
        for (slot, source) in sources.into_iter().enumerate() {
            let bits = model.read_source(instruction, source, lane)?;
            components[slot] = model.as_float(bits);
        }
        let written = match location {
            Some(location) => model.write_mesh_parameter(location, lane, components),
            None => model.write_mesh_position(lane, components),
        };
        if written.is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a position or parameter export needs the vertex outputs a mesh module 
                         declares, and this module is not one - translate at the mesh stage, 
                         which is what a primitive shader is (D688)",
            });
        }
    }
    Ok(())
}

/// `exp pos1` with only its third channel: one vertex's render-target layer, bits 10:0 of the
/// register (D859). The viewport index above it reaches nothing here; a draw that has the hardware
/// read it is refused where the draw is prepared.
fn mesh_layer_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    source: &Operand,
) -> Result<(), TranslateError> {
    if instruction.word & EXPORT_COMPRESSED != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a compressed misc-vector export is not translated",
        });
    }
    for lane in running_lanes(model) {
        let bits = model.read_source(instruction, source, lane)?;
        let mask = model.constant(LAYER_BITS);
        let layer = model.binary(op::BITWISE_AND, bits, mask);
        if model.write_mesh_layer(lane, layer).is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a layer export needs the layers a mesh module declares, and this module ",
                    "is not one - translate at the mesh stage (D688, D859)"
                ),
            });
        }
    }
    Ok(())
}

/// `exp pos1`'s first channel: one vertex's point size, a float (D867). It is the size the point is
/// drawn at where `PA_CL_VS_OUT_CNTL.USE_VTX_POINT_SIZE` has the hardware read it, which radeonsi
/// sets exactly when the shader writes one.
fn mesh_point_size_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    source: &Operand,
) -> Result<(), TranslateError> {
    if instruction.word & EXPORT_COMPRESSED != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a compressed misc-vector export is not translated",
        });
    }
    for lane in running_lanes(model) {
        let bits = model.read_source(instruction, source, lane)?;
        if model.write_mesh_point_size(lane, bits).is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a point size export needs the sizes a mesh module declares, and this module ",
                    "is not one - translate at the mesh stage (D688, D867)"
                ),
            });
        }
    }
    Ok(())
}

/// `PA_CL_VS_OUT_CNTL.VS_OUT_MISC_VEC_ENA`, bit 21 (`gfx103.json`): the misc vector is exported,
/// first after the position.
const VS_OUT_MISC_VEC_ENA: u32 = 1 << 21;
/// `VS_OUT_CCDIST0_VEC_ENA`, bit 22, and `VS_OUT_CCDIST1_VEC_ENA`, bit 23: the vectors of distances
/// 0-3 and 4-7, after the misc vector.
const VS_OUT_CCDIST_VEC_ENA: [u32; 2] = [1 << 22, 1 << 23];
/// `USE_VTX_POINT_SIZE`, bit 16: a point is drawn at the size its vertex exports.
const USE_VTX_POINT_SIZE: u32 = 1 << 16;
/// `USE_VTX_VRS_RATE`, bit 28: the shading rate the misc vector's fourth channel carries is read.
const USE_VTX_VRS_RATE: u32 = 1 << 28;
/// The misc vector's fourth channel: the shading rate.
const EXPORT_VRS_RATE_CHANNEL: u32 = 1 << 3;
/// `CULL_DIST_ENA_0`, bit 8: distance `n` culls where bit `8 + n` is set, as `CLIP_DIST_ENA_n`,
/// bit `n`, clips.
const CULL_DIST_SHIFT: u32 = 8;

/// What a further position export is, by the draw's routing (D867).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PositionVector {
    /// The misc vector: point size, edge flag, layer and viewport index, shading rate.
    Misc,
    /// Four clip or cull distances from this index.
    Distances(u32),
    /// A vector the draw does not enable.
    Unread,
    /// Not known: the draw's routing was not given and this is not `pos1`.
    Unknown,
}

/// What `pos(1 + n)` is under `routing`: the enabled vectors in order - the misc vector, then the
/// two distance vectors (`ac_nir_prerast_utils.c` 355-430, radeonsi `si_state_shaders.cpp`). With
/// no routing, `pos1` is the misc vector, as D859 took it.
fn position_vector(routing: Option<u32>, target: i64) -> PositionVector {
    let Some(cntl) = routing else {
        return if target == EXPORT_MISC {
            PositionVector::Misc
        } else {
            PositionVector::Unknown
        };
    };
    let mut vectors = Vec::with_capacity(3);
    if cntl & VS_OUT_MISC_VEC_ENA != 0 {
        vectors.push(PositionVector::Misc);
    }
    for (vector, enable) in (0u32..).zip(VS_OUT_CCDIST_VEC_ENA) {
        if cntl & enable != 0 {
            vectors.push(PositionVector::Distances(4 * vector));
        }
    }
    usize::try_from(target - EXPORT_MISC)
        .ok()
        .and_then(|at| vectors.get(at).copied())
        .unwrap_or(PositionVector::Unread)
}

/// Whether `pos1` can be the misc vector under `routing`: always where it is not known.
#[must_use]
pub(crate) fn misc_vector_present(routing: Option<u32>) -> bool {
    routing.is_none_or(|cntl| cntl & VS_OUT_MISC_VEC_ENA != 0)
}

/// Whether the draw reads the point size its vertices export: where its routing says so, or is not
/// known - radeonsi sets `USE_VTX_POINT_SIZE` exactly when the shader writes one.
#[must_use]
pub(crate) fn point_size_read(routing: Option<u32>) -> bool {
    routing.is_none_or(|cntl| cntl & VS_OUT_MISC_VEC_ENA != 0 && cntl & USE_VTX_POINT_SIZE != 0)
}

/// The exported distances the draw clips by, one bit each: `CLIP_DIST_ENA_n` for a distance whose
/// vector it enables.
#[must_use]
pub(crate) fn exported_clip_distances(routing: Option<u32>) -> u8 {
    let Some(cntl) = routing else {
        return 0;
    };
    let mut mask = 0u32;
    for (vector, enable) in (0u32..).zip(VS_OUT_CCDIST_VEC_ENA) {
        if cntl & enable != 0 {
            mask |= cntl & 0xF << (4 * vector);
        }
    }
    u8::try_from(mask).unwrap_or(0)
}

/// `exp pos2`/`pos3` (or `pos1`, where the draw exports no misc vector): four clip or cull
/// distances from `first` (D867). One the draw clips by is the vertex's clip distance, whether or
/// not it culls by it too; one it only culls by is refused, not modelled; one it does neither with
/// reaches nothing.
fn mesh_distance_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (first, sources): (u32, [&Operand; 4]),
) -> Result<(), TranslateError> {
    if instruction.word & EXPORT_COMPRESSED != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a compressed distance export is not translated",
        });
    }
    let cntl = model.position_vectors().unwrap_or(0);
    let clips = exported_clip_distances(Some(cntl));
    for (channel, source) in (0u32..).zip(sources) {
        let index = first + channel;
        if instruction.word >> channel & 1 == 0 {
            continue;
        }
        let clipped = clips >> index & 1 != 0;
        // radeonsi culls by every distance it clips by (`si_emit_clip_regs`), which removes nothing
        // the clipping keeps; a distance that only culls is not modelled.
        if cntl >> (CULL_DIST_SHIFT + index) & 1 != 0 && !clipped {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a cull distance that does not also clip (PA_CL_VS_OUT_CNTL.CULL_DIST_ENA_* ",
                    "without CLIP_DIST_ENA_*) is not translated"
                ),
            });
        }
        if !clipped {
            continue;
        }
        for lane in running_lanes(model) {
            let bits = model.read_source(instruction, source, lane)?;
            if model.write_mesh_clip_distance(lane, index, bits).is_none() {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "a clip distance export needs the distances a mesh module declares, and ",
                        "this module is not one - translate at the mesh stage (D688, D867)"
                    ),
                });
            }
        }
    }
    Ok(())
}

/// `exp prim`: the triangle's three vertex indices, packed into one register.
///
/// The SDK's vertex program writes `0x20280600` for vertices 0, 1, 2 with edge flags (oracle record
/// A): `0 | 1 << 10 | 2 << 20` with bits 9, 19 and 29 set, three nine-bit indices each followed by
/// an edge flag. The edge flags select wireframe edges, which the host decides itself, so they are
/// not translated.
fn mesh_primitive_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    packed: &Operand,
) -> Result<(), TranslateError> {
    /// Bits per index; the tenth bit of each field is an edge flag.
    const INDEX_BITS: u32 = 9;
    /// Where each index starts.
    const INDEX_SHIFTS: [u32; 3] = [0, 10, 20];

    // How many indices this primitive uses: three for a triangle, two for a line, one for a point.
    // Assumed for points and lines: the measured packing (oracle record A) is a triangle, and this
    // reads the low `count` nine-bit fields in the same order.
    let count = model.mesh_primitive().indices() as usize;

    for lane in running_lanes(model) {
        let word = model.read_source(instruction, packed, lane)?;
        let mask = model.constant((1 << INDEX_BITS) - 1);
        let mut indices = Vec::with_capacity(count);
        for shift in INDEX_SHIFTS.into_iter().take(count) {
            let amount = model.constant(shift);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word, amount);
            indices.push(model.binary(op::BITWISE_AND, shifted, mask));
        }
        if model.write_mesh_indices(lane, &indices).is_none() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a primitive export needs the index array a mesh module declares, and 
                         this module is not one - translate at the mesh stage (D688)",
            });
        }
    }
    Ok(())
}

/// `mrt0`, the colour output at location zero, which every fragment module declares.
///
/// `mrtK` writes the output at location `K`; which colour buffer it reaches is register state the
/// pipeline binds from what the guest wrote, never invented here (D104).
const MRT0: u32 = 0;
/// `mrt7`, the last colour target an export can name.
pub(crate) const MRT7: u32 = 7;

/// An export's `COMPR` bit, in its first word: the sources carry packed halves
/// (`aco_assembler.cpp:1001` in the collection's Mesa tree).
const EXPORT_COMPRESSED: u32 = 1 << 10;

/// An export's `EN` field, bits 0-3: which of the four channels it writes
/// (`aco_assembler.cpp:1005`).
const EXPORT_ENABLE_MASK: u32 = 0xf;

/// An export's `VM` bit, 12 (`aco_assembler.cpp:1000`): the execution mask is the valid mask, so a
/// pixel whose lane is inactive is discarded. The final colour export of every pixel shader
/// carries it (`aco_assembler.cpp:1474-1475`), and a compiler discards by clearing the lane's bit
/// before it (`aco_select_nir_intrinsics.cpp:4676`).
const EXPORT_VALID_MASK: u32 = 1 << 12;

/// A valid-mask export (`vm`) discards the pixel when its lane is inactive. A model with no
/// execution mask has no inactive lane to discard.
fn discard_inactive<M: Model + ?Sized>(model: &mut M, instruction: &Instruction) {
    if instruction.word & EXPORT_VALID_MASK != 0
        && let Ok((low, high)) = model.read_lane_mask(EXEC_LOW_HALF)
    {
        let active = model.lane_bit(low, high, 0);
        let b = model.builder();
        let (discard, kept) = (b.id(), b.id());
        b.function(op::SELECTION_MERGE, &[kept.0, 0]);
        b.function(op::BRANCH_CONDITIONAL, &[active.0, kept.0, discard.0]);
        b.function(op::LABEL, &[discard.0]);
        b.function(op::KILL, &[]);
        b.function(op::LABEL, &[kept.0]);
    }
}

/// The further position exports: `pos1`'s misc vector (point size, edge flag, layer, viewport
/// index) and the clip and cull distances in `pos2` and `pos3`. `pos1`'s point size
/// ([`mesh_point_size_export`]), edge flag ([`EXPORT_EDGE_FLAG_CHANNEL`]) and layer
/// ([`mesh_layer_export`]) are translated; the rest is refused by what it is, rather than taken for
/// a colour export with nowhere to go.
fn further_position<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (target, sources): (i64, [&Operand; 4]),
) -> Result<(), TranslateError> {
    let routing = model.position_vectors();
    let channels = instruction.word & EXPORT_ENABLE_MASK;
    match position_vector(routing, target) {
        PositionVector::Misc => {
            let mut modelled = EXPORT_POINT_SIZE_CHANNEL | EXPORT_EDGE_FLAG_CHANNEL;
            modelled |= EXPORT_LAYER_CHANNEL;
            // The shading rate in the fourth channel reaches nothing where the draw does not read
            // it.
            if routing.is_some_and(|cntl| cntl & USE_VTX_VRS_RATE == 0) {
                modelled |= EXPORT_VRS_RATE_CHANNEL;
            }
            if channels != 0 && channels & !modelled == 0 {
                if channels & EXPORT_POINT_SIZE_CHANNEL != 0 && point_size_read(routing) {
                    mesh_point_size_export(model, instruction, sources[0])?;
                }
                if channels & EXPORT_LAYER_CHANNEL != 0 {
                    mesh_layer_export(model, instruction, sources[2])?;
                }
                return Ok(());
            }
        }
        PositionVector::Distances(first) => {
            return mesh_distance_export(model, instruction, (first, sources));
        }
        // A vector the draw does not enable reaches nothing on the hardware.
        PositionVector::Unread => return Ok(()),
        PositionVector::Unknown => {}
    }
    Err(TranslateError::Unsupported {
        offset: instruction.offset,
        detail: concat!(
            "a further position export (pos1's point size, edge flag, layer or viewport ",
            "index, or pos2/pos3's clip and cull distances) is not translated"
        ),
    })
}

/// `exp`: hands four registers to a render target.
///
/// The sources are read for lane zero, reinterpreted as floats (not converted), assembled into a
/// `vec4` and stored to the module's colour output; for a colour target that holds integers they
/// are kept as the bits they are, assembled into a `uvec4` (D851). Refused separately:
///
/// - A module with no colour output: a compute dispatch has nowhere to export to
///   ([`Model::colour_output`]).
/// - A target past `mrt7` that is no other kind of export; see [`MRT0`].
///
/// The mask and the compressed bit are read from the instruction's first word. A compressed export
/// is unpacked from its two half-packed sources, red and green from the first and blue and alpha
/// from the second. An export, compressed or not, that enables some channels stores those, and
/// keeps the others as the output held them: the colour target takes only the channels its format
///   holds, and a draw that writes one the shader did not export is refused where the draw is
///   prepared. With `vm` set a pixel whose lane is inactive is discarded ([`EXPORT_VALID_MASK`]);
///   `done` changes nothing a single-export translation does.
fn export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [target, sources @ ..] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export decodes to a target and four sources; this one did not",
        });
    };
    let Operand::Immediate(target) = target else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export's target is not an immediate, so which one it names is unknown",
        });
    };
    let [a, b, c, d] = sources else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export takes four sources, and this one decoded a different number",
        });
    };

    if let Some(done) = geometry_export(model, instruction, *target, [a, b, c, d]) {
        return done;
    }
    if (EXPORT_MISC..=EXPORT_POSITION + 3).contains(target) {
        return further_position(model, instruction, (*target, [a, b, c, d]));
    }

    let Some((vec4, colour)) = model.colour_output(0) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an export needs somewhere to go: a colour attachment at the fragment 
                     stage, or the vertex and primitive outputs at the mesh one. This module 
                     is a compute dispatch and has neither",
        });
    };
    // The null target writes nothing; with `vm` it still discards the pixels whose lanes are
    // inactive, which is what ACO's early exit is for.
    if *target == EXPORT_NULL {
        if instruction.word & EXPORT_ENABLE_MASK != 0 {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a null export that enables channels, which it would write nowhere",
            });
        }
        discard_inactive(model, instruction);
        model.count();
        return Ok(());
    }
    if *target == EXPORT_MRTZ {
        return depth_export(model, instruction, (a, c));
    }
    let (vec4, colour) = colour_target_output(model, instruction, *target, (vec4, colour))?;
    // An integer target takes the exported bits as they are, and a compressed export to one packs
    // 16-bit integers, not halves (D851).
    let integer = u32::try_from(*target).is_ok_and(|target| model.colour_is_integer(target));

    // The channels written (`EN`, bits 0-3, `aco_assembler.cpp:1005`). No channel is an export that
    // writes nothing, such as one raised only to end the wave, so nothing is stored. Each bit is
    // one channel in a compressed export too: Grand Theft Auto V's enables red and green alone,
    // the halves of its first source.
    let enabled = instruction.word & EXPORT_ENABLE_MASK;
    if enabled == 0 {
        return Ok(());
    }

    discard_inactive(model, instruction);

    let mut components = Vec::with_capacity(4);
    if instruction.word & EXPORT_COMPRESSED != 0 {
        // Compressed: two registers of two halves each, (r, g) in the first source and (b, a) in
        // the second, low half first, as `v_cvt_pkrtz_f16_f32` packs them; the third and fourth
        // sources are unused (`aco_select_ps_epilog.cpp:170-185`).
        let mask = model.constant(0xffff);
        let sixteen = model.constant(16);
        for source in [a, b] {
            let packed = model.read_source(instruction, source, 0)?;
            let low = model.binary(op::BITWISE_AND, packed, mask);
            let high = model.binary(op::SHIFT_RIGHT_LOGICAL, packed, sixteen);
            for half in [low, high] {
                if integer {
                    components.push(half.0);
                } else {
                    let bits = model.half_to_float_bits(half);
                    components.push(model.as_float(bits).0);
                }
            }
        }
    } else {
        for source in [a, b, c, d] {
            // Lane zero: one fragment is one pixel.
            let bits = model.read_source(instruction, source, 0)?;
            components.push(if integer {
                bits.0
            } else {
                model.as_float(bits).0
            });
        }
    }

    let builder = model.builder();
    let value = builder.id();
    let mut construct = vec![vec4.0, value.0];
    construct.extend(components);
    builder.function(op::COMPOSITE_CONSTRUCT, &construct);
    let value = only_enabled(builder, (vec4, colour), value, enabled);
    builder.function(op::STORE, &[colour.0, value.0]);
    Ok(())
}

/// The colour output export `target` writes: `mrtK` is the output at location `K`. Which colour
/// buffer that is - the `K`th one `SPI_SHADER_COL_FORMAT` gives a format - is the pipeline's to bind
/// from the registers the guest wrote (D553). `mrt0`'s is `zero`.
fn colour_target_output<M: Model + ?Sized>(
    model: &M,
    instruction: &Instruction,
    target: i64,
    zero: (Id, Id),
) -> Result<(Id, Id), TranslateError> {
    match u32::try_from(target) {
        Ok(MRT0) => Ok(zero),
        Ok(other @ ..=MRT7) => model
            .colour_output(other)
            .ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an export to a colour target the module declared no output for",
            }),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "an export to a target that is no colour target, position, parameter, ",
                "primitive, depth or null"
            ),
        }),
    }
}

/// `exp mrtz`: the depth a pixel replaces its rasterised one with, from the first source, and the
/// samples it keeps, from the third - Z and the sample mask of the 32-bit form, which Mesa puts in
/// channels 0 and 2 (`ac_nir_lower_ps_late.c`, `emit_ps_mrtz_export`). Stencil and the `mrt0` alpha
/// (channels 1 and 3) and a compressed export are refused by name. With `vm` set a pixel whose
/// lane is inactive is discarded, as for a colour export.
fn depth_export<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (depth_source, mask_source): (&Operand, &Operand),
) -> Result<(), TranslateError> {
    let enabled = instruction.word & EXPORT_ENABLE_MASK;
    if enabled & !(MRTZ_DEPTH | MRTZ_SAMPLE_MASK) != 0
        || enabled == 0
        || instruction.word & EXPORT_COMPRESSED != 0
    {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a depth export that writes stencil or alpha, or is compressed, is not ",
                "translated - only Z and the sample mask are"
            ),
        });
    }
    let no_output = |what| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: what,
    };
    let depth = if enabled & MRTZ_DEPTH == 0 {
        None
    } else {
        Some(model.depth_output().ok_or_else(|| {
            no_output(
                "a depth export from a module with no depth output: only a pixel shader has one",
            )
        })?)
    };
    let mask = if enabled & MRTZ_SAMPLE_MASK == 0 {
        None
    } else {
        Some(model.sample_mask_output().ok_or_else(|| {
            no_output("a sample mask export from a module with no sample mask output: only a pixel shader has one")
        })?)
    };
    discard_inactive(model, instruction);
    // Lane zero: one fragment is one pixel.
    if let Some((_, depth)) = depth {
        let bits = model.read_source(instruction, depth_source, 0)?;
        let value = model.as_float(bits);
        model.builder().function(op::STORE, &[depth.0, value.0]);
    }
    if let Some((pointer, mask)) = mask {
        let bits = model.read_source(instruction, mask_source, 0)?;
        let first = model.constant(0);
        let element = model.builder().id();
        model
            .builder()
            .function(op::ACCESS_CHAIN, &[pointer.0, element.0, mask.0, first.0]);
        model.builder().function(op::STORE, &[element.0, bits.0]);
    }
    model.count();
    Ok(())
}

/// The channel of an `exp mrtz` that carries Z.
const MRTZ_DEPTH: u32 = 1;

/// The channel of an `exp mrtz` that carries the sample mask, in the 32-bit form
/// (`ac_nir_lower_ps_late.c`, `emit_ps_mrtz_export`).
const MRTZ_SAMPLE_MASK: u32 = 4;

/// `value` with only the `enabled` channels taken from it, the rest as the output `colour` held
/// them; `value` itself when every channel is enabled.
fn only_enabled(builder: &mut Builder, (vec4, colour): (Id, Id), value: Id, enabled: u32) -> Id {
    if enabled == EXPORT_ENABLE_MASK {
        return value;
    }
    let held = builder.id();
    builder.function(op::LOAD, &[vec4.0, held.0, colour.0]);
    let merged = builder.id();
    let mut shuffle = vec![vec4.0, merged.0, held.0, value.0];
    shuffle.extend((0..4).map(|channel| {
        if enabled >> channel & 1 == 1 {
            4 + channel
        } else {
            channel
        }
    }));
    builder.function(op::VECTOR_SHUFFLE, &shuffle);
    merged
}

/// The lanes a masked-write-only instruction emits code for: every lane but those the model knows
/// are inactive. An instruction that writes a mask loops over every lane instead, because its
/// answer for an inactive lane is a bit that is read.
pub(crate) fn running_lanes<M: Model + ?Sized>(model: &M) -> Vec<u32> {
    // Inside a loop over the lanes the one pass is the loop's lane, whichever it is at run time.
    if model.in_lane_loop() {
        return vec![0];
    }
    (0..model.modelled_lanes())
        .filter(|&lane| model.lane_may_run(lane))
        .collect()
}

/// A mask of every modelled lane's `condition`, a bit a lane, as two halves: lane by lane, or in one
/// loop over the lanes where the model opens one ([`Model::open_carrying_lane_loop`]). Where
/// `skip_idle`, a lane known not to run answers zero unread.
fn every_lane_bit<M: Model + ?Sized>(
    model: &mut M,
    skip_idle: bool,
    mut condition: impl FnMut(&mut M, u32) -> Result<Id, TranslateError>,
) -> Result<(Id, Id), TranslateError> {
    let zero = model.constant(0);
    if let Some(halves) = model.open_carrying_lane_loop((zero, zero)) {
        let holds = condition(model, 0)?;
        let halves = model.set_lane_bit(halves, 0, holds);
        return Ok(model.close_carrying_lane_loop(halves));
    }
    let mut halves = (zero, zero);
    for lane in 0..model.modelled_lanes() {
        if skip_idle && model.lane_known_idle(lane) {
            continue;
        }
        let holds = condition(model, lane)?;
        halves = model.set_lane_bit(halves, lane, holds);
    }
    Ok(halves)
}

/// The instructions [`Model::open_lane_loop`] may run inside a loop over the lanes: each makes one
/// pass over the running lanes, reaches its lane only through the model's per-lane reads and
/// writes, and writes no scalar register or mask, so one pass for a lane known at run time is the
/// lanes' passes one after another. The plain encodings only: a DPP form reads other lanes by
/// number, and the SDWA forms take their own paths.
pub const LANE_LOOPED: &[&str] = &[
    "buffer_load_dword",
    "buffer_load_dwordx2",
    "buffer_load_dwordx3",
    "buffer_load_dwordx4",
    "buffer_store_dword",
    "buffer_store_dwordx2",
    "buffer_store_dwordx3",
    "buffer_store_dwordx4",
    // A part store's compare-and-swap loop once, not once a lane (D977): unrolled over a wave's
    // lanes, a driver's compiler did not finish the module.
    "buffer_store_byte",
    "buffer_store_short",
    // A global access's page lookup (D961) once, not once a lane.
    "global_load_dword",
    "global_load_dwordx2",
    "global_load_dwordx3",
    "global_load_dwordx4",
    "global_load_ubyte",
    "global_load_sbyte",
    "global_load_ushort",
    "global_load_sshort",
    "global_store_dword",
    "global_store_dwordx2",
    "global_store_dwordx3",
    "global_store_dwordx4",
    "global_store_byte",
    "global_store_short",
    // An image access's fetch, sample or store once, not once a lane, and a buffer access that
    // converts by its descriptor's format the same.
    "image_load",
    "image_load_mip",
    "image_store",
    "image_store_mip",
    "image_sample_lz",
    "image_sample_lz_o",
    "image_sample_l",
    "image_sample_d",
    "image_sample_c_lz",
    "image_gather4_lz",
    // A ray's node read and test once, not once a lane: it reads memory and writes only its lane's
    // result registers.
    "image_bvh_intersect_ray",
    "image_bvh64_intersect_ray",
    "buffer_load_format_x",
    "buffer_load_format_xy",
    "buffer_load_format_xyz",
    "buffer_load_format_xyzw",
    "buffer_store_format_x",
    "buffer_store_format_xy",
    "buffer_store_format_xyz",
    "buffer_store_format_xyzw",
    "ds_read_b32",
    "ds_write_b32",
    "ds_read2_b32",
    "ds_write2_b32",
    "ds_read2_b64",
    "ds_write2_b64",
    "ds_read_b64",
    "ds_read_b96",
    "ds_read_b128",
    "ds_write_b64",
    "ds_write_b96",
    "ds_write_b128",
    "v_mov_b32_e32",
    "v_cndmask_b32_e32",
    "v_cndmask_b32_e64",
    "v_add_f32_e32",
    "v_sub_f32_e32",
    "v_subrev_f32_e32",
    "v_mul_f32_e32",
    "v_lshlrev_b32_e32",
    "v_lshrrev_b32_e32",
    "v_ashrrev_i32_e32",
    "v_add_nc_u32_e32",
    "v_sub_nc_u32_e32",
    "v_subrev_nc_u32_e32",
    "v_fmac_f32_e32",
    "v_or_b32_e32",
    "v_xor_b32_e32",
    "v_and_b32_e32",
    "v_mac_f32_e32",
    "v_mul_u32_u24_e32",
    "v_min_u32_e32",
    "v_max_u32_e32",
    "v_min_i32_e32",
    "v_max_i32_e32",
    "v_max_f32_e32",
    "v_min_f32_e32",
    "v_add_f32_e64",
    "v_sub_f32_e64",
    "v_subrev_f32_e64",
    "v_mul_f32_e64",
    "v_mac_f32_e64",
    "v_fma_f32",
    "v_mad_f32",
    "v_madak_f32",
    "v_madmk_f32",
    "v_fmaak_f32",
    "v_fmamk_f32",
    "v_max3_f32",
    "v_min3_f32",
    "v_med3_f32",
    "v_lshl_or_b32",
    "v_lshl_add_u32",
    "v_add3_u32",
    "v_add_lshl_u32",
    "v_and_or_b32",
    "v_or3_b32",
    "v_mul_lo_u32",
    "v_bfe_u32",
    "v_bfe_i32",
    "v_rcp_f32_e32",
    "v_cvt_f32_u32_e32",
    "v_cvt_f32_i32_e32",
    "v_cvt_i32_f32_e32",
    "v_cvt_u32_f32_e32",
    "v_sqrt_f32_e32",
    "v_rsq_f32_e32",
    "v_floor_f32_e32",
    "v_ceil_f32_e32",
    "v_fract_f32_e32",
    "v_trunc_f32_e32",
    "v_rndne_f32_e32",
    "v_exp_f32_e32",
    "v_log_f32_e32",
    "v_sin_f32_e32",
    "v_cos_f32_e32",
    "v_not_b32_e32",
    "v_not_b32_e64",
    "v_ffbh_u32_e32",
    "v_ffbl_b32_e32",
    "v_bfi_b32",
    "v_mad_u32_u16",
    "v_mad_i32_i24",
    "v_mad_u32_u24",
    "v_mul_hi_u32",
    "v_mul_hi_i32",
    "v_bfm_b32",
    "v_bcnt_u32_b32",
    "v_ldexp_f32",
    "v_msad_u8",
    "v_alignbyte_b32",
    "v_alignbit_b32",
    "v_movrels_b32_e32",
    "v_movreld_b32_e32",
    "ds_add_u32",
    "ds_add_rtn_u32",
    "ds_min_u32",
    "ds_max_u32",
    "ds_min_f32",
    "ds_max_f32",
    "buffer_atomic_add",
    "buffer_atomic_sub",
    "buffer_atomic_and",
    "buffer_atomic_or",
    "buffer_atomic_xor",
    "buffer_atomic_umin",
    "buffer_atomic_umax",
    "buffer_atomic_smin",
    "buffer_atomic_smax",
    "buffer_atomic_fmin",
    "buffer_atomic_fmax",
    "buffer_atomic_swap_x2",
    "buffer_atomic_or_x2",
];

/// Whether `instruction` is one of [`LANE_LOOPED`] and on the local share rather than the global:
/// in its plain encoding, or in an SDWA form, which reads and writes only its own lane and so is
/// one pass over a lane (D979) - unless it is a compare or writes a carry, which build a lane mask.
pub fn lane_looped<M: Model + ?Sized>(model: &mut M, instruction: &Instruction) -> bool {
    let Ok(name) = resolve(model, instruction) else {
        return false;
    };
    let name = name.as_str();
    if dpp_word(model.encodings(), instruction).is_some()
        || name.starts_with("ds_") && instruction.word & DS_GDS != 0
    {
        return false;
    }
    if sdwa_word(model.encodings(), instruction).is_some() {
        let family = family_of(model.encodings(), instruction);
        return sdwa::refusal(&family, name).is_none() && !writes_a_carry(name);
    }
    LANE_LOOPED.contains(&name)
}

/// The name of `instruction`'s encoding family, empty where it has none.
fn family_of(encodings: &EncodingTable, instruction: &Instruction) -> String {
    instruction
        .encoding
        .and_then(|index| encodings.encodings().get(usize::from(index)))
        .map_or_else(String::new, |family| family.name.clone())
}

/// Whether a short-form vector instruction writes a carry or borrow into a lane mask (the `_co_`
/// arithmetic), besides its vector result.
fn writes_a_carry(name: &str) -> bool {
    name.contains("_co_")
}

/// How an instruction takes part in a run of lane work fused into one loop over the lanes (D956).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InRun {
    /// It emits nothing, and a run goes on past it.
    Nothing,
    /// One of [`LANE_LOOPED`], which joins a run: whether it reads memory or the local share, and
    /// whether it stores to them or updates them.
    Lanes {
        /// It loads, samples or gathers.
        reads: bool,
        /// It stores, or runs an atomic.
        writes: bool,
    },
    /// Anything else, which ends a run before it.
    Alone,
}

/// The instructions [`instruction`] emits nothing for, which a fused run goes on past.
const EMITS_NOTHING: &[&str] = &[
    "s_waitcnt",
    "s_waitcnt_vscnt",
    "s_waitcnt_depctr",
    "s_clause",
    "s_nop",
    "v_nop",
    "s_inst_prefetch",
    "s_setprio",
];

/// How `instruction` takes part in a fused run of lane work ([`InRun`]).
pub fn in_run<M: Model + ?Sized>(model: &mut M, instruction: &Instruction) -> InRun {
    let Ok(name) = resolve(model, instruction) else {
        return InRun::Alone;
    };
    if EMITS_NOTHING.contains(&name.as_str()) {
        return InRun::Nothing;
    }
    if !lane_looped(model, instruction) {
        return InRun::Alone;
    }
    let writes = name.contains("store")
        || name.contains("atomic")
        || ["ds_write", "ds_add", "ds_min", "ds_max"]
            .iter()
            .any(|update| name.starts_with(update));
    let reads = [
        "load",
        "image_sample",
        "image_gather",
        "ds_read",
        "_intersect_ray",
    ]
    .iter()
    .any(|read| name.contains(read));
    InRun::Lanes { reads, writes }
}

/// Translates one instruction into whichever model it is handed.
///
/// Refusing is the default. An instruction with no arm here is an error, never a no-op: a shader
/// missing one instruction computes the wrong thing while appearing to work.
pub fn instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    model.begin_instruction(instruction.offset);
    let name = resolve(model, instruction)?;
    // A rewrite of a table descriptor's type alone keeps the image it names (D895).
    if model.rewrites_only_an_image_type(instruction.offset) {
        model.count();
        return Ok(());
    }

    if !supports(&name) {
        return Err(match blocked(&name) {
            Some(detail) => TranslateError::Unsupported {
                offset: instruction.offset,
                detail,
            },
            None => TranslateError::NotTranslated {
                offset: instruction.offset,
                mnemonic: name.clone(),
                detail: NO_TRANSLATION,
            },
        });
    }
    let name = name.as_str();

    // A DPP16 or DPP8 form: the instruction with its first source read from another lane of its
    // row or group of eight. Admitted only as `dpp_translated` describes, before translation began.
    if let Some((marker, form)) = dpp_word(model.encodings(), instruction) {
        return dpp_form(model, instruction, (name, marker, form));
    }

    // An SDWA form: the same operation over selected parts of its sources, placed into part of
    // its destination (D979). A compare writes a lane mask rather than a register, so takes its
    // own path; every one- and two-source instruction takes the one shared path.
    if let Some(word) = sdwa_word(model.encodings(), instruction) {
        if name.starts_with("v_cmp_") && name.ends_with("_f16_e32") {
            return half_compare(model, instruction, name, Some(word));
        }
        if sdwa_compare_admitted(name) {
            return sdwa_compare(model, instruction, name, word);
        }
        let family = family_of(model.encodings(), instruction);
        return sdwa::sdwa_form(model, instruction, (&family, name), word);
    }

    match name {
        // Instructions that emit nothing, matched explicitly so "emits nothing" never looks like
        // "nobody handled it":
        //
        // - `s_endpgm` ends the shader; the epilogue is emitted when the module is finished.
        // - `s_waitcnt` waits for memory operations; SPIR-V orders memory through the operations'
        //   own semantics.
        // - `s_nop` inserts wait states for a hardware hazard the host does not have.
        // - `s_inst_prefetch` is an instruction-cache hint with no architectural effect.
        // - `s_clause` groups following instructions for uninterrupted issue; scheduling belongs to
        //   the host driver.
        // - `s_setprio` sets the wave's issue priority among waves, which orders nothing.
        // - `s_waitcnt_vscnt` waits for vector stores, ordered as `s_waitcnt` orders loads.
        // - `s_waitcnt_depctr` holds issue until named dependency counters drain, covering a
        //   hardware hazard between one instruction's write and the next one's read; the host
        //   orders every result before its use.
        // - `s_barrier` waits for the workgroup's other waves. A group of one wave has none to wait
        //   for; a group of several meets in the dispatch loop's own barrier, between the loops
        //   its blocks are cut into at each `s_barrier` (D892).
        "s_endpgm" | "s_waitcnt" | "s_clause" | "s_nop" | "v_nop" | "s_inst_prefetch"
        | "s_setprio" | "s_waitcnt_vscnt" | "s_waitcnt_depctr" | "s_barrier" => Ok(()),
        // A cache invalidation ([`invalidates_a_cache`]).
        name if invalidates_a_cache(name) => {
            model.count();
            Ok(())
        }

        // `s_trap` enters the trap handler, where one is installed and enabled (`TRAP_PRESENT` in
        // the program's `PGM_RSRC2`); radv sets that bit only with its trap handler on
        // (`src/amd/vulkan/radv_shader.c:2259-2289`), while ACO emits `s_trap` for a debug break
        // whatever the setting (`src/amd/compiler/instruction_selection/
        // aco_select_nir_intrinsics.cpp:4720`), so a program runs past one unchanged where no
        // handler is present. The model says whether that is known; elsewhere it is refused.
        "s_trap" => {
            if model.no_trap_handler() {
                model.count();
                Ok(())
            } else {
                Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "s_trap in a stage not known to run without a trap handler (TRAP_PRESENT)",
                })
            }
        }

        // The export, which is why a fragment stage exists at all (D553).
        "exp" => export(model, instruction),

        // Both halves of the interpolation pair answer the whole value (D555).
        "v_interp_p1_f32_e32" | "v_interp_p2_f32_e32" => interpolate(model, instruction, false),

        // The same read, from an input the declaration pass decorated `Flat`.
        "v_interp_mov_f32_e32" => parameter_move(model, instruction),

        // The message a primitive shader opens with, and a mesh module's first act.
        "s_sendmsg" => send_message(model, instruction),

        _ => scalar_instruction(model, instruction, name),
    }
}

/// Translates the scalar ALU instructions, passing any other on to the vector family.
fn scalar_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The scalar moves, split out because they turn on operand width.
        "s_mov_b32" | "s_mov_b64" => scalar_move(model, instruction, name),

        // s_wqm_b64: whole quad mode. Sets each group of four bits of the result if any of the four
        // in the source is set, so a derivative across a quad has all four pixels live.
        "s_wqm_b64" => whole_quad_mode(model, instruction),
        "s_quadmask_b64" => quad_mask(model, instruction),
        "s_brev_b32" => scalar_bit_reverse(model, instruction),
        "s_bitset1_b32" => scalar_bit_set(model, instruction),
        "s_getpc_b64" => program_counter(model, instruction),
        // s_wqm_b32: the same for a 32-lane wavefront, whose mask is one register (`s_wqm_b32
        // exec_lo, exec_lo` before a texture sample).
        "s_wqm_b32" => whole_quad_mode_32(model, instruction),

        // The 64-bit scalar logic a guest computes masks with: narrow by anding with a comparison
        // result, widen by oring, and take the other branch's lanes with `s_andn2_b64`.
        "s_and_b64" | "s_or_b64" | "s_andn2_b64" | "s_xor_b64" | "s_nor_b64" | "s_orn2_b64"
        | "s_nand_b64" => scalar_logic(model, instruction, name),
        // The complement of a mask: `s_not_b64 exec, exec` enters the other side of a branch.
        "s_not_b64" => scalar_not_64(model, instruction),
        "s_not_b32" => scalar_not_32(model, instruction),
        "s_bitreplicate_b64_b32" => bit_replicate(model, instruction),
        "s_bfe_u64" => scalar_bit_field_64(model, instruction),
        "s_lshl_b64" | "s_lshr_b64" => scalar_shift_64(model, instruction, name),
        "s_bitcmp1_b32" | "s_bitcmp0_b32" => {
            scalar_bit_test(model, instruction, name == "s_bitcmp1_b32")
        }
        "s_and_saveexec_b64" => and_save_exec(model, instruction, SaveExec::And),
        "s_andn1_saveexec_b64" => and_save_exec(model, instruction, SaveExec::AndNotSource),
        "s_orn2_saveexec_b64" => and_save_exec(model, instruction, SaveExec::OrNotExec),
        "s_cmp_lg_u64" | "s_cmp_eq_u64" => {
            scalar_compare_64(model, instruction, name == "s_cmp_eq_u64")
        }
        "s_and_saveexec_b32" => and_save_exec_32(model, instruction, SaveExec::And),
        "s_andn1_saveexec_b32" => and_save_exec_32(model, instruction, SaveExec::AndNotSource),
        "s_orn2_saveexec_b32" => and_save_exec_32(model, instruction, SaveExec::OrNotExec),
        "s_bcnt1_i32_b64" => scalar_bit_count_64(model, instruction),
        "s_ff1_i32_b32" | "s_ff1_i32_b64" => scalar_first_one(model, instruction, name),

        // The 32-bit scalar arithmetic and logic, each of which writes the condition code as well
        // as its destination.
        "s_add_i32" | "s_sub_i32" | "s_and_b32" | "s_andn2_b32" | "s_orn2_b32" | "s_or_b32"
        | "s_xor_b32" | "s_lshl_b32" | "s_bfe_u32" | "s_mul_i32" | "s_mul_hi_u32"
        | "s_lshr_b32" | "s_add_u32" | "s_addc_u32" | "s_lshl1_add_u32" | "s_lshl2_add_u32"
        | "s_lshl3_add_u32" | "s_lshl4_add_u32" | "s_cselect_b32" | "s_min_i32" | "s_max_i32"
        | "s_min_u32" | "s_max_u32" | "s_sub_u32" | "s_nor_b32" | "s_nand_b32" | "s_subb_u32" => {
            scalar_integer(model, instruction, name)
        }

        // Field assembly that writes no condition code: two 16-bit halves into one word, and a
        // run of ones into a register pair.
        "s_pack_ll_b32_b16" | "s_pack_hh_b32_b16" => scalar_pack(model, instruction, name),
        "s_bfm_b64" => scalar_bit_mask_64(model, instruction),
        "s_bfm_b32" => scalar_bit_mask(model, instruction),
        "s_cselect_b64" => scalar_select_64(model, instruction),

        // The compact scalar form: a destination and a sixteen-bit immediate.
        "s_movk_i32" | "s_cmpk_eq_i32" | "s_cmpk_lg_i32" | "s_cmpk_gt_i32" | "s_cmpk_ge_i32"
        | "s_cmpk_lt_i32" | "s_cmpk_le_i32" | "s_addk_i32" | "s_mulk_i32" => {
            scalar_immediate(model, instruction, name)
        }
        "s_setreg_b32" | "s_setreg_imm32_b32" => {
            crate::scratch::set_flat_scratch(model, instruction)
        }

        // The scalar compares, which write the condition code the `scc` branches read and have no
        // destination operand.
        "s_cmp_eq_i32" | "s_cmp_lg_i32" | "s_cmp_lg_u32" | "s_cmp_eq_u32" | "s_cmp_gt_i32"
        | "s_cmp_ge_i32" | "s_cmp_lt_i32" | "s_cmp_le_i32" | "s_cmp_gt_u32" | "s_cmp_ge_u32"
        | "s_cmp_lt_u32" | "s_cmp_le_u32" => scalar_compare(model, instruction, name),

        _ => vector_instruction(model, instruction, name),
    }
}

/// `v_cndmask_b32_e32`: the short-form select, whose mask is always the condition mask.
fn short_form_select<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let Some(Operand::Vector(register)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_cndmask_b32 destination is not a vector register",
        });
    };
    let sources: Vec<Operand> = instruction.operands[1..].to_vec();
    select_per_lane(
        model,
        instruction,
        u32::from(*register),
        &sources,
        Modifiers::default(),
    )
}

/// Translates the vector ALU instructions, passing any other on to the memory family.
fn vector_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // Comparisons, which produce masks: every lane compares, and the answers become one value
        // the shader can and into `exec`.
        _ if name.ends_with("_e32") && compare_op(name).is_some() => {
            compare(model, instruction, name)
        }

        // A 64-bit equality, of two register pairs.
        "v_cmp_eq_u64_e64" | "v_cmp_ne_u64_e64" => compare_long_64(model, instruction, name),
        // The long-form compare, into a register pair or a named mask.
        _ if name.ends_with("_e64") && compare_op(name).is_some() => {
            compare_long(model, instruction, name)
        }
        "v_cmpx_le_i16_e32" | "v_cmpx_lt_i16_e32" | "v_cmpx_neq_f32_e32" | "v_cmpx_gt_f32_e32"
        | "v_cmpx_lt_f32_e32" | "v_cmpx_nlt_f32_e32" | "v_cmpx_eq_f32_e32"
        | "v_cmpx_eq_i32_e32" | "v_cmpx_gt_i32_e32" | "v_cmpx_gt_u32_e32" | "v_cmpx_le_i32_e32"
        | "v_cmpx_lt_u32_e32" | "v_cmpx_eq_u32_e32" | "v_cmpx_ne_u32_e32" => {
            compare_into_exec(model, instruction, name)
        }
        // The rest of the family, its ordering read off its name as a compare into `vcc`'s is.
        _ if name.starts_with("v_cmpx_")
            && name.ends_with("_e32")
            && compare_op(&name.replacen("v_cmpx_", "v_cmp_", 1)).is_some() =>
        {
            compare_into_exec(model, instruction, name)
        }

        // The short-form select, whose mask is always the condition mask.
        "v_cndmask_b32_e32" => short_form_select(model, instruction),

        // Integer conversions.
        _ if INTEGER_CONVERSIONS.contains(&name) => integer_conversion(model, instruction, name),

        // A shift then an or, as radeonsi packs a primitive's vertex indices.
        "v_lshl_or_b32" => shift_or(model, instruction),
        _ if MULTIPLY_ADDS.contains(&name) => multiply_add_family(model, instruction, name),
        "v_bfi_b32" => bitfield_insert(model, instruction),
        _ if LONG_FORMS.contains(&name) => integer_long_form(model, instruction, name),
        _ if FLOATS_OF_THREE.contains(&name) => float_min_max_of_three(model, instruction, name),
        "v_med3_u32" => unsigned_median_of_three(model, instruction),
        _ if CUBE_FACES.contains(&name) => cube_face(model, instruction, name),
        "v_readlane_b32" => read_lane(model, instruction),
        "v_readfirstlane_b32" => read_first_lane(model, instruction),
        "v_permlane16_b32" => permute_lanes_16(model, instruction),
        "v_perm_b32" => byte_permute(model, instruction),

        // Sixteen-bit arithmetic on register halves, as ACO forms an image copy's coordinates.
        "v_add_nc_u16" | "v_lshlrev_b16" => half_arithmetic(model, instruction, name),
        "v_pack_b32_f16" => pack_half_words(model, instruction),
        "v_pk_mad_u16" => packed_multiply_add(model, instruction),
        "v_pk_lshlrev_b16" => packed_shift_left(model, instruction),
        "v_pk_sub_u16" | "v_pk_add_u16" | "v_pk_sub_i16" | "v_pk_add_i16" => {
            packed_add_or_subtract(model, instruction, name)
        }
        name if is_sixteen_bit(name) => sixteen_bit(model, instruction, name),
        "v_min3_i16" => half_minimum_of_three(model, instruction),

        // A lane learns its own index by counting the mask bits below itself; no instruction hands
        // it over.
        "v_mbcnt_lo_u32_b32" | "v_mbcnt_hi_u32_b32" => mask_bit_count(model, instruction, name),

        // The long-form vector ALU: the short forms' arithmetic plus per-source negate and absolute
        // flags, in bits neither the operand layout nor the encoding table describes; read
        // separately and refused where not implemented ([`LONG_FORM`]).
        _ if LONG_FORM.contains(&name) => long_form_arithmetic(model, instruction, name),

        // The division pre-scale, and the carry-producing arithmetic that writes a per-lane carry
        // mask as a second destination; 64-bit address arithmetic is built from these.
        "v_div_scale_f32" => division_scale(model, instruction),

        // Integer shifts in the long form: a shift-then-add, and a 64-bit shift of a register pair.
        "v_lshl_add_u32" | "v_add3_u32" | "v_add_lshl_u32" => shift_add(model, instruction, name),
        "v_lshrrev_b64" => shift_right_64(model, instruction),
        "v_lshlrev_b64" => shift_left_64(model, instruction),
        "v_add_co_u32" | "v_sub_co_u32" | "v_add_co_ci_u32_e64" | "v_add_co_ci_u32_e32" => {
            carry_arithmetic(model, instruction, name)
        }

        "v_rcp_f32_e32" | "v_rcp_iflag_f32_e32" => reciprocal(model, instruction),

        "v_mov_b32_e32" => vector_move(model, instruction),
        "v_movrels_b32_e32" => relative_move(model, instruction),
        "v_movreld_b32_e32" => relative_write(model, instruction),
        "v_permlanex16_b32" => permute_across_rows(model, instruction),
        "v_writelane_b32" => write_lane(model, instruction),
        _ if HALF_FLOATS.contains(&name) => half_float_arithmetic(model, instruction, name),

        // The short-form vector ALU: integer address arithmetic and float arithmetic.
        "v_add_f32_e32"
        | "v_sub_f32_e32"
        | "v_subrev_f32_e32"
        | "v_mul_f32_e32"
        | "v_lshlrev_b32_e32"
        | "v_lshrrev_b32_e32"
        | "v_ashrrev_i32_e32"
        | "v_add_nc_u32_e32"
        | "v_sub_nc_u32_e32"
        | "v_subrev_nc_u32_e32"
        | "v_fmac_f32_e32"
        | "v_or_b32_e32"
        | "v_xor_b32_e32"
        | "v_and_b32_e32"
        | "v_mac_f32_e32"
        | "v_mul_u32_u24_e32"
        | "v_min_u32_e32"
        | "v_max_u32_e32"
        | "v_min_i32_e32"
        | "v_max_i32_e32" => short_form_arithmetic(model, instruction, name),

        // Float minimum and maximum, emitted as `GLSL.std.450` FMax/FMin because the core opcode
        // set has none.
        "v_max_f32_e32" | "v_min_f32_e32" => float_min_max(model, instruction, name),

        // Two floats packed into one register as halves, which is how a pixel shader prepares a
        // compressed export for an 8_8_8_8 target.
        "v_cvt_pkrtz_f16_f32_e32" | "v_cvt_pkrtz_f16_f32_e64" => {
            pack_halves(model, instruction, name)
        }

        // Unary vector float ALU and transcendentals: square root, reciprocal square root, sin,
        // cos, base-2 exp and base-2 log.
        "v_sqrt_f32_e32" | "v_rsq_f32_e32" | "v_sin_f32_e32" | "v_cos_f32_e32"
        | "v_exp_f32_e32" | "v_log_f32_e32" | "v_rndne_f32_e32" | "v_floor_f32_e32"
        | "v_ceil_f32_e32" | "v_fract_f32_e32" | "v_trunc_f32_e32" => {
            float_unary(model, instruction, name)
        }
        "v_bfrev_b32_e32" => bit_reverse(model, instruction),
        "v_not_b32_e32" | "v_not_b32_e64" => bitwise_not(model, instruction, name),
        "v_ffbh_u32_e32" => leading_zeros(model, instruction),
        "v_ffbl_b32_e32" => lowest_set_bit(model, instruction),
        _ if compare_64_parts(name).is_some() => compare_64(model, instruction, name),

        _ => memory_or_dropped_store(model, instruction, name),
    }
}

/// [`memory_instruction`], except a legacy vertex shader's draw, which leaves its streamout stores
/// to the streamout pass (D861, D881).
fn memory_or_dropped_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    if model.drops_streamout_stores()
        && (name.starts_with("buffer_store") || name.starts_with("tbuffer_store"))
    {
        model.count();
        return Ok(());
    }
    // A wave's scratch is its own private memory, not guest memory (D954).
    if crate::scratch::access(model, instruction, name)? {
        return Ok(());
    }
    // A device-addressed dispatch reaches what no bound buffer serves by device address (D964).
    if device::access(model, instruction, name)? {
        return Ok(());
    }
    // A lane's own word of the local share, at a place its lane decides; one naming the global
    // share is refused with the other global-share accesses.
    if matches!(name, "ds_write_addtid_b32" | "ds_read_addtid_b32")
        && instruction.word & DS_GDS == 0
    {
        local_share_addtid(model, instruction, name)?;
        model.after_local_access();
        return Ok(());
    }
    memory_instruction(model, instruction, name)
}

/// Translates the memory and image instructions, refusing any other.
fn memory_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // Only the untyped buffer accesses and the local data share are checked against an exact
    // module's window so far; any other access to guest memory is refused there rather than left
    // to fold into it.
    // An image descriptor a dispatch loads from its table names the image the host binds from that
    // table, by the load reaching the image instruction. Where the tracer bound the table's words
    // as a buffer, the load reads them through it too, so the registers hold the guest's words
    // whatever the program reads them as, on every path (D970). A load with no buffer reads no
    // memory here; its registers are then only an image's name.
    if matches!(name, "s_load_dwordx8" | "s_load_dwordx16")
        && model.draw_buffer_slot(instruction).is_none()
        && let Some(Operand::Scalar(first)) = instruction.operands.first()
        && model.load_descriptor_opaquely(u32::from(*first), access_words(name))
    {
        model.count();
        return Ok(());
    }
    // An image a dispatch fetches from or stores to is bound whole and written back exactly, so
    // its accesses need no window; the local data share is not guest memory; and a scalar load
    // the buffer tracer gave a slot - through a descriptor, or from a traced base such as a
    // streamout pass's internal table (D861) - reads the buffer bound there (D746), not the window.
    // A ray intersection checks its node against the window itself.
    if model.exact_memory()
        && !name.starts_with("buffer_")
        && !name.starts_with("ds_")
        && !bvh::is_intersect_ray(name)
        && !crate::draw_buffers::bound_image_access(name)
        && (!traced_load(name) || model.draw_buffer_slot(instruction).is_none())
    {
        if let Some((at, store)) = model.untraced_by() {
            return Err(TranslateError::Untraced {
                offset: instruction.offset,
                mnemonic: name.to_owned(),
                at: *at,
                store: store.clone(),
            });
        }
        return Err(TranslateError::NotTranslated {
            offset: instruction.offset,
            mnemonic: name.to_owned(),
            detail: concat!(
                "a guest dispatch writes guest memory for real, and this access is not yet ",
                "checked against its window, so where it lands could not be made exact"
            ),
        });
    }
    match name {
        // Anything that reaches guest memory.
        "s_load_dword"
        | "s_load_dwordx2"
        | "s_load_dwordx4"
        | "s_load_dwordx8"
        | "s_load_dwordx16"
        | "s_buffer_load_dword"
        | "s_buffer_load_dwordx2"
        | "s_buffer_load_dwordx4"
        | "s_buffer_load_dwordx8"
        | "s_buffer_load_dwordx16"
        | "global_load_dword"
        | "global_load_dwordx2"
        | "global_load_dwordx3"
        | "global_load_dwordx4"
        | "global_load_ubyte"
        | "global_load_sbyte"
        | "global_load_ushort"
        | "global_load_sshort" => memory(model, instruction, name),
        // The global stores, words and parts of one (D977), which `flat_memory` names.
        name if name.starts_with("global_store_") => memory(model, instruction, name),

        // Untyped buffer access at any width: `buffer_memory` reads the width from the name and
        // refuses one it does not know. `tbuffer_` does not start with `buffer_`, so the families
        // stay distinct. The local data share follows.
        name if name.starts_with("buffer_load_format_") => {
            format_buffer_load(model, instruction, name)
        }
        name if name.starts_with("buffer_store_format_") => {
            format_buffer_store(model, instruction, name)
        }
        // Only those `SUPPORTED` names reach here.
        name if name.starts_with("buffer_atomic_") => buffer_atomic(model, instruction, name),
        name if name.starts_with("buffer_") => buffer_memory(model, instruction, name),
        // The sixteen-bit forms pack two channels a register, which is not modelled.
        name if name.starts_with("tbuffer_") && !name.contains("_d16") => {
            typed_buffer_memory(model, instruction, name)
        }
        // The GDS bit (17 on GFX10, `aco_assembler.cpp:513`) points the access at the global data
        // share, shared by every wave and kept between dispatches; carried out on the local share it
        // would vanish with the workgroup.
        "ds_write_b32" | "ds_read_b32" if instruction.word & DS_GDS != 0 => {
            local_share(model, instruction, name)
        }
        name if name.starts_with("ds_") && instruction.word & DS_GDS != 0 => {
            Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a global data share (GDS) access is not modelled; carried out on the ",
                    "local share it would be lost when the workgroup ends"
                ),
            })
        }
        "ds_write_b32" | "ds_read_b32" | "ds_read_b96" | "ds_write2_b32" | "ds_read2_b32"
        | "ds_write_b8" | "ds_read_u8" | "ds_or_b32" | "ds_write_b96" | "ds_write_b128"
        | "ds_min_u32" | "ds_max_u32" | "ds_add_rtn_u32" | "ds_add_u32" | "ds_min_f32"
        | "ds_max_f32" | "ds_write_b64" | "ds_read_b64" | "ds_read_b128" | "ds_write2_b64"
        | "ds_read2_b64" => {
            local_share_instruction(model, instruction, name)?;
            model.after_local_access();
            Ok(())
        }
        // A lane permute through the local share's crossbar, which touches no local memory.
        "ds_bpermute_b32" => backward_permute(model, instruction),

        // A texture sample or fetch; the descriptor rules are D690's.
        "image_sample_lz" | "image_sample_lz_o" | "image_sample_c_lz" | "image_sample_c"
        | "image_sample" | "image_sample_l" | "image_sample_d" | "image_load"
        | "image_load_mip" | "image_gather4_lz" => image_sample(model, instruction, name),
        // A store, the one image instruction that needs its own binding, since nothing writes a
        // sampled image (D692).
        "image_store" | "image_store_mip" => image_store(model, instruction, name),
        // A level's size, asked of the image a fetch would read.
        "image_get_resinfo" => image_size(model, instruction, name),
        // A ray against one node of a bounding-volume hierarchy, read through the window.
        name if bvh::is_intersect_ray(name) => bvh::intersect_ray(model, instruction, name),

        _ => Err(TranslateError::NotTranslated {
            offset: instruction.offset,
            mnemonic: name.to_owned(),
            detail: NO_TRANSLATION,
        }),
    }
}

/// Translates `v_mbcnt_lo_u32_b32` / `v_mbcnt_hi_u32_b32`.
///
/// Counts the set bits of the mask strictly below this lane and adds the second source; `lo` looks
/// at bits 0-31 and `hi` at 32-63. Run as a pair (`lo` with an all-ones mask, then `hi` with the
/// first result as addend) it yields the lane index, the only way a shader learns which lane it is.
/// Including the lane's own bit would shift every active lane's index by one.
fn mask_bit_count<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let high_half = name == MBCNT_HI;
    let (destination, mask, addend) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_mbcnt destination is not a vector register",
        });
    };

    for lane in running_lanes(model) {
        let mask_value = model.read_source(instruction, mask, lane)?;
        let base = model.read_source(instruction, addend, lane)?;

        // Which bits of this half sit below this lane: all of it for a lane in the other half, only
        // what precedes it for a lane in this half.
        let below = match (high_half, lane < 32) {
            // `lo` for a lane in the high half, or `hi` for a lane in the low half: nothing or
            // everything.
            (false, false) => u32::MAX,
            (true, true) => 0,
            _ => {
                let within = lane % 32;
                // Shifting a 32-bit value by 32 is undefined, and lane 0 has nothing below it, so
                // the identity is written out.
                if within == 0 {
                    0
                } else {
                    u32::MAX >> (32 - within)
                }
            }
        };

        // One invocation per lane (D760): the bits below this lane, at run time.
        let below = match model.lane_number(lane) {
            Some(index) => lanes_below(model, index, high_half),
            None => model.constant(below),
        };
        let selected = model.binary(op::BITWISE_AND, mask_value, below);
        let counted = model.bit_count(selected);
        let total = model.add(base, counted);
        model.write_vector_lane(u32::from(*register), lane, total);
    }
    model.count();
    Ok(())
}

/// `v_mbcnt_hi_u32_b32`, which looks at the mask's high half.
const MBCNT_HI: &str = "v_mbcnt_hi_u32_b32";

/// Translates the short-form vector ALU instructions. The short encoding has no source modifiers.
fn short_form_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a short-form vector destination is not a vector register",
        });
    };
    let register = u32::from(*register);

    for lane in running_lanes(model) {
        let lhs = model.read_source(instruction, first, lane)?;
        let rhs = model.read_source(instruction, second, lane)?;
        let value = match name {
            // Integer: address arithmetic, and shifts whose amount comes first; read in written
            // order, `v_lshlrev` would compute `2 << index` instead of `index << 2`.
            "v_add_nc_u32_e32" => model.binary(op::IADD, lhs, rhs),
            // The first source less the second, wrapping.
            "v_sub_nc_u32_e32" => model.binary(op::ISUB, lhs, rhs),
            // The second source less the first, wrapping: `D = S1 - S0`, as the name's `rev` says.
            "v_subrev_nc_u32_e32" => model.binary(op::ISUB, rhs, lhs),
            "v_or_b32_e32" => model.binary(op::BITWISE_OR, lhs, rhs),
            "v_xor_b32_e32" => model.binary(op::BITWISE_XOR, lhs, rhs),
            "v_and_b32_e32" => model.binary(op::BITWISE_AND, lhs, rhs),
            "v_lshlrev_b32_e32" => model.binary(op::SHIFT_LEFT_LOGICAL, rhs, lhs),
            // Logical, not arithmetic: the guest has a separate `v_ashrrev_i32` for the
            // sign-propagating shift.
            "v_lshrrev_b32_e32" => model.binary(op::SHIFT_RIGHT_LOGICAL, rhs, lhs),
            // The sign-propagating shift, by the low five bits of the first source as the RDNA
            // instruction set gives `D = S1 >> S0[4:0]`.
            "v_ashrrev_i32_e32" => {
                let low_five = model.constant(31);
                let amount = model.binary(op::BITWISE_AND, lhs, low_five);
                model.binary(op::SHIFT_RIGHT_ARITHMETIC, rhs, amount)
            }
            // Float. `v_subrev_f32` reverses its operands, as its name says and its encoding does
            // not. `v_fmac_f32` accumulates into its destination, read through the ordinary source
            // path because the destination names a vector register, and is fused
            // ([`Model::f32_fma`]).
            "v_fmac_f32_e32" => {
                let previous = model.read_source(instruction, destination, lane)?;
                model.f32_fma(lhs, rhs, previous)
            }
            // The legacy multiply-add, accumulating into its destination.
            "v_mac_f32_e32" => {
                let previous = model.read_source(instruction, destination, lane)?;
                legacy_multiply_add(model, lhs, rhs, previous)
            }
            // The unsigned and signed minimum and maximum.
            "v_min_u32_e32" | "v_max_u32_e32" | "v_min_i32_e32" | "v_max_i32_e32" => {
                let signed = name.contains("_i32");
                let compare = if signed {
                    op::SLESS_THAN
                } else {
                    op::ULESS_THAN
                };
                let less = model.compare(compare, lhs, rhs);
                let (smaller, larger) = (lhs, rhs);
                if name.starts_with("v_min") {
                    model.select(less, smaller, larger)
                } else {
                    model.select(less, larger, smaller)
                }
            }
            // The low 32 bits of the product of each source's low 24 bits.
            "v_mul_u32_u24_e32" => {
                let low_24 = model.constant(0x00FF_FFFF);
                let lhs = model.binary(op::BITWISE_AND, lhs, low_24);
                let rhs = model.binary(op::BITWISE_AND, rhs, low_24);
                model.binary(op::IMUL, lhs, rhs)
            }
            "v_add_f32_e32" | "v_sub_f32_e32" | "v_subrev_f32_e32" | "v_mul_f32_e32" => {
                let reversed = name == "v_subrev_f32_e32";
                let (lhs, rhs) = if reversed { (rhs, lhs) } else { (lhs, rhs) };
                let operation = match name {
                    "v_add_f32_e32" => op::FADD,
                    "v_sub_f32_e32" | "v_subrev_f32_e32" => op::FSUB,
                    _ => op::FMUL,
                };
                model.f32_binary(operation, lhs, rhs)
            }
            _ => {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "no translation for this short-form vector instruction",
                });
            }
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The `GLSL.std.450` instruction a one-source float instruction is, and whether its source is an
/// angle in revolutions. `v_sin_f32` and `v_cos_f32` compute the sine or cosine of `2*pi * x`;
/// `GLSL.std.450` Sin and Cos take radians, so the argument is scaled by 2*pi first.
fn unary_float_extended(name: &str) -> Option<(u32, bool)> {
    Some(match name {
        "v_sqrt_f32_e32" => (GLSL_SQRT, false),
        "v_rsq_f32_e32" => (GLSL_INVERSE_SQRT, false),
        "v_sin_f32_e32" => (GLSL_SIN, true),
        "v_cos_f32_e32" => (GLSL_COS, true),
        "v_exp_f32_e32" => (GLSL_EXP2, false),
        "v_log_f32_e32" => (GLSL_LOG2, false),
        // Round to the nearest integer, a tie to the even one (`V_RNDNE_F32`).
        "v_rndne_f32_e32" => (GLSL_ROUND_EVEN, false),
        // Round toward negative infinity (`V_FLOOR_F32`); `V_FRACT_F32` subtracts that from its
        // source.
        "v_floor_f32_e32" | "v_fract_f32_e32" => (GLSL_FLOOR, false),
        // Round toward zero (`V_TRUNC_F32`).
        "v_trunc_f32_e32" => (GLSL_TRUNC, false),
        // Round toward positive infinity (`V_CEIL_F32`).
        "v_ceil_f32_e32" => (GLSL_CEIL, false),
        _ => return None,
    })
}

/// GLSL.std.450 instruction number for `Trunc`: the nearest integer no larger in magnitude.
const GLSL_TRUNC: u32 = 3;

/// GLSL.std.450 instruction number for `RoundEven`: to the nearest integer, a tie to the even one.
const GLSL_ROUND_EVEN: u32 = 2;

/// GLSL.std.450 instruction number for `Floor`: the largest integer not above the operand.
const GLSL_FLOOR: u32 = 8;

/// GLSL.std.450 instruction number for `Ceil`: the smallest integer not below the operand.
const GLSL_CEIL: u32 = 9;

/// `v_bfrev_b32 d, s`: `s`'s bits reversed, per lane (`V_BFREV_B32`).
fn bit_reverse<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_bfrev_b32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        let u32_type = model.u32_type();
        let b = model.builder();
        let reversed = b.id();
        b.function(op::BIT_REVERSE, &[u32_type.0, reversed.0, value.0]);
        model.write_vector_lane(register, lane, reversed);
    }
    model.count();
    Ok(())
}

/// `v_rcp_f32`: a reciprocal, per lane. `v_rcp_iflag_f32` is the same reciprocal that also raises
/// the integer divide-by-zero flag (AMD's published RDNA instruction set), which nothing here reads.
///
/// The guest's is an approximation accurate to roughly one part in a million; this emits an exact
/// division. Bit-exact framebuffer comparisons can differ in the last bit.
fn reciprocal<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_rcp_f32 destination is not a vector register",
        });
    };
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        let quotient = reciprocal_f32(model, value);
        model.write_vector_lane(u32::from(*register), lane, quotient);
    }
    model.count();
    Ok(())
}

/// `1 / value` over every float, for `v_rcp_f32`'s forms.
///
/// SPIR-V's `OpFDiv` is only accurate for divisors between 2^-126 and 2^126 (the Vulkan
/// specification's precision table); outside that range a driver may answer anything, and the two
/// wavefront models were seen to get different answers for one subnormal divisor. A divisor
/// outside the range is scaled into it by 2^64 or 2^-64, which is exact, and the quotient scaled
/// back. A divisor that is zero once scaled - zero, or a subnormal the float mode flushes - gives
/// an infinity of its sign, and an infinite one a zero of its sign, as the hardware's reciprocal.
pub(crate) fn reciprocal_f32<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let magnitude_mask = model.constant(0x7fff_ffff);
    let sign_mask = model.constant(0x8000_0000);
    let magnitude = model.binary(op::BITWISE_AND, value, magnitude_mask);
    let sign = model.binary(op::BITWISE_AND, value, sign_mask);
    // Subnormal: a magnitude of 1 to 0x007f_ffff. Large: above 2^126 and finite. Each is one
    // unsigned compare of the magnitude less the range's start.
    let one_bit = model.constant(1);
    let past_zero = model.binary(op::ISUB, magnitude, one_bit);
    let subnormal_span = model.constant(0x007f_ffff);
    let subnormal = model.compare(op::ULESS_THAN, past_zero, subnormal_span);
    let large_start = model.constant(0x7e80_0001);
    let past_large = model.binary(op::ISUB, magnitude, large_start);
    let large_span = model.constant(0x7f80_0000 - 0x7e80_0001);
    let large = model.compare(op::ULESS_THAN, past_large, large_span);
    let one = model.constant(ONE_F32);
    let up = model.constant(0x5f80_0000);
    let down = model.constant(0x1f80_0000);
    let scale = model.select(large, down, one);
    let scale = model.select(subnormal, up, scale);
    let scaled = model.f32_binary_uncontracted(op::FMUL, value, scale);
    let quotient = model.f32_binary_uncontracted(op::FDIV, one, scaled);
    let quotient = model.f32_binary_uncontracted(op::FMUL, quotient, scale);
    let infinity = model.constant(0x7f80_0000);
    let zero = model.constant(0);
    let scaled_magnitude = model.binary(op::BITWISE_AND, scaled, magnitude_mask);
    let zero_divisor = model.compare(op::IEQUAL, scaled_magnitude, zero);
    let infinite_divisor = model.compare(op::IEQUAL, magnitude, infinity);
    let signed_infinity = model.binary(op::BITWISE_OR, sign, infinity);
    let quotient = model.select(infinite_divisor, sign, quotient);
    model.select(zero_divisor, signed_infinity, quotient)
}

/// `v_not_b32 d, s`: `~s`, per lane (`V_NOT_B32`, AMD's published RDNA 2 instruction set, VOP1), in
/// its short form and its long one. The long form's negate and absolute value are float modifiers,
/// which an integer operation does not take, and are refused.
fn bitwise_not<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    if name.ends_with("_e64") {
        no_integer_modifiers(instruction, 1)?;
    }
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_not_b32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        let inverted = model.not(value);
        model.write_vector_lane(register, lane, inverted);
    }
    model.count();
    Ok(())
}

/// `v_ffbh_u32 d, s`: the count of `s`'s leading zero bits, per lane, and `0xffffffff` where `s` is
/// zero (`V_FFBH_U32`, AMD's published RDNA instruction set). Counted by halving: where the top
/// `n` bits are clear, `n` is added and the value shifted up by it, for `n` of 16, 8, 4, 2 and 1.
fn leading_zeros<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_ffbh_u32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        let zero = model.constant(0);
        let mut count = zero;
        let mut rest = value;
        for step in [16, 8, 4, 2, 1] {
            let away = model.constant(32 - step);
            let top = model.binary(op::SHIFT_RIGHT_LOGICAL, rest, away);
            let clear = model.compare(op::IEQUAL, top, zero);
            let by = model.constant(step);
            let more = model.add(count, by);
            count = model.select(clear, more, count);
            let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, rest, by);
            rest = model.select(clear, shifted, rest);
        }
        let none = model.compare(op::IEQUAL, value, zero);
        let all = model.constant(u32::MAX);
        let counted = model.select(none, all, count);
        model.write_vector_lane(register, lane, counted);
    }
    model.count();
    Ok(())
}

/// `v_ffbl_b32 d, s`: the index of `s`'s lowest set bit, per lane, and `0xffffffff` where `s` is
/// zero (`V_FFBL_B32`, AMD's published RDNA 2 instruction set), as [`trailing_zeros`] counts it.
fn lowest_set_bit<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_ffbl_b32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        let count = trailing_zeros(model, value);
        let zero = model.constant(0);
        let none = model.compare(op::IEQUAL, value, zero);
        let all = model.constant(u32::MAX);
        let index = model.select(none, all, count);
        model.write_vector_lane(register, lane, index);
    }
    model.count();
    Ok(())
}

/// How many registers past its source a relative move reads (D943): `m0` from zero to one less,
/// each a register the move selects among. Grand Theft Auto V's cascade lookup reads `m0` of 0, 4,
/// 8 or 12 past `v0` to `v2`.
const RELATIVE_REGISTERS: u32 = 32;

/// `v_movrels_b32 d, s`: vector register `s + m0` into `d`, per lane (`V_MOVRELS_B32`, AMD's
/// published RDNA instruction set), as a selection among the [`RELATIVE_REGISTERS`] from `s`, which
/// every model can read (D943). An `m0` past them reads `s` itself.
fn relative_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let (Operand::Vector(register), Operand::Vector(first)) = (destination, source) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_movrels_b32 names other than vector registers",
        });
    };
    let (register, first) = (u32::from(*register), u32::from(*first));
    let last = (first + RELATIVE_REGISTERS).min(256);
    let index = model.read_m0();
    for lane in running_lanes(model) {
        let mut value = model.read_source(instruction, source, lane)?;
        for (offset, at) in (1..).zip(first + 1..last) {
            let candidate = Operand::Vector(u16::try_from(at).unwrap_or(u16::MAX));
            let read = model.read_source(instruction, &candidate, lane)?;
            let wanted = model.constant(offset);
            let here = model.compare(op::IEQUAL, index, wanted);
            value = model.select(here, read, value);
        }
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// `v_movreld_b32 d, s`: `s` into vector register `d + m0`, per lane (`V_MOVRELD_B32`, AMD's
/// published RDNA instruction set), as a write of each of the [`RELATIVE_REGISTERS`] from `d` that
/// keeps its value except at the one `m0` names (D943).
fn relative_write<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(first) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_movreld_b32 names other than a vector destination",
        });
    };
    let first = u32::from(*first);
    let last = (first + RELATIVE_REGISTERS).min(256);
    let index = model.read_m0();
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        for (offset, at) in (0..).zip(first..last) {
            let held = Operand::Vector(u16::try_from(at).unwrap_or(u16::MAX));
            let old = model.read_source(instruction, &held, lane)?;
            let wanted = model.constant(offset);
            let here = model.compare(op::IEQUAL, index, wanted);
            let written = model.select(here, value, old);
            model.write_vector_lane(at, lane, written);
        }
    }
    model.count();
    Ok(())
}

/// `v_permlanex16_b32 d, s, sel_lo, sel_hi`: each lane takes `s` from the lane of the other row of
/// sixteen in its half of the wave that four bits of the selects name - lane `i` of a row the bits
/// at `4 * i` of `sel_lo` for `i` below eight, of `sel_hi` above (`V_PERMLANEX16_B32`, AMD's
/// published RDNA instruction set). Every lane's source is read as it stands, as with fetch-inactive
/// set (assumed).
fn permute_across_rows<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let [
        Operand::Vector(destination),
        Operand::Vector(source),
        select_low,
        select_high,
    ] = instruction.operands.as_slice()
    else {
        return Err(refuse(
            "v_permlanex16_b32 does not name a vector destination, a vector source and two selects",
        ));
    };
    let (destination, source) = (u32::from(*destination), u32::from(*source));
    let low = model.read_source(instruction, select_low, 0)?;
    let high = model.read_source(instruction, select_high, 0)?;
    let nibble = model.constant(0xf);
    for lane in running_lanes(model) {
        let (word, shift, row) = if let Some(index) = model.lane_number(lane) {
            // An invocation's lane known at run time (D760, D973): the same fields of it.
            let (fifteen, seven, eight, four) = (
                model.constant(15),
                model.constant(7),
                model.constant(8),
                model.constant(4),
            );
            let within = model.binary(op::BITWISE_AND, index, fifteen);
            let upper = model.compare(op::UGREATER_THAN_EQUAL, within, eight);
            let word = model.select(upper, high, low);
            let place = model.binary(op::BITWISE_AND, within, seven);
            let shift = model.binary(op::IMUL, place, four);
            // The other row of sixteen in this half: this lane's with bit four flipped, less its
            // place in the row.
            let sixteen = model.constant(16);
            let flipped = model.binary(op::BITWISE_XOR, index, sixteen);
            let row = model.binary(op::ISUB, flipped, within);
            (word, shift, row)
        } else {
            let within = lane % 16;
            let (word, shift) = if within < 8 {
                (low, within * 4)
            } else {
                (high, (within - 8) * 4)
            };
            let other_row = (lane & !31) + (((lane >> 4) & 1) ^ 1) * 16;
            (word, model.constant(shift), model.constant(other_row))
        };
        let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
        let chosen = model.binary(op::BITWISE_AND, shifted, nibble);
        let from = model.add(row, chosen);
        let value = model.read_vector_at(source, from).ok_or_else(|| {
            refuse("a permutation across rows in a model that holds one lane alone")
        })?;
        model.write_vector_lane(destination, lane, value);
    }
    model.count();
    Ok(())
}

/// `s_bitset1_b32 d, s`: bit `s & 31` of the scalar register `d` set, the others kept
/// (`S_BITSET1_B32`, AMD's published RDNA instruction set); the condition code is not written.
fn scalar_bit_set<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_bitset1_b32 destination is not a scalar register",
        });
    };
    let register = u32::from(*register);
    let bit = model.read_source(instruction, source, 0)?;
    let five = model.constant(31);
    let at = model.binary(op::BITWISE_AND, bit, five);
    let one = model.constant(1);
    let mask = model.binary(op::SHIFT_LEFT_LOGICAL, one, at);
    let held = model.read_scalar(register);
    let set = model.binary(op::BITWISE_OR, held, mask);
    model.write_scalar(register, set);
    model.count();
    Ok(())
}

/// `s_getpc_b64`: the address of the instruction after it, into a register pair - the program's own
/// place, which ACO forms the address of its constant data from (D842).
fn program_counter<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let Some(Operand::Scalar(register)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_getpc_b64 destination is not a scalar register pair",
        });
    };
    let register = u32::from(*register);
    let Some(base) = model.program_address() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "s_getpc_b64 reads the program's own address, and this translation was not told ",
                "where the program was loaded (D842)"
            ),
        });
    };
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_getpc_b64 runs past the end of the register file",
        });
    }
    let next = base + u64::from(instruction.offset) + 4;
    let low = model.constant(next as u32);
    let high = model.constant((next >> 32) as u32);
    model.write_scalar(register, low);
    model.write_scalar(register + 1, high);
    model.count();
    Ok(())
}

/// `S_ADDC_U32`: the sum of `left`, `right` and the condition code's carry in, and the carry out of
/// either add, which becomes the condition code.
fn scalar_add_with_carry<M: Model + ?Sized>(
    model: &mut M,
    left: Id,
    right: Id,
) -> (Id, Option<Id>) {
    let carry_in = crate::control::read_condition_code(model, true);
    let one = model.constant(1);
    let zero = model.constant(0);
    let carry_in = model.select(carry_in, one, zero);
    let sum = model.add(left, right);
    let result = model.add(sum, carry_in);
    let first = model.compare(op::ULESS_THAN, sum, left);
    let second = model.compare(op::ULESS_THAN, result, sum);
    (result, Some(model.either(first, second)))
}

/// `S_SUBB_U32`: `left` less `right` less the condition code's borrow in, and the borrow out of
/// either subtraction, which becomes the condition code (AMD's published RDNA 2 instruction set,
/// SOP2): the mirror of [`scalar_add_with_carry`].
fn scalar_subtract_with_borrow<M: Model + ?Sized>(
    model: &mut M,
    left: Id,
    right: Id,
) -> (Id, Option<Id>) {
    let borrow_in = crate::control::read_condition_code(model, true);
    let one = model.constant(1);
    let zero = model.constant(0);
    let borrow_in = model.select(borrow_in, one, zero);
    let difference = model.binary(op::ISUB, left, right);
    let result = model.binary(op::ISUB, difference, borrow_in);
    let first = model.compare(op::ULESS_THAN, left, right);
    let second = model.compare(op::ULESS_THAN, difference, borrow_in);
    (result, Some(model.either(first, second)))
}

/// `s_brev_b32`: a scalar's bits in reverse order, into a scalar register or a lane mask's half used
/// as one (`s_brev_b32 vcc_hi, -3`). The condition code is untouched.
/// `s_not_b32 d, s`: the bitwise complement, and the condition code whether it is non-zero
/// (`S_NOT_B32`); the destination any scalar word, a lane mask's half among them.
fn scalar_not_32<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let value = model.read_source(instruction, source, 0)?;
    let inverted = model.not(value);
    write_scalar_operand(model, instruction, destination, inverted)?;
    let non_zero = model.is_not_zero(inverted);
    model.set_condition_code(non_zero);
    model.count();
    Ok(())
}

fn scalar_bit_reverse<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let value = model.read_source(instruction, source, 0)?;
    let u32_type = model.u32_type();
    let b = model.builder();
    let reversed = b.id();
    b.function(op::BIT_REVERSE, &[u32_type.0, reversed.0, value.0]);
    if let Operand::Scalar(register) = destination {
        model.write_scalar(u32::from(*register), reversed);
    } else if let Some(mask) = mask_destination(destination) {
        write_mask_low(model, mask, reversed)?;
    } else if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_high_name(name)
    {
        write_mask_high(model, mask, reversed)?;
    } else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_brev_b32 destination is neither a scalar register nor a lane mask's half",
        });
    }
    model.count();
    Ok(())
}

/// The high thirty-two bits of the unsigned product of `a` and `b`, from four sixteen-bit partial
/// products, each of which, and every sum of them taken here, fits thirty-two bits.
fn multiply_high<M: Model + ?Sized>(model: &mut M, a: Id, b: Id) -> Id {
    let sixteen = model.constant(16);
    let low_mask = model.constant(0xffff);
    let split = |model: &mut M, value| {
        (
            model.binary(op::BITWISE_AND, value, low_mask),
            model.binary(op::SHIFT_RIGHT_LOGICAL, value, sixteen),
        )
    };
    let (a_low, a_high) = split(model, a);
    let (b_low, b_high) = split(model, b);
    let low_low = model.binary(op::IMUL, a_low, b_low);
    let low_high = model.binary(op::IMUL, a_low, b_high);
    let high_low = model.binary(op::IMUL, a_high, b_low);
    let high_high = model.binary(op::IMUL, a_high, b_high);
    // The carry into bit 32: the middle column's sum above its sixteen bits.
    let carried = model.binary(op::SHIFT_RIGHT_LOGICAL, low_low, sixteen);
    let (low_high_low, low_high_high) = split(model, low_high);
    let (high_low_low, high_low_high) = split(model, high_low);
    let middle = model.add(carried, low_high_low);
    let middle = model.add(middle, high_low_low);
    let middle_carry = model.binary(op::SHIFT_RIGHT_LOGICAL, middle, sixteen);
    let high = model.add(high_high, low_high_high);
    let high = model.add(high, high_low_high);
    model.add(high, middle_carry)
}

/// GLSL.std.450 instruction number for `FMin` - the smaller of two floats.
const GLSL_FMIN: u32 = 37;
/// GLSL.std.450 instruction number for `FMax` - the larger of two floats.
const GLSL_FMAX: u32 = 40;

/// `v_max_f32` / `v_min_f32`: the per-lane float maximum and minimum, emitted as `GLSL.std.450`
/// extended instructions.
fn float_min_max<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a min/max destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let extended = if name == "v_max_f32_e32" {
        GLSL_FMAX
    } else {
        GLSL_FMIN
    };
    for lane in running_lanes(model) {
        let lhs = model.read_source(instruction, first, lane)?;
        let rhs = model.read_source(instruction, second, lane)?;
        let value = model.f32_ext_binary(extended, lhs, rhs);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// `v_cvt_pkrtz_f16_f32`: two floats, each narrowed to a half rounding toward zero, packed low then
/// high (AMD's published RDNA instruction set).
///
/// The first source lands in bits 0-15 and the second in bits 16-31, the order Mesa's ACO packs
/// `(r, g)` and `(b, a)` for an FP16 colour export (`aco_select_ps_epilog.cpp:170-178`). The
/// narrowing is done in integers ([`Model::float_bits_to_half_toward_zero`]), since `OpFConvert`
/// rounds to nearest. The long form reaches [`pack_two_halves`] through
/// [`long_form_arithmetic`], its modifiers applied first.
fn pack_halves<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    if name.ends_with("_e64") {
        return long_form_arithmetic(model, instruction, name);
    }
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a half-pack's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let low = model.read_source(instruction, first, lane)?;
        let high = model.read_source(instruction, second, lane)?;
        let packed = pack_two_halves(model, low, high);
        model.write_vector_lane(register, lane, packed);
    }
    model.count();
    Ok(())
}

/// The long-form vector ALU instructions [`long_form_arithmetic`] translates: the short forms'
/// arithmetic under per-source negate and absolute flags, and the 16-bit packs a pixel shader
/// prepares a compressed export for a 16-bit target with.
const LONG_FORM: [&str; 19] = [
    "v_cndmask_b32_e64",
    "v_add_f32_e64",
    "v_sub_f32_e64",
    "v_subrev_f32_e64",
    "v_mul_f32_e64",
    "v_mac_f32_e64",
    "v_fma_f32",
    "v_mad_f32",
    "v_div_fixup_f32",
    "v_div_fmas_f32",
    "v_rcp_f32_e64",
    "v_cvt_pknorm_u16_f32",
    "v_cvt_pknorm_i16_f32",
    "v_cvt_pk_u16_u32",
    "v_cvt_pk_i16_i32",
    "v_cvt_f32_u32_e64",
    "v_cvt_f32_i32_e64",
    "v_cvt_i32_f32_e64",
    "v_cvt_u32_f32_e64",
];

/// Where word `word` of a scalar buffer load starts from its buffer's start, and one past it: from
/// the immediate `bytes` alone, or from `first`, the immediate plus the offset register already
/// dword-aligned - then with whether the word's end wrapped past the top, which reads nothing.
fn scalar_word_place<M: Model + ?Sized>(
    model: &mut M,
    (bytes, first): (u32, Option<Id>),
    word: u32,
) -> (Id, Id, Option<Id>) {
    let Some(first) = first else {
        return (
            model.constant(bytes + word * 4),
            model.constant(bytes + word * 4 + 4),
            None,
        );
    };
    let skip = model.constant(word * 4);
    let at = model.add(first, skip);
    let four = model.constant(4);
    let end = model.add(at, four);
    let wrapped = model.compare(op::ULESS_THAN, end, first);
    (at, end, Some(wrapped))
}

/// The value of a scalar memory access's offset register, from its `SOFFSET` field `code`: `None`
/// for `null`, the register's value for a scalar register or a named one such as `vcc_lo`.
///
/// # Errors
///
/// A code naming no scalar register.
fn scalar_offset_register<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    code: u32,
) -> Result<Option<Id>, TranslateError> {
    if code == SOFFSET_NULL {
        return Ok(None);
    }
    let register = sdwa_operand_table()
        .map(|table| table.classify(code, None))
        .filter(|operand| matches!(operand, Operand::Scalar(_) | Operand::Named(_)))
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar memory access's offset field names no scalar register",
        })?;
    model.read_source(instruction, &register, 0).map(Some)
}

/// A float's bits as a normalised 16-bit value in the low half (AMD's published RDNA instruction
/// set, `V_CVT_PKNORM_U16_F32`/`_I16_F32`; Mesa's `pack_unorm_2x16` and `pack_snorm_2x16`
/// lowerings): a NaN read as zero, clamped to `[0, 1]` or, `signed`, `[-1, 1]`, scaled by 65535 or
/// 32767 and rounded to the nearest, a tie to the even.
fn normalised_16<M: Model + ?Sized>(model: &mut M, bits: Id, signed: bool) -> Id {
    let float = model.as_float(bits);
    let nan = {
        let bool_type = model.bool_type();
        let b = model.builder();
        let nan = b.id();
        b.function(op::IS_NAN, &[bool_type.0, nan.0, float.0]);
        nan
    };
    let zero = model.constant(0);
    let bits = model.select(nan, zero, bits);
    let (bottom, scale) = if signed {
        ((-1.0_f32).to_bits(), 32767.0_f32.to_bits())
    } else {
        (0, 65535.0_f32.to_bits())
    };
    let bottom = model.constant(bottom);
    let one = model.constant(ONE_F32);
    let clamped = model.f32_ext_binary(GLSL_FMAX, bits, bottom);
    let clamped = model.f32_ext_binary(GLSL_FMIN, clamped, one);
    let scale = model.constant(scale);
    let scaled = model.f32_binary(op::FMUL, clamped, scale);
    let rounded = model.f32_ext_unary(GLSL_ROUND_EVEN, scaled);
    let whole = if signed {
        float_to_signed_saturated(model, rounded)
    } else {
        float_to_unsigned_saturated(model, rounded)
    };
    let low_half = model.constant(0xFFFF);
    model.binary(op::BITWISE_AND, whole, low_half)
}

/// An integer as a 16-bit one in the low half, saturated: unsigned to `[0, 65535]`, or `signed` to
/// `[-32768, 32767]` (AMD's published RDNA instruction set, `V_CVT_PK_U16_U32`/`_I16_I32`).
fn saturated_16<M: Model + ?Sized>(model: &mut M, value: Id, signed: bool) -> Id {
    let saturated = if signed {
        let (top, bottom) = (model.constant(0x7FFF), model.constant(0xFFFF_8000));
        let over = model.compare(op::SGREATER_THAN, value, top);
        let under = model.compare(op::SLESS_THAN, value, bottom);
        let kept = model.select(over, top, value);
        model.select(under, bottom, kept)
    } else {
        let top = model.constant(0xFFFF);
        let over = model.compare(op::UGREATER_THAN, value, top);
        model.select(over, top, value)
    };
    let low_half = model.constant(0xFFFF);
    model.binary(op::BITWISE_AND, saturated, low_half)
}

/// Two 16-bit values in their words' low halves, the first in bits 15:0 and the second in 31:16.
fn pack_two_16<M: Model + ?Sized>(model: &mut M, low: Id, high: Id) -> Id {
    let sixteen = model.constant(16);
    let high = model.binary(op::SHIFT_LEFT_LOGICAL, high, sixteen);
    model.binary(op::BITWISE_OR, low, high)
}

/// Two floats' bits as halves rounded toward zero, the first in bits 15:0.
fn pack_two_halves<M: Model + ?Sized>(model: &mut M, low: Id, high: Id) -> Id {
    let low = model.float_bits_to_half_toward_zero(low);
    let high = model.float_bits_to_half_toward_zero(high);
    let sixteen = model.constant(16);
    let high = model.binary(op::SHIFT_LEFT_LOGICAL, high, sixteen);
    model.binary(op::BITWISE_OR, low, high)
}

/// GLSL.std.450 instruction number for `Sin`.
const GLSL_SIN: u32 = 13;
/// GLSL.std.450 instruction number for `Cos`.
const GLSL_COS: u32 = 14;
/// GLSL.std.450 instruction number for `Exp2` (2^x).
/// `GLSL.std.450` `FAbs`: a float's absolute value.
const GLSL_FABS: u32 = 4;

const GLSL_EXP2: u32 = 29;
/// GLSL.std.450 instruction number for `Log2` (log2(x)).
const GLSL_LOG2: u32 = 30;
/// GLSL.std.450 instruction number for `Sqrt`.
const GLSL_SQRT: u32 = 31;
/// GLSL.std.450 instruction number for `InverseSqrt` (1 / sqrt(x)).
const GLSL_INVERSE_SQRT: u32 = 32;

/// Unary vector float ALU and transcendental operations, emitted as `GLSL.std.450` extended
/// instructions through `OpExtInst`.
fn float_unary<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a unary float destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let Some((extended, revolutions)) = unary_float_extended(name) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "unknown unary float instruction",
        });
    };
    for lane in running_lanes(model) {
        let operand = model.read_source(instruction, source, lane)?;
        let argument = if revolutions {
            let turn = model.constant(TWO_PI_F32);
            model.f32_binary(op::FMUL, operand, turn)
        } else {
            operand
        };
        let value = model.f32_ext_unary(extended, argument);
        // `x - floor(x)`, as the RDNA ISA's `V_FRACT_F32` and NIR's `ffract` define it.
        let value = if name == "v_fract_f32_e32" {
            model.f32_binary(op::FSUB, argument, value)
        } else {
            value
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The bit pattern of 1.0f.
const ONE_F32: u32 = 0x3F80_0000;

/// The bit pattern of `2*pi` as an f32 (`6.2831855`): the scale from the revolutions
/// `v_sin_f32`/`v_cos_f32` read to the radians `GLSL.std.450` Sin/Cos take.
const TWO_PI_F32: u32 = 0x40C9_0FDB;

/// `v_subrev_f32_e64`, which takes its operands the other way round.
const REVERSE_SUBTRACT: &str = "v_subrev_f32_e64";

/// `v_cndmask_b32`, which picks per lane from a 64-bit mask.
const CNDMASK: &str = "v_cndmask_b32_e64";

/// Where a second, scalar destination sits when an instruction has one: bits 8 to 14 of the first
/// word, where the other sub-encoding keeps per-source absolute-value flags.
const SCALAR_DESTINATION: (u32, u32) = (0, 8);

/// Whether an instruction carries a second, scalar destination.
///
/// The long-form vector ALU has two sub-encodings: one keeps per-source absolute-value flags in
/// bits 8 to 14 of the first word, the other a scalar destination (a carry-out or a pre-scale
/// flag), and nothing in the instruction says which. Misread, `vcc` as a carry destination (code
/// 106) looks like an absolute-value flag and an integer addition loses an operand's sign. The
/// operand solver records where each opcode's operands are, and the sub-encodings differ exactly in
/// whether an operand occupies those bits, so each opcode classifies itself. An instruction whose
/// layout is unknown is refused before translation, so this is only asked about solved opcodes.
pub(crate) fn has_scalar_destination(encodings: &EncodingTable, instruction: &Instruction) -> bool {
    let (word, shift) = SCALAR_DESTINATION;
    instruction
        .encoding
        .and_then(|index| encodings.encodings().get(usize::from(index)))
        .and_then(|family| encodings.operands_for(&family.name, instruction.opcode))
        .is_some_and(|slots| {
            slots
                .iter()
                .any(|slot| slot.word == word && slot.shift == shift)
        })
}

/// Translates the carry-producing vector arithmetic.
///
/// `vdst = src0 op src1`, plus one bit per lane into the scalar destination saying whether that
/// lane carried or borrowed; 64-bit address arithmetic depends on the carry. Needs a model with
/// lanes, because the second destination is a per-lane mask.
fn carry_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    carry_arithmetic_of_parts(model, instruction, name, [(SDWA_DWORD, false); 2])
}

/// Where a carry goes.
#[derive(Clone, Copy)]
enum CarryDestination {
    /// A lane mask, whole, or only its high half - a wave of 32's whole carry, in `vcc_hi`.
    Mask(&'static str, bool),
    /// Ordinary scalar registers from this one: one word in a wave of 32, a pair in a wave of 64.
    Registers(u32),
}

/// Where a carry goes, or `None` for `null`, which drops the carry as the guest asked.
fn carry_destination<M: Model + ?Sized>(
    model: &M,
    instruction: &Instruction,
    destination: &Operand,
) -> Result<Option<CarryDestination>, TranslateError> {
    match destination {
        _ if discards(destination) => Ok(None),
        Operand::Named(name) => lane_mask_name(name)
            .map(|mask| CarryDestination::Mask(mask, false))
            .or_else(|| {
                lane_mask_high_name(name)
                    .filter(|_| model.lanes() <= 32)
                    .map(|mask| CarryDestination::Mask(mask, true))
            })
            .map(Some)
            .ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a carry destination this translator does not know",
            }),
        Operand::Scalar(register) => {
            let words = if model.lanes() > 32 { 2 } else { 1 };
            if u32::from(*register) + words > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a carry destination runs past the scalar registers",
                });
            }
            Ok(Some(CarryDestination::Registers(u32::from(*register))))
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a carry destination this translator does not know",
        }),
    }
}

/// [`carry_arithmetic`] on the part of each source `selects` names, as an SDWA form's word does
/// ([`sdwa_select`]): the whole register, or a byte or word zero- or sign-extended.
fn carry_arithmetic_of_parts<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    selects: [(u32, bool); 2],
) -> Result<(), TranslateError> {
    // No absolute-value flags in this sub-encoding; a negate is meaningless on integer arithmetic
    // and is refused.
    let modifiers = Modifiers::read(instruction, true)?;
    for source in 0..3 {
        if modifiers.touches(source) {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a source modifier on integer carry arithmetic, which is not ",
                    "translated"
                ),
            });
        }
    }

    let operands = &instruction.operands;
    let (Some(Operand::Vector(vector_destination)), Some(scalar_destination)) =
        (operands.first(), operands.get(1))
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "carry arithmetic needs a vector destination and a scalar one",
        });
    };
    let mask_name = carry_destination(model, instruction, scalar_destination)?;

    let carry_in = if is_add_with_carry_in(name) {
        Some(operands.get(4).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the carry-in form has no carry-in operand",
        })?)
    } else {
        None
    };
    let (first, second) = (
        operands.get(2).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "carry arithmetic has too few sources",
        })?,
        operands.get(3).ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "carry arithmetic has too few sources",
        })?,
    );

    let carry_in = match carry_in {
        Some(operand) => Some(lane_mask_source(model, instruction, operand)?),
        None => None,
    };

    let zero = model.constant(0);
    let mut mask = (zero, zero);
    // Every lane, not only the running ones: the carry is a mask, and its bit for an inactive lane
    // is part of the answer.
    for lane in 0..model.modelled_lanes() {
        let left = model.read_source(instruction, first, lane)?;
        let left = sdwa_select(model, instruction, left, selects[0])?;
        let right = model.read_source(instruction, second, lane)?;
        let right = sdwa_select(model, instruction, right, selects[1])?;

        let (value, carried) = match name {
            // Unsigned add carried exactly when the wrapped sum is below an operand, which is exact
            // at the wrap point.
            "v_add_co_u32" => {
                let sum = model.binary(op::IADD, left, right);
                let carried = model.compare(op::ULESS_THAN, sum, left);
                (sum, carried)
            }
            // Unsigned subtract borrows exactly when the left side was smaller.
            "v_sub_co_u32" => {
                let difference = model.binary(op::ISUB, left, right);
                let borrowed = model.compare(op::ULESS_THAN, left, right);
                (difference, borrowed)
            }
            _ if is_add_with_carry_in(name) => {
                let carry_in = carry_in.ok_or(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "the carry-in form lost its carry-in",
                })?;
                add_with_carry(model, carry_in, lane, left, right)
            }
            _ => {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "no translation for this carry instruction",
                });
            }
        };

        model.write_vector_lane(u32::from(*vector_destination), lane, value);
        mask = model.set_lane_bit(mask, lane, carried);
    }

    // The carry, where the instruction asked for one. It falls out of the same arithmetic as the
    // sum, so only the write is conditional.
    match mask_name {
        Some(CarryDestination::Mask(mask_name, true)) => write_mask_high(model, mask_name, mask.0)?,
        Some(CarryDestination::Mask(mask_name, false)) => {
            write_wave_mask(model, mask_name, mask)?;
        }
        Some(CarryDestination::Registers(register)) => {
            model.write_scalar(register, mask.0);
            if model.lanes() > 32 {
                model.write_scalar(register + 1, mask.1);
            }
        }
        None => {}
    }
    model.count();
    Ok(())
}

/// One lane of `v_addc_co_u32`: the two sources plus the carry-in bit.
///
/// Two additions and two carry tests: a single test misses the case where the first addition did
/// not carry and adding the carry-in did, which happens when the sources sum to the largest value.
fn add_with_carry<M: Model + ?Sized>(
    model: &mut M,
    carry_in: (Id, Id),
    lane: u32,
    left: Id,
    right: Id,
) -> (Id, Id) {
    let (low, high) = carry_in;
    let bit = model.lane_bit(low, high, lane);
    let one = model.constant(1);
    let zero = model.constant(0);
    let u32_type = model.u32_type();

    let b = model.builder();
    let addend = b.id();
    b.function(op::SELECT, &[u32_type.0, addend.0, bit.0, one.0, zero.0]);

    let partial = model.binary(op::IADD, left, right);
    let first_carry = model.compare(op::ULESS_THAN, partial, left);
    let sum = model.binary(op::IADD, partial, addend);
    let second_carry = model.compare(op::ULESS_THAN, sum, partial);
    (sum, model.either(first_carry, second_carry))
}

/// The add that takes a carry in as well as producing one, in both its forms.
///
/// The long form names its carry registers and the short form leaves them implicit at `vcc`, which
/// the operand table records as implicit operands in the same positions, so one layout serves both.
/// A vertex program forms a 64-bit address with the short form, as in `v_add_co_ci_u32 v19, vcc,
/// s3, v1, vcc`.
fn is_add_with_carry_in(name: &str) -> bool {
    matches!(name, "v_add_co_ci_u32_e64" | "v_add_co_ci_u32_e32")
}

/// Translates the long-form vector ALU instructions.
///
/// One arm for the two-, three- and four-operand shapes: they differ only in how many sources they
/// read, and the modifiers apply the same way.
fn long_form_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read_allowing_clamp(
        instruction,
        has_scalar_destination(model.encodings(), instruction),
    )?;
    let Some(Operand::Vector(register)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a long-form vector destination is not a vector register",
        });
    };
    // A conversion from an integer has no float source for negate or absolute value to act on.
    if matches!(name, "v_cvt_f32_u32_e64" | "v_cvt_f32_i32_e64") && modifiers.touches(0) {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a source modifier on an integer source, which is a float operation",
        });
    }
    let destination = *register;
    let register = u32::from(destination);
    let mut sources: Vec<Operand> = instruction.operands[1..].to_vec();
    // The accumulating form adds into its destination, read as a third source: the encoding has no
    // third operand, so modifier bits naming one are not this translation's to interpret.
    if name == "v_mac_f32_e64" {
        if modifiers.negate[2] || modifiers.absolute[2] {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an accumulating multiply-add sets modifiers on the addend it has no field for",
            });
        }
        sources.push(Operand::Vector(destination));
    }
    arithmetic_with_modifiers(model, instruction, name, (register, &sources), modifiers)
}

/// A long-form vector ALU operation `name` writing `register` from `sources`, under its source and
/// output modifiers: the body every form whose modifiers are the long form's shares.
fn arithmetic_with_modifiers<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    (register, sources): (u32, &[Operand]),
    modifiers: Modifiers,
) -> Result<(), TranslateError> {
    // The output multiplier: applied to a 32-bit float result, before the clamp, and refused by
    // name otherwise.
    let scale = output_scale(modifiers.output_multiplier);
    if scale.is_some() && (name == CNDMASK || name == "v_div_fmas_f32" || !result_is_f32(name)) {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "this instruction scales a result this translation does not scale by its output ",
                "multiplier - on a select, a scaled multiply-add or a non-float result it is not ",
                "translated"
            ),
        });
    }

    // The output clamp: applied to a 32-bit float result in a stage whose NaN rule is known, and
    // refused by name otherwise.
    let clamp = if modifiers.clamp {
        if name == CNDMASK || name == "v_div_fmas_f32" || !result_is_f32(name) {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps a result this translation does not clamp - on an ",
                    "integer the flag saturates, and on a select or a scaled multiply-add it is ",
                    "not translated"
                ),
            });
        }
        let Some(nan_to_zero) = model.dx10_clamp() else {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps its result to [0, 1], and what that does to a NaN ",
                    "is the stage's DX10_CLAMP mode, which this translation was not given"
                ),
            });
        };
        Some(nan_to_zero)
    } else {
        None
    };

    if name == CNDMASK {
        return select_per_lane(model, instruction, register, sources, modifiers);
    }
    if name == "v_div_fmas_f32" {
        return division_fmas(model, instruction, register, sources, modifiers);
    }

    for lane in running_lanes(model) {
        let mut read = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let raw = model.read_source(instruction, source, lane)?;
            read.push(apply_modifiers(model, raw, modifiers, index));
        }
        let value = combine(model, instruction, name, &read)?;
        let value = match scale {
            Some(bits) => {
                let factor = model.constant(bits);
                model.f32_binary(op::FMUL, value, factor)
            }
            None => value,
        };
        let value = match clamp {
            Some(nan_to_zero) => clamp_unit(model, value, nan_to_zero),
            None => value,
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The factor an output multiplier field scales a float result by, as `f32` bits: 1 is two, 2 is
/// four, 3 is a half (RDNA ISA, `OMOD`); 0 is none.
const fn output_scale(field: u32) -> Option<u32> {
    match field {
        1 => Some(0x4000_0000),
        2 => Some(0x4080_0000),
        3 => Some(0x3F00_0000),
        _ => None,
    }
}

/// Whether a long-form vector instruction's result is a 32-bit float, from its name: an `_f32`
/// operation that is not a comparison, and, for a conversion `v_cvt_<to>_<from>`, one converting to
/// `f32`. `v_cvt_i32_f32` produces an integer and `v_cvt_pkrtz_f16_f32` packs halves.
fn result_is_f32(name: &str) -> bool {
    let base = name.strip_suffix("_e64").unwrap_or(name);
    // A conversion names its result first: `v_cvt_f32_i32` makes a float of an integer.
    if let Some(conversion) = base.strip_prefix("v_cvt_") {
        return conversion.starts_with("f32_");
    }
    !base.starts_with("v_cmp") && base.ends_with("_f32")
}

/// The output clamp on a 32-bit float result: `[0, 1]`, with a NaN either zero (`DX10_CLAMP` set)
/// or passed through (clear), as the stage's `RSRC1` says.
///
/// `FMin`/`FMax` are undefined on a NaN in `GLSL.std.450`, so the NaN takes its own select.
fn clamp_unit<M: Model + ?Sized>(model: &mut M, value: Id, nan_to_zero: bool) -> Id {
    let zero = model.constant(0);
    let one = model.constant(0x3F80_0000);
    let above = model.f32_ext_binary(GLSL_FMAX, value, zero);
    let clamped = model.f32_ext_binary(GLSL_FMIN, above, one);
    let nan = float_is(model, op::IS_NAN, value);
    let for_nan = if nan_to_zero { zero } else { value };
    pick(model, nan, for_nan, clamped)
}

/// Chooses between two 32-bit values on a boolean, without branching.
fn pick<M: Model + ?Sized>(model: &mut M, condition: Id, when_true: Id, when_false: Id) -> Id {
    let u32_type = model.u32_type();
    let b = model.builder();
    let result = b.id();
    b.function(
        op::SELECT,
        &[u32_type.0, result.0, condition.0, when_true.0, when_false.0],
    );
    result
}

/// A unary predicate on a float held as raw bits.
fn float_is<M: Model + ?Sized>(model: &mut M, opcode: u16, value: Id) -> Id {
    let as_float = model.as_float(value);
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(opcode, &[bool_type.0, result.0, as_float.0]);
    result
}

/// Both of two booleans.
fn both<M: Model + ?Sized>(model: &mut M, left: Id, right: Id) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(op::LOGICAL_AND, &[bool_type.0, result.0, left.0, right.0]);
    result
}

/// Translates `v_div_fixup_f32`: the special cases of a division, applied to a quotient.
///
/// The last step of the division sequence: the quotient from reciprocal and Newton-Raphson
/// refinement is replaced wherever zero over zero, infinity over infinity, a signed zero or a NaN
/// applies. The sources are the quotient, the denominator and the numerator, in that order.
///
/// The decision tree follows the instruction-set reference's pseudocode branch for branch. Its
/// `underflow` and `overflow` are IEEE-754 terms under round-to-nearest: below half the smallest
/// subnormal rounds to a signed zero, above the largest finite becomes a signed infinity; the
/// reference's underflow threshold (an exponent difference below -150) is exactly that point.
/// `Quiet(x)` sets the most significant mantissa bit. The tree is built from its default upwards,
/// so the last select applied is the first branch of the pseudocode, which gives the reference's
/// priority.
fn division_fixup<M: Model + ?Sized>(
    model: &mut M,
    quotient: Id,
    denominator: Id,
    numerator: Id,
) -> Id {
    let magnitude = model.constant(0x7FFF_FFFF);
    let sign_bit = model.constant(0x8000_0000);
    let infinity = model.constant(0x7F80_0000);
    // The reference gives this bit pattern for both `0/0` and `inf/inf`: a negative quiet NaN, sign
    // included.
    let indeterminate = model.constant(0xFFC0_0000);
    let quiet_bit = model.constant(0x0040_0000);
    let zero = model.constant(0);

    // sign_out = sign(denominator) ^ sign(numerator)
    let signs = model.binary(op::BITWISE_XOR, denominator, numerator);
    let sign_out = model.binary(op::BITWISE_AND, signs, sign_bit);

    let denominator_magnitude = model.binary(op::BITWISE_AND, denominator, magnitude);
    let numerator_magnitude = model.binary(op::BITWISE_AND, numerator, magnitude);
    let denominator_zero = model.compare(op::IEQUAL, denominator_magnitude, zero);
    let numerator_zero = model.compare(op::IEQUAL, numerator_magnitude, zero);

    let denominator_nan = float_is(model, op::IS_NAN, denominator);
    let numerator_nan = float_is(model, op::IS_NAN, numerator);
    let denominator_infinite = float_is(model, op::IS_INF, denominator);
    let numerator_infinite = float_is(model, op::IS_INF, numerator);

    let signed_zero = sign_out;
    let signed_infinity = model.binary(op::BITWISE_OR, sign_out, infinity);

    // The default: the computed quotient, with the sign the operands imply.
    let magnitude_of_quotient = model.binary(op::BITWISE_AND, quotient, magnitude);
    let mut result = model.binary(op::BITWISE_OR, sign_out, magnitude_of_quotient);

    // exponent(denominator) == 255: an infinity or NaN, already handled above, and translated
    // because the reference states the branch.
    let shift = model.constant(23);
    let exponent_mask = model.constant(0xFF);
    let shifted_denominator = model.binary(op::SHIFT_RIGHT_LOGICAL, denominator, shift);
    let denominator_exponent = model.binary(op::BITWISE_AND, shifted_denominator, exponent_mask);
    let shifted_numerator = model.binary(op::SHIFT_RIGHT_LOGICAL, numerator, shift);
    let numerator_exponent = model.binary(op::BITWISE_AND, shifted_numerator, exponent_mask);
    let all_ones = model.constant(255);
    let denominator_saturated = model.compare(op::IEQUAL, denominator_exponent, all_ones);
    result = pick(model, denominator_saturated, signed_infinity, result);

    // exponent(numerator) - exponent(denominator) < -150: the quotient is below half the smallest
    // subnormal and rounds to a signed zero. Compared as signed on biased exponents; the bias
    // cancels.
    let difference = model.binary(op::ISUB, numerator_exponent, denominator_exponent);
    let threshold = model.constant((-150_i32) as u32);
    let underflows = model.compare(op::SLESS_THAN, difference, threshold);
    result = pick(model, underflows, signed_zero, result);

    // x/inf, or 0/y.
    let vanishes = model.either(denominator_infinite, numerator_zero);
    result = pick(model, vanishes, signed_zero, result);

    // x/0, or inf/y.
    let diverges = model.either(denominator_zero, numerator_infinite);
    result = pick(model, diverges, signed_infinity, result);

    // inf/inf, then 0/0. Both are the indeterminate form.
    let both_infinite = both(model, denominator_infinite, numerator_infinite);
    result = pick(model, both_infinite, indeterminate, result);
    let both_zero = both(model, denominator_zero, numerator_zero);
    result = pick(model, both_zero, indeterminate, result);

    // A NaN operand propagates, quietened. The denominator is tested first so the numerator's
    // select, applied last, wins, in the reference's order.
    let quiet_denominator = model.binary(op::BITWISE_OR, denominator, quiet_bit);
    result = pick(model, denominator_nan, quiet_denominator, result);
    let quiet_numerator = model.binary(op::BITWISE_OR, numerator, quiet_bit);
    pick(model, numerator_nan, quiet_numerator, result)
}

/// Translates `v_div_fmas_f32`: a multiply-add that scales its result when the condition mask says
/// the operands were pre-scaled.
///
/// The middle of the division sequence: `v_div_scale_f32` may scale an operand by a power of two to
/// keep the reciprocal out of the subnormal range and records that in the condition mask; this
/// undoes it per lane by the documented factor of 2^32. The mask is read implicitly, so this needs
/// a model with lanes.
fn division_fmas<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    register: u32,
    sources: &[Operand],
    modifiers: Modifiers,
) -> Result<(), TranslateError> {
    let [first, second, third] = sources else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_div_fmas_f32 does not have three sources",
        });
    };
    let (low, high) = model.read_lane_mask(CONDITION_MASK)?;

    // 2^32, exactly representable, so the scaling is exact.
    let scale = model.constant(0x4F80_0000);

    for lane in running_lanes(model) {
        let a = model.read_source(instruction, first, lane)?;
        let a = apply_modifiers(model, a, modifiers, 0);
        let b = model.read_source(instruction, second, lane)?;
        let b = apply_modifiers(model, b, modifiers, 1);
        let c = model.read_source(instruction, third, lane)?;
        let c = apply_modifiers(model, c, modifiers, 2);

        // Fused, as `V_DIV_FMAS_F32` is ([`Model::f32_fma`]).
        let sum = model.f32_fma(a, b, c);
        let scaled = model.f32_binary(op::FMUL, sum, scale);

        let bit = model.lane_bit(low, high, lane);
        let value = pick(model, bit, scaled, sum);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The condition mask, which the division sequence passes its scaling flag through, named by its
/// low half as every 64-bit mask is.
const CONDITION_MASK: &str = VCC_LOW_HALF;

/// The negation of a boolean.
fn negate<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, result.0, value.0]);
    result
}

/// Whether a float's exponent field is all zeroes: a subnormal or a zero.
///
/// The division pre-scale asks whether a computed quotient is subnormal, and a Vulkan device may
/// flush subnormals to zero. Wherever this is used the true result cannot be zero (earlier branches
/// exclude a zero numerator or an infinite operand), so an all-zero exponent means "subnormal" on a
/// flushing device and a preserving one alike. Testing bits rather than comparing against the
/// smallest normal avoids depending on how a comparison treats a flushed operand.
fn exponent_is_zero<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let shift = model.constant(23);
    let mask = model.constant(0xFF);
    let zero = model.constant(0);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, shift);
    let exponent = model.binary(op::BITWISE_AND, shifted, mask);
    model.compare(op::IEQUAL, exponent, zero)
}

/// Translates `v_div_scale_f32`: the pre-scale that keeps a division out of the subnormal range.
///
/// The first step of the division sequence. It multiplies one of the numerator and denominator by a
/// power of two so the reciprocal that follows does not land among the subnormals, and records in
/// the condition mask whether it scaled, for `v_div_fmas_f32` to undo. `S0` is the operand to scale
/// and equals either the denominator or the numerator; several branches scale only when `S0` is the
/// operand they concern.
///
/// The decision tree, thresholds and scale factors follow the instruction-set reference's
/// pseudocode; the tree is built from its default upwards so the first branch wins. For the
/// zero-operand case the reference gives no NaN bit pattern, so the canonical quiet NaN is used;
/// `v_div_fixup_f32` replaces any NaN downstream.
fn division_scale<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read(instruction, true)?;
    let operands = &instruction.operands;
    let (register, mask_name) = division_scale_destinations(instruction)?;
    let sources: Vec<Operand> = operands[2..].to_vec();
    let [first, second, third] = sources.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the division pre-scale does not have three sources",
        });
    };

    let constants = ScaleConstants::new(model);

    // Started from zero: the reference opens with `VCC = 0` and then writes every lane.
    let mut halves = (constants.zero, constants.zero);

    // Every lane: this writes a mask as well as values (see `carry_arithmetic`).
    for lane in 0..model.modelled_lanes() {
        let (value, set) = division_scale_lane(
            model,
            instruction,
            [first, second, third],
            modifiers,
            &constants,
            lane,
        )?;
        halves = model.set_lane_bit(halves, lane, set);
        model.write_vector_lane(register, lane, value);
    }

    // The flag, where the instruction asked for one. It comes out of the same selects as the value,
    // so only the write is conditional.
    if let Some(mask_name) = mask_name {
        write_wave_mask(model, mask_name, halves)?;
    }
    model.count();
    Ok(())
}

/// Reads the division pre-scale's vector destination register and the lane mask its flag goes
/// to, `None` when the shader discards the flag.
fn division_scale_destinations(
    instruction: &Instruction,
) -> Result<(u32, Option<&'static str>), TranslateError> {
    let operands = &instruction.operands;
    let (Some(Operand::Vector(destination)), Some(scalar_destination)) =
        (operands.first(), operands.get(1))
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "the division pre-scale needs a vector destination and a scalar one",
        });
    };
    let register = u32::from(*destination);
    // `null` means the flag is not wanted: the shader needs only the scaled operand, so the flag is
    // written nowhere.
    let mask_name = match scalar_destination {
        _ if discards(scalar_destination) => None,
        Operand::Named(named) => {
            Some(lane_mask_name(named).ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "the division pre-scale writes a destination this translator does ",
                    "not know as a lane mask"
                ),
            })?)
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "the division pre-scale writes its flag somewhere other than a ",
                    "lane mask, which is not translated"
                ),
            });
        }
    };
    Ok((register, mask_name))
}

/// The constants every lane of the division pre-scale uses, declared once before the lanes.
struct ScaleConstants {
    magnitude: Id,
    zero: Id,
    canonical_nan: Id,
    up: Id,
    down: Id,
    shift: Id,
    exponent_mask: Id,
    ninety_six: Id,
    twenty_three: Id,
    truth: Id,
    falsehood: Id,
}

impl ScaleConstants {
    /// Declares the constants, in field order.
    fn new<M: Model + ?Sized>(model: &mut M) -> Self {
        let magnitude = model.constant(0x7FFF_FFFF);
        let zero = model.constant(0);
        let canonical_nan = model.constant(0x7FC0_0000);
        // Two to the sixty-fourth and its reciprocal, both exact.
        let up = model.constant(0x5F80_0000);
        let down = model.constant(0x1F80_0000);
        let shift = model.constant(23);
        let exponent_mask = model.constant(0xFF);
        let ninety_six = model.constant(96);
        let twenty_three = model.constant(23);
        // The flag is carried as a word through the same selects as the value and turned back into
        // a bit at the end.
        let truth = model.constant(1);
        let falsehood = zero;
        Self {
            magnitude,
            zero,
            canonical_nan,
            up,
            down,
            shift,
            exponent_mask,
            ninety_six,
            twenty_three,
            truth,
            falsehood,
        }
    }
}

/// Computes one lane of the division pre-scale: the scaled value, and whether it scaled.
fn division_scale_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    [first, second, third]: [&Operand; 3],
    modifiers: Modifiers,
    constants: &ScaleConstants,
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    let ScaleConstants {
        magnitude,
        zero,
        canonical_nan,
        up,
        down,
        shift,
        exponent_mask,
        ninety_six,
        twenty_three,
        truth,
        falsehood,
    } = *constants;
    let scaled_input = model.read_source(instruction, first, lane)?;
    let scaled_input = apply_modifiers(model, scaled_input, modifiers, 0);
    let denominator = model.read_source(instruction, second, lane)?;
    let denominator = apply_modifiers(model, denominator, modifiers, 1);
    let numerator = model.read_source(instruction, third, lane)?;
    let numerator = apply_modifiers(model, numerator, modifiers, 2);

    let denominator_magnitude = model.binary(op::BITWISE_AND, denominator, magnitude);
    let numerator_magnitude = model.binary(op::BITWISE_AND, numerator, magnitude);
    let denominator_zero = model.compare(op::IEQUAL, denominator_magnitude, zero);
    let numerator_zero = model.compare(op::IEQUAL, numerator_magnitude, zero);

    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, denominator, shift);
    let denominator_exponent = model.binary(op::BITWISE_AND, shifted, exponent_mask);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, numerator, shift);
    let numerator_exponent = model.binary(op::BITWISE_AND, shifted, exponent_mask);

    // A subnormal denominator: exponent all zeroes, but not the value zero.
    let denominator_flat = model.compare(op::IEQUAL, denominator_exponent, zero);
    let not_zero = negate(model, denominator_zero);
    let denominator_subnormal = both(model, denominator_flat, not_zero);

    // The quotient is near the top of the range.
    let spread = model.binary(op::ISUB, numerator_exponent, denominator_exponent);
    let very_wide = model.compare(op::SGREATER_THAN_EQUAL, spread, ninety_six);
    let numerator_tiny = model.compare(op::SLESS_THAN_EQUAL, numerator_exponent, twenty_three);

    // The two questions that need an actual division. See `exponent_is_zero`.
    let one = model.constant(0x3F80_0000);
    let reciprocal = model.f32_binary(op::FDIV, one, denominator);
    let reciprocal_subnormal = exponent_is_zero(model, reciprocal);
    let quotient = model.f32_binary(op::FDIV, numerator, denominator);
    let quotient_subnormal = exponent_is_zero(model, quotient);
    let both_subnormal = both(model, reciprocal_subnormal, quotient_subnormal);

    let scaled_is_denominator = model.compare(op::IEQUAL, scaled_input, denominator);
    let scaled_is_numerator = model.compare(op::IEQUAL, scaled_input, numerator);

    let scaled_up = model.f32_binary(op::FMUL, scaled_input, up);
    let scaled_down = model.f32_binary(op::FMUL, scaled_input, down);
    let up_if_denominator = pick(model, scaled_is_denominator, scaled_up, scaled_input);
    let up_if_numerator = pick(model, scaled_is_numerator, scaled_up, scaled_input);

    // Built from the default upwards, so the last applied is the first branch.
    let mut value = scaled_input;
    let mut flag = falsehood;

    value = pick(model, numerator_tiny, scaled_up, value);

    value = pick(model, quotient_subnormal, up_if_numerator, value);
    flag = pick(model, quotient_subnormal, truth, flag);

    value = pick(model, reciprocal_subnormal, scaled_down, value);
    flag = pick(model, reciprocal_subnormal, falsehood, flag);

    value = pick(model, both_subnormal, up_if_denominator, value);
    flag = pick(model, both_subnormal, truth, flag);

    value = pick(model, denominator_subnormal, scaled_up, value);
    flag = pick(model, denominator_subnormal, falsehood, flag);

    value = pick(model, very_wide, up_if_denominator, value);
    flag = pick(model, very_wide, truth, flag);

    let either_zero = model.either(denominator_zero, numerator_zero);
    value = pick(model, either_zero, canonical_nan, value);
    flag = pick(model, either_zero, falsehood, flag);

    let set = model.is_not_zero(flag);
    Ok((value, set))
}

/// Writes the low half of a lane mask, leaving the upper half as it was.
///
/// A 32-lane shader manipulates its mask with 32-bit scalar instructions (`s_mov_b32 exec_lo,
/// ...`). Treated as ordinary scalar writes those would miss the model's mask and leave every lane
/// active. The upper half is read back and rewritten unchanged, because a 64-lane shader may write
/// `exec_lo` alone.
fn write_mask_low<M: Model + ?Sized>(
    model: &mut M,
    mask: &'static str,
    value: Id,
) -> Result<(), TranslateError> {
    let (_, high) = model.read_lane_mask(mask)?;
    model.write_lane_mask(mask, value, high)
}

/// Writes the lane mask a vector instruction answers into `vcc` or the execution mask: both halves
/// in a wave of 64, the low half alone in a wave of 32. There `vcc_hi` and `exec_hi` are registers
/// of their own the instruction leaves as they were (AMD's published RDNA instruction set: a wave32
/// VOPC writes `VCC_LO`), and a compiler keeps values in them: Grand Theft Auto V's treelet program
/// at `0x3dc9a00` in its image holds its saved execution mask in `vcc_hi` across a loop of
/// `v_cmp_lt_f32_e32 vcc_lo`.
fn write_wave_mask<M: Model + ?Sized>(
    model: &mut M,
    mask: &'static str,
    (low, high): (Id, Id),
) -> Result<(), TranslateError> {
    if model.lanes() > 32 {
        model.write_lane_mask(mask, low, high)
    } else {
        write_mask_low(model, mask, low)
    }
}

/// Writes the high half of a lane mask, leaving the low half as it was.
fn write_mask_high<M: Model + ?Sized>(
    model: &mut M,
    mask: &'static str,
    value: Id,
) -> Result<(), TranslateError> {
    let (low, _) = model.read_lane_mask(mask)?;
    model.write_lane_mask(mask, low, value)
}

/// Writes a 32-bit scalar result to `destination`: a scalar register, or a lane mask's half named as
/// one - `vcc_lo` and `vcc_hi` are scalar registers 106 and 107, `exec_lo` and `exec_hi` 126 and 127
/// (AMD's RDNA2 instruction set, section 3.2) - written as that half with the other kept.
fn write_scalar_or_mask_half<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    destination: &Operand,
    value: Id,
) -> Result<(), TranslateError> {
    if let Some(mask) = mask_destination(destination) {
        return write_mask_low(model, mask, value);
    }
    if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_high_name(name)
    {
        return write_mask_high(model, mask, value);
    }
    let register = scalar_destination(instruction, destination)?;
    model.write_scalar(register, value);
    Ok(())
}

/// The lane mask an operand names, if it names one.
fn mask_destination(operand: &Operand) -> Option<&'static str> {
    match operand {
        Operand::Named(named) => lane_mask_name(named),
        _ => None,
    }
}

/// Translates `v_cndmask_b32`: per lane, the mask's bit picks which source to copy.
///
/// Its sources are raw bits and are not bitcast, and its third operand is a sixty-four-bit mask, so
/// it needs a model with lanes; the per-lane model refuses it.
fn select_per_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    register: u32,
    sources: &[Operand],
    modifiers: Modifiers,
) -> Result<(), TranslateError> {
    let [when_clear, when_set, mask] = sources else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_cndmask_b32 does not have three sources",
        });
    };
    let (low, high) = lane_mask_source(model, instruction, mask)?;

    for lane in running_lanes(model) {
        let clear = model.read_source(instruction, when_clear, lane)?;
        let set = model.read_source(instruction, when_set, lane)?;
        let clear = apply_modifiers(model, clear, modifiers, 0);
        let set = apply_modifiers(model, set, modifiers, 1);
        let bit = model.lane_bit(low, high, lane);

        let u32_type = model.u32_type();
        let b = model.builder();
        let value = b.id();
        // A set bit picks the second source.
        b.function(op::SELECT, &[u32_type.0, value.0, bit.0, set.0, clear.0]);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// Applies a source's negate and absolute flags: absolute first, then negate, so `-|x|` is
/// expressible, as the encoding means.
///
/// Both act on the bit pattern. Negation has a core opcode; absolute value is clearing the sign
/// bit.
fn apply_modifiers<M: Model + ?Sized>(
    model: &mut M,
    value: Id,
    modifiers: Modifiers,
    source: usize,
) -> Id {
    if !modifiers.touches(source) {
        return value;
    }
    let mut value = value;
    if modifiers.absolute[source] {
        let mask = model.constant(0x7FFF_FFFF);
        value = model.binary(op::BITWISE_AND, value, mask);
    }
    if modifiers.negate[source] {
        let sign = model.constant(0x8000_0000);
        value = model.binary(op::BITWISE_XOR, value, sign);
    }
    value
}

/// Combines the sources a long-form arithmetic instruction read.
fn combine<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    sources: &[Id],
) -> Result<Id, TranslateError> {
    match (name, sources) {
        ("v_cvt_pkrtz_f16_f32_e64", [low, high]) => Ok(pack_two_halves(model, *low, *high)),
        ("v_cvt_pknorm_u16_f32" | "v_cvt_pknorm_i16_f32", [low, high]) => {
            let signed = name == "v_cvt_pknorm_i16_f32";
            let low = normalised_16(model, *low, signed);
            let high = normalised_16(model, *high, signed);
            Ok(pack_two_16(model, low, high))
        }
        ("v_cvt_pk_u16_u32" | "v_cvt_pk_i16_i32", [low, high]) => {
            let signed = name == "v_cvt_pk_i16_i32";
            let low = saturated_16(model, *low, signed);
            let high = saturated_16(model, *high, signed);
            Ok(pack_two_16(model, low, high))
        }
        ("v_max_f32_e64", [first, second]) => Ok(model.f32_ext_binary(GLSL_FMAX, *first, *second)),
        ("v_min_f32_e64", [first, second]) => Ok(model.f32_ext_binary(GLSL_FMIN, *first, *second)),
        // The conversions, as their short forms convert (D867's clip-distance shaders).
        ("v_cvt_f32_i32_e64", [value]) => Ok(integer_to_float(model, *value, true)),
        ("v_cvt_f32_u32_e64", [value]) => Ok(integer_to_float(model, *value, false)),
        ("v_cvt_i32_f32_e64", [value]) => Ok(float_to_signed_saturated(model, *value)),
        ("v_cvt_u32_f32_e64", [value]) => Ok(float_to_unsigned_saturated(model, *value)),
        // The reciprocal ACO takes of a cube coordinate's major axis, `|ma|` through the long
        // form's absolute value: an exact division, as the short form's.
        ("v_rcp_f32_e64", [value]) => Ok(reciprocal_f32(model, *value)),
        // `x - floor(x)`, as the short form computes it.
        ("v_fract_f32_e64", [value]) => {
            let floor = model.f32_ext_unary(GLSL_FLOOR, *value);
            Ok(model.f32_binary(op::FSUB, *value, floor))
        }
        (
            "v_add_f32_e64" | "v_sub_f32_e64" | REVERSE_SUBTRACT | "v_mul_f32_e64",
            [first, second],
        ) => {
            // The reverse-subtract takes its operands the other way round, as the short form does.
            let (left, right) = if name == REVERSE_SUBTRACT {
                (*second, *first)
            } else {
                (*first, *second)
            };
            let spirv = match name {
                "v_add_f32_e64" => op::FADD,
                "v_sub_f32_e64" | REVERSE_SUBTRACT => op::FSUB,
                _ => op::FMUL,
            };
            Ok(model.f32_binary(spirv, left, right))
        }
        // The fused multiply-add, rounded once ([`Model::f32_fma`]).
        ("v_fma_f32", [a, b, c]) => Ok(model.f32_fma(*a, *b, *c)),
        ("v_mad_f32" | "v_mac_f32_e64", [a, b, c]) => Ok(legacy_multiply_add(model, *a, *b, *c)),
        ("v_div_fixup_f32", [quotient, denominator, numerator]) => {
            Ok(division_fixup(model, *quotient, *denominator, *numerator))
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a long-form vector instruction has an arity this does not handle",
        }),
    }
}

/// `S_LSHL1_ADD_U32` to `S_LSHL4_ADD_U32`: `value` shifted left by `amount` (one to four) and
/// `addend` added, with the condition code the carry out of the 64-bit sum (AMD's published RDNA
/// instruction set: `tmp = (S0.u64 << n) + S1.u64; SCC = tmp >= 2^32`) - a bit shifted out, or a
/// wrapped add.
fn shift_and_add<M: Model + ?Sized>(
    model: &mut M,
    (value, amount): (Id, u32),
    addend: Id,
) -> (Id, Option<Id>) {
    let (by, back) = (model.constant(amount), model.constant(32 - amount));
    let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, value, by);
    let lost = model.binary(op::SHIFT_RIGHT_LOGICAL, value, back);
    let result = model.add(shifted, addend);
    let shifted_out = model.is_not_zero(lost);
    let wrapped = model.compare(op::ULESS_THAN, result, shifted);
    (result, Some(model.either(shifted_out, wrapped)))
}

/// Translates the 32-bit scalar arithmetic and logic.
///
/// Every one but the multiply writes the condition code: the logical operations to whether the
/// result is non-zero, the add and subtract to whether the signed operation overflowed. Dropping it
/// would leave the next branch reading a stale code. `s_mul_i32` leaves it as it was.
fn scalar_integer<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let mask = mask_destination(destination);
    let to_m0 = matches!(destination, Operand::Named(named) if named == M0);
    let high_half = match destination {
        Operand::Named(named) => lane_mask_high_name(named),
        _ => None,
    };
    // A mask, a mask's high half or `m0` has no register number, so the register is only resolved
    // when there is one to resolve.
    let register = match mask {
        Some(_) => 0,
        None if to_m0 || high_half.is_some() => 0,
        None => scalar_destination(instruction, destination)?,
    };

    let left = model.read_source(instruction, first, 0)?;
    let right = model.read_source(instruction, second, 0)?;

    let (result, condition) = match name {
        "s_add_i32" | "s_sub_i32" => {
            let result = if name == "s_add_i32" {
                model.binary(op::IADD, left, right)
            } else {
                model.binary(op::ISUB, left, right)
            };
            // Signed overflow: the operands agreed in sign and the result does not, tested in bits
            // because no core opcode reports overflow.
            let right = if name == "s_add_i32" {
                right
            } else {
                // Subtraction overflows when the operands *differ* in sign, which is the
                // same test applied to the negated right-hand side.
                let sign = model.constant(0x8000_0000);
                model.binary(op::BITWISE_XOR, right, sign)
            };
            let left_differs = model.binary(op::BITWISE_XOR, left, result);
            let right_differs = model.binary(op::BITWISE_XOR, right, result);
            let both = model.binary(op::BITWISE_AND, left_differs, right_differs);
            let sign = model.constant(0x8000_0000);
            let overflow = model.binary(op::BITWISE_AND, both, sign);
            (result, Some(model.is_not_zero(overflow)))
        }
        "s_and_b32" | "s_or_b32" | "s_xor_b32" => {
            let spirv = match name {
                "s_and_b32" => op::BITWISE_AND,
                "s_or_b32" => op::BITWISE_OR,
                _ => op::BITWISE_XOR,
            };
            let result = model.binary(spirv, left, right);
            (result, Some(model.is_not_zero(result)))
        }
        // `S_ANDN2_B32`, `S_ORN2_B32` and `S_NOR_B32`, and the condition code.
        "s_andn2_b32" | "s_orn2_b32" | "s_nor_b32" | "s_nand_b32" => {
            negated_logic(model, name, left, right)
        }
        // `S_LSHL_B32`: `D = S0 << S1[4:0]`, the condition code whether the result is non-zero.
        "s_lshl_b32" => {
            let five = model.constant(31);
            let amount = model.binary(op::BITWISE_AND, right, five);
            let result = model.binary(op::SHIFT_LEFT_LOGICAL, left, amount);
            (result, Some(model.is_not_zero(result)))
        }
        // `S_BFE_U32`: `D = (S0 >> S1[4:0]) & ((1 << S1[22:16]) - 1)`, and the condition code.
        "s_bfe_u32" => {
            let result = bit_field_extract(model, left, right);
            (result, Some(model.is_not_zero(result)))
        }
        // `S_MUL_I32`: the product's low 32 bits, which signed and unsigned multiplication share;
        // the condition code is not written.
        "s_mul_i32" => (model.binary(op::IMUL, left, right), None),
        "s_sub_u32" | "s_min_i32" | "s_max_i32" | "s_min_u32" | "s_max_u32" => {
            difference_or_minimum(model, name, left, right)
        }
        // `S_MUL_HI_U32`: the unsigned product's high 32 bits; the condition code is not written.
        "s_mul_hi_u32" => (multiply_high(model, left, right), None),
        // `S_LSHR_B32`: `D = S0 >> S1[4:0]`, logical, and whether it is non-zero.
        "s_lshr_b32" => {
            let five = model.constant(31);
            let amount = model.binary(op::BITWISE_AND, right, five);
            let result = model.binary(op::SHIFT_RIGHT_LOGICAL, left, amount);
            (result, Some(model.is_not_zero(result)))
        }
        // `S_ADD_U32`: the sum, and the carry out - a wrapped sum is below either addend.
        "s_add_u32" => {
            let result = model.add(left, right);
            (result, Some(model.compare(op::ULESS_THAN, result, left)))
        }
        "s_addc_u32" => scalar_add_with_carry(model, left, right),
        "s_subb_u32" => scalar_subtract_with_borrow(model, left, right),
        "s_lshl1_add_u32" => shift_and_add(model, (left, 1), right),
        "s_lshl2_add_u32" => shift_and_add(model, (left, 2), right),
        "s_lshl3_add_u32" => shift_and_add(model, (left, 3), right),
        "s_lshl4_add_u32" => shift_and_add(model, (left, 4), right),
        // `S_CSELECT_B32`: the first source where the condition code is set; it writes none.
        "s_cselect_b32" => {
            let condition = crate::control::read_condition_code(model, true);
            (model.select(condition, left, right), None)
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "no translation for this scalar integer instruction",
            });
        }
    };

    // A mask destination goes to the mask, not the register file: `s_and_b32 exec_lo, exec_lo, s2`
    // is how a 32-lane shader narrows its execution mask. `m0` is state outside the file too:
    // `s_or_b32 m0, s0, s2` is how radeonsi's primitive shader forms its allocation request.
    match (mask, high_half) {
        (Some(mask), _) => write_mask_low(model, mask, result)?,
        (None, Some(mask)) => write_mask_high(model, mask, result)?,
        (None, None) if to_m0 => model.write_m0(result),
        (None, None) => model.write_scalar(register, result),
    }
    if let Some(condition) = condition {
        model.set_condition_code(condition);
    }
    model.count();
    Ok(())
}

/// `S_SUB_U32`: the difference, and the borrow - whether the second source exceeded the first;
/// `S_MIN_I32` and `S_MIN_U32`: the minimum, signed or unsigned, and whether the first source is the
/// smaller; or `S_MAX_I32` and `S_MAX_U32`: the maximum, and whether the first source is the larger.
/// Grand Theft Auto V's compute programs keep the result in `vcc_lo`.
fn difference_or_minimum<M: Model + ?Sized>(
    model: &mut M,
    name: &str,
    left: Id,
    right: Id,
) -> (Id, Option<Id>) {
    if name == "s_sub_u32" {
        let result = model.binary(op::ISUB, left, right);
        return (result, Some(model.compare(op::ULESS_THAN, left, right)));
    }
    let ordering = match name {
        "s_max_i32" => op::SGREATER_THAN,
        "s_max_u32" => op::UGREATER_THAN,
        "s_min_u32" => op::ULESS_THAN,
        _ => op::SLESS_THAN,
    };
    let first = model.compare(ordering, left, right);
    (model.select(first, left, right), Some(first))
}

/// `s_andn2_b32`, `s_orn2_b32`, `s_nor_b32` and `s_nand_b32`: `S0 & ~S1`, `S0 | ~S1`,
/// `~(S0 | S1)` and `~(S0 & S1)`, with the condition code whether the result is non-zero.
fn negated_logic<M: Model + ?Sized>(
    model: &mut M,
    name: &str,
    left: Id,
    right: Id,
) -> (Id, Option<Id>) {
    let result = if name == "s_nor_b32" {
        // `S_NOR_B32`: `D = ~(S0 | S1)` (AMD's published RDNA 2 instruction set, SOP2).
        let either = model.binary(op::BITWISE_OR, left, right);
        model.not(either)
    } else if name == "s_nand_b32" {
        // `S_NAND_B32`: `D = ~(S0 & S1)` (AMD's published RDNA 2 instruction set, SOP2).
        let both = model.binary(op::BITWISE_AND, left, right);
        model.not(both)
    } else {
        let inverted = model.not(right);
        let spirv = if name == "s_andn2_b32" {
            op::BITWISE_AND
        } else {
            op::BITWISE_OR
        };
        model.binary(spirv, left, inverted)
    };
    (result, Some(model.is_not_zero(result)))
}

/// Translates the compact scalar form: a destination and a sixteen-bit immediate.
///
/// The immediate is signed; the decoder reports the field as encoded (a disassembler prints -2 as
/// 65534), and sign extension happens here. `s_addk_i32` and `s_mulk_i32` also read their
/// destination, because they accumulate.
fn scalar_immediate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, immediate) = two_operands(instruction)?;
    let Operand::Immediate(raw) = immediate else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a compact scalar instruction carries no immediate",
        });
    };
    let value = i64::from(sign_extend_16(*raw));
    let constant = model.constant(value as u32);
    // `m0` is state outside the register file: `s_movk_i32 m0, n` is how ACO sets it to a small
    // constant before a local-data-share access or a message.
    if name == "s_movk_i32" && matches!(destination, Operand::Named(named) if named == "m0") {
        model.write_m0(constant);
        model.count();
        return Ok(());
    }
    // A lane mask's half used as a scalar, as Grand Theft Auto V's `s_mulk_i32 vcc_lo, 0x50` and
    // `s_addk_i32 vcc_hi, 0x80` are: `vcc_lo` and `vcc_hi` are scalar registers 106 and 107 to
    // every scalar ALU instruction (AMD's RDNA2 instruction set, section 3.2; LLVM's SReg_32
    // class), read and written as that half, the other kept.
    let half = match destination {
        Operand::Named(named) => lane_mask_name(named)
            .map(|mask| (mask, false))
            .or_else(|| lane_mask_high_name(named).map(|mask| (mask, true))),
        _ => None,
    };
    let register = match half {
        Some(_) => 0,
        None => scalar_destination(instruction, destination)?,
    };
    let read = |model: &mut M| -> Result<Id, TranslateError> {
        Ok(match half {
            Some((mask, high)) => {
                let (low_now, high_now) = model.read_lane_mask(mask)?;
                if high { high_now } else { low_now }
            }
            None => model.read_scalar(register),
        })
    };
    let write = |model: &mut M, value: Id| -> Result<(), TranslateError> {
        match half {
            Some((mask, true)) => write_mask_high(model, mask, value),
            Some((mask, false)) => write_mask_low(model, mask, value),
            None => {
                model.write_scalar(register, value);
                Ok(())
            }
        }
    };

    match name {
        // s_movk_i32: a move, and the only one here that leaves the code alone.
        "s_movk_i32" => {
            write(model, constant)?;
        }
        // The compact compares. Signed, like every scalar compare.
        "s_cmpk_eq_i32" | "s_cmpk_lg_i32" | "s_cmpk_gt_i32" | "s_cmpk_ge_i32" | "s_cmpk_lt_i32"
        | "s_cmpk_le_i32" => {
            let current = read(model)?;
            let spirv = match name {
                "s_cmpk_eq_i32" => op::IEQUAL,
                "s_cmpk_lg_i32" => op::INOT_EQUAL,
                "s_cmpk_gt_i32" => op::SGREATER_THAN,
                "s_cmpk_ge_i32" => op::SGREATER_THAN_EQUAL,
                "s_cmpk_lt_i32" => op::SLESS_THAN,
                _ => op::SLESS_THAN_EQUAL,
            };
            let condition = model.compare(spirv, current, constant);
            model.set_condition_code(condition);
        }
        // s_addk_i32 accumulates and sets the code on signed overflow.
        "s_addk_i32" => {
            let current = read(model)?;
            let result = model.binary(op::IADD, current, constant);
            let left_differs = model.binary(op::BITWISE_XOR, current, result);
            let right_differs = model.binary(op::BITWISE_XOR, constant, result);
            let both = model.binary(op::BITWISE_AND, left_differs, right_differs);
            let sign = model.constant(0x8000_0000);
            let overflow = model.binary(op::BITWISE_AND, both, sign);
            let condition = model.is_not_zero(overflow);
            write(model, result)?;
            model.set_condition_code(condition);
        }
        // s_mulk_i32 accumulates and leaves the code alone, as the instruction set documents.
        "s_mulk_i32" => {
            let current = read(model)?;
            let result = model.binary(op::IMUL, current, constant);
            write(model, result)?;
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "no translation for this compact scalar instruction",
            });
        }
    }
    model.count();
    Ok(())
}

/// Sign-extends a sixteen-bit immediate. The width comes from the instruction's definition, since
/// the operand layout records only the field it observed.
fn sign_extend_16(raw: i64) -> i32 {
    i32::from(i16::try_from(raw & 0xFFFF).unwrap_or(raw as i16))
}

/// The scalar register a destination operand names.
fn scalar_destination(
    instruction: &Instruction,
    destination: &Operand,
) -> Result<u32, TranslateError> {
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar destination is not a scalar register",
        });
    };
    Ok(u32::from(*register))
}

/// Translates a scalar comparison into the condition code.
///
/// These compare signed integers; an unsigned comparison agrees on non-negative pairs and reverses
/// the order wherever one operand is negative. The condition code is one bit of hidden state, not
/// an operand.
/// How a save-exec instruction forms the new execution mask from its source and the old one.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SaveExec {
    /// The source and the mask (`s_and_saveexec_b64`).
    And,
    /// The source's complement and the mask (`s_andn1_saveexec_b64`).
    AndNotSource,
    /// The source or the mask's complement (`s_orn2_saveexec_b64`).
    OrNotExec,
}

/// `s_cmp_lg_u64 a, b`: whether two 64-bit scalar sources differ, into the condition code
/// (`S_CMP_LG_U64`, AMD's published RDNA instruction set); `s_cmp_eq_u64`, where `equal`, whether
/// they are the same (`S_CMP_EQ_U64`).
fn scalar_compare_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    equal: bool,
) -> Result<(), TranslateError> {
    let (first, second) = two_operands(instruction)?;
    let (first_low, first_high) = sixty_four_bit_source(model, instruction, first)?;
    let (second_low, second_high) = sixty_four_bit_source(model, instruction, second)?;
    let low = model.compare(op::INOT_EQUAL, first_low, second_low);
    let high = model.compare(op::INOT_EQUAL, first_high, second_high);
    let bool_type = model.bool_type();
    let b = model.builder();
    let differ = b.id();
    b.function(op::LOGICAL_OR, &[bool_type.0, differ.0, low.0, high.0]);
    let answer = if equal {
        let same = b.id();
        b.function(op::LOGICAL_NOT, &[bool_type.0, same.0, differ.0]);
        same
    } else {
        differ
    };
    model.set_condition_code(answer);
    model.count();
    Ok(())
}

fn scalar_compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let opcode = op_for_scalar_compare(instruction, name)?;
    let (first, second) = two_operands(instruction)?;

    // Scalar, so once for the wavefront. A scalar instruction has no lanes; a vector source here is
    // refused by the operand check below.
    let left = model.read_source(instruction, first, 0)?;
    let right = model.read_source(instruction, second, 0)?;
    let condition = model.compare(opcode, left, right);

    // Widened to a word, because the code lives in a private variable and a boolean has no defined
    // size in a storage class.
    let one = model.constant(1);
    let zero = model.constant(0);
    let u32_type = model.u32_type();
    let pointer = model.condition_code();
    let b = model.builder();
    let value = b.id();
    b.function(
        op::SELECT,
        &[u32_type.0, value.0, condition.0, one.0, zero.0],
    );
    b.function(op::STORE, &[pointer.0, value.0]);

    model.count();
    Ok(())
}

/// The SPIR-V opcode a scalar comparison maps to: signed for an `_i32` ordering, unsigned for a
/// `_u32` one.
fn op_for_scalar_compare(instruction: &Instruction, name: &str) -> Result<u16, TranslateError> {
    match name {
        // Equality does not depend on sign: the unsigned and signed forms agree.
        "s_cmp_eq_i32" | "s_cmp_eq_u32" => Ok(op::IEQUAL),
        // Equality does not depend on sign: the unsigned and signed forms agree.
        "s_cmp_lg_i32" | "s_cmp_lg_u32" => Ok(op::INOT_EQUAL),
        "s_cmp_gt_i32" => Ok(op::SGREATER_THAN),
        "s_cmp_ge_i32" => Ok(op::SGREATER_THAN_EQUAL),
        "s_cmp_lt_i32" => Ok(op::SLESS_THAN),
        "s_cmp_le_i32" => Ok(op::SLESS_THAN_EQUAL),
        "s_cmp_gt_u32" => Ok(op::UGREATER_THAN),
        "s_cmp_ge_u32" => Ok(op::UGREATER_THAN_EQUAL),
        "s_cmp_lt_u32" => Ok(op::ULESS_THAN),
        "s_cmp_le_u32" => Ok(op::ULESS_THAN_EQUAL),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this scalar comparison",
        }),
    }
}

/// Translates a float comparison into a lane mask.
///
/// The 32-bit form's destination is implicit (the condition mask), so it arrives as a named operand
/// carrying no bits. The registers are bitcast to floats first: comparing them as integers orders
/// negative floats backwards.
fn compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (opcode, floats) = op_for_compare(instruction, name)?;
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Named(name) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a comparison's destination is not a named mask",
        });
    };
    let name = lane_mask_name(name).ok_or(TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a comparison's destination is not a lane mask this translator knows",
    })?;

    // Every lane: the answer is a mask (see `carry_arithmetic`). A lane known not to run answers
    // zero whatever it holds, so it is not read.
    let halves = every_lane_bit(model, true, |model, lane| {
        let left = model.read_source(instruction, first, lane)?;
        let right = model.read_source(instruction, second, lane)?;
        let (left, right) = if floats {
            (model.as_float(left), model.as_float(right))
        } else {
            (left, right)
        };
        Ok(lane_compare(model, opcode, left, right))
    })?;
    let halves = running_bits(model, halves)?;

    write_wave_mask(model, name, halves)?;
    model.count();
    Ok(())
}

/// A compare into the execution mask: `v_cmpx_le_i16_e32 a, b`, whether `a`'s low sixteen bits are
/// at most `b`'s as signed integers, and `v_cmpx_neq_f32_e32 a, b`, whether the floats are unequal
/// or unordered. A lane that was not running answers zero, so the mask only narrows. On this
/// generation a `v_cmpx` writes the execution mask alone (ACO:
/// `definitions = dst(EXEC if cmpx else VCC)`, `aco_opcodes.py:1203-1205`).
fn compare_into_exec<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [first, second] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a compare into the execution mask does not have two sources",
        });
    };
    let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    let zero = model.constant(0);
    let sixteen = model.constant(16);
    // Every lane: the answer is a mask (see `carry_arithmetic`).
    // The compare, and how the sources are read: as floats, as the low sixteen bits signed, or
    // whole.
    let (opcode, reading) = match name {
        // Not equal or unordered: true for a NaN, as `v_cmp_neq_f32` is.
        "v_cmpx_neq_f32_e32" => (op::FUNORD_NOT_EQUAL, 0),
        "v_cmpx_gt_f32_e32" => (op::FORD_GREATER_THAN, 0),
        "v_cmpx_lt_f32_e32" => (op::FORD_LESS_THAN, 0),
        "v_cmpx_eq_f32_e32" => (op::FORD_EQUAL, 0),
        // Not less than: true for a NaN, as `v_cmp_nlt_f32` is.
        "v_cmpx_nlt_f32_e32" => (op::FUNORD_GREATER_THAN_EQUAL, 0),
        "v_cmpx_le_i16_e32" => (op::SLESS_THAN_EQUAL, 1),
        "v_cmpx_lt_i16_e32" => (op::SLESS_THAN, 1),
        "v_cmpx_eq_i32_e32" => (op::IEQUAL, 2),
        "v_cmpx_le_i32_e32" => (op::SLESS_THAN_EQUAL, 2),
        "v_cmpx_gt_u32_e32" => (op::UGREATER_THAN, 2),
        // Any other, by its ordering, as `compare_op` reads it: floats as floats, the rest whole.
        _ => compare_op(&name.replacen("v_cmpx_", "v_cmp_", 1))
            .map_or((op::SGREATER_THAN, 2), |(opcode, float)| {
                (opcode, if float { 0 } else { 2 })
            }),
    };
    let halves = every_lane_bit(model, false, |model, lane| {
        let mut sides = [zero; 2];
        for (slot, source) in sides.iter_mut().zip([first, second]) {
            let value = model.read_source(instruction, source, lane)?;
            *slot = match reading {
                0 => model.as_float(value),
                1 => {
                    let raised = model.binary(op::SHIFT_LEFT_LOGICAL, value, sixteen);
                    model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, sixteen)
                }
                _ => value,
            };
        }
        Ok(lane_compare(model, opcode, sides[0], sides[1]))
    })?;
    let low = model.binary(op::BITWISE_AND, halves.0, running_low);
    let high = model.binary(op::BITWISE_AND, halves.1, running_high);
    write_wave_mask(model, EXEC_LOW_HALF, (low, high))?;
    model.count();
    Ok(())
}

/// A sixty-four-bit integer compare's parts: whether it writes the execution mask (a `cmpx`), its
/// condition, and whether it compares signed. `None` for any other instruction.
fn compare_64_parts(name: &str) -> Option<(bool, &str, bool)> {
    let (into_exec, rest) = match name.strip_prefix("v_cmpx_") {
        Some(rest) => (true, rest),
        None => (false, name.strip_prefix("v_cmp_")?),
    };
    let (condition, kind) = rest.strip_suffix("_e32")?.rsplit_once('_')?;
    let signed = match kind {
        "i64" => true,
        "u64" => false,
        _ => return None,
    };
    matches!(condition, "eq" | "ne" | "lt" | "le" | "gt" | "ge")
        .then_some((into_exec, condition, signed))
}

/// One lane's sixty-four-bit source, low word first: a vector register pair, or anything a scalar
/// sixty-four-bit source may be ([`sixty_four_bit_source`]).
fn lane_pair<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    source: &Operand,
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    match source {
        Operand::Vector(register) => {
            if u32::from(*register) + 2 > VECTOR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a 64-bit source runs past the end of the vector register file",
                });
            }
            Ok((
                model.read_source(instruction, source, lane)?,
                model.read_source(instruction, &Operand::Vector(*register + 1), lane)?,
            ))
        }
        _ => sixty_four_bit_source(model, instruction, source),
    }
}

/// `v_cmp_<condition>_u64` and `_i64`, and their `cmpx` forms (VOPC): each lane compares two
/// register pairs as sixty-four-bit integers, unsigned or signed (AMD's published RDNA 2 instruction
/// set) - the high words decide where they differ, and the low words, unsigned, where they do not.
/// The answer goes to `vcc`, or narrows the execution mask; a lane that does not run answers zero.
/// Grand Theft Auto V's in-world compute programs test a pointer with `v_cmpx_ne_u64 0, v[0:1]`.
fn compare_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let Some((into_exec, condition, signed)) = compare_64_parts(name) else {
        return Err(refuse("not a sixty-four-bit compare"));
    };
    // A compare into `vcc` names it first; the two sources are last either way.
    let [.., first, second] = instruction.operands.as_slice() else {
        return Err(refuse("a sixty-four-bit compare does not have two sources"));
    };
    let ordered = if signed {
        (op::SLESS_THAN, op::SGREATER_THAN)
    } else {
        (op::ULESS_THAN, op::UGREATER_THAN)
    };
    let zero = model.constant(0);
    let mut halves = (zero, zero);
    for lane in 0..model.modelled_lanes() {
        if model.lane_known_idle(lane) {
            continue;
        }
        let (a_low, a_high) = lane_pair(model, instruction, first, lane)?;
        let (b_low, b_high) = lane_pair(model, instruction, second, lane)?;
        let high_equal = model.compare(op::IEQUAL, a_high, b_high);
        let low_equal = model.compare(op::IEQUAL, a_low, b_low);
        let equal = model.both(high_equal, low_equal);
        let mut ordering = [zero; 2];
        for (slot, (by_high, by_low)) in ordering
            .iter_mut()
            .zip([(ordered.0, op::ULESS_THAN), (ordered.1, op::UGREATER_THAN)])
        {
            let decided = model.compare(by_high, a_high, b_high);
            let tied = model.compare(by_low, a_low, b_low);
            let tied = model.both(high_equal, tied);
            *slot = model.either(decided, tied);
        }
        let [lesser, greater] = ordering;
        let (answer, negated) = match condition {
            "eq" => (equal, false),
            "ne" => (equal, true),
            "lt" => (lesser, false),
            "ge" => (lesser, true),
            "gt" => (greater, false),
            _ => (greater, true),
        };
        let answer = if negated {
            let bool_type = model.bool_type();
            let b = model.builder();
            let not = b.id();
            b.function(op::LOGICAL_NOT, &[bool_type.0, not.0, answer.0]);
            not
        } else {
            answer
        };
        halves = model.set_lane_bit(halves, lane, answer);
    }
    let (low, high) = running_bits(model, halves)?;
    write_wave_mask(
        model,
        if into_exec {
            EXEC_LOW_HALF
        } else {
            VCC_LOW_HALF
        },
        (low, high),
    )?;
    model.count();
    Ok(())
}

/// The SPIR-V opcode a comparison maps to, and whether its operands are floats; the two travel
/// together because reading a register as the wrong type is silent.
fn op_for_compare(instruction: &Instruction, name: &str) -> Result<(u16, bool), TranslateError> {
    compare_op(name).ok_or(TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "no translation for this comparison",
    })
}

/// The SPIR-V comparison a `v_cmp_<condition>_<type>` performs, in either encoding, and whether it
/// compares floats.
///
/// A float condition is ordered - false where either operand is a NaN - and its `n` form is the
/// negation, so unordered: `nlt` is not-less-than, true for a NaN. `neq` is the unordered
/// not-equal and `lg` the ordered one. The ordered and unordered tests (`o`, `u`) answer whether
/// neither, or either, operand is a NaN. The integer conditions take their signedness from the
/// type. The always-true and always-false tests (`t`, `f`) are not here.
fn compare_op(name: &str) -> Option<(u16, bool)> {
    let base = name
        .strip_suffix("_e32")
        .or_else(|| name.strip_suffix("_e64"))?;
    let (condition, kind) = base.strip_prefix("v_cmp_")?.rsplit_once('_')?;
    let opcode = match (kind, condition) {
        ("f32", "lt") => op::FORD_LESS_THAN,
        ("f32", "eq") => op::FORD_EQUAL,
        ("f32", "le") => op::FORD_LESS_THAN_EQUAL,
        ("f32", "gt") => op::FORD_GREATER_THAN,
        ("f32", "lg") => op::FORD_NOT_EQUAL,
        ("f32", "ge") => op::FORD_GREATER_THAN_EQUAL,
        ("f32", "nge") => op::FUNORD_LESS_THAN,
        ("f32", "nlg") => op::FUNORD_EQUAL,
        ("f32", "ngt") => op::FUNORD_LESS_THAN_EQUAL,
        ("f32", "nle") => op::FUNORD_GREATER_THAN,
        ("f32", "neq") => op::FUNORD_NOT_EQUAL,
        ("f32", "nlt") => op::FUNORD_GREATER_THAN_EQUAL,
        ("f32", "o") => op::ORDERED,
        ("f32", "u") => op::UNORDERED,
        ("i32" | "u32", "eq") => op::IEQUAL,
        ("i32" | "u32", "ne") => op::INOT_EQUAL,
        ("i32", "lt") => op::SLESS_THAN,
        ("i32", "le") => op::SLESS_THAN_EQUAL,
        ("i32", "gt") => op::SGREATER_THAN,
        ("i32", "ge") => op::SGREATER_THAN_EQUAL,
        ("u32", "lt") => op::ULESS_THAN,
        ("u32", "le") => op::ULESS_THAN_EQUAL,
        ("u32", "gt") => op::UGREATER_THAN,
        ("u32", "ge") => op::UGREATER_THAN_EQUAL,
        // A class test reads its value's bits and its mask's word as they are.
        ("f32", "class") => return Some((CLASS_TEST, false)),
        _ => return None,
    };
    Some((opcode, kind == "f32"))
}

/// The pseudo-opcode [`compare_op`] gives `v_cmp_class_f32`, which no one SPIR-V comparison is:
/// [`lane_compare`] answers it from the value's class (D911).
const CLASS_TEST: u16 = u16::MAX;

/// One lane's answer to a vector compare: `opcode` on its two sides, or for [`CLASS_TEST`] whether
/// the first side's class is one the second side's mask names.
fn lane_compare<M: Model + ?Sized>(model: &mut M, opcode: u16, left: Id, right: Id) -> Id {
    if opcode == CLASS_TEST {
        float_class_in(model, left, right)
    } else {
        model.compare(opcode, left, right)
    }
}

/// Whether the float whose bits are `bits` is of a class `mask` names (AMD's published RDNA
/// instruction set, `V_CMP_CLASS_F32`): bit 0 a signalling NaN, 1 a quiet one, 2 to 5 negative
/// infinity, normal, denormal and zero, 6 to 9 positive zero, denormal, normal and infinity. The
/// class's index is found from the bits, and the mask's bit there read.
fn float_class_in<M: Model + ?Sized>(model: &mut M, bits: Id, mask: Id) -> Id {
    let constant = |model: &mut M, value: u32| model.constant(value);
    let magnitude_mask = constant(model, 0x7FFF_FFFF);
    let magnitude = model.binary(op::BITWISE_AND, bits, magnitude_mask);
    let infinity = constant(model, 0x7F80_0000);
    let smallest_normal = constant(model, 0x0080_0000);
    let zero = constant(model, 0);
    // Positive zero, denormal, normal and infinity are 6 to 9; their negatives 5 down to 2.
    let (six, seven, eight, nine, eleven) = (
        constant(model, 6),
        constant(model, 7),
        constant(model, 8),
        constant(model, 9),
        constant(model, 11),
    );
    let is_infinity = model.compare(op::IEQUAL, magnitude, infinity);
    let is_normal = model.compare(op::UGREATER_THAN_EQUAL, magnitude, smallest_normal);
    let is_denormal = model.compare(op::INOT_EQUAL, magnitude, zero);
    let finite = model.select(is_denormal, seven, six);
    let finite = model.select(is_normal, eight, finite);
    let positive = model.select(is_infinity, nine, finite);
    let negated = model.binary(op::ISUB, eleven, positive);
    let sign_bit = constant(model, 0x8000_0000);
    let sign = model.binary(op::BITWISE_AND, bits, sign_bit);
    let is_negative = model.compare(op::INOT_EQUAL, sign, zero);
    let ordered = model.select(is_negative, negated, positive);
    // A NaN is 0, signalling, or 1, quiet: the mantissa's top bit.
    let is_nan = model.compare(op::UGREATER_THAN, magnitude, infinity);
    let quiet_bit = constant(model, 0x0040_0000);
    let quiet = model.binary(op::BITWISE_AND, bits, quiet_bit);
    let is_quiet = model.compare(op::INOT_EQUAL, quiet, zero);
    let one = constant(model, 1);
    let nan = model.select(is_quiet, one, zero);
    let class = model.select(is_nan, nan, ordered);
    let named = model.binary(op::SHIFT_RIGHT_LOGICAL, mask, class);
    let named = model.binary(op::BITWISE_AND, named, one);
    model.compare(op::INOT_EQUAL, named, zero)
}

/// Translates a sixty-four-bit scalar logical operation.
///
/// Both halves independently, since the operation is bitwise. The destination is often the
/// execution mask: `s_and_b64 exec, exec, s[n:n+1]` enters a conditional region.
fn scalar_logic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let opcode = op_for_logic(instruction, name)?;
    let (destination, first, second) = three_operands(instruction)?;

    let (first_low, first_high) = sixty_four_bit_source(model, instruction, first)?;
    let (second_low, second_high) = sixty_four_bit_source(model, instruction, second)?;

    // `s_andn2_b64` ands with the complement of the second operand; there is no single SPIR-V
    // opcode, so it is a complement then an and.
    // `s_orn2_b64` likewise ors with it.
    let (second_low, second_high) = if name == ANDN2 || name == "s_orn2_b64" {
        (model.not(second_low), model.not(second_high))
    } else {
        (second_low, second_high)
    };

    let low = model.binary(opcode, first_low, second_low);
    let high = model.binary(opcode, first_high, second_high);
    // `s_nor_b64` is the complement of the or, `s_nand_b64` of the and.
    let (low, high) = if name == "s_nor_b64" || name == "s_nand_b64" {
        (model.not(low), model.not(high))
    } else {
        (low, high)
    };

    // These set the condition code to whether the result is non-zero, as the instruction set
    // documents: `s_and_b64 exec, exec, vcc` then a branch on the code skips a block once no lane
    // survives.
    write_logic_result(model, instruction, destination, (low, high))
}

/// `s_not_b64`: the complement of a 64-bit source, both halves.
fn scalar_not_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [destination, source] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a 64-bit complement decodes to a destination and a source; this one did not",
        });
    };
    let (low, high) = sixty_four_bit_source(model, instruction, source)?;
    let (low, high) = (model.not(low), model.not(high));
    write_logic_result(model, instruction, destination, (low, high))
}

/// A 64-bit scalar logic result written to `destination` - a register pair or a lane mask - with
/// the condition code set to whether it is non-zero, as the instruction set documents for the
/// whole family: `s_and_b64 exec, exec, vcc` then a branch on the code skips a block once no lane
/// survives.
fn write_logic_result<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    destination: &Operand,
    (low, high): (Id, Id),
) -> Result<(), TranslateError> {
    let either = model.binary(op::BITWISE_OR, low, high);
    let non_zero = model.is_not_zero(either);
    model.set_condition_code(non_zero);

    if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_name(name)
    {
        model.write_lane_mask(mask, low, high)?;
        model.count();
        return Ok(());
    }

    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a 64-bit scalar logical destination is neither a register pair nor ",
                "the execution mask"
            ),
        });
    };
    let register = u32::from(*register);
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a 64-bit scalar logical result runs past the end of the register file",
        });
    }
    model.write_scalar(register, low);
    model.write_scalar(register + 1, high);
    model.count();
    Ok(())
}

/// `s_andn2_b64`, whose second operand is complemented.
const ANDN2: &str = "s_andn2_b64";

/// The SPIR-V opcode a 64-bit scalar logical instruction maps to.
fn op_for_logic(instruction: &Instruction, name: &str) -> Result<u16, TranslateError> {
    match name {
        "s_and_b64" | ANDN2 | "s_nand_b64" => Ok(op::BITWISE_AND),
        "s_or_b64" | "s_nor_b64" | "s_orn2_b64" => Ok(op::BITWISE_OR),
        "s_xor_b64" => Ok(op::BITWISE_XOR),
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this scalar logical instruction",
        }),
    }
}

/// `s_bfe_u64 d, s, field`: `(s >> field[5:0]) & ((1 << field[22:16]) - 1)` across a pair (AMD's
/// published RDNA instruction set, `S_BFE_U64`), the condition code whether it is non-zero. A width
/// of sixty-four or more keeps every bit shifted down. The destination may be a lane mask: ACO
/// makes the mask of a wave's first lanes as `s_bfe_u64 exec, -1, count`.
fn scalar_bit_field_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source, field) = three_operands(instruction)?;
    let mask = match destination {
        Operand::Named(name) => lane_mask_name(name),
        _ => None,
    };
    let register = match mask {
        Some(_) => 0,
        None => scalar_destination(instruction, destination)?,
    };
    if mask.is_none() && register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_bfe_u64 runs past the end of the register file",
        });
    }
    let (low, high) = pair_source(model, instruction, source, 0)?;
    let field = model.read_source(instruction, field, 0)?;
    let six_bits = model.constant(63);
    let offset = model.binary(op::BITWISE_AND, field, six_bits);
    let sixteen = model.constant(16);
    let seven_bits = model.constant(0x7f);
    let width = model.binary(op::SHIFT_RIGHT_LOGICAL, field, sixteen);
    let width = model.binary(op::BITWISE_AND, width, seven_bits);
    let sixty_four = model.constant(64);
    let wide = model.compare(op::UGREATER_THAN_EQUAL, width, sixty_four);
    let width = model.select(wide, sixty_four, width);
    let (low, high) = shift_pair_right(model, (low, high), offset);
    let zero = model.constant(0);
    let (keep_low, keep_high) = ones_between(model, zero, width);
    let low = model.binary(op::BITWISE_AND, low, keep_low);
    let high = model.binary(op::BITWISE_AND, high, keep_high);
    let either = model.binary(op::BITWISE_OR, low, high);
    let non_zero = model.is_not_zero(either);
    if let Some(mask) = mask {
        model.write_lane_mask(mask, low, high)?;
    } else {
        model.write_scalar(register, low);
        model.write_scalar(register + 1, high);
    }
    model.set_condition_code(non_zero);
    model.count();
    Ok(())
}

/// `s_lshl_b64 d, s, amount` and `s_lshr_b64 d, s, amount`: a pair shifted by `amount[5:0]` (AMD's
/// published RDNA instruction set), the condition code whether the result is non-zero. The
/// destination may be a lane mask: PPSA03416's vertex shader narrows its execution mask as
/// `s_lshr_b64 exec, -1, vcc_lo`.
fn scalar_shift_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, source, amount) = three_operands(instruction)?;
    let mask = match destination {
        Operand::Named(named) => lane_mask_name(named),
        _ => None,
    };
    let register = match mask {
        Some(_) => 0,
        None => scalar_destination(instruction, destination)?,
    };
    if mask.is_none() && register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a 64-bit shift runs past the end of the register file",
        });
    }
    let pair = pair_source(model, instruction, source, 0)?;
    let amount = model.read_source(instruction, amount, 0)?;
    let six_bits = model.constant(63);
    let amount = model.binary(op::BITWISE_AND, amount, six_bits);
    let (low, high) = if name == "s_lshl_b64" {
        shift_pair_left(model, pair, amount)
    } else {
        shift_pair_right(model, pair, amount)
    };
    let either = model.binary(op::BITWISE_OR, low, high);
    let non_zero = model.is_not_zero(either);
    if let Some(mask) = mask {
        model.write_lane_mask(mask, low, high)?;
    } else {
        model.write_scalar(register, low);
        model.write_scalar(register + 1, high);
    }
    model.set_condition_code(non_zero);
    model.count();
    Ok(())
}

/// A pair shifted left by `amount` (below sixty-four), in halves, the mirror of
/// [`shift_pair_right`].
fn shift_pair_left<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
    amount: Id,
) -> (Id, Id) {
    let zero = model.constant(0);
    let thirty_one = model.constant(31);
    let thirty_two = model.constant(32);
    let within = model.binary(op::BITWISE_AND, amount, thirty_one);
    let back = model.binary(op::ISUB, thirty_two, within);
    let back = model.binary(op::BITWISE_AND, back, thirty_one);
    let carried = model.binary(op::SHIFT_RIGHT_LOGICAL, low, back);
    let none = model.compare(op::IEQUAL, within, zero);
    let carried = model.select(none, zero, carried);
    let shifted_high = model.binary(op::SHIFT_LEFT_LOGICAL, high, within);
    let short_high = model.binary(op::BITWISE_OR, shifted_high, carried);
    let short_low = model.binary(op::SHIFT_LEFT_LOGICAL, low, within);
    let far = model.compare(op::UGREATER_THAN_EQUAL, amount, thirty_two);
    let low = model.select(far, zero, short_low);
    let high = model.select(far, short_low, short_high);
    (low, high)
}

/// A pair shifted right by `amount` (below sixty-four), logically, in halves: SPIR-V leaves a shift
/// by a word's width or more undefined, so zero, below thirty-two and above are selected apart.
fn shift_pair_right<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
    amount: Id,
) -> (Id, Id) {
    let zero = model.constant(0);
    let thirty_one = model.constant(31);
    let thirty_two = model.constant(32);
    let within = model.binary(op::BITWISE_AND, amount, thirty_one);
    let back = model.binary(op::ISUB, thirty_two, within);
    let back = model.binary(op::BITWISE_AND, back, thirty_one);
    let carried = model.binary(op::SHIFT_LEFT_LOGICAL, high, back);
    let none = model.compare(op::IEQUAL, within, zero);
    let carried = model.select(none, zero, carried);
    let shifted_low = model.binary(op::SHIFT_RIGHT_LOGICAL, low, within);
    let short_low = model.binary(op::BITWISE_OR, shifted_low, carried);
    let short_high = model.binary(op::SHIFT_RIGHT_LOGICAL, high, within);
    let long_low = short_high;
    let far = model.compare(op::UGREATER_THAN_EQUAL, amount, thirty_two);
    let low = model.select(far, long_low, short_low);
    let high = model.select(far, zero, short_high);
    (low, high)
}

/// `s_bitcmp1_b32 s, bit`: the condition code is bit `bit[4:0]` of `s` (`S_BITCMP1_B32`); with
/// `set_wanted` false, `s_bitcmp0_b32`, whether that bit is clear (`S_BITCMP0_B32`, AMD's published
/// RDNA 2 instruction set, SOPC).
fn scalar_bit_test<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    set_wanted: bool,
) -> Result<(), TranslateError> {
    let [value, bit] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar bit test does not have two sources",
        });
    };
    let value = model.read_source(instruction, value, 0)?;
    let bit = model.read_source(instruction, bit, 0)?;
    let five = model.constant(31);
    let bit = model.binary(op::BITWISE_AND, bit, five);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, bit);
    let one = model.constant(1);
    let set = model.binary(op::BITWISE_AND, shifted, one);
    let answer = if set_wanted {
        set
    } else {
        model.binary(op::BITWISE_XOR, set, one)
    };
    let answer = model.is_not_zero(answer);
    model.set_condition_code(answer);
    model.count();
    Ok(())
}

/// `s_and_saveexec_b64 d, s`: `d` takes the execution mask, the mask becomes `s & exec`, and the
/// condition code says whether any lane remains (`S_AND_SAVEEXEC_B64`) - how a branch enters a
/// conditional region and keeps the mask to restore. `d` may be a register pair or a lane mask, as
/// `s_and_saveexec_b64 vcc, vcc` keeps the old mask in the condition mask.
fn and_save_exec<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    combine: SaveExec,
) -> Result<(), TranslateError> {
    let [destination, source] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_and_saveexec_b64 does not have a destination and a source",
        });
    };
    let saved_mask = mask_destination(destination);
    let register = match saved_mask {
        Some(_) => 0,
        None => scalar_destination(instruction, destination)?,
    };
    if saved_mask.is_none() && register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_and_saveexec_b64 runs past the end of the register file",
        });
    }
    let (source_low, source_high) = sixty_four_bit_source(model, instruction, source)?;
    // `s_andn1_saveexec_b64` inverts its source (GFX11's `s_and_not0_saveexec`,
    // `aco_opcodes.py:656`).
    let all = model.constant(u32::MAX);
    let (source_low, source_high) = if combine == SaveExec::AndNotSource {
        (
            model.binary(op::BITWISE_XOR, source_low, all),
            model.binary(op::BITWISE_XOR, source_high, all),
        )
    } else {
        (source_low, source_high)
    };
    let (exec_low, exec_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    // `s_orn2_saveexec_b64`: the source or the old mask's complement (`S_ORN2_SAVEEXEC_B64`, AMD's
    // published RDNA instruction set).
    let (low, high) = if combine == SaveExec::OrNotExec {
        let not_low = model.binary(op::BITWISE_XOR, exec_low, all);
        let not_high = model.binary(op::BITWISE_XOR, exec_high, all);
        (
            model.binary(op::BITWISE_OR, source_low, not_low),
            model.binary(op::BITWISE_OR, source_high, not_high),
        )
    } else {
        (
            model.binary(op::BITWISE_AND, source_low, exec_low),
            model.binary(op::BITWISE_AND, source_high, exec_high),
        )
    };
    if let Some(mask) = saved_mask {
        model.write_lane_mask(mask, exec_low, exec_high)?;
    } else {
        model.write_scalar(register, exec_low);
        model.write_scalar(register + 1, exec_high);
    }
    model.write_lane_mask(EXEC_LOW_HALF, low, high)?;
    let either = model.binary(op::BITWISE_OR, low, high);
    let non_zero = model.is_not_zero(either);
    model.set_condition_code(non_zero);
    model.count();
    Ok(())
}

/// `s_and_saveexec_b32 d, s`: the 32-lane form of [`and_save_exec`] - `d` takes `exec_lo`, which
/// becomes `s & exec_lo`, and the condition code says whether any lane remains
/// (`S_AND_SAVEEXEC_B32`). A 32-lane wave's mask is `exec_lo` alone; `exec_hi` is left as it is.
/// `s_andn1_saveexec_b32` ands the source's complement instead, and `s_orn2_saveexec_b32` ors the
/// source with the old mask's complement, as their 64-bit forms do.
fn and_save_exec_32<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    combine: SaveExec,
) -> Result<(), TranslateError> {
    let [destination, source] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a 32-bit saving mask instruction does not have a destination and a source",
        });
    };
    let source = model.read_source(instruction, source, 0)?;
    let (exec_low, exec_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    let all = model.constant(u32::MAX);
    let low = match combine {
        SaveExec::And => model.binary(op::BITWISE_AND, source, exec_low),
        SaveExec::AndNotSource => {
            let inverted = model.binary(op::BITWISE_XOR, source, all);
            model.binary(op::BITWISE_AND, inverted, exec_low)
        }
        SaveExec::OrNotExec => {
            let inverted = model.binary(op::BITWISE_XOR, exec_low, all);
            model.binary(op::BITWISE_OR, source, inverted)
        }
    };
    // Any scalar word, `vcc_hi` among them in a wave of 32.
    write_scalar_operand(model, instruction, destination, exec_low)?;
    model.write_lane_mask(EXEC_LOW_HALF, low, exec_high)?;
    let non_zero = model.is_not_zero(low);
    model.set_condition_code(non_zero);
    model.count();
    Ok(())
}

/// `s_ff1_i32_b32 d, s` and `s_ff1_i32_b64 d, s`: the index of the source's lowest set bit, a pair's
/// counted from its low half, and `0xffffffff` where none is set (`S_FF1_I32_B32`, `S_FF1_I32_B64`,
/// AMD's published RDNA instruction set). No condition code. Grand Theft Auto V's compute programs
/// take `s_ff1_i32_b64 vcc_lo, exec_lo` for the first running lane.
fn scalar_first_one<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let (low, high) = if name == "s_ff1_i32_b64" {
        sixty_four_bit_source(model, instruction, source)?
    } else {
        let low = model.read_source(instruction, source, 0)?;
        (low, model.constant(0))
    };
    let zero = model.constant(0);
    let (in_low, in_high) = (trailing_zeros(model, low), trailing_zeros(model, high));
    let thirty_two = model.constant(32);
    let in_high = model.add(in_high, thirty_two);
    let low_clear = model.compare(op::IEQUAL, low, zero);
    let index = model.select(low_clear, in_high, in_low);
    let either = model.binary(op::BITWISE_OR, low, high);
    let none = model.compare(op::IEQUAL, either, zero);
    let all = model.constant(u32::MAX);
    let index = model.select(none, all, index);
    write_scalar_operand(model, instruction, destination, index)?;
    model.count();
    Ok(())
}

/// How many of `value`'s low bits are clear below its lowest set one, counted by halving; 32 for
/// zero.
fn trailing_zeros<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let zero = model.constant(0);
    let mut count = zero;
    let mut rest = value;
    for step in [16, 8, 4, 2, 1] {
        let mask = model.constant((1 << step) - 1);
        let low = model.binary(op::BITWISE_AND, rest, mask);
        let clear = model.compare(op::IEQUAL, low, zero);
        let by = model.constant(step);
        let more = model.add(count, by);
        count = model.select(clear, more, count);
        let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, rest, by);
        rest = model.select(clear, shifted, rest);
    }
    // Five halvings reach the last bit; a value with none set counts one more.
    let one = model.constant(1);
    let last = model.binary(op::BITWISE_AND, rest, one);
    let none = model.compare(op::IEQUAL, last, zero);
    let more = model.add(count, one);
    model.select(none, more, count)
}

/// `s_bcnt1_i32_b64 d, s`: how many bits of the pair are set, and the condition code whether any
/// are (`S_BCNT1_I32_B64`).
fn scalar_bit_count_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let (low, high) = sixty_four_bit_source(model, instruction, source)?;
    let low = model.bit_count(low);
    let high = model.bit_count(high);
    let count = model.add(low, high);
    let non_zero = model.is_not_zero(count);
    write_scalar_or_mask_half(model, instruction, destination, count)?;
    model.set_condition_code(non_zero);
    model.count();
    Ok(())
}

/// Translates a scalar move, both widths together because `s_mov_b64` is not two `s_mov_b32`s.
fn scalar_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // s_mov_b64: two consecutive registers from two consecutive sources.
        //
        // A constant source is extended to sixty-four bits, not repeated: `s_mov_b64 s[0:1], -1`
        // sets both halves to all ones and `s_mov_b64 s[0:1], 1` sets s0 to one and s1 to zero.
        // This is how the execution mask is set.
        "s_mov_b64" => {
            let (destination, source) = two_operands(instruction)?;
            let (low, high) = sixty_four_bit_source(model, instruction, source)?;

            // `s_mov_b64 exec, ...` turns lanes off; the destination decodes as the mask's low
            // half, as a sixty-four-bit operand names its pair.
            if let Operand::Named(register) = destination
                && let Some(mask) = lane_mask_name(register)
            {
                model.write_lane_mask(mask, low, high)?;
                model.count();
                return Ok(());
            }

            let Operand::Scalar(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "s_mov_b64 destination is neither a scalar register nor the ",
                        "execution mask"
                    ),
                });
            };
            let register = u32::from(*register);
            if register + 2 > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "s_mov_b64 runs past the end of the register file",
                });
            }

            model.write_scalar(register, low);
            model.write_scalar(register + 1, high);
            if let Operand::Scalar(from) = source {
                let from = u32::from(*from);
                model.note_scalar_copy(register, from);
                model.note_scalar_copy(register + 1, from + 1);
            }
            model.count();
            Ok(())
        }

        "s_mov_b32" => {
            let (destination, source) = two_operands(instruction)?;
            let value = model.read_source(instruction, source, 0)?;

            // `s_mov_b32 exec_lo, ...` sets a 32-lane shader's execution mask, as `s_mov_b64 exec,
            // ...` does for a 64-lane one.
            if let Some(mask) = mask_destination(destination) {
                write_mask_low(model, mask, value)?;
                model.count();
                return Ok(());
            }
            // And `vcc_hi` or `exec_hi`, a mask's high half used as an ordinary scalar.
            if let Operand::Named(name) = destination
                && let Some(mask) = lane_mask_high_name(name)
            {
                write_mask_high(model, mask, value)?;
                model.count();
                return Ok(());
            }

            // `s_mov_b32 m0, sN` is how a pixel shader hands the interpolator its primitive mask
            // (orbistoun-gpu `tests/oracle_gl_cube.rs`).
            if let Operand::Named(name) = destination
                && name == M0
            {
                model.write_m0(value);
                model.count();
                return Ok(());
            }

            let Operand::Scalar(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: concat!(
                        "s_mov_b32 destination is neither a scalar register nor a ",
                        "lane mask"
                    ),
                });
            };
            model.write_scalar(u32::from(*register), value);
            if let Operand::Scalar(from) = source {
                model.note_scalar_copy(u32::from(*register), u32::from(*from));
            }
            model.count();
            Ok(())
        }

        // v_mov_b32: per lane, masked.
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this scalar move",
        }),
    }
}

/// `v_mov_b32`: each running lane's source, into the destination; a copy of a scalar register is
/// noted, so a level moved out of the user data is still found there.
fn vector_move<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_mov_b32 destination is not a vector register",
        });
    };
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, source, lane)?;
        model.write_vector_lane(u32::from(*register), lane, value);
    }
    if let Operand::Scalar(from) = source {
        model.note_vector_copy(u32::from(*register), u32::from(*from));
    }
    model.count();
    Ok(())
}

/// Translates a local-data-share access.
///
/// The address is a byte address in a vector register plus a byte offset in the instruction, which
/// the reference omits when it is zero. Reads are unmasked and writes masked, as for guest memory:
/// an inactive lane must not write what an active one will read.
fn local_share<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (first, second, offset) = three_operands(instruction)?;
    let Operand::Immediate(offset) = offset else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a local-data-share access carries no byte offset - the operand ",
                "layout for this opcode is missing one"
            ),
        });
    };
    let offset = u32::try_from(*offset).map_err(|_| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a negative local-data-share offset",
    })?;

    let reading = name == DS_READ;
    // A read names its destination then its address; a write names its address then its data. The
    // layouts differ.
    let (destination, address) = if reading {
        (first, second)
    } else {
        (second, first)
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a local-data-share operand is not a vector register",
        });
    };
    let register = u32::from(*register);

    // The GDS bit sends the access to the global data share (D789).
    let global = instruction.word & DS_GDS != 0;
    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        let byte_offset = model.constant(offset);
        let byte_address = model.add(base, byte_offset);
        if global {
            let in_range = within_global_share(model, byte_address);
            if reading {
                let value = model.read_global_share(instruction, (byte_address, in_range))?;
                model.write_vector_lane(register, lane, value);
            } else {
                let value = model.read_source(instruction, destination, lane)?;
                model.write_global_share(instruction, (byte_address, in_range), value, lane)?;
            }
            continue;
        }
        let index = model.local_word_index(byte_address);

        if reading {
            let value = model.read_local(index)?;
            model.write_vector_lane(register, lane, value);
        } else {
            let value = model.read_source(instruction, destination, lane)?;
            model.write_local(index, value, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// `ds_read_b32`.
const DS_READ: &str = "ds_read_b32";

/// `ds_write_addtid_b32 data offset:n` and `ds_read_addtid_b32 d offset:n`: a lane's own word of the
/// local share, at the offset plus `M0[15:0]` plus four bytes for each lane below it in the wave
/// (AMD's published RDNA 2 instruction set, DS). The write stores the data register; the read loads
/// the destination. The address names no register.
fn local_share_addtid<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [data @ Operand::Vector(register), Operand::Immediate(offset)] =
        instruction.operands.as_slice()
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a lane-addressed local-share access is not a register and an offset",
        });
    };
    let offset = u32::try_from(*offset).map_err(|_| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a negative local-data-share offset",
    })?;
    let register = u32::from(*register);
    let reading = name == "ds_read_addtid_b32";
    let m0 = model.read_m0();
    let low_half = model.constant(0xffff);
    let base = model.binary(op::BITWISE_AND, m0, low_half);
    let offset = model.constant(offset);
    let base = model.add(base, offset);
    let four = model.constant(4);
    for lane in running_lanes(model) {
        // The lane in the wave: the invocation's own where each is one lane (D760).
        let id = match model.lane_number(lane) {
            Some(id) => id,
            None => model.constant(lane),
        };
        let step = model.binary(op::IMUL, id, four);
        let byte_address = model.add(base, step);
        let index = model.local_word_index(byte_address);
        if reading {
            let value = model.read_local(index)?;
            model.write_vector_lane(register, lane, value);
        } else {
            let value = model.read_source(instruction, data, lane)?;
            model.write_local(index, value, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// `ds_read_b96 d, address offset`: `words` consecutive words from the address plus the byte
/// offset into `d` and the registers after it (AMD's published RDNA instruction set,
/// `DS_READ_B96`). The local share only: a global-share form is refused before it gets here.
fn local_share_words<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    words: u32,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let (destination, address, offset) = three_operands(instruction)?;
    let (Operand::Vector(register), Operand::Immediate(offset)) = (destination, offset) else {
        return Err(refuse(
            "a multi-word local-data-share read did not decode to a register and a byte offset",
        ));
    };
    let offset =
        u32::try_from(*offset).map_err(|_| refuse("a negative local-data-share offset"))?;
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        for word in 0..words {
            let byte_offset = model.constant(offset + 4 * word);
            let byte_address = model.add(base, byte_offset);
            let index = model.local_word_index(byte_address);
            let value = model.read_local(index)?;
            model.write_vector_lane(register + word, lane, value);
        }
    }
    model.count();
    Ok(())
}

/// `ds_min_u32 address, d offset`, `ds_max_u32`, `ds_add_u32` and `ds_add_rtn_u32 r, address, d
/// offset`: each running lane's `d` folded into the word at the address plus the byte offset - its
/// unsigned minimum, maximum or sum - as one indivisible step, `r` taking what the word held before
/// (AMD's published RDNA instruction set, `DS_MIN_U32`, `DS_MAX_U32`, `DS_ADD_U32`,
/// `DS_ADD_RTN_U32`). The local share only: a global-share form is refused before it gets here.
fn local_share_atomic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let returning = name == "ds_add_rtn_u32";
    let (returned, address, data, offset) = match instruction.operands.as_slice() {
        [Operand::Vector(returned), address, data, offset] if returning => {
            (Some(u32::from(*returned)), address, data, offset)
        }
        [address, data, offset] if !returning => (None, address, data, offset),
        _ => {
            return Err(refuse(
                "a local-data-share atomic did not decode to its operands",
            ));
        }
    };
    let Operand::Immediate(offset) = offset else {
        return Err(refuse("a local-data-share atomic carries no byte offset"));
    };
    let offset =
        u32::try_from(*offset).map_err(|_| refuse("a negative local-data-share offset"))?;
    let opcode = match name {
        "ds_min_u32" => op::ATOMIC_U_MIN,
        "ds_max_u32" => op::ATOMIC_U_MAX,
        _ => op::ATOMIC_I_ADD,
    };
    let float = match name {
        "ds_min_f32" => Some(false),
        "ds_max_f32" => Some(true),
        _ => None,
    };
    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        let byte_offset = model.constant(offset);
        let byte_address = model.add(base, byte_offset);
        let index = model.local_word_index(byte_address);
        let value = model.read_source(instruction, data, lane)?;
        // A float minimum or maximum, as two integer atomics split by the value's sign (D919);
        // where one invocation runs the wave, folded in order as floats.
        if let Some(maximum) = float {
            let [(first, kept), (second, other)] = float_atomic_parts(model, value, maximum);
            if model.atomic_local(first, index, kept, lane).is_some() {
                model.atomic_local(second, index, other, lane);
            } else {
                fold_float_in_order(model, index, (value, maximum), lane)?;
            }
            continue;
        }
        let before = if let Some(before) = model.atomic_local(opcode, index, value, lane) {
            before
        } else {
            fold_in_order(model, (opcode, index), value, lane)?
        };
        if let Some(returned) = returned {
            model.write_vector_lane(returned, lane, before);
        }
    }
    model.count();
    Ok(())
}

/// A local-data-share access, by its name: an atomic, or a read or write of one to four words.
fn local_share_instruction<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        "ds_min_u32" | "ds_max_u32" | "ds_add_rtn_u32" | "ds_add_u32" | "ds_min_f32"
        | "ds_max_f32" => local_share_atomic(model, instruction, name),
        "ds_write_b32" | "ds_read_b32" => local_share(model, instruction, name),
        "ds_read_b64" => local_share_words(model, instruction, 2),
        "ds_read_b128" => local_share_words(model, instruction, 4),
        "ds_write_b64" => local_share_write_words(model, instruction, 2),
        "ds_read_b96" => local_share_words(model, instruction, 3),
        "ds_write_b96" => local_share_write_words(model, instruction, 3),
        "ds_write_b128" => local_share_write_words(model, instruction, 4),
        "ds_write2_b32" | "ds_read2_b32" | "ds_write2_b64" | "ds_read2_b64" => {
            local_share_pair(model, instruction, name)
        }
        _ => local_share_merge(model, instruction, name),
    }
}

/// The identity of an atomic `opcode`: what folded in changes nothing.
#[must_use]
pub const fn atomic_identity(opcode: u16) -> u32 {
    match opcode {
        op::ATOMIC_U_MIN | op::ATOMIC_AND => u32::MAX,
        op::ATOMIC_S_MIN => 0x7FFF_FFFF,
        op::ATOMIC_S_MAX => 0x8000_0000,
        _ => 0,
    }
}

/// A float minimum (or, where `maximum`, maximum) of a word and `value`, as two integer atomics
/// split by `value`'s sign (D919): a value whose sign bit is clear orders against any float as its
/// bits do as a signed integer, so the signed minimum keeps the smaller; one whose sign bit is set
/// orders against any float reversed as an unsigned integer, so the unsigned maximum keeps the
/// smaller. Each lane gives the atomic its sign does not choose that atomic's identity. A NaN and
/// the zeros' order are not modelled.
fn float_atomic_parts<M: Model + ?Sized>(
    model: &mut M,
    value: Id,
    maximum: bool,
) -> [(u16, Id); 2] {
    let sign_bit = model.constant(0x8000_0000);
    let sign = model.binary(op::BITWISE_AND, value, sign_bit);
    let zero = model.constant(0);
    let positive = model.compare(op::IEQUAL, sign, zero);
    let (by_sign, reversed) = if maximum {
        (op::ATOMIC_S_MAX, op::ATOMIC_U_MIN)
    } else {
        (op::ATOMIC_S_MIN, op::ATOMIC_U_MAX)
    };
    let by_sign_identity = model.constant(atomic_identity(by_sign));
    let reversed_identity = model.constant(atomic_identity(reversed));
    let first = model.select(positive, value, by_sign_identity);
    let second = model.select(positive, reversed_identity, value);
    [(by_sign, first), (reversed, second)]
}

/// [`local_share_atomic`]'s float step where one invocation runs the wave: the word read, the float
/// minimum (or maximum) of it and `value` written back, lane after lane.
fn fold_float_in_order<M: Model + ?Sized>(
    model: &mut M,
    index: Id,
    (value, maximum): (Id, bool),
    lane: u32,
) -> Result<(), TranslateError> {
    let before = model.read_local(index)?;
    let (new, old) = (model.as_float(value), model.as_float(before));
    let less = model.compare(op::FORD_LESS_THAN, new, old);
    let after = if maximum {
        model.select(less, before, value)
    } else {
        model.select(less, value, before)
    };
    model.write_local(index, after, lane)
}

/// [`local_share_atomic`]'s step where one invocation runs the wave: the word read, `value` folded in
/// by `opcode` - the unsigned minimum, maximum or sum - and written back, lane after lane. Answers
/// what the word held before.
fn fold_in_order<M: Model + ?Sized>(
    model: &mut M,
    (opcode, index): (u16, Id),
    value: Id,
    lane: u32,
) -> Result<Id, TranslateError> {
    let before = model.read_local(index)?;
    let after = if opcode == op::ATOMIC_I_ADD {
        model.add(before, value)
    } else {
        let less = model.compare(op::ULESS_THAN, value, before);
        if opcode == op::ATOMIC_U_MIN {
            model.select(less, value, before)
        } else {
            model.select(less, before, value)
        }
    };
    model.write_local(index, after, lane)?;
    Ok(before)
}

/// `ds_write_b96 address, d offset` and `ds_write_b128`: `words` consecutive words from `d` and the
/// registers after it to the address plus the byte offset (AMD's published RDNA instruction set,
/// `DS_WRITE_B96`, `DS_WRITE_B128`). The local share only: a global-share form is refused before it
/// gets here.
fn local_share_write_words<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    words: u32,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let (address, data, offset) = three_operands(instruction)?;
    let (Operand::Vector(register), Operand::Immediate(offset)) = (data, offset) else {
        return Err(refuse(
            "a multi-word local-data-share write did not decode to a register and a byte offset",
        ));
    };
    let offset =
        u32::try_from(*offset).map_err(|_| refuse("a negative local-data-share offset"))?;
    let register = *register;
    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        for word in 0..words {
            let byte_offset = model.constant(offset + 4 * word);
            let byte_address = model.add(base, byte_offset);
            let index = model.local_word_index(byte_address);
            let value = model.read_source(
                instruction,
                &Operand::Vector(register + u16::try_from(word).unwrap_or(0)),
                lane,
            )?;
            model.write_local(index, value, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// Whether a global-data-share word at `byte_address` lies within the share `m0` declares: for a
/// GDS access `m0` is the share's size in bytes, as LLVM sets it from `amdgpu-gds-size` (D789). A
/// word reaching past it is dropped by a store and reads zero.
fn within_global_share<M: Model + ?Sized>(model: &mut M, byte_address: Id) -> Id {
    let size = model.read_m0();
    let four = model.constant(4);
    let end = model.add(byte_address, four);
    // An address within four bytes of the top wraps `end` below it, and is out of range.
    let no_wrap = model.compare(op::UGREATER_THAN_EQUAL, end, byte_address);
    let fits = model.compare(op::UGREATER_THAN_EQUAL, size, end);
    model.both(no_wrap, fits)
}

/// `ds_write2_b32 address, a, b offset0 offset1` and `ds_read2_b32 d, address offset0 offset1`: two
/// words at the address plus each eight-bit offset in words (AMD's published RDNA instruction set,
/// `DS_WRITE2_B32`, `DS_READ2_B32`); a read fills `d` and the register after it. The `_b64` forms
/// move two words at each, their offsets counted in eight bytes (`DS_WRITE2_B64`, `DS_READ2_B64`):
/// a write takes each value from a register and the one after it, and a read fills four registers
/// from `d`.
fn local_share_pair<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    // Words an element holds, so its offsets' unit is that many words.
    let words: u32 = if name.ends_with("_b64") { 2 } else { 1 };
    let element_offsets = |first: &Operand, second: &Operand| match (first, second) {
        (Operand::Immediate(a), Operand::Immediate(b)) => u32::try_from(*a)
            .ok()
            .zip(u32::try_from(*b).ok())
            .map(|(a, b)| [a * 4 * words, b * 4 * words]),
        _ => None,
    };
    let reading = name.starts_with("ds_read2_");
    let (address, offsets, registers) = match instruction.operands.as_slice() {
        [Operand::Vector(destination), address, first, second] if reading => (
            address,
            element_offsets(first, second),
            [u32::from(*destination), u32::from(*destination) + words],
        ),
        [
            address,
            Operand::Vector(a),
            Operand::Vector(b),
            first,
            second,
        ] if !reading => (
            address,
            element_offsets(first, second),
            [u32::from(*a), u32::from(*b)],
        ),
        _ => {
            return Err(refuse(
                "a pair local-data-share access did not decode to its layout",
            ));
        }
    };
    let offsets = offsets.ok_or_else(|| refuse("a pair access's offsets are not immediates"))?;
    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        for (offset, first) in offsets.into_iter().zip(registers) {
            for word in 0..words {
                let (offset, register) = (offset + 4 * word, first + word);
                let offset = model.constant(offset);
                let byte_address = model.add(base, offset);
                let index = model.local_word_index(byte_address);
                if reading {
                    let value = model.read_local(index)?;
                    model.write_vector_lane(register, lane, value);
                } else {
                    let value =
                        model.read_source(instruction, &Operand::Vector(register as u16), lane)?;
                    model.write_local(index, value, lane)?;
                }
            }
        }
    }
    model.count();
    Ok(())
}

/// A local-data-share instruction's GDS bit, 17 of its first word on GFX10 (`aco_assembler.cpp:513`).
pub(crate) const DS_GDS: u32 = 1 << 17;

/// The local-data-share accesses that merge into a word: `ds_write_b8 address, data offset` stores
/// the data's low byte at its byte, `ds_read_u8 d, address offset` reads one byte zero-extended,
/// and `ds_or_b32 address, data offset` ors the data into the word (AMD's published RDNA
/// instruction set). A wave's lanes run in order here, so a lane's merge sees the ones before it,
/// as the share serialises them.
fn local_share_merge<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (first, second, offset) = three_operands(instruction)?;
    let Operand::Immediate(offset) = offset else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a local-data-share access carries no byte offset",
        });
    };
    let offset = u32::try_from(*offset).map_err(|_| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a negative local-data-share offset",
    })?;
    let reading = name == "ds_read_u8";
    let (destination, address) = if reading {
        (first, second)
    } else {
        (second, first)
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a local-data-share operand is not a vector register",
        });
    };
    let register = u32::from(*register);
    let byte = model.constant(0xff);
    let three = model.constant(3);
    let eight = model.constant(8);
    for lane in running_lanes(model) {
        let base = model.read_source(instruction, address, lane)?;
        let offset = model.constant(offset);
        let byte_address = model.add(base, offset);
        let index = model.local_word_index(byte_address);
        let within = model.binary(op::BITWISE_AND, byte_address, three);
        let shift = model.binary(op::IMUL, within, eight);
        // One invocation per lane (D760): a write merges indivisibly, keeping other lanes' bytes.
        if name != "ds_read_u8" && model.lane_number(lane).is_some() {
            let data = model.read_source(instruction, destination, lane)?;
            let (keep, set) = if name == "ds_write_b8" {
                let data = model.binary(op::BITWISE_AND, data, byte);
                let placed = model.binary(op::SHIFT_LEFT_LOGICAL, data, shift);
                let hole = model.binary(op::SHIFT_LEFT_LOGICAL, byte, shift);
                (model.not(hole), placed)
            } else {
                (model.constant(u32::MAX), data)
            };
            if model.merge_local(index, (keep, set), lane) {
                continue;
            }
        }
        let word = model.read_local(index)?;
        match name {
            "ds_read_u8" => {
                let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
                let value = model.binary(op::BITWISE_AND, shifted, byte);
                model.write_vector_lane(register, lane, value);
            }
            "ds_write_b8" => {
                let data = model.read_source(instruction, destination, lane)?;
                let data = model.binary(op::BITWISE_AND, data, byte);
                let placed = model.binary(op::SHIFT_LEFT_LOGICAL, data, shift);
                let hole = model.binary(op::SHIFT_LEFT_LOGICAL, byte, shift);
                let hole = model.not(hole);
                let kept = model.binary(op::BITWISE_AND, word, hole);
                let merged = model.binary(op::BITWISE_OR, kept, placed);
                model.write_local(index, merged, lane)?;
            }
            _ => {
                let data = model.read_source(instruction, destination, lane)?;
                let merged = model.binary(op::BITWISE_OR, word, data);
                model.write_local(index, merged, lane)?;
            }
        }
    }
    model.count();
    Ok(())
}

/// `null` in a scalar memory instruction's `SOFFSET` field, bits 31:25 of its second word: no
/// offset register. The code the operand numbering gives `null`, and the one LLVM encodes when a
/// probe gives only an immediate.
const SOFFSET_NULL: u32 = 125;

/// `s_buffer_load_dword` and its wider forms: consecutive words from a buffer descriptor's base
/// plus a byte offset, into consecutive scalar registers.
///
/// The descriptor is read as the vector buffer accesses read it ([`read_buffer_resource`]). Each
/// word is range-checked against the buffer's bytes a word at a time, and one that does not fit
/// reads zero: from GFX8 a scalar access out of bounds does not reach memory (Mesa
/// `ac_gpu_info.h:187`, `ac_nir_lower_mem_access_bit_sizes.c:212-217`, which clamps the offset to
/// the record count on the generations where it does). The record count is in bytes with no stride
/// and in strides with one (radeonsi `si_state.c:3506-3527`: GFX6-7 and 10, and GFX9's SMEM), the
/// address the base plus the offset either way. A descriptor asking for addressing no access here
/// models reads zero, as D147 has the vector accesses do.
///
/// The destination may be a lane mask, as the AGC formatted copy loads its count into `vcc_lo`
/// (PPSA03416): one word writes the mask's low half, two its whole. One word into a mask's high
/// half, `vcc_hi` or `exec_hi`, writes that half (PPSA02664).
///
/// An offset register (`SOFFSET` other than `null`), which the solved layout has no operand for, is
/// read from its field and added to the immediate, the sum's low two bits ignored (AMD's published
/// RDNA instruction set, scalar memory). Refused by name: offset bits above the sixteen the probes
/// solved, and an offset field naming no scalar register.
fn scalar_buffer_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let words = access_words(name);
    let (destination, base, offset) = three_operands(instruction)?;
    let mask = mask_destination(destination).filter(|_| words <= 2);
    let high_mask = match destination {
        Operand::Named(named) if words == 1 => lane_mask_high_name(named),
        _ => None,
    };
    let (register, Operand::Scalar(base_register), Operand::Immediate(bytes)) =
        (destination, base, offset)
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar buffer load needs a scalar destination, a descriptor and an immediate",
        });
    };
    let register = match (register, mask) {
        (Operand::Scalar(register), _) => *register,
        (_, Some(_)) => 0,
        _ if high_mask.is_some() => 0,
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a scalar buffer load needs a scalar destination, a descriptor and an immediate",
            });
        }
    };
    let second = instruction.second_word.unwrap_or(0);
    let added = scalar_offset_register(model, instruction, (second >> 25) & 0x7f)?;
    if (second >> 16) & 0x1f != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar buffer load's offset sets bits above the sixteen the probes solved",
        });
    }
    let bytes = u32::try_from(*bytes).map_err(|_| TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "negative load offset",
    })?;
    let register = u32::from(register);
    if register + words > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scalar load runs past the end of the register file",
        });
    }

    let resource = read_buffer_resource(model, u32::from(*base_register));
    let zero = model.constant(0);
    let strided = model.is_not_zero(resource.stride);
    let strided_bytes = model.binary(op::IMUL, resource.records, resource.stride);
    let bytes_in_buffer = model.select(strided, strided_bytes, resource.records);
    let refused = model.is_not_zero(resource.unsupported);
    let slot = model.draw_buffer_slot(instruction);
    let sampled = scalar_sampled(model, instruction, slot, (&resource, refused))?;
    let mut values = Vec::with_capacity(words as usize);
    // The immediate plus the offset register, dword-aligned; a constant where there is none.
    let first = added.map(|added| {
        let immediate = model.constant(bytes);
        let sum = model.add(immediate, added);
        let aligned = model.constant(!3);
        model.binary(op::BITWISE_AND, sum, aligned)
    });
    for word in 0..words {
        let (at, end, wrapped) = scalar_word_place(model, (bytes, first), word);
        let past = model.compare(op::UGREATER_THAN, end, bytes_in_buffer);
        let past = match wrapped {
            Some(wrapped) => model.either(past, wrapped),
            None => past,
        };
        let outside = model.either(past, refused);
        // Through a table selected at run time, the word is at the guest address the record's
        // descriptor names, in the buffer of the table's pages (D966).
        let paged = slot.and_then(|slot| model.table_pages(slot));
        let loaded = if let Some(pages) = paged {
            let address = model.add(resource.base, at);
            let place = (pages, resource.base, resource.high);
            read_table_word(model, place, address, (outside, None))?
        } else if let Some(slot) = slot {
            // Through a bound draw buffer the word is the offset's, from the buffer's start (D733).
            let at = scalar_sampled_word(model, sampled, at, outside);
            let two = model.constant(2);
            let index = model.binary(op::SHIFT_RIGHT_LOGICAL, at, two);
            model.read_draw_buffer(slot, index)?
        } else if model.device_addressed() {
            // Unbound, a device-addressed dispatch reads the descriptor's guest bytes (D964).
            device::read_from_base(model, (resource.base, resource.high), at, outside)?
        } else {
            let address = model.add(resource.base, at);
            let index = model.word_index(address);
            model.read_memory(index)
        };
        values.push(model.select(outside, zero, loaded));
    }
    match (mask, values.as_slice()) {
        (None, [high]) if let Some(mask) = high_mask => write_mask_high(model, mask, *high)?,
        (Some(mask), [low]) => write_mask_low(model, mask, *low)?,
        (Some(mask), [low, high]) => model.write_lane_mask(mask, *low, *high)?,
        _ => {
            for (word, value) in (0..).zip(values) {
                model.write_scalar(register + word, value);
            }
        }
    }
    model.count();
    Ok(())
}

/// How many consecutive words a multi-word access carries, from its name.
///
/// `s_load_dword` is one, `s_load_dwordx2` two, and so on; the flat accesses spell it the same way.
/// The suffix is the instruction's own statement of its width, where opcode adjacency is not. An
/// unparseable suffix answers one, which under-reads visibly rather than writing registers the
/// instruction never named.
fn access_words(name: &str) -> u32 {
    match name.rsplit_once("dwordx") {
        Some((_, count)) => count.parse().unwrap_or(1),
        None => 1,
    }
}

/// A buffer resource constant, read out of four consecutive scalar registers.
///
/// Holds the fields addressing needs; channel selects and data format describe a conversion the
/// untyped accesses do not do.
#[derive(Clone, Copy)]
struct BufferResource {
    /// Byte address of the buffer. The reference gives 48 bits; see [`buffer_address`].
    base: Id,
    /// Bytes per record, 0 to 16383. Zero means a raw buffer.
    stride: Id,
    /// In units of stride when there is one, otherwise in bytes.
    records: Id,
    /// Set when the descriptor asks for addressing this translator does not do.
    unsupported: Id,
    /// Bits 47:32 of the base, which an exact module checks against its window's high half.
    high: Id,
}

/// Reads a buffer resource constant from the register file.
///
/// From the instruction-set reference's descriptor table: base address in bits 47:0, stride in
/// 61:48, record count in 95:64, swizzle enable at 63, add-thread-id at 119 and the out-of-bounds
/// mode in 125:124, landing across four registers as the shifts below. The descriptor lives in
/// registers, so addressing is emitted as run-time arithmetic.
fn read_buffer_resource<M: Model + ?Sized>(model: &mut M, first: u32) -> BufferResource {
    let base = model.read_scalar(first);
    let second = model.read_scalar(first + 1);
    let records = model.read_scalar(first + 2);
    let flags = model.read_scalar(first + 3);

    let sixteen = model.constant(16);
    let stride_mask = model.constant(0x3FFF);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, second, sixteen);
    let stride = model.binary(op::BITWISE_AND, shifted, stride_mask);

    // Swizzled addressing and add-thread-id change where an access lands, and neither is modelled.
    // A translated shader cannot refuse at run time, so such an access is forced out of bounds,
    // which reads zero and drops writes (D147).
    let swizzle_bit = model.constant(1 << 31);
    let swizzled = model.binary(op::BITWISE_AND, second, swizzle_bit);
    let add_tid_bit = model.constant(1 << 23);
    let add_tid = model.binary(op::BITWISE_AND, flags, add_tid_bit);
    let unsupported = model.binary(op::BITWISE_OR, swizzled, add_tid);
    let high_mask = model.constant(0xFFFF);
    let high = model.binary(op::BITWISE_AND, second, high_mask);

    BufferResource {
        base,
        stride,
        records,
        unsupported,
        high,
    }
}

/// The byte address a buffer access reads or writes.
///
/// The reference gives it as
///
/// ```text ADDR = Base + baseOffset + Inst_offset + Voffset + Stride * (Vindex + TID) ```
///
/// where `baseOffset` is the scalar offset operand, `Inst_offset` the literal in the instruction,
/// `Voffset` a vector register present when the instruction sets `offen`, and `Vindex` one present
/// when it sets `idxen`. The thread-id term is excluded, with the descriptors that ask for it.
///
/// Only the low thirty-two bits are computed: guest memory here is a window indexed from its base,
/// as for the flat accesses.
fn buffer_address<M: Model + ?Sized>(
    model: &mut M,
    resource: &BufferResource,
    scalar_offset: Id,
    instruction_offset: Id,
    voffset: Option<Id>,
    vindex: Option<Id>,
) -> Id {
    let mut address = model.add(resource.base, scalar_offset);
    address = model.add(address, instruction_offset);
    if let Some(voffset) = voffset {
        address = model.add(address, voffset);
    }
    if let Some(vindex) = vindex {
        let scaled = model.binary(op::IMUL, resource.stride, vindex);
        address = model.add(address, scaled);
    }
    address
}

/// Where a buffer access reads: through the window, or through the draw buffer at a slot (D733).
///
/// A draw buffer starts at the descriptor's base, so an access through one is addressed from zero:
/// the descriptor's base is where the bound buffer begins, and the rest of the equation is the
/// offset into it. A store through one is admitted where `stores` - an untyped store, whose
/// dispatch's buffer is written back (D746) - and refused otherwise.
///
/// A buffer placed from a sample of a dispatch's waves (D951) starts at a guest base of its own
/// instead: the access is addressed from the descriptor's base less the buffer's, and comes back
/// with what [`note_sampled_escape`] checks it against.
fn buffer_reach<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (loading, stores): (bool, bool),
    resource: BufferResource,
) -> Result<(Option<u32>, BufferResource, Option<Sampled>), TranslateError> {
    let Some(slot) = model.draw_buffer_slot(instruction) else {
        return Ok((None, resource, None));
    };
    if !loading && !stores {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed store through a bound buffer, which is not written back (D746)",
        });
    }
    // A record of a table selected at run time is read where its own descriptor says, through the
    // buffer of the table's pages (D966), which only loads read: nothing is written back from it.
    if model.table_pages(slot).is_some() {
        if !loading {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a store through a descriptor selected from a table at run time, whose records are bound only to be read (D966)",
            });
        }
        if !model.exact_memory() {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a descriptor selected from a table at run time in a module that cannot check an access against the records bound (D966)",
            });
        }
        return Ok((Some(slot), resource, None));
    }
    if model.sampled_buffer(slot) {
        let sampled = sampled_place(model, instruction, slot, (resource.base, resource.high))?;
        let refused = Some(model.is_not_zero(resource.unsupported));
        let sampled = Sampled { refused, ..sampled };
        let base = model.binary(op::ISUB, resource.base, sampled.low);
        return Ok((
            Some(slot),
            BufferResource { base, ..resource },
            Some(sampled),
        ));
    }
    let base = model.constant(0);
    Ok((Some(slot), BufferResource { base, ..resource }, None))
}

/// A bound buffer placed from a sample of a dispatch's waves (D951), as an access through it is
/// checked against: where the buffer starts, how much it holds, and where the descriptor the access
/// computed starts.
#[derive(Clone, Copy)]
struct Sampled {
    /// The low half of the guest address the bound buffer starts at.
    low: Id,
    /// Its bits 47:32.
    high: Id,
    /// How many words the bound buffer holds.
    words: Id,
    /// The low half of the base the access computed: a descriptor's, or a scalar load's pair.
    from_low: Id,
    /// Its bits 47:32.
    from_high: Id,
    /// Set where the descriptor asks for addressing this translator does not do, which the bound
    /// buffer cannot answer; `None` where there is no descriptor.
    refused: Option<Id>,
}

/// What an access through the sampled buffer at `slot` is checked against (D951), from the base
/// `(low, high)` the access computed.
///
/// # Errors
///
/// A model that cannot check it: not exact, or carrying no base or length for the slot. A buffer
/// placed from a sample is never read unchecked.
fn sampled_place<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    slot: u32,
    (from_low, from_high): (Id, Id),
) -> Result<Sampled, TranslateError> {
    let refuse = || TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a buffer placed from a sample in a module that cannot check an access against it (D951)",
    };
    if !model.exact_memory() {
        return Err(refuse());
    }
    let (low, high) = model.sampled_buffer_base(slot).ok_or_else(refuse)?;
    let words = model.draw_buffer_words(slot).ok_or_else(refuse)?;
    Ok(Sampled {
        low,
        high,
        words,
        from_low,
        from_high,
        refused: None,
    })
}

/// A scalar buffer load's sampled buffer (D951), where the draw buffer at `slot` was placed so:
/// what it is checked against, and where the descriptor's base lies from the buffer's - the
/// load's offsets are added to that. `refused` holds where the descriptor's addressing is not
/// modelled.
fn scalar_sampled<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    slot: Option<u32>,
    (resource, refused): (&BufferResource, Id),
) -> Result<Option<(Sampled, Id)>, TranslateError> {
    let Some(slot) = slot.filter(|&slot| model.sampled_buffer(slot)) else {
        return Ok(None);
    };
    let place = sampled_place(model, instruction, slot, (resource.base, resource.high))?;
    let start = model.binary(op::ISUB, resource.base, place.low);
    let refused = Some(refused);
    Ok(Some((Sampled { refused, ..place }, start)))
}

/// The byte a scalar buffer load's word `at` from the descriptor's base is in its bound buffer:
/// the same, unless the buffer was placed from a sample (D951), when it is counted from the
/// buffer's own base and checked against it for the scalar unit.
fn scalar_sampled_word<M: Model + ?Sized>(
    model: &mut M,
    sampled: Option<(Sampled, Id)>,
    at: Id,
    outside: Id,
) -> Id {
    let Some((place, start)) = sampled else {
        return at;
    };
    let at = model.add(start, at);
    note_sampled_escape(model, Some(place), (at, 4), outside, None);
    at
}

/// Records an access through a buffer placed from a sample (D951) as an escape in `lane`, or for
/// the scalar unit, where the bound buffer does not hold it - its base computed in another
/// four-gigabyte span than the buffer's, carried past its span's low half, or with its first or last
/// byte outside the buffer's words - or where its descriptor asks for addressing that is not
/// modelled. `address` is the access's byte from the buffer's start, `bytes` long. An access the
/// descriptor's own bounds refuse (`outside`) reads zero and writes nothing on the hardware too,
/// and is none: the sample placed the range, the check keeps it honest.
fn note_sampled_escape<M: Model + ?Sized>(
    model: &mut M,
    sampled: Option<Sampled>,
    (address, bytes): (Id, u32),
    outside: Id,
    lane: Option<u32>,
) {
    let Some(sampled) = sampled else {
        return;
    };
    let two = model.constant(2);
    let first_word = model.binary(op::SHIFT_RIGHT_LOGICAL, address, two);
    let reach = model.constant(bytes.max(1) - 1);
    let end = model.add(address, reach);
    let last_word = model.binary(op::SHIFT_RIGHT_LOGICAL, end, two);
    // Unsigned, so an address below the base wraps past the end and is outside too, and so is an
    // end that wraps past the span.
    let first_out = model.compare(op::UGREATER_THAN_EQUAL, first_word, sampled.words);
    let last_out = model.compare(op::UGREATER_THAN_EQUAL, last_word, sampled.words);
    let wrapped = model.compare(op::ULESS_THAN, end, address);
    // The address as the hardware forms it, its low half: below the computed base it carried into
    // the next span, which the buffer's low half does not say.
    let absolute = model.add(address, sampled.low);
    let carried = model.compare(op::ULESS_THAN, absolute, sampled.from_low);
    let other_span = model.compare(op::INOT_EQUAL, sampled.from_high, sampled.high);
    let mut unheld = model.either(first_out, last_out);
    for also in [wrapped, carried, other_span] {
        unheld = model.either(unheld, also);
    }
    let bool_type = model.bool_type();
    let b = model.builder();
    let admitted = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, admitted.0, outside.0]);
    let mut escaped = model.both(admitted, unheld);
    if let Some(refused) = sampled.refused {
        escaped = model.either(escaped, refused);
    }
    model.note_escape(escaped, lane);
}

/// Stores `value` at `address` in `lane` unless `outside`: through the window, or at `address / 4`
/// of the bound buffer at `slot` (D746). Out of range keeps what was there, in either.
fn store_reached<M: Model + ?Sized>(
    model: &mut M,
    slot: Option<u32>,
    (address, outside): (Id, Id),
    value: Id,
    lane: u32,
) -> Result<(), TranslateError> {
    match slot {
        None => {
            let word = model.word_index(address);
            let previous = model.read_memory(word);
            let kept = pick(model, outside, previous, value);
            write_guarded(model, address, kept, lane);
        }
        Some(slot) => {
            let two = model.constant(2);
            let word = model.binary(op::SHIFT_RIGHT_LOGICAL, address, two);
            let previous = model.read_draw_buffer(slot, word)?;
            let kept = pick(model, outside, previous, value);
            model.write_draw_buffer(slot, (word, kept), outside, lane)?;
        }
    }
    Ok(())
}

/// The most rounds a part store's compare-and-swap takes before it gives up (D977). A round fails
/// only where another invocation changed the word between this one's read and its swap, so each
/// round some contender lands; a word still contended after this many is a device that is not
/// progressing, and the store is reported as an escape rather than spun on.
pub(crate) const MERGE_ROUNDS: u32 = 1024;

/// `OpLoopMerge`'s `DontUnroll`.
const DONT_UNROLL: u32 = 0x2;

/// Replaces the bits of the guest word at `pointer` that `keep` clears with `set`'s, as one
/// indivisible step (D977): the word is read, merged and swapped in where it still holds what was
/// read, and read again and retried where another invocation changed it meanwhile, up to
/// [`MERGE_ROUNDS`]. Answers whether it gave up. The caller is inside the block that runs only where
/// the store lands.
fn merge_bits<M: Model + ?Sized>(model: &mut M, pointer: Id, (keep, set): (Id, Id)) -> Id {
    let (u32_type, bool_type) = (model.u32_type(), model.bool_type());
    let (device, relaxed, zero, one, rounds) = (
        model.constant(orbistoun_spirv::scope::DEVICE),
        model.constant(0),
        model.constant(0),
        model.constant(1),
        model.constant(MERGE_ROUNDS),
    );
    let b = model.builder();
    let [initial, entry, header, body, next, merge] = [(); 6].map(|()| b.id());
    let [expected, round, seen, after] = [(); 4].map(|()| b.id());
    b.function(
        op::LOAD,
        &[
            u32_type.0,
            initial.0,
            pointer.0,
            orbistoun_spirv::MEMORY_ALIGNED | orbistoun_spirv::MEMORY_VOLATILE,
            4,
        ],
    );
    b.function(op::BRANCH, &[entry.0]);
    b.function(op::LABEL, &[entry.0]);
    b.function(op::BRANCH, &[header.0]);
    b.function(op::LABEL, &[header.0]);
    b.function(
        op::PHI,
        &[u32_type.0, expected.0, initial.0, entry.0, seen.0, next.0],
    );
    b.function(
        op::PHI,
        &[u32_type.0, round.0, zero.0, entry.0, after.0, next.0],
    );
    b.function(op::LOOP_MERGE, &[merge.0, next.0, DONT_UNROLL]);
    b.function(op::BRANCH, &[body.0]);
    b.function(op::LABEL, &[body.0]);
    let kept = model.binary(op::BITWISE_AND, expected, keep);
    let desired = model.binary(op::BITWISE_OR, kept, set);
    let b = model.builder();
    b.function(
        op::ATOMIC_COMPARE_EXCHANGE,
        &[
            u32_type.0, seen.0, pointer.0, device.0, relaxed.0, relaxed.0, desired.0, expected.0,
        ],
    );
    b.function(op::IADD, &[u32_type.0, after.0, round.0, one.0]);
    let landed = model.compare(op::IEQUAL, seen, expected);
    let spent = model.compare(op::UGREATER_THAN_EQUAL, after, rounds);
    let stop = model.either(landed, spent);
    let b = model.builder();
    b.function(op::BRANCH, &[next.0]);
    b.function(op::LABEL, &[next.0]);
    b.function(op::BRANCH_CONDITIONAL, &[stop.0, merge.0, header.0]);
    b.function(op::LABEL, &[merge.0]);
    let gave_up = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, gave_up.0, landed.0]);
    gave_up
}

/// What a store of `value`'s low `bits` bits at byte `address` does to the word holding that byte
/// and to the next (D977): for each, the bits to keep and the bits to set; and whether it reaches
/// the next word at all, as a half at a word's last byte does.
pub(crate) fn sub_word_parts<M: Model + ?Sized>(
    model: &mut M,
    (address, value): (Id, Id),
    bits: u32,
) -> ([(Id, Id); 2], Id) {
    let (three, eight, one, thirty_one) = (
        model.constant(3),
        model.constant(8),
        model.constant(1),
        model.constant(31),
    );
    let (field, all) = (model.constant((1u32 << bits) - 1), model.constant(u32::MAX));
    let byte = model.binary(op::BITWISE_AND, address, three);
    let shift = model.binary(op::IMUL, byte, eight);
    let data = model.binary(op::BITWISE_AND, value, field);
    // The next word's part is what shifts past bit 31: `x >> (32 - shift)`, as two shifts so a
    // shift of zero moves nothing there.
    let rest = model.binary(op::ISUB, thirty_one, shift);
    let mut parts = [(all, all); 2];
    for (part, low) in parts.iter_mut().zip([true, false]) {
        let [mask, set] = [field, data].map(|bits| {
            if low {
                model.binary(op::SHIFT_LEFT_LOGICAL, bits, shift)
            } else {
                let halved = model.binary(op::SHIFT_RIGHT_LOGICAL, bits, one);
                model.binary(op::SHIFT_RIGHT_LOGICAL, halved, rest)
            }
        });
        *part = (model.binary(op::BITWISE_XOR, mask, all), set);
    }
    let spans = model.compare(op::INOT_EQUAL, parts[1].0, all);
    (parts, spans)
}

/// Stores `value`'s low `bytes` bytes at `address` in `lane` unless `outside` (D977): through the
/// window, or the draw buffer at `slot`, merged into the word holding the first byte and, where it
/// reaches it, the next - each a part of a word other lanes may be storing the rest of.
fn store_narrow<M: Model + ?Sized>(
    model: &mut M,
    slot: Option<u32>,
    (address, outside): (Id, Id),
    (value, bytes): (Id, u32),
    lane: u32,
) -> Result<(), TranslateError> {
    let (parts, spans) = sub_word_parts(model, (address, value), 8 * bytes);
    let bool_type = model.bool_type();
    let b = model.builder();
    let landed = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, landed.0, outside.0]);
    let aligned = model.constant(!3);
    let first = model.binary(op::BITWISE_AND, address, aligned);
    for (word, (keep, set)) in [0u32, 1].into_iter().zip(parts) {
        let step = model.constant(word * 4);
        let at = model.add(first, step);
        let lands = if word == 0 {
            landed
        } else {
            model.both(landed, spans)
        };
        match slot {
            None => {
                let inside = model.address_within_window(at);
                let lands = model.both(lands, inside);
                let index = model.word_index(at);
                model.merge_memory(index, (keep, set), lands, lane);
            }
            Some(slot) => {
                let two = model.constant(2);
                let index = model.binary(op::SHIFT_RIGHT_LOGICAL, at, two);
                merge_draw_buffer(model, slot, (index, (keep, set)), lands, lane)?;
            }
        }
    }
    Ok(())
}

/// What an untyped store writes at `address`: a word ([`store_reached`]), or the `narrow` bytes
/// fewer than one, merged into their word ([`store_narrow`], D977).
fn store_access<M: Model + ?Sized>(
    model: &mut M,
    slot: Option<u32>,
    reached: (Id, Id),
    (value, narrow): (Id, Option<u32>),
    lane: u32,
) -> Result<(), TranslateError> {
    match narrow {
        Some(bytes) => store_narrow(model, slot, reached, (value, bytes), lane),
        None => store_reached(model, slot, reached, value, lane),
    }
}

/// [`Model::merge_memory`] on word `index` of the draw buffer at `slot` (D977): the word swapped in
/// whole where `lane` runs, `landed` holds and the buffer holds it ([`merge_bits`]).
fn merge_draw_buffer<M: Model + ?Sized>(
    model: &mut M,
    slot: u32,
    (index, parts): (Id, (Id, Id)),
    landed: Id,
    lane: u32,
) -> Result<(), TranslateError> {
    let (pointer, inside) = model.draw_buffer_word(slot, index)?;
    let lands = model.both(landed, inside);
    let merge = open_if_active(model, lane, Some(lands));
    let gave_up = merge_bits(model, pointer, parts);
    model.note_escape(gave_up, Some(lane));
    close_if_active(model, merge);
    Ok(())
}

/// Where a buffer access's words are read, past the window: a bound buffer, from the access's byte
/// in it (D733), or the buffer of pages holding every record of a table selected at run time, at the
/// guest address the selected record names (D966).
#[derive(Clone, Copy)]
enum Reach {
    /// The draw buffer at the slot.
    Bound(u32),
    /// The buffer of pages at `pages`, read at addresses formed from the selected descriptor's base:
    /// `from` its low half, `high` its bits 47:32.
    Pages { pages: u32, from: Id, high: Id },
}

/// Where the access through the draw buffer `slot`, with the descriptor `resource` it read, reads
/// its words: `None` through the window.
fn reach_of<M: Model + ?Sized>(
    model: &M,
    slot: Option<u32>,
    resource: &BufferResource,
) -> Option<Reach> {
    let slot = slot?;
    Some(match model.table_pages(slot) {
        Some(pages) => Reach::Pages {
            pages,
            from: resource.base,
            high: resource.high,
        },
        None => Reach::Bound(slot),
    })
}

/// The word at the guest `address` in the buffer of a table's pages (D966), placed by
/// `(pages, from, high)`: its high half is `high`, carried where `address` wrapped below `from`,
/// the base it was formed from. A word the buffer does not hold reads zero, and where the access's
/// bounds admit it (not `outside`) it is an escape, for `lane` or the scalar unit.
fn read_table_word<M: Model + ?Sized>(
    model: &mut M,
    (pages, from, high): (u32, Id, Id),
    address: Id,
    (outside, lane): (Id, Option<u32>),
) -> Result<Id, TranslateError> {
    let high = carried(model, (from, address), high);
    let (value, unheld) = global_page_word(model, pages, (address, high))?;
    if model.exact_memory() {
        let bool_type = model.bool_type();
        let b = model.builder();
        let admitted = b.id();
        b.function(op::LOGICAL_NOT, &[bool_type.0, admitted.0, outside.0]);
        let escaped = model.both(admitted, unheld);
        model.note_escape(escaped, lane);
    }
    Ok(value)
}

/// Reads the word at `address`: through the window, or at `address / 4` of the draw buffer at
/// `slot`, the low two bits dropped as the window's word index drops them, or at the guest address
/// in a table's pages (D966), checked against `outside` in `lane`.
fn read_reached<M: Model + ?Sized>(
    model: &mut M,
    slot: Option<Reach>,
    address: Id,
    checked: (Id, Option<u32>),
) -> Result<Id, TranslateError> {
    match slot {
        None => Ok(read_guarded(model, address)),
        Some(Reach::Bound(slot)) => {
            let two = model.constant(2);
            let index = model.binary(op::SHIFT_RIGHT_LOGICAL, address, two);
            model.read_draw_buffer(slot, index)
        }
        Some(Reach::Pages { pages, from, high }) => {
            read_table_word(model, (pages, from, high), address, checked)
        }
    }
}

/// A packed element's words from `base`: through the window, or from a draw buffer or a table's
/// pages at the element's own byte, which need not be a word's first.
fn read_element<M: Model + ?Sized>(
    model: &mut M,
    slot: Option<Reach>,
    base: Id,
    (words, checked): (u32, (Id, Option<u32>)),
) -> Result<Vec<Id>, TranslateError> {
    let mut packed = Vec::with_capacity(words as usize);
    for word in 0..words {
        let step = model.constant(word * 4);
        let address = model.add(base, step);
        packed.push(match slot {
            Some(reach) => read_draw_buffer_bytes(model, reach, address, checked)?,
            None => read_guarded(model, address),
        });
    }
    Ok(packed)
}

/// The four bytes of the draw buffer or table's pages `reach` from byte `address`, as a
/// little-endian word: the word holding the first byte shifted down, with the next word's low bytes
/// above it.
fn read_draw_buffer_bytes<M: Model + ?Sized>(
    model: &mut M,
    reach: Reach,
    address: Id,
    checked: (Id, Option<u32>),
) -> Result<Id, TranslateError> {
    let (two, three, one, four, eight, thirty_two) = (
        model.constant(2),
        model.constant(3),
        model.constant(1),
        model.constant(4),
        model.constant(8),
        model.constant(32),
    );
    let (low, high) = match reach {
        Reach::Bound(slot) => {
            let index = model.binary(op::SHIFT_RIGHT_LOGICAL, address, two);
            let low = model.read_draw_buffer(slot, index)?;
            let next = model.add(index, one);
            (low, model.read_draw_buffer(slot, next)?)
        }
        Reach::Pages { pages, from, high } => {
            let place = (pages, from, high);
            let low = read_table_word(model, place, address, checked)?;
            let next = model.add(address, four);
            // An aligned word takes nothing of the next, which need not be held.
            let (outside, lane) = checked;
            let byte = model.binary(op::BITWISE_AND, address, three);
            let zero = model.constant(0);
            let aligned = model.compare(op::IEQUAL, byte, zero);
            let unneeded = model.either(outside, aligned);
            (low, read_table_word(model, place, next, (unneeded, lane))?)
        }
    };
    let byte = model.binary(op::BITWISE_AND, address, three);
    let shift = model.binary(op::IMUL, byte, eight);
    let down = model.binary(op::SHIFT_RIGHT_LOGICAL, low, shift);
    // A shift by thirty-two is undefined, so the aligned case takes the low word whole.
    let up_by = model.binary(op::ISUB, thirty_two, shift);
    let up = model.binary(op::SHIFT_LEFT_LOGICAL, high, up_by);
    let joined = model.binary(op::BITWISE_OR, down, up);
    let zero = model.constant(0);
    let aligned = model.compare(op::IEQUAL, byte, zero);
    Ok(pick(model, aligned, low, joined))
}

/// Whether a buffer access falls outside the buffer.
///
/// The reference defines four modes, selected by two descriptor bits known only at run time, so all
/// four are evaluated: mode 0 checks index >= records or offset >= stride (structured buffers);
/// mode 1 checks index >= records (raw buffers); mode 2 checks records == 0; mode 3 checks offset +
/// payload > records (raw, unswizzled). Mode 3's payload is read as bytes: the reference calls it
/// dwords, but every other term is a byte count, and bytes give the ordinary range check. The two
/// bits are `OOB_SELECT`, bits 29:28 of the descriptor's fourth word on this generation (Mesa
/// `src/amd/registers/gfx10-rsrc.json`, `SQ_BUF_RSRC_WORD3`).
fn buffer_out_of_bounds<M: Model + ?Sized>(
    model: &mut M,
    resource: &BufferResource,
    flags: Id,
    offset: Id,
    index: Id,
    payload_bytes: u32,
) -> Id {
    let payload = model.constant(payload_bytes);
    buffer_out_of_bounds_placed(model, resource, flags, (offset, offset), index, payload)
}

/// [`buffer_out_of_bounds`] for a payload the module reads as it runs - a format load's element,
/// whose format its descriptor names (D967) - and an access placed apart from its offset: mode 3
/// checks `place + payload` against the records, where `place` is a swizzled access's byte (Mesa
/// `src/amd/common/ac_descriptors.c:824-833`, `OOB_SELECT` 3 on GFX10: "if SWIZZLE_ENABLE:
/// swizzle_address >= NUM_RECORDS") and its offset otherwise. Modes 0 and 1 check the offset and
/// index as given.
fn buffer_out_of_bounds_placed<M: Model + ?Sized>(
    model: &mut M,
    resource: &BufferResource,
    flags: Id,
    (offset, place): (Id, Id),
    index: Id,
    payload: Id,
) -> Id {
    let zero = model.constant(0);
    let two = model.constant(2);
    // The selector is two bits wide.
    let three = model.constant(3);
    let twenty_eight = model.constant(28);

    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, flags, twenty_eight);
    let mode = model.binary(op::BITWISE_AND, shifted, three);

    let index_past = model.compare(op::UGREATER_THAN_EQUAL, index, resource.records);
    let offset_past_stride = model.compare(op::UGREATER_THAN_EQUAL, offset, resource.stride);
    let no_records = model.compare(op::IEQUAL, resource.records, zero);
    let reach = model.add(place, payload);
    let reach_past = model.compare(op::UGREATER_THAN, reach, resource.records);

    let structured = model.either(index_past, offset_past_stride);
    let one = model.constant(1);

    // Mode three is the default and the others override it, so the checks apply in descending order
    // and the last select wins.
    let mut out = reach_past;
    let is_two = model.compare(op::IEQUAL, mode, two);
    out = select_bool(model, is_two, no_records, out);
    // Mode one checks the index alone, with no stride as well, where every access is record zero
    // and nothing is bounded (D917, measured: obSCEne gt22).
    let is_one = model.compare(op::IEQUAL, mode, one);
    out = select_bool(model, is_one, index_past, out);
    let is_zero = model.compare(op::IEQUAL, mode, zero);
    out = select_bool(model, is_zero, structured, out);

    // A descriptor asking for addressing this does not do is treated as out of bounds.
    let refused = model.is_not_zero(resource.unsupported);
    model.either(out, refused)
}

/// Chooses between two booleans.
fn select_bool<M: Model + ?Sized>(
    model: &mut M,
    condition: Id,
    when_true: Id,
    when_false: Id,
) -> Id {
    let bool_type = model.bool_type();
    let b = model.builder();
    let result = b.id();
    b.function(
        op::SELECT,
        &[
            bool_type.0,
            result.0,
            condition.0,
            when_true.0,
            when_false.0,
        ],
    );
    result
}

/// The typed-buffer format table, parsed once.
///
/// A table for the reasons in [`orbistoun_shader::formats`]; behind a lock because parsing it per
/// instruction would dominate translation.
static BUFFER_FORMATS: std::sync::OnceLock<orbistoun_shader::FormatTable> =
    std::sync::OnceLock::new();

/// How many components a typed access moves, from its name.
///
/// The channel letters (`_x` is one, `_xyzw` four) count registers, which may differ from the
/// format's component count; this reads only the letters.
fn typed_channels(name: &str) -> Option<u32> {
    let suffix = name.rsplit_once("_format_")?.1;
    let count = suffix.len();
    // Contiguous from `x`, so `xz` is not an access this understands.
    ("xyzw".starts_with(suffix) && count >= 1).then_some(count as u32)
}

/// A typed buffer access: the descriptor and addressing of an untyped one, with a format saying how
/// to read what was fetched.
///
/// Formats whose components are all thirty-two bits move whole words through [`buffer_access`].
/// Narrower components are extracted and converted by [`packed_buffer_memory`]. Anything neither
/// handles is refused by name, never approximated. The component count must match the channel
/// count: the hardware's rule when they differ is unmeasured.
fn typed_buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let loading = name.starts_with("tbuffer_load");
    let Some(channels) = typed_channels(name) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access does not name its channels",
        });
    };

    let formats =
        BUFFER_FORMATS.get_or_init(|| orbistoun_shader::FormatTable::builtin().unwrap_or_default());
    let code = orbistoun_shader::FormatTable::field(instruction.word);
    let Some(format) = formats.get(code) else {
        // The explicitly invalid code or a reserved one; both mean the shader is wrong.
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access names a format code with no meaning",
        });
    };
    // A load of fewer channels than a format of whole words reads the element's first words: ACO
    // relies on it ("if a larger format is selected, it's still OK to load a smaller amount from
    // it", `aco_select_nir_intrinsics.cpp`, `mtbuf_load_callback`). Any other mismatch is refused.
    let fewer_words = loading && format.is_plain_words() && format.components() > channels as usize;
    if format.components() != channels as usize && !fewer_words {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a typed buffer access whose format and channel count disagree",
        });
    }

    // A plain-word format moves whole dwords through the untyped path; a narrow format packs
    // components inside words and goes to `packed_buffer_memory`.
    if format.is_plain_words() {
        buffer_access(model, instruction, loading, (channels, None))
    } else {
        packed_buffer_memory(model, instruction, format, loading)
    }
}

/// A format load through a null descriptor - a fourth word of zero, as radv writes for a null
/// buffer: every `DST_SEL` is `SQ_SEL_0`, so each channel written reads zero whatever the format.
fn null_format_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    channels: u32,
) -> Result<(), TranslateError> {
    let Some(Operand::Vector(register)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format load's data is not vector registers",
        });
    };
    let zero = model.constant(0);
    for lane in running_lanes(model) {
        for channel in 0..channels {
            model.write_vector_lane(u32::from(*register) + channel, lane, zero);
        }
    }
    model.count();
    Ok(())
}

/// `SQ_BUF_RSRC_WORD3.FORMAT`, bits 18:12 (`gfx10-rsrc.json`): a `GFX10_FORMAT` code, numbered as
/// the typed-buffer table's are - all 77 of the table's codes name the same format there.
const DESCRIPTOR_FORMAT_SHIFT: u32 = 12;

/// `buffer_load_format_x` .. `_xyzw` (D738): a typed load whose format is its descriptor's. The
/// element is fetched and converted as a typed load of that format converts it, then each channel
/// written is what the descriptor's select for it names - `DST_SEL_X..W`, bits 11:0, `SQ_SEL_0` and
/// `SQ_SEL_1` the constants and 4 to 7 the element's components. One is `1` for an integer format
/// and `1.0` for the others. A select of a component the format lacks, or a reserved code, is
/// refused rather than guessed. An element outside the buffer reads as zeros under the same
/// selects, as an untyped load answers zero.
/// The fourth descriptor word of the draw buffer in `slot`, which a format load or store converts by
/// (D738): refused as needing the draw's descriptors where none were given, and naming the slot
/// where they were and this one was not resolved.
fn buffer_format_word<M: Model + ?Sized>(
    model: &M,
    instruction: &Instruction,
    slot: u32,
) -> Result<u32, TranslateError> {
    match model.buffer_descriptor_format(slot) {
        Some(word3) => Ok(word3),
        None if model.buffer_formats_given() => Err(TranslateError::UnresolvedBufferFormat {
            offset: instruction.offset,
            slot,
        }),
        None => Err(TranslateError::NeedsBufferFormats),
    }
}

fn format_buffer_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    use orbistoun_shader::ComponentKind;
    let channels = typed_channels(name).ok_or(TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a format load does not name its channels",
    })?;
    let slot = model
        .draw_buffer_slot(instruction)
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format load through a buffer a draw does not bind, whose format is not known",
        })?;
    let word3 = buffer_format_word(model, instruction, slot)?;
    if word3 == 0 {
        return null_format_load(model, instruction, channels);
    }
    let (format, selects, total_bits) = descriptor_format(instruction, word3, channels)?;
    let one = if matches!(format.kind, ComponentKind::Uint | ComponentKind::Sint) {
        1
    } else {
        1.0f32.to_bits()
    };

    let [data, vaddr, resource_operand, soffset] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access does not have four operands",
        });
    };
    let (Operand::Vector(register), Operand::Scalar(resource_base)) = (data, resource_operand)
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format load's data is not vector registers or its resource not scalar ones",
        });
    };
    let (register, resource_base) = (u32::from(*register), u32::from(*resource_base));
    if resource_base + 4 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant runs past the end of the register file",
        });
    }
    let word = instruction.word;
    let literal_offset = word & 0xFFF;
    let offen = word & (1 << 12) != 0;
    let idxen = word & (1 << 13) != 0;
    if offen && idxen {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format load with both address modifiers, which is not translated",
        });
    }
    let resource = read_buffer_resource(model, resource_base);
    let (slot, addressed, sampled) = buffer_reach(model, instruction, (true, false), resource)?;
    let reach = reach_of(model, slot, &resource);
    let flags = model.read_scalar(resource_base + 3);
    let instruction_offset = model.constant(literal_offset);
    let element_bytes = total_bits.div_ceil(8);
    let element_words = total_bits.div_ceil(32);
    let zero = model.constant(0);
    let one = model.constant(one);

    for lane in running_lanes(model) {
        let scalar_offset = model.read_source(instruction, soffset, lane)?;
        let address_register = (idxen || offen)
            .then(|| model.read_source(instruction, vaddr, lane))
            .transpose()?;
        let (vindex, voffset) = if idxen {
            (address_register, None)
        } else {
            (None, address_register)
        };
        let offset = model.add(instruction_offset, voffset.unwrap_or(zero));
        let index = vindex.unwrap_or(zero);
        let base = buffer_address(
            model,
            &addressed,
            scalar_offset,
            instruction_offset,
            voffset,
            vindex,
        );
        let outside = buffer_out_of_bounds(model, &resource, flags, offset, index, element_bytes);
        note_sampled_escape(model, sampled, (base, element_bytes), outside, Some(lane));
        let fetched = read_element(model, reach, base, (element_words, (outside, Some(lane))))?;
        let values = element_components(model, format, &fetched, outside);
        for (channel, &select) in selects.iter().take(channels as usize).enumerate() {
            let value = match select {
                0 => zero,
                1 => one,
                component => values[(component - 4) as usize],
            };
            model.write_vector_lane(register + channel as u32, lane, value);
        }
    }
    model.count();
    Ok(())
}

/// `buffer_store_format_x` .. `_xyzw`: a typed store whose format is its descriptor's (D738), written
/// as a typed store of that format writes it. Whole-word formats move their words; a packed format
/// goes through the packed store, which admits only the conversions measured for it. The channels
/// must be the format's components and the descriptor's selects the identity over them: what a
/// store does with a reordering select, or with fewer components than channels, is unmeasured and
/// refused by name.
fn format_buffer_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let channels = typed_channels(name).ok_or(TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a format store does not name its channels",
    })?;
    let slot = model
        .draw_buffer_slot(instruction)
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format store through a buffer a dispatch does not bind, whose format is not known",
        })?;
    let word3 = buffer_format_word(model, instruction, slot)?;
    let (format, selects, _) = descriptor_format(instruction, word3, channels)?;
    // More channels than the format has components writes the format's components, from the first
    // channels (D790): Mesa stores an image buffer's four channels whatever its format
    // (`ac_nir_to_llvm.c:2281-2293`). Fewer, or more through a packed format, is unmeasured.
    let components = format.components();
    let written = u32::try_from(components).unwrap_or(u32::MAX);
    if components > channels as usize
        || (components < channels as usize && !format.is_plain_words())
    {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format store whose descriptor's format and channel count disagree",
        });
    }
    if selects
        .iter()
        .take(components)
        .zip(4..)
        .any(|(&select, identity)| select != identity)
    {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format store through a descriptor whose selects reorder or replace components",
        });
    }
    if format.is_plain_words() {
        buffer_access(model, instruction, false, (written, None))
    } else {
        packed_buffer_memory(model, instruction, format, false)
    }
}

/// A fetched element's components in order, x first, each converted as its format says and zero
/// when `outside` the buffer.
fn element_components<M: Model + ?Sized>(
    model: &mut M,
    format: &orbistoun_shader::BufferFormat,
    fetched: &[Id],
    outside: Id,
) -> Vec<Id> {
    let zero = model.constant(0);
    let mut values = Vec::with_capacity(format.components());
    if format.is_plain_words() {
        values.extend(fetched.iter().copied());
    } else {
        let mut bit = 0u32;
        for &width in format.widths.iter().rev() {
            let word = fetched[(bit / 32) as usize];
            values.push(packed_component(model, word, bit % 32, width, format.kind));
            bit += width;
        }
    }
    values
        .into_iter()
        .map(|value| pick(model, outside, zero, value))
        .collect()
}

/// What a format load's descriptor word says it converts by (D738): the format its `FORMAT` code
/// names, the selects for each channel, and the element's width in bits. Refused for a code with no
/// meaning, a format the typed loads do not convert, or a select of a component the format lacks
/// among the `channels` written.
fn descriptor_format(
    instruction: &Instruction,
    word3: u32,
    channels: u32,
) -> Result<(&'static orbistoun_shader::BufferFormat, [u32; 4], u32), TranslateError> {
    let formats =
        BUFFER_FORMATS.get_or_init(|| orbistoun_shader::FormatTable::builtin().unwrap_or_default());
    let format = formats.get(word3 >> DESCRIPTOR_FORMAT_SHIFT & 0x7F).ok_or(
        TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format access's descriptor names a format code with no meaning",
        },
    )?;
    let components = u32::try_from(format.components()).unwrap_or(u32::MAX);
    let selects = [word3 & 7, word3 >> 3 & 7, word3 >> 6 & 7, word3 >> 9 & 7];
    if selects
        .iter()
        .take(channels as usize)
        .any(|&select| !matches!(select, 0 | 1) && !(4..4 + components).contains(&select))
    {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a format access selects a component its format does not have, or a reserved select",
        });
    }
    // The element's bits: whole words for a plain format, the packed width otherwise.
    let total_bits = if format.is_plain_words() {
        components * 32
    } else {
        packed_format_admitted(instruction, format, true)?
    };
    Ok((format, selects, total_bits))
}

/// One narrow integer component pulled out of a packed word, sign-extended when `signed`.
///
/// The component occupies bits `bit..bit + width` of `packed`, with `width < 32`. Unsigned: shift
/// down and mask. Signed: shift its high bit up to bit 31, then shift arithmetically right. Both
/// are exact.
fn packed_integer_component<M: Model + ?Sized>(
    model: &mut M,
    packed: Id,
    bit: u32,
    width: u32,
    signed: bool,
) -> Id {
    if signed {
        let left = 32 - bit - width;
        let up = if left == 0 {
            packed
        } else {
            let amount = model.constant(left);
            model.binary(op::SHIFT_LEFT_LOGICAL, packed, amount)
        };
        let right = model.constant(32 - width);
        model.binary(op::SHIFT_RIGHT_ARITHMETIC, up, right)
    } else {
        let shifted = if bit == 0 {
            packed
        } else {
            let amount = model.constant(bit);
            model.binary(op::SHIFT_RIGHT_LOGICAL, packed, amount)
        };
        let mask = model.constant((1u32 << width) - 1);
        model.binary(op::BITWISE_AND, shifted, mask)
    }
}

/// Extracts one component from the packed word and produces the value the register holds: the
/// integer for the integer kinds, or a float's bits for the converting ones.
///
/// The kinds handled are exactly those [`packed_buffer_memory`] admits, so the `unreachable!`
/// guards a case that cannot arise.
fn packed_component<M: Model + ?Sized>(
    model: &mut M,
    packed: Id,
    bit: u32,
    width: u32,
    kind: orbistoun_shader::ComponentKind,
) -> Id {
    let field = packed_integer_component(model, packed, bit, width, signed_kind(kind));
    convert_field(model, field, FieldWidth::Fixed(width), kind)
}

/// Whether a component of `kind` is read sign-extended from its field.
fn signed_kind(kind: orbistoun_shader::ComponentKind) -> bool {
    use orbistoun_shader::ComponentKind;
    matches!(
        kind,
        ComponentKind::Sint | ComponentKind::Snorm | ComponentKind::Sscaled
    )
}

/// How wide a component's field is: known when the module is translated, or read from the
/// descriptor as the module runs (D967), when it is under thirty-two bits for every kind but a
/// whole-word one, whose field is the word.
#[derive(Clone, Copy)]
enum FieldWidth {
    Fixed(u32),
    Running(Id),
}

/// `2^bits - 1` for the width `bits` less `less`, for the normalised kinds' maxima.
fn field_maximum<M: Model + ?Sized>(model: &mut M, width: FieldWidth, less: u32) -> Id {
    match width {
        FieldWidth::Fixed(width) => model.constant((1u32 << (width - less)) - 1),
        FieldWidth::Running(width) => {
            let (one, less) = (model.constant(1), model.constant(less));
            let bits = model.binary(op::ISUB, width, less);
            let thirty_one = model.constant(31);
            let bits = model.binary(op::BITWISE_AND, bits, thirty_one);
            let power = model.binary(op::SHIFT_LEFT_LOGICAL, one, bits);
            model.binary(op::ISUB, power, one)
        }
    }
}

/// `GLSL.std.450` `Fma`'s instruction number: `a * b + c` rounded once.
const GLSL_FMA: u32 = 50;

/// A normalised component's field over its maximum, as float bits. A maximum compiled in is a
/// constant the driver divides by exactly; one read as the module runs (D967) it divides by
/// through a reciprocal, a unit in the last place off - 255 over 255 came back just under 1.0 - so
/// the quotient is refined once by its remainder, fused (`q + (a - q * b) / b`), which rounds it
/// correctly for the fields' integers.
fn normalised<M: Model + ?Sized>(
    model: &mut M,
    (field, maximum): (Id, Id),
    width: FieldWidth,
) -> Id {
    let quotient = model.f32_binary(op::FDIV, field, maximum);
    if matches!(width, FieldWidth::Fixed(_)) {
        return quotient;
    }
    let (u32_type, f32_type, set) = (model.u32_type(), model.f32_type(), model.glsl_set());
    let (one, zero) = (model.constant(1.0f32.to_bits()), model.constant(0));
    let reciprocal = model.f32_binary(op::FDIV, one, maximum);
    let negated = model.f32_binary(op::FSUB, zero, quotient);
    let [a, b, q, y, negated] =
        [field, maximum, quotient, reciprocal, negated].map(|bits| model.as_float(bits));
    let builder = model.builder();
    let remainder = builder.ext_inst(f32_type, set, GLSL_FMA, &[negated, b, a]);
    let refined = builder.ext_inst(f32_type, set, GLSL_FMA, &[remainder, y, q]);
    let bits = builder.id();
    builder.function(op::BITCAST, &[u32_type.0, bits.0, refined.0]);
    bits
}

/// What the register holds for a component's `field` - zero-extended, or sign-extended for the
/// signed kinds ([`signed_kind`]) - of `width` bits: the integer for the integer kinds, or a
/// float's bits for the converting ones. One conversion for a format the module is translated for
/// and one the descriptor names as it runs (D967).
///
/// The kinds handled are exactly those [`packed_buffer_memory`] admits, so the `unreachable!`
/// guards a case that cannot arise.
fn convert_field<M: Model + ?Sized>(
    model: &mut M,
    field: Id,
    width: FieldWidth,
    kind: orbistoun_shader::ComponentKind,
) -> Id {
    use orbistoun_shader::ComponentKind;
    match kind {
        ComponentKind::Uint | ComponentKind::Sint => field,
        // A UNORM component is its unsigned field over the field's maximum, in 0.0..=1.0. The
        // maximum is converted from the integer domain the same way the field is.
        ComponentKind::Unorm => {
            let field_f = model.unsigned_to_float_bits(field);
            let maximum = field_maximum(model, width, 0);
            let maximum_f = model.unsigned_to_float_bits(maximum);
            normalised(model, (field_f, maximum_f), width)
        }
        // A SNORM component is its signed field over 2^(width-1) - 1, in -1.0..=1.0. The
        // most-negative code lands just past -1.0 (-128/127 for a byte) and the reference pins it
        // at -1.0, so the low end is clamped by selecting between the two bit patterns.
        ComponentKind::Snorm => {
            let field_f = model.signed_to_float_bits(field);
            let maximum = field_maximum(model, width, 1);
            let maximum_f = model.unsigned_to_float_bits(maximum);
            let normalized = normalised(model, (field_f, maximum_f), width);
            let minus_one = model.constant((-1.0f32).to_bits());
            let value_f = model.as_float(normalized);
            let low_f = model.as_float(minus_one);
            let below = model.compare(op::FORD_LESS_THAN, value_f, low_f);
            pick(model, below, minus_one, normalized)
        }
        // USCALED and SSCALED are UNORM and SNORM without the division: the field's value as a
        // float, so an eight-bit 255 is 255.0. No range, so no clamp.
        ComponentKind::Uscaled => model.unsigned_to_float_bits(field),
        ComponentKind::Sscaled => model.signed_to_float_bits(field),
        // A float, by one of two routes: at width 16 an IEEE half widened by `FConvert`; at 11 and
        // 10 bits (formats like `10_11_11`) a sign-less packed float built by hand. A width read as
        // the module runs takes whichever its width names, and a whole word as it is.
        ComponentKind::Float => match width {
            FieldWidth::Fixed(16) => model.half_to_float_bits(field),
            FieldWidth::Fixed(width) => model.narrow_float_to_float_bits(field, width),
            FieldWidth::Running(width) => {
                let half = model.half_to_float_bits(field);
                let eleven = model.narrow_float_to_float_bits(field, 11);
                let ten = model.narrow_float_to_float_bits(field, 10);
                let mut value = field;
                for (bits, converted) in [(10, ten), (11, eleven), (16, half)] {
                    let bits = model.constant(bits);
                    let is = model.compare(op::IEQUAL, width, bits);
                    value = model.select(is, converted, value);
                }
                value
            }
        },
        // sRGB is a transfer curve rather than a scale; `packed_buffer_memory` refuses it before a
        // lane is emitted, and a format the descriptor names is not converted (D967).
        ComponentKind::Srgb => {
            unreachable!(
                "packed_buffer_memory admits only the integer, normalised, scaled and half kinds"
            )
        }
    }
}

/// Whether a packed format is one [`packed_buffer_memory`] can translate, and its width in bits.
///
/// Translated kinds, all within whole words: `UINT`/`SINT` (exact bit fields), `UNORM`/`SNORM`,
/// `USCALED`/`SSCALED`, and `FLOAT` at widths 16, 11 and 10. `SRGB`, other float widths and
/// components spanning words are refused with a detail naming which. Every admitted kind has
/// arithmetic in [`packed_component`], whose `unreachable!` catches a kind that bypassed this.
fn packed_format_admitted(
    instruction: &Instruction,
    format: &orbistoun_shader::BufferFormat,
    loading: bool,
) -> Result<u32, TranslateError> {
    use orbistoun_shader::ComponentKind;

    let total_bits: u32 = format.widths.iter().sum();
    // Floats are admitted at 16 bits (IEEE halves) and at 11 and 10 bits (the sign-less packed
    // floats `narrow_float_to_float_bits` widens, pinned by `MEASURED_10_11_11` in
    // `tests/execute.rs`). Other float widths are refused: their bias and subnormal rules are
    // unmeasured.
    let known_float_widths = format.kind == ComponentKind::Float
        && format
            .widths
            .iter()
            .all(|&width| matches!(width, 16 | 11 | 10));
    let convertible = matches!(
        format.kind,
        ComponentKind::Uint
            | ComponentKind::Sint
            | ComponentKind::Unorm
            | ComponentKind::Snorm
            | ComponentKind::Uscaled
            | ComponentKind::Sscaled
    ) || known_float_widths;

    // An element is a little-endian byte sequence with component zero first, so the component at
    // bit `b` is in word `b / 32` at bit `b % 32`. A component spanning a word boundary would need
    // a rule nothing has measured; no format in the table has one, and it is refused.
    let mut bit = 0;
    let straddles = format.widths.iter().any(|&width| {
        let crosses = bit / 32 != (bit + width - 1) / 32;
        bit += width;
        crosses
    });

    // A store is admitted only for the packed 10/11-bit floats measured for it
    // (`MEASURED_10_11_11_STORE` in `tests/execute.rs`), which fill exactly one word and pack by
    // clamp-and-truncate (`Model::float_to_narrow_float_bits`). Other store conversions have
    // unmeasured rounding and saturation rules.
    let packed_float_store = format.kind == ComponentKind::Float
        && total_bits == 32
        && format.widths.iter().all(|&width| matches!(width, 11 | 10));
    let direction_admitted = loading || packed_float_store;

    if !direction_admitted || total_bits > ELEMENT_BITS || straddles || !convertible {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a packed typed buffer format needing component conversion ",
                "(integer, normalised, scaled and half-float loads, and the packed ",
                "10/11-bit float stores, whose components do not span a word are translated so far)"
            ),
        });
    }
    Ok(total_bits)
}

/// The widest element a packed typed access can move: four whole-word components, the four
/// registers a `_xyzw` access names.
const ELEMENT_BITS: u32 = 128;

fn packed_buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    format: &orbistoun_shader::BufferFormat,
    loading: bool,
) -> Result<(), TranslateError> {
    let total_bits = packed_format_admitted(instruction, format, loading)?;

    // Operands and addressing modifiers are read as the untyped path reads them; only the
    // per-component work differs.
    let [data, vaddr, resource_operand, soffset] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access does not have four operands",
        });
    };
    let Operand::Vector(register) = data else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access names something other than a vector register for its data",
        });
    };
    let register = u32::from(*register);
    let Operand::Scalar(resource_base) = resource_operand else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant is not in scalar registers",
        });
    };
    let resource_base = u32::from(*resource_base);
    if resource_base + 4 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant runs past the end of the register file",
        });
    }

    let word = instruction.word;
    let literal_offset = word & 0xFFF;
    let offen = word & (1 << 12) != 0;
    let idxen = word & (1 << 13) != 0;

    let resource = read_buffer_resource(model, resource_base);
    let (slot, addressed, sampled) = buffer_reach(model, instruction, (loading, false), resource)?;
    let reach = reach_of(model, slot, &resource);
    let flags = model.read_scalar(resource_base + 3);
    let instruction_offset = model.constant(literal_offset);

    // The element is bounds-checked once, whole, then read a word at a time; a per-word check would
    // let half an element be read at the end of a buffer.
    let element_bytes = total_bits.div_ceil(8);
    let element_words = total_bits.div_ceil(32);

    for lane in running_lanes(model) {
        let scalar_offset = model.read_source(instruction, soffset, lane)?;
        let (vindex, voffset) = match (idxen, offen) {
            (false, false) => (None, None),
            (true, false) => (Some(model.read_source(instruction, vaddr, lane)?), None),
            (false, true) => (None, Some(model.read_source(instruction, vaddr, lane)?)),
            (true, true) => {
                let Operand::Vector(first) = vaddr else {
                    return Err(TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: concat!(
                            "a buffer access with both address modifiers does not ",
                            "name a vector register pair"
                        ),
                    });
                };
                let base = *first;
                let index = model.read_source(instruction, &Operand::Vector(base), lane)?;
                let offset = model.read_source(instruction, &Operand::Vector(base + 1), lane)?;
                (Some(index), Some(offset))
            }
        };

        let zero = model.constant(0);
        let offset_term = voffset.unwrap_or(zero);
        let offset = model.add(instruction_offset, offset_term);
        let index = vindex.unwrap_or(zero);

        let base = buffer_address(
            model,
            &addressed,
            scalar_offset,
            instruction_offset,
            voffset,
            vindex,
        );

        let outside = buffer_out_of_bounds(model, &resource, flags, offset, index, element_bytes);
        note_sampled_escape(model, sampled, (base, element_bytes), outside, Some(lane));

        // `widths` lists components as the format's name does, highest bits first, so the last
        // entry is the component at bit 0 (`x`) and both directions walk them in reverse.
        // `MEASURED_10_11_11` in `tests/execute.rs` pins this: in `10_11_11`, `x` is an eleven-bit
        // channel at bit 0.
        if loading {
            let packed = read_element(model, reach, base, (element_words, (outside, Some(lane))))?;
            let mut bit = 0u32;
            for (component, &width) in format.widths.iter().rev().enumerate() {
                let register_component = register
                    + u32::try_from(component).expect("a format has at most four components");
                // `packed_format_admitted` has refused anything spanning a word, so the component
                // is inside this one.
                let value = packed_component(
                    model,
                    packed[(bit / 32) as usize],
                    bit % 32,
                    width,
                    format.kind,
                );
                // Out of range reads zero, exactly as the untyped path answers it.
                let kept = pick(model, outside, zero, value);
                model.write_vector_lane(register_component, lane, kept);
                bit += width;
            }
        } else {
            // A store is admitted only for a packed float that is exactly one word; `packed_store`
            // assembles and writes it.
            packed_store(model, instruction, data, format, base, outside, lane)?;
        }
    }
    model.count();
    Ok(())
}

/// Packs a lane's channels and writes the one word a packed float store produces.
///
/// The store half of [`packed_buffer_memory`]'s per-lane body. Each channel is read from its
/// register, packed by clamp-and-truncate ([`Model::float_to_narrow_float_bits`]) and shifted into
/// place; out of bounds the previous contents are kept, as the untyped store does.
fn packed_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    data: &Operand,
    format: &orbistoun_shader::BufferFormat,
    base: Id,
    outside: Id,
    lane: u32,
) -> Result<(), TranslateError> {
    let mut assembled = model.constant(0);
    let mut bit = 0u32;
    for (component, &width) in format.widths.iter().rev().enumerate() {
        let source = step_operand(
            data,
            u32::try_from(component).expect("at most four components"),
        )?;
        let value_bits = model.read_source(instruction, &source, lane)?;
        let field = model.float_to_narrow_float_bits(value_bits, width);
        let shift = model.constant(bit);
        let placed = model.binary(op::SHIFT_LEFT_LOGICAL, field, shift);
        assembled = model.binary(op::BITWISE_OR, assembled, placed);
        bit += width;
    }
    let word = model.word_index(base);
    let previous = model.read_memory(word);
    let kept = pick(model, outside, previous, assembled);
    write_guarded(model, base, kept, lane);
    Ok(())
}

/// Translates the untyped buffer accesses.
///
/// `buffer_load_dword`-family and `buffer_store_dword`-family accesses through a resource constant
/// in four scalar registers. Swizzled buffers and thread-id addressing are forced out of bounds at
/// run time (D147). The typed accesses share the body through [`buffer_access`]; see
/// [`typed_buffer_memory`].
fn buffer_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // Untyped: no format and no conversion, so the name adds only a width. `dwordx2`/`x3`/`x4` move
    // that many consecutive dwords into consecutive registers, as `buffer_access` does per
    // component.
    let loading = name.starts_with("buffer_load");
    // A sixteen-bit load reads its half word zero-extended into one register.
    if name == "buffer_load_ushort" {
        return buffer_access(model, instruction, true, (1, Some(2)));
    }
    // A byte or half store merges into the word holding it (D977).
    match name {
        "buffer_store_byte" => return buffer_access(model, instruction, false, (1, Some(1))),
        "buffer_store_short" => return buffer_access(model, instruction, false, (1, Some(2))),
        _ => {}
    }
    let components = match name.rsplit_once("dword") {
        Some((_, "")) => 1,
        Some((_, "x2")) => 2,
        Some((_, "x3")) => 3,
        Some((_, "x4")) => 4,
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an untyped buffer access names a width this does not translate",
            });
        }
    };
    buffer_access(model, instruction, loading, (components, None))
}

/// A buffer atomic's record index and offset in `lane`, as a load reads them: one register for
/// either, a pair, index first, for both (AMD's published RDNA instruction set).
fn atomic_index_and_offset<M: Model + ?Sized>(
    model: &mut M,
    (instruction, vaddr): (&Instruction, &Operand),
    (idxen, offen): (bool, bool),
    lane: u32,
) -> Result<(Option<Id>, Option<Id>), TranslateError> {
    Ok(match (idxen, offen) {
        (false, false) => (None, None),
        (true, false) => (Some(model.read_source(instruction, vaddr, lane)?), None),
        (false, true) => (None, Some(model.read_source(instruction, vaddr, lane)?)),
        (true, true) => {
            let Operand::Vector(first) = vaddr else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a buffer atomic with both address modifiers does not name a register pair",
                });
            };
            let index = model.read_source(instruction, &Operand::Vector(*first), lane)?;
            let offset = model.read_source(instruction, &Operand::Vector(*first + 1), lane)?;
            (Some(index), Some(offset))
        }
    })
}

/// The one-word buffer atomics through a dispatch's bound buffer (D919) - `buffer_atomic_add`,
/// `_sub`, `_and`, `_or`, `_xor`, `_umin`, `_umax`, `_smin`, `_smax`, `_fmin` and `_fmax`: one word
/// at the address and bounds an untyped access has, combined as one indivisible step - a float
/// minimum or maximum as two integer atomics split by the value's sign - answering what it held
/// into the data register where `GLC` (bit 14) asks. An
/// access out of bounds leaves the word and returns zero. `buffer_atomic_swap_x2` and
/// `buffer_atomic_or_x2` do the same over eight bytes, an exchange or an or of the data register
/// pair, low word first (AMD's published RDNA 2 instruction set, MUBUF), as one 64-bit step: Grand
/// Theft Auto V's in-world scan publishes each group's flag and sum as one such pair and polls it
/// with an or of zero, so its two words must never be seen apart (D955).
fn buffer_atomic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let [
        Operand::Vector(register),
        vaddr,
        Operand::Scalar(resource_base),
        soffset,
    ] = instruction.operands.as_slice()
    else {
        return Err(refuse(
            "a buffer atomic is not a data register, an address, a descriptor and an offset",
        ));
    };
    let (register, resource_base) = (u32::from(*register), u32::from(*resource_base));
    if resource_base + 4 > SCALAR_REGISTERS {
        return Err(refuse(
            "a buffer resource constant runs past the end of the register file",
        ));
    }
    // A 64-bit atomic's data is a register pair, low word first, and it reaches eight bytes.
    let bytes = if name.ends_with("_x2") { 8 } else { 4 };
    if register + bytes / 4 > VECTOR_REGISTERS {
        return Err(refuse(
            "a buffer atomic's data runs past the end of the vector register file",
        ));
    }
    let word = instruction.word;
    let (literal_offset, offen, idxen) = (word & 0xFFF, word & 1 << 12 != 0, word & 1 << 13 != 0);
    let returning = word & 1 << 14 != 0;
    let resource = read_buffer_resource(model, resource_base);
    let (slot, addressed, sampled) = buffer_reach(model, instruction, (false, true), resource)?;
    let Some(slot) = slot else {
        return Err(refuse(
            "a buffer atomic through the memory window rather than a bound buffer (D919)",
        ));
    };
    let flags = model.read_scalar(resource_base + 3);
    let instruction_offset = model.constant(literal_offset);
    for lane in running_lanes(model) {
        let scalar_offset = model.read_source(instruction, soffset, lane)?;
        let (vindex, voffset) =
            atomic_index_and_offset(model, (instruction, vaddr), (idxen, offen), lane)?;
        let zero = model.constant(0);
        let offset = model.add(instruction_offset, voffset.unwrap_or(zero));
        let index = vindex.unwrap_or(zero);
        let address = buffer_address(
            model,
            &addressed,
            scalar_offset,
            instruction_offset,
            voffset,
            vindex,
        );
        let outside = buffer_out_of_bounds(model, &resource, flags, offset, index, bytes);
        note_sampled_escape(model, sampled, (address, bytes), outside, Some(lane));
        let bool_type = model.bool_type();
        let b = model.builder();
        let landed = b.id();
        b.function(op::LOGICAL_NOT, &[bool_type.0, landed.0, outside.0]);
        let two = model.constant(2);
        let word_index = model.binary(op::SHIFT_RIGHT_LOGICAL, address, two);
        let held = atomic_answer(
            model,
            instruction,
            name,
            (slot, register, lane),
            (word_index, landed),
        )?;
        if returning {
            for (at, held) in (register..).zip(held) {
                let kept = pick(model, outside, zero, held);
                model.write_vector_lane(at, lane, kept);
            }
        }
    }
    model.count();
    Ok(())
}

/// One lane's buffer atomic through the bound buffer at `slot`, its data from `register` up, and
/// what the word or pair held, a word a register: a pair through the 64-bit view, a float minimum
/// or maximum as two integer atomics by the value's sign (D919), and a sum or an or as one.
fn atomic_answer<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    (slot, register, lane): (u32, u32, u32),
    (word_index, landed): (Id, Id),
) -> Result<Vec<Id>, TranslateError> {
    let value = model.read_source(instruction, &Operand::Vector(address_of(register)), lane)?;
    let pair = match name {
        "buffer_atomic_swap_x2" => Some(op::ATOMIC_EXCHANGE),
        "buffer_atomic_or_x2" => Some(op::ATOMIC_OR),
        _ => None,
    };
    if let Some(opcode) = pair {
        let high = Operand::Vector(address_of(register + 1));
        let high = model.read_source(instruction, &high, lane)?;
        let held = model.atomic_draw_buffer_pair(
            slot,
            (word_index, opcode, [value, high]),
            landed,
            lane,
        )?;
        return Ok(held.to_vec());
    }
    let float = match name {
        "buffer_atomic_fmin" => Some(false),
        "buffer_atomic_fmax" => Some(true),
        _ => None,
    };
    let Some(maximum) = float else {
        // Each folds in what changes nothing where the lane does not land ([`atomic_identity`]).
        let opcode = match name {
            "buffer_atomic_or" => op::ATOMIC_OR,
            "buffer_atomic_and" => op::ATOMIC_AND,
            "buffer_atomic_xor" => op::ATOMIC_XOR,
            "buffer_atomic_sub" => op::ATOMIC_I_SUB,
            "buffer_atomic_umin" => op::ATOMIC_U_MIN,
            "buffer_atomic_umax" => op::ATOMIC_U_MAX,
            "buffer_atomic_smin" => op::ATOMIC_S_MIN,
            "buffer_atomic_smax" => op::ATOMIC_S_MAX,
            _ => op::ATOMIC_I_ADD,
        };
        let held = model.atomic_draw_buffer(slot, (word_index, opcode, value), landed, lane)?;
        return Ok(vec![held]);
    };
    let [(first, kept), (second, other)] = float_atomic_parts(model, value, maximum);
    let by_sign = model.atomic_draw_buffer(slot, (word_index, first, kept), landed, lane)?;
    let reversed = model.atomic_draw_buffer(slot, (word_index, second, other), landed, lane)?;
    let (sign_bit, zero) = (model.constant(0x8000_0000), model.constant(0));
    let sign = model.binary(op::BITWISE_AND, value, sign_bit);
    let positive = model.compare(op::IEQUAL, sign, zero);
    Ok(vec![model.select(positive, by_sign, reversed)])
}

/// The body shared by the typed and untyped buffer accesses.
///
/// They differ only in whether a format was checked and how many components move, and share the
/// descriptor, the addressing equation and both bounds checks. `components` are consecutive dwords
/// at consecutive addresses, written to consecutive registers. `narrow`, for a load of fewer bytes
/// than a word, is how many: read from the access's own byte, zero-extended.
fn buffer_access<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    loading: bool,
    (components, narrow): (u32, Option<u32>),
) -> Result<(), TranslateError> {
    let [data, vaddr, resource_operand, soffset] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer access does not have four operands",
        });
    };
    let Operand::Vector(register) = data else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a buffer access names something other than a vector register for ",
                "its data"
            ),
        });
    };
    let register = u32::from(*register);
    let Operand::Scalar(resource_base) = resource_operand else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant is not in scalar registers",
        });
    };
    let resource_base = u32::from(*resource_base);
    if resource_base + 4 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a buffer resource constant runs past the end of the register file",
        });
    }

    // The addressing modifiers are bits of the instruction rather than operands: the
    // literal offset in bits 11:0, `offen` at 12 and `idxen` at 13.
    let word = instruction.word;
    let literal_offset = word & 0xFFF;
    let offen = word & (1 << 12) != 0;
    let idxen = word & (1 << 13) != 0;

    let resource = read_buffer_resource(model, resource_base);
    let (slot, addressed, sampled) = buffer_reach(model, instruction, (loading, true), resource)?;
    let reach = reach_of(model, slot, &resource);
    let flags = model.read_scalar(resource_base + 3);
    let instruction_offset = model.constant(literal_offset);

    for lane in running_lanes(model) {
        let scalar_offset = model.read_source(instruction, soffset, lane)?;

        // With both modifiers the address register is a pair, index first then offset; with one it
        // is a single register holding that one.
        let (vindex, voffset) = match (idxen, offen) {
            (false, false) => (None, None),
            (true, false) => (Some(model.read_source(instruction, vaddr, lane)?), None),
            (false, true) => (None, Some(model.read_source(instruction, vaddr, lane)?)),
            (true, true) => {
                let Operand::Vector(first) = vaddr else {
                    return Err(TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: concat!(
                            "a buffer access with both address modifiers does not ",
                            "name a vector register pair"
                        ),
                    });
                };
                let base = *first;
                let index = model.read_source(instruction, &Operand::Vector(base), lane)?;
                let offset = model.read_source(instruction, &Operand::Vector(base + 1), lane)?;
                (Some(index), Some(offset))
            }
        };

        let zero = model.constant(0);
        let offset_term = voffset.unwrap_or(zero);
        let offset = model.add(instruction_offset, offset_term);
        let index = vindex.unwrap_or(zero);

        let base = buffer_address(
            model,
            &addressed,
            scalar_offset,
            instruction_offset,
            voffset,
            vindex,
        );

        for component in 0..components {
            // Consecutive dwords. The step is added to both the address and the offset the bounds
            // check sees, so a multi-channel access at the end of a buffer is checked per word.
            let step = model.constant(component * 4);
            let address = model.add(base, step);
            let offset = model.add(offset, step);

            // Two bounds: the buffer's record count and the module's guest-memory window. An access
            // can satisfy one and not the other.
            let outside =
                buffer_out_of_bounds(model, &resource, flags, offset, index, narrow.unwrap_or(4));
            let register = register + component;
            // A bound buffer holds every word its descriptor admits; only the window can be
            // escaped, and a buffer placed from a sample (D951).
            if model.exact_memory() && slot.is_none() {
                let escaped = buffer_escape(model, &resource, address, outside);
                model.note_escape(escaped, Some(lane));
            }
            let bytes = narrow.unwrap_or(4);
            note_sampled_escape(model, sampled, (address, bytes), outside, Some(lane));

            if loading {
                // Out of range reads zero, as the reference states.
                let value = read_access(model, reach, address, (narrow, (outside, Some(lane))))?;
                let kept = pick(model, outside, zero, value);
                model.write_vector_lane(register, lane, kept);
            } else {
                let source = step_operand(data, component)?;
                let value = model.read_source(instruction, &source, lane)?;
                store_access(model, slot, (address, outside), (value, narrow), lane)?;
            }
        }
    }
    model.count();
    Ok(())
}

/// What an untyped load reads at `address`: a word, or the `narrow` bytes fewer than one,
/// zero-extended, checked as [`read_reached`] checks it.
fn read_access<M: Model + ?Sized>(
    model: &mut M,
    reach: Option<Reach>,
    address: Id,
    (narrow, checked): (Option<u32>, (Id, Option<u32>)),
) -> Result<Id, TranslateError> {
    match narrow {
        Some(bytes) => read_narrow(model, reach, address, (bytes, checked)),
        None => read_reached(model, reach, address, checked),
    }
}

/// The `bytes` at byte `address`, fewer than a word, zero-extended: through the window, or from the
/// draw buffer at `slot` at the access's own byte.
fn read_narrow<M: Model + ?Sized>(
    model: &mut M,
    slot: Option<Reach>,
    address: Id,
    (bytes, checked): (u32, (Id, Option<u32>)),
) -> Result<Id, TranslateError> {
    let element = read_element(model, slot, address, (1, checked))?;
    let mask = model.constant((1_u32 << (8 * bytes)) - 1);
    Ok(model.binary(op::BITWISE_AND, element[0], mask))
}

/// Whether an exact module's buffer access leaves the memory it can place: a descriptor whose
/// addressing is not modelled (forced out of bounds, which the hardware would not do), or an
/// access the buffer's bounds admit at an address outside the window - below or past it, or in
/// another four-gigabyte span than the window's.
fn buffer_escape<M: Model + ?Sized>(
    model: &mut M,
    resource: &BufferResource,
    address: Id,
    outside: Id,
) -> Id {
    let refused = model.is_not_zero(resource.unsupported);
    let inside = model.address_within_window(address);
    let window_high = model.constant(model.memory_high());
    let same_span = model.compare(op::IEQUAL, resource.high, window_high);
    let placed = both(model, inside, same_span);
    let bool_type = model.bool_type();
    let b = model.builder();
    let admitted = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, admitted.0, outside.0]);
    let unplaced = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, unplaced.0, placed.0]);
    let lost = both(model, admitted, unplaced);
    model.either(refused, lost)
}

/// Refuses any source modifier on a long-form integer instruction: negate and absolute value
/// have no integer meaning here, and a clamp would saturate.
fn no_integer_modifiers(instruction: &Instruction, sources: usize) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read(instruction, false)?;
    if (0..sources).any(|source| modifiers.touches(source)) {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a source modifier on long-form integer arithmetic, which is not translated",
        });
    }
    Ok(())
}

/// The SDWA marker a short vector instruction's first source reads (`aco_assembler.cpp:1185`).
pub(crate) const SDWA_MARKER: u32 = 249;

/// The SDWA select codes for a register's low and high sixteen bits (`aco_ir.h:1679-1688`).
const SDWA_WORD_0: u32 = 4;
/// See [`SDWA_WORD_0`].
const SDWA_WORD_1: u32 = 5;

/// The short-form compares translated in their SDWA form over whole registers.
pub const SDWA_COMPARE: &[&str] = &[
    // A class test, its float source negated or made absolute as the word says (D911).
    "v_cmp_class_f32_e32",
    // Integer compares, which take no negate or absolute value.
    "v_cmp_eq_i32_e32",
    "v_cmp_ne_i32_e32",
    "v_cmp_lt_u32_e32",
    "v_cmp_le_u32_e32",
    "v_cmp_eq_u32_e32",
    "v_cmp_ne_u32_e32",
    "v_cmp_ge_u32_e32",
    "v_cmp_gt_u32_e32",
    "v_cmp_lt_i32_e32",
    "v_cmp_le_i32_e32",
    "v_cmp_gt_i32_e32",
    "v_cmp_ge_i32_e32",
    "v_cmp_lt_f32_e32",
    "v_cmp_eq_f32_e32",
    "v_cmp_le_f32_e32",
    "v_cmp_gt_f32_e32",
    "v_cmp_ge_f32_e32",
    "v_cmp_neq_f32_e32",
    "v_cmp_ngt_f32_e32",
    "v_cmp_nlt_f32_e32",
    "v_cmp_lg_f32_e32",
    "v_cmp_nge_f32_e32",
    "v_cmp_nlg_f32_e32",
    "v_cmp_nle_f32_e32",
    "v_cmp_u_f32_e32",
    // The forms whose answer narrows the execution mask: every float compare above's.
    "v_cmpx_lt_f32_e32",
    "v_cmpx_eq_f32_e32",
    "v_cmpx_le_f32_e32",
    "v_cmpx_gt_f32_e32",
    "v_cmpx_ge_f32_e32",
    "v_cmpx_neq_f32_e32",
    "v_cmpx_ngt_f32_e32",
    "v_cmpx_nlt_f32_e32",
    "v_cmpx_lg_f32_e32",
    "v_cmpx_nge_f32_e32",
    "v_cmpx_nlg_f32_e32",
    "v_cmpx_nle_f32_e32",
];

/// Whether a short-form compare is translated in its SDWA form: one [`SDWA_COMPARE`] lists, or any
/// thirty-two-bit compare, into `vcc` or the execution mask, whose ordering the plain form reads off
/// its name. SDWA has no sixty-four-bit form (AMD's published RDNA 2 instruction set, SDWA), and
/// the sixteen-bit float compares take the half path.
fn sdwa_compare_admitted(name: &str) -> bool {
    if SDWA_COMPARE.contains(&name) {
        return true;
    }
    let Some(rest) = name
        .strip_prefix("v_cmp_")
        .or_else(|| name.strip_prefix("v_cmpx_"))
    else {
        return false;
    };
    rest.ends_with("_e32")
        && ["_f32_", "_i32_", "_u32_", "_i16_", "_u16_"]
            .iter()
            .any(|kind| rest.contains(kind))
        && compare_op(&widened_compare(&format!("v_cmp_{rest}"))).is_some()
}

/// A sixteen-bit integer compare's name as the thirty-two-bit compare of the same ordering and
/// signedness: its sources, narrowed to their low sixteen bits by [`sixteen_bit_side`], compare as
/// that one does. Any other name is itself.
fn widened_compare(name: &str) -> String {
    name.replacen("_i16_", "_i32_", 1)
        .replacen("_u16_", "_u32_", 1)
}

/// Whether a compare is a sixteen-bit integer one, and if so whether it is signed.
fn sixteen_bit_compare(name: &str) -> Option<bool> {
    if name.contains("_i16_") {
        Some(true)
    } else {
        name.contains("_u16_").then_some(false)
    }
}

/// A sixteen-bit integer compare's side: the low sixteen bits of `value`, sign-extended for a
/// signed compare and zero-extended for an unsigned one, as the operation reads its operand
/// whatever the SDWA select extended it to.
fn sixteen_bit_side<M: Model + ?Sized>(model: &mut M, value: Id, signed: bool) -> Id {
    let sixteen = model.constant(16);
    let raised = model.binary(op::SHIFT_LEFT_LOGICAL, value, sixteen);
    let down = if signed {
        op::SHIFT_RIGHT_ARITHMETIC
    } else {
        op::SHIFT_RIGHT_LOGICAL
    };
    model.binary(down, raised, sixteen)
}

/// A compare in its SDWA form, as ACO encodes one (`aco_assembler.cpp`, its SDWA emission): the
/// sources from the modifier word - the first's code in 7:0, scalar where bit 23 says, the second
/// the instruction's own, scalar where bit 31 says - each negated (20, 28) and made absolute (21,
/// 29) as the word says; the answer into `vcc`, or, where bit 15 is set, into the scalar register
/// pair bits 14:8 name, bit 13 among them. Only a compare into `vcc` has a clamp bit, 13
/// (`aco_assembler.cpp:1193-1199`). Each source is the part of its register its select (18:16,
/// 26:24) names, sign-extended where 19 or 27 says, for an integer source; a float source is read
/// whole, and a select of part of one, or that clamp, is refused. A `cmpx` narrows the execution
/// mask to its answer, as its plain form does.
fn sdwa_compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    word: u32,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let fields = SdwaFields::read(word);
    if word >> 15 & 1 == 0 && word >> 13 & 1 != 0 {
        return Err(refuse("an SDWA compare clamps, which is not translated"));
    }
    // A `cmpx` is its compare whose answer narrows the execution mask, which names no other
    // destination.
    let into_exec = name.strip_prefix("v_cmpx_");
    if into_exec.is_some() && word >> 15 & 1 != 0 {
        return Err(refuse(
            "an SDWA compare into the execution mask names a destination of its own",
        ));
    }
    let compare = into_exec.map_or_else(|| name.to_owned(), |rest| format!("v_cmp_{rest}"));
    // A sixteen-bit integer compare reads its sides' low halves, signed or not, and compares them
    // as the thirty-two-bit compare of its ordering does.
    let sixteen = sixteen_bit_compare(&compare);
    let (opcode, floats) = op_for_compare(instruction, &widened_compare(&compare))?;
    // Negate and absolute value are float modifiers; on an integer compare they are refused. A
    // class test's first source is a float, its second a mask.
    if (!floats && opcode != CLASS_TEST || opcode == CLASS_TEST && word & 0b11 << 28 != 0)
        && word & (0b11 << 20 | 0b11 << 28) != 0
    {
        return Err(refuse(
            "an SDWA integer compare negates or takes the absolute value of a source",
        ));
    }
    // A select of part of a register reads that part's bits, zero- or sign-extended
    // (`sdwa_select`), whatever the source holds: a float source compares those bits as a float,
    // as any operation reads its selected part (D979).
    // A compare's operands begin with the `vcc` it writes; a `cmpx` writes none of its own.
    let second = if into_exec.is_some() { 1 } else { 2 };
    let Some(Operand::Vector(vsrc1)) = instruction.operands.get(second) else {
        return Err(refuse(
            "an SDWA compare's second source is not its register field",
        ));
    };
    let sources = [
        sdwa_operand(instruction, fields.src0, fields.src0_scalar)?,
        sdwa_operand(instruction, u32::from(*vsrc1), fields.src1_scalar)?,
    ];
    let bit = |at: u32| word >> at & 1 != 0;
    let modifiers = Modifiers {
        negate: [bit(20), bit(28), false],
        absolute: [bit(21), bit(29), false],
        clamp: false,
        output_multiplier: 0,
    };
    let zero = model.constant(0);
    let halves = every_lane_bit(model, true, |model, lane| {
        let mut sides = [zero, zero];
        for (index, (side, operand)) in sides.iter_mut().zip(&sources).enumerate() {
            let value = model.read_source(instruction, operand, lane)?;
            let value = sdwa_select(model, instruction, value, fields.select[index])?;
            let value = sixteen.map_or(value, |signed| sixteen_bit_side(model, value, signed));
            let value = apply_modifiers(model, value, modifiers, index);
            *side = if floats { model.as_float(value) } else { value };
        }
        Ok(lane_compare(model, opcode, sides[0], sides[1]))
    })?;
    let halves = running_bits(model, halves)?;
    if bit(15) {
        // The register the word names: a scalar, a pair in a wave of 64; or the condition mask,
        // whole or a wave of 32's half of it.
        let register = word >> 8 & 0x7f;
        let words = if model.lanes() > 32 { 2 } else { 1 };
        match (register, words) {
            (VCC_LOW_CODE, 2) => model.write_lane_mask(VCC_LOW_HALF, halves.0, halves.1)?,
            (VCC_LOW_CODE, _) => write_mask_low(model, VCC_LOW_HALF, halves.0)?,
            (VCC_HIGH_CODE, 1) => write_mask_high(model, VCC_LOW_HALF, halves.0)?,
            _ if register + words <= SCALAR_REGISTERS => {
                model.write_scalar(register, halves.0);
                if words == 2 {
                    model.write_scalar(register + 1, halves.1);
                }
            }
            _ => {
                return Err(refuse(
                    "an SDWA compare's destination is not a scalar register or the condition mask",
                ));
            }
        }
    } else if into_exec.is_some() {
        let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
        let low = model.binary(op::BITWISE_AND, halves.0, running_low);
        let high = model.binary(op::BITWISE_AND, halves.1, running_high);
        write_wave_mask(model, EXEC_LOW_HALF, (low, high))?;
    } else {
        write_wave_mask(model, VCC_LOW_HALF, halves)?;
    }
    model.count();
    Ok(())
}

/// A compare's answer with every lane that does not run zeroed, as the hardware writes it (AMD's
/// published RDNA instruction set, VOPC): its two halves anded with the execution mask's.
fn running_bits<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
) -> Result<(Id, Id), TranslateError> {
    let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    Ok((
        model.binary(op::BITWISE_AND, low, running_low),
        model.binary(op::BITWISE_AND, high, running_high),
    ))
}

/// Whether an instruction's modifier - `marker`, from its first source - is one translated: SDWA
/// on a compare, or on any one- or two-source vector instruction that has an SDWA form (D979,
/// `sdwa::refusal`). What the word asks of that instruction is checked as it is translated.
#[must_use]
pub fn sdwa_translated(marker: u32, instruction: &Instruction, encodings: &EncodingTable) -> bool {
    marker == SDWA_MARKER
        && instruction
            .encoding
            .and_then(|index| encodings.encodings().get(usize::from(index)))
            .and_then(|family| {
                encodings
                    .mnemonic_for(&family.name, instruction.opcode)
                    .map(|name| (family.name.as_str(), name))
            })
            .is_some_and(|(family, name)| {
                (name.starts_with("v_cmp_") && name.ends_with("_f16_e32"))
                    || sdwa_compare_admitted(name)
                    || sdwa::refusal(family, name).is_none()
            })
}

/// The first-source code that marks an instruction's DPP16 form, its second word the DPP fields.
const DPP16_MARKER: u32 = 250;

/// The first-source codes that mark an instruction's DPP8 form, without and with `fi:1`: its second
/// word the source register (7:0) and eight three-bit lane selects (31:8). `llvm-mc` for `gfx1030`
/// encodes `v_mov_b32_dpp v0, v1 dpp8:[7,6,5,4,3,2,1,0]` as `0x7e0002e9 0x05397701`, and with
/// `fi:1` its first byte `0xea`.
const DPP8_MARKER: u32 = 233;
/// See [`DPP8_MARKER`].
const DPP8_FETCH_INACTIVE_MARKER: u32 = 234;

/// The families whose DPP forms are translated: the one- and two-source vector instructions,
/// whose first source the DPP word replaces. A compare has no DPP form on this generation (LLVM's
/// assembler refuses `v_cmp_*_dpp` for `gfx1030`).
const DPP_FAMILIES: [&str; 2] = ["VOP1", "VOP2"];

/// The vector register a DPP form's permuted first source is staged in before the instruction
/// itself runs: one past the 256 an instruction can name, so no guest register is overwritten.
/// An SDWA form stages its first source there too (D979).
pub const DPP_STAGING_REGISTER: u32 = 256;

/// A DPP form, DPP16 or DPP8, as one shape (D979): `word` laid out as a DPP16 word - the source
/// register (7:0), `FETCH_INACTIVE` (18), `BOUND_CTRL` (19), each source's negate and absolute value
/// (23:20), the bank and row masks (27:24, 31:28) - and the lane of its group each lane reads.
///
/// A DPP8 form reads within groups of eight with every row and bank enabled and no source
/// modifier. Its source lane is always within the group; one the execution mask leaves out reads
/// zero unless `fi:1` (LLVM's `AMDGPUModifierSyntax`, `fi` on `dpp8`), so its lane is always
/// written: the DPP16 shape with `BOUND_CTRL` set.
#[derive(Clone, Copy)]
pub(crate) struct DppForm {
    word: u32,
    control: DppControl,
}

impl DppForm {
    /// The form a first source `marker` and modifier word `second` name, or `None` for a marker
    /// that is no DPP form, or a DPP16 row control the instruction set leaves reserved.
    fn read(marker: u32, second: u32) -> Option<Self> {
        match marker {
            DPP16_MARKER => Some(Self {
                word: second,
                control: dpp_control(second)?,
            }),
            DPP8_MARKER | DPP8_FETCH_INACTIVE_MARKER => Some(Self {
                word: (second & 0xff)
                    | u32::from(marker == DPP8_FETCH_INACTIVE_MARKER) << 18
                    | 1 << 19
                    | 0xff << 24,
                control: DppControl::Lanes8(core::array::from_fn(|lane| {
                    (second >> (8 + 3 * lane)) & 7
                })),
            }),
            _ => None,
        }
    }
}

/// Whether an instruction's modifier - `marker`, from its first source - is a DPP form this
/// translates: DPP16 with a row control, or DPP8, on a one- or two-source vector instruction. What
/// the form asks of the instruction is checked as it is translated.
#[must_use]
pub fn dpp_translated(marker: u32, instruction: &Instruction, encodings: &EncodingTable) -> bool {
    instruction
        .encoding
        .and_then(|index| encodings.encodings().get(usize::from(index)))
        .is_some_and(|family| DPP_FAMILIES.contains(&family.name.as_str()))
        && instruction
            .second_word
            .and_then(|second| DppForm::read(marker, second))
            .is_some()
}

/// An instruction's DPP form, DPP16 or DPP8, when it carries one: its modifier word's marker, and
/// the form, `None` within where the marker's word names no form translated.
pub(crate) fn dpp_word(
    encodings: &EncodingTable,
    instruction: &Instruction,
) -> Option<(u32, Option<DppForm>)> {
    let family = encodings
        .encodings()
        .get(usize::from(instruction.encoding?))?;
    let marker = family.modifier_selected(&[instruction.word])?;
    if !matches!(
        marker,
        DPP16_MARKER | DPP8_MARKER | DPP8_FETCH_INACTIVE_MARKER
    ) {
        return None;
    }
    let second = instruction.second_word?;
    Some((marker, DppForm::read(marker, second)))
}

/// The lane each lane of a row of sixteen reads under a DPP16 word's control (16:8), as AMD's
/// published RDNA instruction set lays the word out, or under a DPP8 word's lane selects.
#[derive(Clone, Copy)]
enum DppControl {
    /// `quad_perm:[a,b,c,d]`, below 0x100: two bits per lane, the lane of its quad each reads.
    QuadPermute([u32; 4]),
    /// `row_shl:n`, 0x101-0x10f: the lane `n` above, none past the row's end.
    ShiftLeft(u32),
    /// `row_shr:n`, 0x111-0x11f: the lane `n` below, none before the row's start.
    ShiftRight(u32),
    /// `row_ror:n`, 0x121-0x12f: the lane `n` below, around the row.
    RotateRight(u32),
    /// `row_mirror`, 0x140: the row reversed.
    Mirror,
    /// `row_half_mirror`, 0x141: each half of the row reversed.
    HalfMirror,
    /// `row_share:n`, 0x150-0x15f: lane `n` of the row, for every lane.
    Share(u32),
    /// `row_xmask:n`, 0x160-0x16f: the lane whose place is this one's exclusive-or `n`.
    Xmask(u32),
    /// `dpp8:[a,b,c,d,e,f,g,h]`: the lane of its group of eight each lane reads.
    Lanes8([u32; 8]),
}

impl DppControl {
    /// The place in its row the lane at `place` reads, or `None` when that lies past the row.
    fn source(self, place: u32) -> Option<u32> {
        match self {
            Self::QuadPermute(permute) => Some((place & !3) | permute[(place & 3) as usize]),
            Self::ShiftLeft(by) => (place + by < 16).then_some(place + by),
            Self::ShiftRight(by) => place.checked_sub(by),
            Self::RotateRight(by) => Some((place + 16 - by) & 15),
            Self::Mirror => Some(15 - place),
            Self::HalfMirror => Some((place & 8) | (7 - (place & 7))),
            Self::Share(lane) => Some(lane),
            Self::Xmask(mask) => Some(place ^ mask),
            Self::Lanes8(selects) => Some((place & 8) | selects[(place & 7) as usize]),
        }
    }
}

/// The row control of a DPP16 word; `None` for a control the instruction set leaves reserved.
fn dpp_control(word: u32) -> Option<DppControl> {
    let control = (word >> 8) & 0x1ff;
    let by = control & 0xf;
    Some(match control {
        0..=0xff => DppControl::QuadPermute([0, 1, 2, 3].map(|place| (control >> (2 * place)) & 3)),
        0x101..=0x10f => DppControl::ShiftLeft(by),
        0x111..=0x11f => DppControl::ShiftRight(by),
        0x121..=0x12f => DppControl::RotateRight(by),
        0x140 => DppControl::Mirror,
        0x141 => DppControl::HalfMirror,
        0x150..=0x15f => DppControl::Share(by),
        0x160..=0x16f => DppControl::Xmask(by),
        _ => return None,
    })
}

/// Whether a DPP16 word enables every row (31:28) and every bank (27:24) of the wave.
fn dpp_masks_full(word: u32) -> bool {
    word >> 24 == 0xff
}

/// An instruction in its DPP16 form, `op_dpp d, v, .. row_shr:1` and the like: the instruction,
/// with each lane's first source `v` taken from the lane of its row the control names (AMD's
/// published RDNA instruction set, DPP16). A source lane the execution mask leaves out is not read
/// unless `FETCH_INACTIVE` (18) is set, and one past the row's edge is never read; for either,
/// under `BOUND_CTRL` (19) the source reads 0, and otherwise the lane is not written at all. Nor is
/// a lane whose row (31:28) or bank of four (27:24) the word leaves out.
///
/// The permuted source is staged in [`DPP_STAGING_REGISTER`] and the instruction translated as
/// though it named that register; a lane the rules leave unwritten then gets its destination back.
/// The word's negate and absolute value (20, 21 for the first source; 22, 23 for the second) flip
/// and clear a float or bit-pattern source's top bit (D979): the first after it is permuted, the
/// second staged in the destination, whose old bits are kept apart. An instruction that writes a
/// carry is refused where a lane may be left unwritten, since its carry bit cannot be given back.
fn dpp_form<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (name, marker, form): (&str, u32, Option<DppForm>),
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let first = sdwa::marked_source(instruction, marker).unwrap_or(1);
    let (Some(Operand::Vector(destination)), Some(_), Some(DppForm { word, control })) = (
        instruction.operands.first(),
        instruction.operands.get(first),
        form,
    ) else {
        return Err(refuse(
            "a DPP form other than a row control or a DPP8 select of a source into a vector register is not translated",
        ));
    };
    let destination = u32::from(*destination);
    let (fetch_inactive, bound_zero) = ((word >> 18) & 1 == 1, (word >> 19) & 1 == 1);
    // Every lane is written when every row and bank is, and no lane's source can be refused it: a
    // zero stands in for any source, or every source lies in the row and is read whether it runs.
    let always_written = dpp_masks_full(word)
        && (bound_zero || (fetch_inactive && (0..16).all(|place| control.source(place).is_some())));
    if writes_a_carry(name) && !always_written {
        return Err(refuse(
            "a DPP form that writes a carry and may leave a lane unwritten is not translated",
        ));
    }
    let accumulates = name.contains("mac");
    let (first_flags, second_flags) = ((word >> 20) & 0b11, (word >> 22) & 0b11);
    let (second, source_kinds) = dpp_modified_sources(instruction, name, (word, first))?;
    let zero = model.constant(0);
    // An instruction that accumulates into its destination reads it as a source, so its result
    // cannot be staged apart from it, nor can a modified second source; any other runs its phases
    // as loops over the lanes, where the model runs the whole wave in one invocation.
    if !accumulates && second.is_none() && model.open_lane_loop() {
        return dpp_form_looped(
            model,
            instruction,
            (word, control, first, source_kinds[0]),
            (destination, fetch_inactive, bound_zero),
        );
    }

    // The permuted source, staged; and, where a lane may be left unwritten, each destination lane
    // as it was.
    let fetched = permuted_first_source(model, instruction, word, control)?;
    let keep = !always_written || second.is_some();
    let (inactive_read, zero_bound) = (
        model.constant(u32::from(fetch_inactive)),
        model.constant(u32::from(bound_zero)),
    );
    let mut kept = Vec::new();
    for Fetched {
        lane,
        value,
        running,
        within,
        enabled,
    } in fetched
    {
        // Read: the source lies in the row, and runs or is fetched regardless.
        let readable = model.binary(op::BITWISE_OR, running, inactive_read);
        let readable = model.binary(op::BITWISE_AND, within, readable);
        let readable_bool = model.compare(op::INOT_EQUAL, readable, zero);
        let value = if bound_zero {
            model.select(readable_bool, value, zero)
        } else {
            value
        };
        let value = sdwa::sign_flags(model, value, first_flags, source_kinds[0]);
        model.write_vector_lane(DPP_STAGING_REGISTER, lane, value);
        if keep {
            // Written: the lane's row and bank are enabled, and its source read or bound to zero.
            let written = model.binary(op::BITWISE_OR, readable, zero_bound);
            let written = model.binary(op::BITWISE_AND, enabled, written);
            let written = model.compare(op::INOT_EQUAL, written, zero);
            let before = model.read_source(instruction, &instruction.operands[0], lane)?;
            kept.push((lane, before, written));
        }
        // A modified second source, staged in the destination once its old bits are kept.
        if let Some((_, register)) = second {
            let value = model.read_source(instruction, &Operand::Vector(register), lane)?;
            let value = sdwa::sign_flags(model, value, second_flags, source_kinds[1]);
            model.write_vector_lane(destination, lane, value);
        }
    }

    // The instruction itself, reading its first source from the staged register. The word's
    // first-source field names an ordinary register so the form is not seen again; the operand
    // list is what translation reads.
    let mut plain = instruction.clone();
    plain.operands[first] = Operand::Vector(DPP_STAGING_REGISTER as u16);
    if let Some((at, _)) = second {
        plain.operands[at] = Operand::Vector(destination as u16);
    }
    plain.word = (plain.word & !0x1ff) | 0x100;
    plain.second_word = None;
    self::instruction(model, &plain)?;

    for (lane, before, written) in kept {
        let now = model.read_source(&plain, &plain.operands[0], lane)?;
        let value = model.select(written, now, before);
        model.write_vector_lane(destination, lane, value);
    }
    Ok(())
}

/// What [`dpp_modified_sources`] answers: the place and register of a second source the word
/// modifies, and each source's kind.
type DppSources = (Option<(usize, u16)>, [sdwa::Kind; 2]);

/// A DPP form's sources as its word modifies them (23:20): the second source where the word negates
/// it or takes its absolute value - the register field after the first source at `first`, refused
/// where the instruction accumulates into its destination - and each source's kind. A negate or an
/// absolute value of an integer source is refused.
fn dpp_modified_sources(
    instruction: &Instruction,
    name: &str,
    (word, first): (u32, usize),
) -> Result<DppSources, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let (first_flags, second_flags) = ((word >> 20) & 0b11, (word >> 22) & 0b11);
    let kinds = sdwa::kinds(name);
    let second = match (second_flags, instruction.operands.get(first + 1)) {
        (0, _) => None,
        (_, Some(Operand::Vector(register))) if !name.contains("mac") => {
            Some((first + 1, *register))
        }
        _ => {
            return Err(refuse(
                "a DPP form modifies a second source that is not its register field, or one it accumulates into",
            ));
        }
    };
    for (flags, index) in [(first_flags, 0), (second_flags, 1)] {
        if flags != 0 && !kinds.is_some_and(|(_, sources)| sources[index].signed_by_bit()) {
            return Err(refuse(
                "a DPP form negates or takes the absolute value of an integer source",
            ));
        }
    }
    let source_kinds = kinds.map_or([sdwa::Kind::Bits { bits: 32 }; 2], |(_, sources)| sources);
    Ok((second, source_kinds))
}

/// [`dpp_form`] in three loops over the lanes, the first of which the model has opened: every
/// lane's permuted first source staged, as each reads the source before any lane writes; the
/// instruction over the staged source, its result staged in its place, so the destination stays as
/// it was; then each lane's result into the destination where its row and bank are enabled and its
/// source read or bound to zero, the destination kept elsewhere.
fn dpp_form_looped<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (word, control, first, kind): (u32, DppControl, usize, sdwa::Kind),
    (destination, fetch_inactive, bound_zero): (u32, bool, bool),
) -> Result<(), TranslateError> {
    let zero = model.constant(0);
    let (inactive_read, zero_bound) = (
        model.constant(u32::from(fetch_inactive)),
        model.constant(u32::from(bound_zero)),
    );
    // Read: the source lies in the row, and runs or is fetched regardless.
    let readable = |model: &mut M, fetched: &Fetched| {
        let readable = model.binary(op::BITWISE_OR, fetched.running, inactive_read);
        model.binary(op::BITWISE_AND, fetched.within, readable)
    };
    for fetched in permuted_first_source(model, instruction, word, control)? {
        let value = if bound_zero {
            let read = readable(model, &fetched);
            let read = model.compare(op::INOT_EQUAL, read, zero);
            model.select(read, fetched.value, zero)
        } else {
            fetched.value
        };
        let value = sdwa::sign_flags(model, value, (word >> 20) & 0b11, kind);
        model.write_vector_lane(DPP_STAGING_REGISTER, fetched.lane, value);
    }
    model.close_lane_loop();

    let staged = Operand::Vector(DPP_STAGING_REGISTER as u16);
    let mut plain = instruction.clone();
    plain.operands[0] = staged.clone();
    plain.operands[first] = staged.clone();
    plain.word = (plain.word & !0x1ff) | 0x100;
    plain.second_word = None;
    let looped = lane_looped(model, &plain) && model.open_lane_loop();
    self::instruction(model, &plain)?;
    if looped {
        model.close_lane_loop();
    }

    let looped = model.open_lane_loop();
    for fetched in permuted_first_source(model, instruction, word, control)? {
        // Written: the lane's row and bank are enabled, and its source read or bound to zero.
        let read = readable(model, &fetched);
        let written = model.binary(op::BITWISE_OR, read, zero_bound);
        let written = model.binary(op::BITWISE_AND, fetched.enabled, written);
        let written = model.compare(op::INOT_EQUAL, written, zero);
        let result = model.read_source(&plain, &staged, fetched.lane)?;
        let before = model.read_source(instruction, &instruction.operands[0], fetched.lane)?;
        let value = model.select(written, result, before);
        model.write_vector_lane(destination, fetched.lane, value);
    }
    if looped {
        model.close_lane_loop();
    }
    Ok(())
}

/// One lane's DPP first source, read before anything is written: its value, and as 0 or 1 whether
/// the source lane runs, whether it lies within the row, and whether the lane's own row and bank
/// are enabled.
struct Fetched {
    lane: u32,
    value: Id,
    running: Id,
    within: Id,
    enabled: Id,
}

/// One invocation per lane (D760): the lane it reads under `control`, chosen among its row's
/// sixteen places at run time, whether that lane is in the wave's upper half, and as 0 or 1
/// whether it lies within the row and whether `word`'s row and bank masks enable this lane.
fn lane_source_at_run_time<M: Model + ?Sized>(
    model: &mut M,
    control: DppControl,
    index: Id,
    word: u32,
) -> (Id, Id, Id, Id) {
    let fifteen = model.constant(15);
    let place = model.binary(op::BITWISE_AND, index, fifteen);
    let base = model.binary(op::ISUB, index, place);
    let (mut from_place, mut within) = (model.constant(0), model.constant(0));
    for at in 0..16 {
        let here = model.constant(at);
        let here = model.compare(op::IEQUAL, place, here);
        let (read, inside) = control.source(at).map_or((at, 0), |read| (read, 1));
        let (read, inside) = (model.constant(read), model.constant(inside));
        from_place = model.select(here, read, from_place);
        within = model.select(here, inside, within);
    }
    let from = model.add(base, from_place);
    let thirty_two = model.constant(32);
    let upper = model.compare(op::UGREATER_THAN_EQUAL, from, thirty_two);
    // Enabled: the row's bit of the row mask and the bank's of the bank mask.
    let (four, two, three, one) = (
        model.constant(4),
        model.constant(2),
        model.constant(3),
        model.constant(1),
    );
    let row = model.binary(op::SHIFT_RIGHT_LOGICAL, index, four);
    let row = model.binary(op::BITWISE_AND, row, three);
    let bank = model.binary(op::SHIFT_RIGHT_LOGICAL, index, two);
    let bank = model.binary(op::BITWISE_AND, bank, three);
    let (rows, banks) = (
        model.constant(word >> 28),
        model.constant((word >> 24) & 0xf),
    );
    let row = model.binary(op::SHIFT_RIGHT_LOGICAL, rows, row);
    let bank = model.binary(op::SHIFT_RIGHT_LOGICAL, banks, bank);
    let enabled = model.binary(op::BITWISE_AND, row, bank);
    let enabled = model.binary(op::BITWISE_AND, enabled, one);
    (from, upper, within, enabled)
}

/// Each running lane's DPP first source - the source register at the lane of its row the control
/// names - with that source lane's execution bit, before anything is written.
///
/// In a fragment module one invocation is one pixel (D716), and the lanes of its quad are the
/// host's own 2x2 quad, which the guest numbers as Vulkan does: top-left, top-right, bottom-left,
/// bottom-right. There the neighbour's register and execution bit are read by quad broadcast, so
/// only a quad permute over every row and bank is translated. Elsewhere every lane is the
/// module's, read by index.
fn permuted_first_source<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    word: u32,
    control: DppControl,
) -> Result<Vec<Fetched>, TranslateError> {
    let source_register = word & 0xff;
    let source = Operand::Vector(source_register as u16);
    let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    let (one, thirty_one) = (model.constant(1), model.constant(31));
    let (rows, banks) = (word >> 28, (word >> 24) & 0xf);
    let unavailable = || TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "a DPP form here needs lanes beyond the pixel's quad",
    };

    // Every lane's value and its source's execution bit first: each reads the source before any
    // lane writes the destination.
    let mut fetched = Vec::new();
    if let Some(place) = model.quad_position() {
        let (DppControl::QuadPermute(permute), true) = (control, dpp_masks_full(word)) else {
            return Err(unavailable());
        };
        let own = model.read_source(instruction, &source, 0)?;
        let own_running = model.binary(op::BITWISE_AND, running_low, one);
        let mut chosen = None;
        for at in (0..4_u32).rev() {
            let value = model
                .quad_broadcast(own, permute[at as usize])
                .ok_or_else(unavailable)?;
            let running = model
                .quad_broadcast(own_running, permute[at as usize])
                .ok_or_else(unavailable)?;
            chosen = Some(match chosen {
                None => (value, running),
                Some((other, other_running)) => {
                    let here = model.constant(at);
                    let here = model.compare(op::IEQUAL, place, here);
                    (
                        model.select(here, value, other),
                        model.select(here, running, other_running),
                    )
                }
            });
        }
        let (value, running) = chosen.ok_or_else(unavailable)?;
        fetched.push(Fetched {
            lane: 0,
            value,
            running,
            within: one,
            enabled: one,
        });
    } else {
        for lane in running_lanes(model) {
            // The lane is a run-time value where each invocation is one, or a loop over the lanes
            // is open.
            let index = model.lane_number(lane).or(model.looped_lane());
            let (from, half, within, enabled) = if let Some(index) = index {
                let (from, upper, within, enabled) =
                    lane_source_at_run_time(model, control, index, word);
                (
                    from,
                    model.select(upper, running_high, running_low),
                    within,
                    enabled,
                )
            } else {
                let read = control.source(lane & 15);
                let from = (lane & !15) | read.unwrap_or(lane & 15);
                let half = if from < 32 { running_low } else { running_high };
                let enabled = (rows >> ((lane >> 4) & 3)) & (banks >> ((lane >> 2) & 3)) & 1;
                (
                    model.constant(from),
                    half,
                    model.constant(u32::from(read.is_some())),
                    model.constant(enabled),
                )
            };
            let value = model
                .read_vector_at(source_register, from)
                .ok_or_else(unavailable)?;
            let bit = model.binary(op::BITWISE_AND, from, thirty_one);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, half, bit);
            let running = model.binary(op::BITWISE_AND, shifted, one);
            fetched.push(Fetched {
                lane,
                value,
                running,
                within,
                enabled,
            });
        }
    }
    Ok(fetched)
}

/// An instruction's SDWA modifier word, when it carries one.
fn sdwa_word(encodings: &EncodingTable, instruction: &Instruction) -> Option<u32> {
    let family = encodings
        .encodings()
        .get(usize::from(instruction.encoding?))?;
    (family.modifier_selected(&[instruction.word]) == Some(SDWA_MARKER))
        .then_some(instruction.second_word)
        .flatten()
}

/// An SDWA modifier word's fields (`aco_assembler.cpp:1200-1225`).
#[derive(Debug, Clone, Copy)]
struct SdwaFields {
    /// The real first source's code, bits 7:0.
    src0: u32,
    /// Bit 23: the first source is a scalar register or constant rather than a vector register.
    src0_scalar: bool,
    /// Bit 31: the second source's field names a scalar register rather than a vector one.
    src1_scalar: bool,
    /// Each source's select (18:16, 26:24) and sign extension (19, 27).
    select: [(u32, bool); 2],
    /// Where the result lands (10:8), and what fills the rest (12:11): zero, the sign, or the
    /// destination's old bits.
    dst_sel: u32,
    dst_unused: u32,
}

impl SdwaFields {
    fn read(word: u32) -> Self {
        Self {
            src0: word & 0xff,
            src0_scalar: word >> 23 & 1 != 0,
            src1_scalar: word >> 31 != 0,
            select: [
                (word >> 16 & 0x7, word >> 19 & 1 != 0),
                (word >> 24 & 0x7, word >> 27 & 1 != 0),
            ],
            dst_sel: word >> 8 & 0x7,
            dst_unused: word >> 11 & 0x3,
        }
    }
}

/// The SDWA select codes: bytes 0 to 3, words 0 and 1, and the whole dword (`aco_ir.h:1679-1688`,
/// `to_sdwa_sel`).
const SDWA_DWORD: u32 = 6;
/// `dst_unused`: zero-fill, sign-extend, keep the destination's bits.
const SDWA_UNUSED_SEXT: u32 = 1;
const SDWA_UNUSED_PRESERVE: u32 = 2;

/// The operand an SDWA source code names: a scalar register or inline integer when its scalar flag
/// is set, a vector register otherwise. Anything else in the scalar space is refused.
fn sdwa_operand(
    instruction: &Instruction,
    code: u32,
    scalar: bool,
) -> Result<Operand, TranslateError> {
    let refuse = TranslateError::Unsupported {
        offset: instruction.offset,
        detail: "an SDWA source names a special register, which is not translated",
    };
    // An eight-bit code always fits.
    let register = u16::try_from(code & 0xff).unwrap_or(u16::MAX);
    if !scalar {
        return Ok(Operand::Vector(register));
    }
    match code {
        0..SCALAR_REGISTERS => Ok(Operand::Scalar(register)),
        // The inline integers: 128 is zero, 129-192 are 1 to 64, 193-208 are -1 to -16.
        128..=192 => Ok(Operand::Integer(i64::from(code) - 128)),
        193..=208 => Ok(Operand::Integer(192 - i64::from(code))),
        // The rest of the scalar space is the ordinary source encoding's, so the solved operand
        // table names it, and a name is read as the ordinary source reads it: an inline float, a
        // lane mask's half (`v_mul_f32_sdwa v15, s31, vcc_lo`), `m0` or the condition code. A
        // name no source reads stays refused.
        _ => match sdwa_operand_table().map(|table| table.classify(code, None)) {
            Some(Operand::Named(name)) if sdwa_readable_name(&name) => Ok(Operand::Named(name)),
            _ => Err(refuse),
        },
    }
}

/// Whether a named operand is one an SDWA source reads as the ordinary source path does.
fn sdwa_readable_name(name: &str) -> bool {
    inline_float_bits(name).is_some()
        || lane_mask_name(name).is_some()
        || lane_mask_high_name(name).is_some()
        || name == M0
        || name == SRC_SCC
}

/// The solved operand table, parsed once for the SDWA sources that name its constants.
fn sdwa_operand_table() -> Option<&'static orbistoun_shader::OperandTable> {
    static TABLE: std::sync::OnceLock<Option<orbistoun_shader::OperandTable>> =
        std::sync::OnceLock::new();
    TABLE
        .get_or_init(|| orbistoun_shader::OperandTable::builtin().ok())
        .as_ref()
}

/// One source's selected part, as a 32-bit value: a byte or a word zero- or sign-extended, or the
/// whole register.
fn sdwa_select<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    value: Id,
    (select, sign_extend): (u32, bool),
) -> Result<Id, TranslateError> {
    let (bits, index) = match select {
        0..=3 => (8, select),
        4 | 5 => (16, select - 4),
        SDWA_DWORD => return Ok(value),
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an SDWA source select names no part of a register",
            });
        }
    };
    let shift = model.constant(bits * index);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, shift);
    let mask = model.constant((1 << bits) - 1);
    let field = model.binary(op::BITWISE_AND, shifted, mask);
    if !sign_extend {
        return Ok(field);
    }
    // Sign extension by shifting the field's top bit to bit 31 and back arithmetically.
    let up = model.constant(32 - bits);
    let raised = model.binary(op::SHIFT_LEFT_LOGICAL, field, up);
    Ok(model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, up))
}

/// Places a result into the destination's selected part: the part itself from the result's low
/// bits, the rest zero, the part's sign, or the destination's old bits.
fn sdwa_place<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    result: Id,
    old: Id,
    fields: SdwaFields,
) -> Result<Id, TranslateError> {
    let (bits, index) = match fields.dst_sel {
        0..=3 => (8, fields.dst_sel),
        4 | 5 => (16, fields.dst_sel - 4),
        SDWA_DWORD => return Ok(result),
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "an SDWA destination select names no part of a register",
            });
        }
    };
    let low = (1_u32 << bits) - 1;
    let place = bits * index;
    let mask = model.constant(low);
    let field = model.binary(op::BITWISE_AND, result, mask);
    let at = model.constant(place);
    let placed = model.binary(op::SHIFT_LEFT_LOGICAL, field, at);
    match fields.dst_unused {
        0 => Ok(placed),
        SDWA_UNUSED_SEXT => {
            // The part's sign fills every bit above it.
            let up = model.constant(32 - bits);
            let raised = model.binary(op::SHIFT_LEFT_LOGICAL, field, up);
            let extended = model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, up);
            let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, extended, at);
            Ok(shifted)
        }
        SDWA_UNUSED_PRESERVE => {
            let keep = model.constant(!(low << place));
            let kept = model.binary(op::BITWISE_AND, old, keep);
            Ok(model.binary(op::BITWISE_OR, kept, placed))
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "an SDWA destination's unused-bits mode is reserved",
        }),
    }
}

/// `op_sel`, bits 14:11 of a long-form vector instruction's first word: for each source and then
/// the destination, whether its sixteen-bit operand is the register's high half (AMD's published
/// RDNA instruction set, `OPSEL`). On this generation a sixteen-bit result keeps the other half of
/// its destination (ACO `instr_is_16bit` and `can_use_opsel`, `aco_ir.cpp:600-743`).
const OP_SEL_SHIFT: u32 = 11;

/// One sixteen-bit operand as a 32-bit value, zero above: its register's low half, or its high half
/// when `high`. An inline float is a half here and a constant's high half is not established, so
/// both are refused; a named register - the condition or execution mask's half, `m0` - is a word
/// as any register is (Grand Theft Auto V's `v_mul_f16_sdwa v16, vcc_lo, v17`).
fn read_half<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    operand: &Operand,
    lane: u32,
    high: bool,
) -> Result<Id, TranslateError> {
    // A literal is one dword whose halves are two values: its high half is its upper word, as a
    // register's is (ACO reads a packed operand's high select as `SubdwordSel::uword1` whatever
    // the operand, `aco_optimizer.cpp:1314-1320`; LLVM prints `0x20001` as the halves 1 and 2).
    // What an inline constant's high half holds is not established, so it stays refused.
    let named_register = matches!(
        operand,
        Operand::Named(name)
            if matches!(name.as_str(), "vcc_lo" | "vcc_hi" | "exec_lo" | "exec_hi" | "m0")
    );
    let whole_word = named_register
        || matches!(
            operand,
            Operand::Vector(_) | Operand::Scalar(_) | Operand::Literal(_)
        );
    if (matches!(operand, Operand::Named(_)) && !named_register) || (high && !whole_word) {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: concat!(
                "a sixteen-bit source that is an inline float, or the high half of an inline ",
                "constant, which is not translated"
            ),
        });
    }
    let value = model.read_source(instruction, operand, lane)?;
    if high {
        let sixteen = model.constant(16);
        return Ok(model.binary(op::SHIFT_RIGHT_LOGICAL, value, sixteen));
    }
    let low = model.constant(0xFFFF);
    Ok(model.binary(op::BITWISE_AND, value, low))
}

/// Writes a sixteen-bit result into a register's low half, or its high half when `high`, keeping
/// the other half.
fn write_half<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (register, lane): (u32, u32),
    result: Id,
    high: bool,
) -> Result<(), TranslateError> {
    let old = model.read_source(
        instruction,
        &Operand::Vector(u16::try_from(register).unwrap_or(u16::MAX)),
        lane,
    )?;
    let low = model.constant(0xFFFF);
    let field = model.binary(op::BITWISE_AND, result, low);
    let (placed, kept) = if high {
        let sixteen = model.constant(16);
        (
            model.binary(op::SHIFT_LEFT_LOGICAL, field, sixteen),
            model.binary(op::BITWISE_AND, old, low),
        )
    } else {
        let high_half = model.constant(0xFFFF_0000);
        (field, model.binary(op::BITWISE_AND, old, high_half))
    };
    let merged = model.binary(op::BITWISE_OR, kept, placed);
    model.write_vector_lane(register, lane, merged);
    Ok(())
}

/// `v_add_nc_u16` and `v_lshlrev_b16` (AMD's published RDNA instruction set): sixteen-bit operands
/// chosen by `op_sel`, a sixteen-bit result into the half it names. The add wraps; the reversed
/// shift takes its amount from the first source's low four bits. Clamp, which saturates the add,
/// and the float modifiers are refused.
fn half_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 2)?;
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit result's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let select = instruction.word >> OP_SEL_SHIFT & 0xF;
    let low_four = model.constant(15);
    for lane in running_lanes(model) {
        let a = read_half(model, instruction, first, lane, select & 1 != 0)?;
        let b = read_half(model, instruction, second, lane, select & 2 != 0)?;
        let result = if name == "v_add_nc_u16" {
            model.binary(op::IADD, a, b)
        } else {
            let amount = model.binary(op::BITWISE_AND, a, low_four);
            model.binary(op::SHIFT_LEFT_LOGICAL, b, amount)
        };
        write_half(
            model,
            instruction,
            (register, lane),
            result,
            select & 8 != 0,
        )?;
    }
    model.count();
    Ok(())
}

/// `v_pack_b32_f16 d, a, b`: the first source's half in bits 15:0 and the second's in 31:16, each
/// the half `op_sel` names; a whole word, whatever the destination held (AMD's published RDNA
/// instruction set; ACO `can_use_opsel` gives it no destination select). The bits move unchanged.
fn pack_half_words<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 2)?;
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a half pack's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let select = instruction.word >> OP_SEL_SHIFT & 0xF;
    if select & 8 != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_pack_b32_f16 with a destination select, which it does not take",
        });
    }
    let sixteen = model.constant(16);
    for lane in running_lanes(model) {
        let low = read_half(model, instruction, first, lane, select & 1 != 0)?;
        let high = read_half(model, instruction, second, lane, select & 2 != 0)?;
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, high, sixteen);
        let packed = model.binary(op::BITWISE_OR, low, high);
        model.write_vector_lane(register, lane, packed);
    }
    model.count();
    Ok(())
}

/// A packed instruction's half selects (AMD's published RDNA instruction set, `VOP3P`): `op_sel`,
/// bits 13:11 of the first word, picks each source's half for the low result; `op_sel_hi` - bits
/// 28:27 of the second word for the first two sources, bit 14 of the first for the third - for
/// the high result. Negation (the first word's 10:8, the second's 31:29) and clamp (bit 15) are
/// refused.
fn packed_selects(instruction: &Instruction) -> Result<[[bool; 3]; 2], TranslateError> {
    let second = instruction.second_word.unwrap_or(0);
    if instruction.word & (0x7 << 8 | 1 << 15) != 0 || second >> 29 != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a packed instruction's negation or clamp, which is not translated",
        });
    }
    let low = [11, 12, 13].map(|bit| instruction.word >> bit & 1 != 0);
    let high = [
        second >> 27 & 1 != 0,
        second >> 28 & 1 != 0,
        instruction.word >> 14 & 1 != 0,
    ];
    Ok([low, high])
}

/// The mixed-precision multiply-adds.
const MIXED: [&str; 3] = ["v_fma_mix_f32", "v_fma_mixlo_f16", "v_fma_mixhi_f16"];

/// The three-way float minimum, maximum and median [`float_min_max_of_three`] carries out.
const FLOATS_OF_THREE: [&str; 3] = ["v_max3_f32", "v_min3_f32", "v_med3_f32"];

/// The cube-map face instructions [`cube_face`] carries out.
const CUBE_FACES: [&str; 4] = [
    "v_cubeid_f32",
    "v_cubesc_f32",
    "v_cubetc_f32",
    "v_cubema_f32",
];

/// Every multiply-add [`multiply_add_family`] carries out.
const MULTIPLY_ADDS: [&str; 7] = [
    "v_madak_f32",
    "v_madmk_f32",
    "v_fmaak_f32",
    "v_fmamk_f32",
    "v_fma_mix_f32",
    "v_fma_mixlo_f16",
    "v_fma_mixhi_f16",
];

/// A multiply-add with its constant `K` the word after it, or a mixed-precision one ([`MIXED`]).
fn multiply_add_family<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    if MIXED.contains(&name) {
        mixed_multiply_add(model, instruction, name)
    } else {
        multiply_add_constant(model, instruction, name)
    }
}

/// `v_fma_mix_f32 d, a, b, c`, `v_fma_mixlo_f16` and `v_fma_mixhi_f16`: the fused multiply-add
/// `a * b + c` over sources each a single-precision float or a half widened to one, as ACO
/// encodes them (`aco_ir.h:1589-1599`, `aco_print_ir.cpp:1054-1063`): `op_sel_hi` - the second
/// word's 28:27 for the first two sources, the first word's bit 14 for the third - makes a source a
/// half, and `op_sel` (the first word's 13:11) takes its high half rather than its low. The VOP3P
/// negation fields are the mix's modifiers: the first word's 10:8 the absolute value of each
/// source, the second word's 31:29 its negation, applied in that order. Bit 15 clamps the result
/// to `[0, 1]`. `v_fma_mix_f32` writes the float; `v_fma_mixlo_f16` and `v_fma_mixhi_f16` narrow
/// it to a half, rounded to nearest, into the low or high half of the destination, keeping the
/// other half. The rounding mode a shader may set around a mix is not modelled.
fn mixed_multiply_add<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [destination, first, second_source, third] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a mixed-precision multiply-add does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a mixed-precision multiply-add's destination is not a vector register",
        });
    };
    let word = instruction.word;
    let second = instruction.second_word.unwrap_or(0);
    let half = [
        second >> 27 & 1 != 0,
        second >> 28 & 1 != 0,
        word >> 14 & 1 != 0,
    ];
    let high = [11, 12, 13].map(|bit| word >> bit & 1 != 0);
    let modifiers = Modifiers {
        negate: [29, 30, 31].map(|bit| second >> bit & 1 != 0),
        absolute: [8, 9, 10].map(|bit| word >> bit & 1 != 0),
        clamp: false,
        output_multiplier: 0,
    };
    let clamp = if word >> 15 & 1 != 0 {
        let Some(nan_to_zero) = model.dx10_clamp() else {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps its result to [0, 1], and what that does to a NaN ",
                    "is the stage's DX10_CLAMP mode, which this translation was not given"
                ),
            });
        };
        Some(nan_to_zero)
    } else {
        None
    };
    let register = u32::from(*register);
    let sources = [first, second_source, third];
    for lane in running_lanes(model) {
        let mut read = [None; 3];
        for (index, source) in sources.iter().enumerate() {
            let raw = model.read_source(instruction, source, lane)?;
            let value = if half[index] {
                let shift = model.constant(if high[index] { 16 } else { 0 });
                let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, raw, shift);
                let mask = model.constant(0xFFFF);
                let field = model.binary(op::BITWISE_AND, shifted, mask);
                model.half_to_float_bits(field)
            } else {
                raw
            };
            read[index] = Some(apply_modifiers(model, value, modifiers, index));
        }
        let [Some(a), Some(b), Some(c)] = read else {
            unreachable!("every source was read above");
        };
        // Fused: once in single precision for `v_fma_mix_f32`, once to a half for the narrowing
        // forms where every source is a half, whose product is then exact in single precision.
        // A narrowing form with a single-precision source rounds to single precision first.
        let value = if name != "v_fma_mix_f32" && half.iter().all(|half| *half) {
            fused_to_half(model, a, b, c)
        } else {
            model.f32_fma(a, b, c)
        };
        let value = match clamp {
            Some(nan_to_zero) => clamp_unit(model, value, nan_to_zero),
            None => value,
        };
        let value = if name == "v_fma_mix_f32" {
            value
        } else {
            let narrowed = model.float_bits_to_half(value);
            let old = model.read_source(instruction, destination, lane)?;
            let (kept, placed) = if name == "v_fma_mixlo_f16" {
                (model.constant(0xFFFF_0000), narrowed)
            } else {
                let sixteen = model.constant(16);
                (
                    model.constant(0x0000_FFFF),
                    model.binary(op::SHIFT_LEFT_LOGICAL, narrowed, sixteen),
                )
            };
            let kept = model.binary(op::BITWISE_AND, old, kept);
            model.binary(op::BITWISE_OR, kept, placed)
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// `v_pk_mad_u16 d, a, b, c`: in each half of the result, the product of `a` and `b`'s selected
/// halves plus `c`'s, wrapping at sixteen bits (AMD's published RDNA instruction set). The low
/// result takes the halves `op_sel` picks and the high one those `op_sel_hi` picks.
fn packed_multiply_add<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let selects = packed_selects(instruction)?;
    let [destination, multiplicand, multiplier, addend] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_pk_mad_u16 does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_pk_mad_u16 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let low = model.constant(0xFFFF);
    let sixteen = model.constant(16);
    for lane in running_lanes(model) {
        let mut halves = Vec::with_capacity(2);
        for select in selects {
            let left = read_half(model, instruction, multiplicand, lane, select[0])?;
            let right = read_half(model, instruction, multiplier, lane, select[1])?;
            let offset = read_half(model, instruction, addend, lane, select[2])?;
            let product = model.binary(op::IMUL, left, right);
            let sum = model.binary(op::IADD, product, offset);
            halves.push(model.binary(op::BITWISE_AND, sum, low));
        }
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, halves[1], sixteen);
        let packed = model.binary(op::BITWISE_OR, halves[0], high);
        model.write_vector_lane(register, lane, packed);
    }
    model.count();
    Ok(())
}

/// `v_pk_lshlrev_b16 d, a, b`: in each half of the result, `b`'s selected half shifted left by the
/// low four bits of `a`'s, wrapping at sixteen bits (AMD's published RDNA instruction set). The low
/// result takes the halves `op_sel` picks and the high one those `op_sel_hi` picks.
fn packed_shift_left<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let selects = packed_selects(instruction)?;
    let (destination, amount, value) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_pk_lshlrev_b16 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let low = model.constant(0xFFFF);
    let low_four = model.constant(15);
    let sixteen = model.constant(16);
    for lane in running_lanes(model) {
        let mut halves = Vec::with_capacity(2);
        for select in selects {
            let by = read_half(model, instruction, amount, lane, select[0])?;
            let by = model.binary(op::BITWISE_AND, by, low_four);
            let shifted = read_half(model, instruction, value, lane, select[1])?;
            let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, shifted, by);
            halves.push(model.binary(op::BITWISE_AND, shifted, low));
        }
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, halves[1], sixteen);
        let packed = model.binary(op::BITWISE_OR, halves[0], high);
        model.write_vector_lane(register, lane, packed);
    }
    model.count();
    Ok(())
}

/// `v_pk_add_u16 d, a, b` and `v_pk_sub_u16 d, a, b`: in each half of the result, `a`'s selected
/// half plus or less `b`'s, wrapping at sixteen bits (AMD's published RDNA instruction set). The low
/// result takes the halves `op_sel` picks and the high one those `op_sel_hi` picks; clamp, which
/// saturates, is refused. The signed forms, `v_pk_add_i16` and `v_pk_sub_i16`, wrap to the same
/// bits: only their clamp would differ.
fn packed_add_or_subtract<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let combine = if name.starts_with("v_pk_add_") {
        op::IADD
    } else {
        op::ISUB
    };
    let selects = packed_selects(instruction)?;
    let (destination, minuend, subtrahend) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a packed sum's or difference's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let low = model.constant(0xFFFF);
    let sixteen = model.constant(16);
    for lane in running_lanes(model) {
        let mut halves = Vec::with_capacity(2);
        for select in selects {
            let left = read_half(model, instruction, minuend, lane, select[0])?;
            let right = read_half(model, instruction, subtrahend, lane, select[1])?;
            let combined = model.binary(combine, left, right);
            halves.push(model.binary(op::BITWISE_AND, combined, low));
        }
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, halves[1], sixteen);
        let packed = model.binary(op::BITWISE_OR, halves[0], high);
        model.write_vector_lane(register, lane, packed);
    }
    model.count();
    Ok(())
}

/// `v_min3_i16 d, a, b, c`: the least of three signed sixteen-bit halves, each the one `op_sel`
/// names, into the half it names for the destination, the other half kept (AMD's published RDNA
/// instruction set; the sixteen-bit VOP3 forms as `half_arithmetic` writes them).
fn half_minimum_of_three<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 3)?;
    let [destination, first, second, third] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_min3_i16 does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_min3_i16 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let select = instruction.word >> OP_SEL_SHIFT & 0xF;
    let sixteen = model.constant(16);
    let low = model.constant(0xFFFF);
    for lane in running_lanes(model) {
        let mut least: Option<Id> = None;
        for (source, bit) in [(first, 1), (second, 2), (third, 4)] {
            let half = read_half(model, instruction, source, lane, select & bit != 0)?;
            // Sign-extended, so the comparison is a signed one.
            let raised = model.binary(op::SHIFT_LEFT_LOGICAL, half, sixteen);
            let signed = model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, sixteen);
            least = Some(match least {
                None => signed,
                Some(so_far) => {
                    let below = model.compare(op::SLESS_THAN, signed, so_far);
                    model.select(below, signed, so_far)
                }
            });
        }
        let least = least.expect("three sources");
        let result = model.binary(op::BITWISE_AND, least, low);
        write_half(
            model,
            instruction,
            (register, lane),
            result,
            select & 8 != 0,
        )?;
    }
    model.count();
    Ok(())
}

/// `v_lshl_or_b32 d, a, b, c`: `(a << b[4:0]) | c` per lane (AMD's published RDNA instruction set,
/// `V_LSHL_OR_B32`).
fn shift_or<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 3)?;
    let [destination, value, amount, other] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_lshl_or_b32 does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_lshl_or_b32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let low_five = model.constant(31);
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, value, lane)?;
        let amount = model.read_source(instruction, amount, lane)?;
        let other = model.read_source(instruction, other, lane)?;
        let amount = model.binary(op::BITWISE_AND, amount, low_five);
        let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, value, amount);
        let combined = model.binary(op::BITWISE_OR, shifted, other);
        model.write_vector_lane(register, lane, combined);
    }
    model.count();
    Ok(())
}

/// `v_perm_b32`: each byte of the result chosen by the matching byte of the third source, from the
/// eight bytes of the first two - the second's low, the first's high - or a constant.
///
/// Mesa's `byte_perm_amd` (`nir_opcodes.py:1551-1568`), which ACO emits as this instruction with
/// its sources in order (`aco_select_nir_alu.cpp:3355-3357`): a selector below 8 picks that byte of
/// `first:second`; 8 to 11 repeat a sign bit - bit 15 or 31 of the second, then of the first - as
/// a byte of ones or zeros; 12 is zero; 13 and above are `0xff`.
fn byte_permute<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 3)?;
    let [destination, first, second, selectors] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_perm_b32 does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_perm_b32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let first = model.read_source(instruction, first, lane)?;
        let second = model.read_source(instruction, second, lane)?;
        let selectors = model.read_source(instruction, selectors, lane)?;
        let mut result = model.constant(0);
        for byte in 0..4 {
            let shift = model.constant(byte * 8);
            let chosen = permuted_byte(model, (first, second), selectors, shift);
            let placed = model.binary(op::SHIFT_LEFT_LOGICAL, chosen, shift);
            result = model.binary(op::BITWISE_OR, result, placed);
        }
        model.write_vector_lane(register, lane, result);
    }
    model.count();
    Ok(())
}

/// One byte of [`byte_permute`]'s result: the one its selector, at `shift` in `selectors`, names.
fn permuted_byte<M: Model + ?Sized>(
    model: &mut M,
    (first, second): (Id, Id),
    selectors: Id,
    shift: Id,
) -> Id {
    let byte_mask = model.constant(0xff);
    let three = model.constant(3);
    let (one, two, four, eight, twelve) = (
        model.constant(1),
        model.constant(2),
        model.constant(4),
        model.constant(8),
        model.constant(12),
    );
    let selector = model.binary(op::SHIFT_RIGHT_LOGICAL, selectors, shift);
    let selector = model.binary(op::BITWISE_AND, selector, byte_mask);
    // 0-7: byte `selector & 3` of the second source below 4, of the first from 4.
    let in_second = model.compare(op::ULESS_THAN, selector, four);
    let word = model.select(in_second, second, first);
    let index = model.binary(op::BITWISE_AND, selector, three);
    let bits = model.binary(op::SHIFT_LEFT_LOGICAL, index, three);
    let picked = model.binary(op::SHIFT_RIGHT_LOGICAL, word, bits);
    let picked = model.binary(op::BITWISE_AND, picked, byte_mask);
    // 8-11: bit 15 (even) or 31 (odd) of the second (8, 9) or the first (10, 11), as a byte.
    let zero = model.constant(0);
    let of_first = model.binary(op::BITWISE_AND, selector, two);
    let of_first = model.compare(op::INOT_EQUAL, of_first, zero);
    let word = model.select(of_first, first, second);
    let odd = model.binary(op::BITWISE_AND, selector, one);
    let fifteen = model.constant(15);
    let sixteen = model.constant(16);
    let odd_shift = model.binary(op::IMUL, odd, sixteen);
    let bit = model.binary(op::IADD, fifteen, odd_shift);
    let sign = model.binary(op::SHIFT_RIGHT_LOGICAL, word, bit);
    let sign = model.binary(op::BITWISE_AND, sign, one);
    let sign = model.binary(op::IMUL, sign, byte_mask);
    // 12 is zero; 13 and above all ones.
    let is_twelve = model.compare(op::IEQUAL, selector, twelve);
    let constant = model.select(is_twelve, zero, byte_mask);
    let below_twelve = model.compare(op::ULESS_THAN, selector, twelve);
    let high = model.select(below_twelve, sign, constant);
    let below_eight = model.compare(op::ULESS_THAN, selector, eight);
    model.select(below_eight, picked, high)
}

/// `v_mad_f32` and `v_mac_f32`: `a * b + c`, rounded after the multiply and again after the add,
/// with every input, the product and the result's denormals flushed to a signed zero. These two
/// always flush, whatever the mode (ACO, `aco_ir.h:138-139`), and are not fused, so each step is
/// decorated `NoContraction` to keep a driver from fusing them.
fn legacy_multiply_add<M: Model + ?Sized>(model: &mut M, a: Id, b: Id, c: Id) -> Id {
    let (a, b, c) = (
        flush_denormal(model, a),
        flush_denormal(model, b),
        flush_denormal(model, c),
    );
    let product = model.f32_binary_uncontracted(op::FMUL, a, b);
    let product = flush_denormal(model, product);
    let sum = model.f32_binary_uncontracted(op::FADD, product, c);
    flush_denormal(model, sum)
}

/// `a * b + c` of three halves widened to single precision, rounded so that narrowing the result to
/// a half rounds the exact value once, as a fused sixteen-bit multiply-add does. The product of two
/// halves is exact in single precision, so the sum is the only rounding: its error is found exactly
/// (`TwoSum`), and an inexact sum whose last bit is even is moved one step toward the exact value -
/// rounded to odd. Rounding to odd at 24 bits and then to nearest at 11 is the single rounding to
/// nearest at 11 (Boldo and Melquiond, "Emulation of FMA and correctly rounded sums: proved
/// algorithms using rounding to odd", IEEE Trans. Computers 57(4), 2008), where rounding a nearest
/// sum twice can round a tie the wrong way. Every step is kept from contraction.
pub(crate) fn fused_to_half<M: Model + ?Sized>(model: &mut M, a: Id, b: Id, c: Id) -> Id {
    let product = model.f32_binary_uncontracted(op::FMUL, a, b);
    let sum = model.f32_binary_uncontracted(op::FADD, product, c);
    let part_c = model.f32_binary_uncontracted(op::FSUB, sum, product);
    let part_product = model.f32_binary_uncontracted(op::FSUB, sum, part_c);
    let lost_product = model.f32_binary_uncontracted(op::FSUB, product, part_product);
    let lost_c = model.f32_binary_uncontracted(op::FSUB, c, part_c);
    let error = model.f32_binary_uncontracted(op::FADD, lost_product, lost_c);
    let magnitude_mask = model.constant(0x7FFF_FFFF);
    let (zero, one) = (model.constant(0), model.constant(1));
    let error_magnitude = model.binary(op::BITWISE_AND, error, magnitude_mask);
    let inexact = model.compare(op::INOT_EQUAL, error_magnitude, zero);
    let last = model.binary(op::BITWISE_AND, sum, one);
    let even = model.compare(op::IEQUAL, last, zero);
    // Finite: an exponent short of all ones. An infinite or NaN sum is left as it is.
    let sum_magnitude = model.binary(op::BITWISE_AND, sum, magnitude_mask);
    let infinity = model.constant(0x7F80_0000);
    let finite = model.compare(op::ULESS_THAN, sum_magnitude, infinity);
    // The exact value lies beyond the sum's magnitude where the error has the sum's sign.
    let signs = model.binary(op::BITWISE_XOR, error, sum);
    let sign_bit = model.constant(0x8000_0000);
    let differing = model.binary(op::BITWISE_AND, signs, sign_bit);
    let outward = model.compare(op::IEQUAL, differing, zero);
    let up = model.add(sum, one);
    let down = model.binary(op::ISUB, sum, one);
    let stepped = model.select(outward, up, down);
    let odd = model.both(inexact, even);
    let odd = model.both(odd, finite);
    model.select(odd, stepped, sum)
}

/// A float's bits with a denormal replaced by the zero of its sign; every other value, zero and the
/// infinities and NaNs among them, unchanged.
fn flush_denormal<M: Model + ?Sized>(model: &mut M, bits: Id) -> Id {
    let magnitude_mask = model.constant(0x7FFF_FFFF);
    let magnitude = model.binary(op::BITWISE_AND, bits, magnitude_mask);
    // A denormal's magnitude is 1 to 0x007F_FFFF; less one, it is below 0x007F_FFFF, and a zero
    // wraps far above it.
    let one = model.constant(1);
    let less_one = model.binary(op::ISUB, magnitude, one);
    let largest = model.constant(0x007F_FFFF);
    let denormal = model.compare(op::ULESS_THAN, less_one, largest);
    let sign_mask = model.constant(0x8000_0000);
    let signed_zero = model.binary(op::BITWISE_AND, bits, sign_mask);
    model.select(denormal, signed_zero, bits)
}

/// `S_BFE_U32`'s field: `(value >> control[4:0]) & ((1 << control[22:16]) - 1)` (AMD's published
/// RDNA instruction set). A width of thirty-two or more keeps every bit the shift left.
fn bit_field_extract<M: Model + ?Sized>(model: &mut M, value: Id, control: Id) -> Id {
    let low_five = model.constant(31);
    let offset = model.binary(op::BITWISE_AND, control, low_five);
    let sixteen = model.constant(16);
    let width_field = model.binary(op::SHIFT_RIGHT_LOGICAL, control, sixteen);
    let seven_bits = model.constant(0x7f);
    let width = model.binary(op::BITWISE_AND, width_field, seven_bits);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, value, offset);
    // A shift by thirty-two or more is undefined in SPIR-V, so the whole-word case is a select
    // rather than `(1 << width) - 1` computed at every width.
    let thirty_two = model.constant(32);
    let narrow = model.compare(op::ULESS_THAN, width, thirty_two);
    let width_below_32 = model.binary(op::BITWISE_AND, width, low_five);
    let one = model.constant(1);
    let bit = model.binary(op::SHIFT_LEFT_LOGICAL, one, width_below_32);
    let mask = model.binary(op::ISUB, bit, one);
    let all = model.constant(u32::MAX);
    let mask = model.select(narrow, mask, all);
    model.binary(op::BITWISE_AND, shifted, mask)
}

/// `s_pack_ll_b32_b16` and `s_pack_hh_b32_b16`: the low (or high) sixteen bits of each source, the
/// first into the low half of the result. No condition code.
fn scalar_pack<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let first = model.read_source(instruction, first, 0)?;
    let second = model.read_source(instruction, second, 0)?;
    let sixteen = model.constant(16);
    let low_half = model.constant(0xffff);
    let high_half = model.constant(0xffff_0000);
    let packed = if name == "s_pack_ll_b32_b16" {
        let low = model.binary(op::BITWISE_AND, first, low_half);
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, second, sixteen);
        model.binary(op::BITWISE_OR, low, high)
    } else {
        let low = model.binary(op::SHIFT_RIGHT_LOGICAL, first, sixteen);
        let high = model.binary(op::BITWISE_AND, second, high_half);
        model.binary(op::BITWISE_OR, low, high)
    };
    write_scalar_operand(model, instruction, destination, packed)?;
    model.count();
    Ok(())
}

/// `s_bfm_b64`: `((1 << S0[5:0]) - 1) << S1[5:0]` across a register pair or a lane mask (AMD's
/// published RDNA instruction set, `S_BFM_B64`): `s_bfm_b64 exec, 32, 32` runs the upper half of
/// the wave, as Grand Theft Auto V's compute program at `0x400003e16400` does. No condition code.
fn scalar_bit_mask_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, width, offset) = three_operands(instruction)?;
    let mask = match destination {
        Operand::Named(name) => lane_mask_name(name),
        _ => None,
    };
    let register = match mask {
        Some(_) => 0,
        None => scalar_destination(instruction, destination)?,
    };
    if mask.is_none() && register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_bfm_b64 runs past the end of the register file",
        });
    }
    let width = model.read_source(instruction, width, 0)?;
    let offset = model.read_source(instruction, offset, 0)?;
    let six_bits = model.constant(63);
    let width = model.binary(op::BITWISE_AND, width, six_bits);
    let offset = model.binary(op::BITWISE_AND, offset, six_bits);
    // Bit `i` of the pair is set when `offset <= i < offset + width`, computed per half so no shift
    // reaches thirty-two.
    let (low, high) = ones_between(model, offset, width);
    if let Some(mask) = mask {
        model.write_lane_mask(mask, low, high)?;
    } else {
        model.write_scalar(register, low);
        model.write_scalar(register + 1, high);
    }
    model.count();
    Ok(())
}

/// `s_bfm_b32`: `((1 << S0[4:0]) - 1) << S1[4:0]` (AMD's published RDNA instruction set,
/// `S_BFM_B32`), the ones past bit thirty-one dropped. No condition code.
fn scalar_bit_mask<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, width, offset) = three_operands(instruction)?;
    let width = model.read_source(instruction, width, 0)?;
    let offset = model.read_source(instruction, offset, 0)?;
    let five_bits = model.constant(31);
    let width = model.binary(op::BITWISE_AND, width, five_bits);
    let offset = model.binary(op::BITWISE_AND, offset, five_bits);
    let one = model.constant(1);
    let bit = model.binary(op::SHIFT_LEFT_LOGICAL, one, width);
    let ones = model.binary(op::ISUB, bit, one);
    let mask = model.binary(op::SHIFT_LEFT_LOGICAL, ones, offset);
    // `m0` is state outside the register file: AGC's GDS-writing compute program forms its `m0`
    // with `s_bfm_b32 m0, 2, 14`. GTA V's lighting forms `vcc_lo` with `s_bfm_b32 vcc_lo, 8, 23`.
    write_scalar_operand(model, instruction, destination, mask)?;
    model.count();
    Ok(())
}

/// `s_cselect_b64 d, a, b`: the pair `a` where the condition code is set, else `b` (AMD's published
/// RDNA instruction set, `S_CSELECT_B64`). It reads the condition code and writes none.
fn scalar_select_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let (first_low, first_high) = pair_source(model, instruction, first, 0)?;
    let (second_low, second_high) = pair_source(model, instruction, second, 0)?;
    let condition = crate::control::read_condition_code(model, true);
    let low = model.select(condition, first_low, second_low);
    let high = model.select(condition, first_high, second_high);
    // A lane mask is the pair it names, as a source is (`s_cselect_b64 vcc, exec, 0`).
    if let Some(mask) = mask_destination(destination) {
        model.write_lane_mask(mask, low, high)?;
        model.count();
        return Ok(());
    }
    let register = scalar_destination(instruction, destination)?;
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_cselect_b64 runs past the end of the register file",
        });
    }
    model.write_scalar(register, low);
    model.write_scalar(register + 1, high);
    model.count();
    Ok(())
}

/// `v_madak_f32 d, a, b, K` (`a * b + K`) and `v_madmk_f32 d, a, K, b` (`a * K + b`): the legacy
/// multiply-add of `v_mad_f32` with a constant from the literal word after the instruction (AMD's
/// published RDNA instruction set, `V_MADAK_F32`, `V_MADMK_F32`). In the order the operands are
/// written, both are `first * second + third`. `v_fmaak_f32` and `v_fmamk_f32` are the same shapes
/// with the fused multiply-add of `v_fma_f32` (`V_FMAAK_F32`, `V_FMAMK_F32`), carried out as it is.
fn multiply_add_constant<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [destination, first, second, third] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a multiply-add with a constant does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a multiply-add with a constant's destination is not a vector register",
        });
    };
    // The constant is the third operand of `v_madak_f32` and the second of `v_madmk_f32`: the
    // instruction's second word, decoded as an immediate.
    let constant_last = matches!(name, "v_madak_f32" | "v_fmaak_f32");
    let legacy = name.starts_with("v_mad");
    let (constant, sources) = if constant_last {
        (third, [first, second])
    } else {
        (second, [first, third])
    };
    let Operand::Immediate(constant) = constant else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a multiply-add with a constant whose constant is not the instruction's word",
        });
    };
    let constant = model.constant(*constant as u32);
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let a = model.read_source(instruction, sources[0], lane)?;
        let other = model.read_source(instruction, sources[1], lane)?;
        let (b, c) = if constant_last {
            (other, constant)
        } else {
            (constant, other)
        };
        let value = if legacy {
            legacy_multiply_add(model, a, b, c)
        } else {
            model.f32_fma(a, b, c)
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The bit-field instructions of the long form, from their three sources:
///
/// - `V_BFE_U32`: the field `c` bits wide at bit `b` of `a`.
/// - `V_BFE_I32`: the same, sign-extended; none wide is zero.
/// - `V_ALIGNBYTE_B32`: `{a, b}` as one 64-bit value, `a` high, shifted right by `c[1:0]` bytes,
///   its low 32 bits.
///
/// As AMD's published RDNA instruction set gives them.
fn bit_field<M: Model + ?Sized>(model: &mut M, name: &str, [a, b, c]: [Id; 3]) -> Id {
    match name {
        // `{a, b} >> shift`, the low word: `v_alignbyte_b32` shifts by `c[1:0]` bytes, and
        // `v_alignbit_b32` by `c[4:0]` bits (`V_ALIGNBIT_B32`).
        "v_alignbyte_b32" | "v_alignbit_b32" => {
            let bits = if name == "v_alignbit_b32" {
                let thirty_one = model.constant(31);
                model.binary(op::BITWISE_AND, c, thirty_one)
            } else {
                let three = model.constant(3);
                let bytes = model.binary(op::BITWISE_AND, c, three);
                let eight = model.constant(8);
                model.binary(op::IMUL, bytes, eight)
            };
            let low = model.binary(op::SHIFT_RIGHT_LOGICAL, b, bits);
            let thirty_two = model.constant(32);
            let rest = model.binary(op::ISUB, thirty_two, bits);
            let high = model.binary(op::SHIFT_LEFT_LOGICAL, a, rest);
            let joined = model.binary(op::BITWISE_OR, low, high);
            let zero = model.constant(0);
            let whole = model.compare(op::IEQUAL, bits, zero);
            model.select(whole, b, joined)
        }
        "v_bfe_i32" => {
            let five = model.constant(31);
            let offset = model.binary(op::BITWISE_AND, b, five);
            let width = model.binary(op::BITWISE_AND, c, five);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, a, offset);
            let thirty_two = model.constant(32);
            let up = model.binary(op::ISUB, thirty_two, width);
            let raised = model.binary(op::SHIFT_LEFT_LOGICAL, shifted, up);
            let field = model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, up);
            let zero = model.constant(0);
            let none = model.compare(op::IEQUAL, width, zero);
            model.select(none, zero, field)
        }
        _ => {
            let five = model.constant(31);
            let offset = model.binary(op::BITWISE_AND, b, five);
            let width = model.binary(op::BITWISE_AND, c, five);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, a, offset);
            let one = model.constant(1);
            let bit = model.binary(op::SHIFT_LEFT_LOGICAL, one, width);
            let mask = model.binary(op::ISUB, bit, one);
            model.binary(op::BITWISE_AND, shifted, mask)
        }
    }
}

/// The names [`integer_long_form`] translates.
const LONG_FORMS: [&str; 16] = [
    "v_mad_u32_u16",
    "v_mad_i32_i24",
    "v_mad_u32_u24",
    "v_and_or_b32",
    "v_or3_b32",
    "v_mul_lo_u32",
    "v_mul_hi_u32",
    "v_mul_hi_i32",
    "v_bfm_b32",
    "v_bcnt_u32_b32",
    "v_ldexp_f32",
    "v_msad_u8",
    "v_bfe_u32",
    "v_bfe_i32",
    "v_alignbyte_b32",
    "v_alignbit_b32",
];

/// The two-source forms Grand Theft Auto V's compute programs add, as AMD's published RDNA
/// instruction set gives them: `v_mul_hi_i32`, the signed product's high 32 bits; `v_bfm_b32`,
/// `((1 << a[4:0]) - 1) << b[4:0]`; `v_bcnt_u32_b32`, `a`'s set bits plus `b`; and `v_ldexp_f32`,
/// `a * 2^b` for an integer `b`.
fn two_source_long_form<M: Model + ?Sized>(model: &mut M, name: &str, a: Id, b: Id) -> Id {
    match name {
        // The unsigned high product, less each source where the other is negative.
        "v_mul_hi_i32" => {
            let high = multiply_high(model, a, b);
            let thirty_one = model.constant(31);
            let a_negative = model.binary(op::SHIFT_RIGHT_ARITHMETIC, a, thirty_one);
            let b_negative = model.binary(op::SHIFT_RIGHT_ARITHMETIC, b, thirty_one);
            let less_b = model.binary(op::BITWISE_AND, a_negative, b);
            let less_a = model.binary(op::BITWISE_AND, b_negative, a);
            let high = model.binary(op::ISUB, high, less_b);
            model.binary(op::ISUB, high, less_a)
        }
        "v_bfm_b32" => {
            let five = model.constant(31);
            let width = model.binary(op::BITWISE_AND, a, five);
            let offset = model.binary(op::BITWISE_AND, b, five);
            let one = model.constant(1);
            let bit = model.binary(op::SHIFT_LEFT_LOGICAL, one, width);
            let ones = model.binary(op::ISUB, bit, one);
            model.binary(op::SHIFT_LEFT_LOGICAL, ones, offset)
        }
        "v_bcnt_u32_b32" => {
            let count = model.bit_count(a);
            model.add(count, b)
        }
        _ => {
            let exponent = model.signed_to_float_bits(b);
            let scale = model.f32_ext_unary(GLSL_EXP2, exponent);
            model.f32_binary(op::FMUL, a, scale)
        }
    }
}

/// The long-form integer arithmetic of a culling primitive shader (AMD's published RDNA instruction
/// set), per lane, wrapping:
///
/// - `v_mad_u32_u16 d, a, b, c`: `a`'s and `b`'s sixteen-bit halves multiplied, plus `c`: the low
///   halves, or the high where `op_sel` bit 0 or 1 picks it. `op_sel` on the 32-bit `c` or
///   destination is refused.
/// - `v_mad_i32_i24 d, a, b, c`: the sign-extended low 24 bits multiplied, plus `c`.
/// - `v_mul_lo_u32 d, a, b`: the product's low 32 bits.
/// - `v_and_or_b32 d, a, b, c`: `(a & b) | c`.
/// - `v_or3_b32 d, a, b, c`: `a | b | c`.
/// - `v_bfe_u32 d, a, b, c`: `(a >> b[4:0]) & ((1 << c[4:0]) - 1)`, the field of `c[4:0]` bits from
///   bit `b[4:0]`.
/// - `v_msad_u8 d, a, b, c`: `c` plus, for each byte of `b` that is not zero, its absolute
///   difference from `a`'s - NIR's `msad_4x8` (`nir_constant_expressions.py:413`) with its
///   reference as the second source, as ACO emits it (`aco_select_nir_alu.cpp:3303`).
fn integer_long_form<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let sources = if matches!(
        name,
        "v_mul_lo_u32"
            | "v_mul_hi_u32"
            | "v_mul_hi_i32"
            | "v_bfm_b32"
            | "v_bcnt_u32_b32"
            | "v_ldexp_f32"
    ) {
        2
    } else {
        3
    };
    no_integer_modifiers(instruction, sources)?;
    let op_sel = (instruction.word >> 11) & 0xf;
    if name == "v_mad_u32_u16" && op_sel & 0b1100 != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_mad_u32_u16 with op_sel on its 32-bit addend or destination is not translated",
        });
    }
    let Some((Operand::Vector(register), operands)) = instruction.operands.split_first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a long-form integer destination is not a vector register",
        });
    };
    if operands.len() < sources {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a long-form integer instruction has fewer sources than it takes",
        });
    }
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let mut read = [None; 3];
        for (slot, operand) in read.iter_mut().zip(&operands[..sources]) {
            *slot = Some(model.read_source(instruction, operand, lane)?);
        }
        let zero = model.constant(0);
        let [a, b, c] = read.map(|value| value.unwrap_or(zero));
        let value = match name {
            "v_mad_u32_u16" => {
                let low = model.constant(0xffff);
                let sixteen = model.constant(16);
                let half = |model: &mut M, value, high: bool| {
                    let value = if high {
                        model.binary(op::SHIFT_RIGHT_LOGICAL, value, sixteen)
                    } else {
                        value
                    };
                    model.binary(op::BITWISE_AND, value, low)
                };
                let a = half(model, a, op_sel & 1 != 0);
                let b = half(model, b, op_sel & 2 != 0);
                let product = model.binary(op::IMUL, a, b);
                model.add(product, c)
            }
            // The low 32 bits of the product of each source's low 24 bits, plus the third.
            "v_mad_u32_u24" => {
                let low_24 = model.constant(0x00FF_FFFF);
                let a = model.binary(op::BITWISE_AND, a, low_24);
                let b = model.binary(op::BITWISE_AND, b, low_24);
                let product = model.binary(op::IMUL, a, b);
                model.add(product, c)
            }
            "v_mad_i32_i24" => {
                let eight = model.constant(8);
                let extend = |model: &mut M, value| {
                    let raised = model.binary(op::SHIFT_LEFT_LOGICAL, value, eight);
                    model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, eight)
                };
                let a = extend(model, a);
                let b = extend(model, b);
                let product = model.binary(op::IMUL, a, b);
                model.add(product, c)
            }
            "v_mul_lo_u32" => model.binary(op::IMUL, a, b),
            "v_mul_hi_i32" | "v_bfm_b32" | "v_bcnt_u32_b32" | "v_ldexp_f32" => {
                two_source_long_form(model, name, a, b)
            }
            "v_mul_hi_u32" => multiply_high(model, a, b),
            "v_and_or_b32" => {
                let both = model.binary(op::BITWISE_AND, a, b);
                model.binary(op::BITWISE_OR, both, c)
            }
            "v_or3_b32" => {
                let either = model.binary(op::BITWISE_OR, a, b);
                model.binary(op::BITWISE_OR, either, c)
            }
            "v_bfe_u32" | "v_bfe_i32" | "v_alignbyte_b32" | "v_alignbit_b32" => {
                bit_field(model, name, [a, b, c])
            }
            // The hardware's second source is the reference whose zero bytes are skipped: ACO
            // hands NIR's `msad_4x8` reference over as `b` (`swap_srcs`,
            // aco_select_nir_alu.cpp:3303), and radeonsi's culling sums packed counts as
            // `v_msad_u8 d, 0, packed, 0` (ac_nir.c:846).
            _ => masked_sum_of_differences(model, b, a, c),
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// NIR's `msad`: `accumulator` plus `|reference.byte - source.byte|` over the bytes where the
/// reference's is not zero.
fn masked_sum_of_differences<M: Model + ?Sized>(
    model: &mut M,
    reference: Id,
    source: Id,
    accumulator: Id,
) -> Id {
    let byte = model.constant(0xff);
    let zero = model.constant(0);
    let mut sum = accumulator;
    for shift in [0, 8, 16, 24] {
        let shift = model.constant(shift);
        let r = model.binary(op::SHIFT_RIGHT_LOGICAL, reference, shift);
        let r = model.binary(op::BITWISE_AND, r, byte);
        let s = model.binary(op::SHIFT_RIGHT_LOGICAL, source, shift);
        let s = model.binary(op::BITWISE_AND, s, byte);
        let above = model.compare(op::UGREATER_THAN, r, s);
        let forward = model.binary(op::ISUB, r, s);
        let backward = model.binary(op::ISUB, s, r);
        let difference = model.select(above, forward, backward);
        let counted = model.compare(op::INOT_EQUAL, r, zero);
        let difference = model.select(counted, difference, zero);
        sum = model.add(sum, difference);
    }
    sum
}

/// `v_max3_f32` and `v_min3_f32`: the larger or smaller of three floats, each source with its
/// absolute and negate flags, as two of `v_max_f32`'s or `v_min_f32`'s steps. `v_med3_f32`: the
/// median, `max(min(a, b), min(max(a, b), c))` in the same steps (D785). The output clamp to
/// `[0, 1]` applies as the long form's does, under the stage's NaN rule.
fn float_min_max_of_three<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read_allowing_clamp(instruction, false)?;
    // The output multiplier, before the clamp, as the long form applies it.
    let scale = output_scale(modifiers.output_multiplier);
    let clamp = if modifiers.clamp {
        let Some(nan_to_zero) = model.dx10_clamp() else {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "this instruction clamps its result to [0, 1], and what that does to a NaN ",
                    "is the stage's DX10_CLAMP mode, which this translation was not given"
                ),
            });
        };
        Some(nan_to_zero)
    } else {
        None
    };
    let [Operand::Vector(register), first, second, third] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a three-way min/max does not have a vector destination and three sources",
        });
    };
    let register = u32::from(*register);
    let extended = if name == "v_max3_f32" {
        GLSL_FMAX
    } else {
        GLSL_FMIN
    };
    for lane in running_lanes(model) {
        let mut values = [Id(0); 3];
        for (source, (slot, operand)) in values.iter_mut().zip([first, second, third]).enumerate() {
            let value = model.read_source(instruction, operand, lane)?;
            *slot = apply_modifiers(model, value, modifiers, source);
        }
        let value = if name == "v_med3_f32" {
            let low = model.f32_ext_binary(GLSL_FMIN, values[0], values[1]);
            let high = model.f32_ext_binary(GLSL_FMAX, values[0], values[1]);
            let capped = model.f32_ext_binary(GLSL_FMIN, high, values[2]);
            model.f32_ext_binary(GLSL_FMAX, low, capped)
        } else {
            let pair = model.f32_ext_binary(extended, values[0], values[1]);
            model.f32_ext_binary(extended, pair, values[2])
        };
        let value = match scale {
            Some(bits) => {
                let factor = model.constant(bits);
                model.f32_binary(op::FMUL, value, factor)
            }
            None => value,
        };
        let value = match clamp {
            Some(nan_to_zero) => clamp_unit(model, value, nan_to_zero),
            None => value,
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The cube-map face instructions over a direction `(x, y, z)`, each source with its absolute and
/// negate flags (D810): the major axis is z if its magnitude is at least x's and y's, else y if at
/// least x's, else x. `v_cubeid_f32` is the face, 0 to 5 as a float, odd for a negative axis;
/// `v_cubema_f32` twice the major component; `v_cubesc_f32` and `v_cubetc_f32` the face's two
/// coordinates, as NIR's `cube_amd` gives them.
fn cube_face<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let modifiers = Modifiers::read(instruction, false)?;
    let [Operand::Vector(register), first, second, third] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a cube-map face instruction does not have a vector destination and three sources",
        });
    };
    let register = u32::from(*register);
    let magnitude = model.constant(0x7FFF_FFFF);
    let sign = model.constant(0x8000_0000);
    let zero = model.constant(0);
    let zero_f = model.as_float(zero);
    for lane in running_lanes(model) {
        let mut values = [Id(0); 3];
        for (source, (slot, operand)) in values.iter_mut().zip([first, second, third]).enumerate() {
            let value = model.read_source(instruction, operand, lane)?;
            *slot = apply_modifiers(model, value, modifiers, source);
        }
        let [x, y, z] = values;
        let mut absolute = [Id(0); 3];
        for (slot, value) in absolute.iter_mut().zip(values) {
            let bits = model.binary(op::BITWISE_AND, value, magnitude);
            *slot = model.as_float(bits);
        }
        let [ax, ay, az] = absolute;
        let z_over_x = model.compare(op::FORD_GREATER_THAN_EQUAL, az, ax);
        let z_over_y = model.compare(op::FORD_GREATER_THAN_EQUAL, az, ay);
        let z_major = model.both(z_over_x, z_over_y);
        let y_major = model.compare(op::FORD_GREATER_THAN_EQUAL, ay, ax);
        let mut negative = [Id(0); 3];
        for (slot, value) in negative.iter_mut().zip(values) {
            let float = model.as_float(value);
            *slot = model.compare(op::FORD_LESS_THAN, float, zero_f);
        }
        let flip = |model: &mut M, value: Id| model.binary(op::BITWISE_XOR, value, sign);
        // Each axis's answer, then the chain: z over y over x.
        let [on_x, on_y, on_z] = match name {
            "v_cubeid_f32" => [0.0f32, 2.0, 4.0].map(|face| {
                let positive = model.constant(face.to_bits());
                let negative_face = model.constant((face + 1.0).to_bits());
                (positive, negative_face)
            }),
            "v_cubema_f32" => {
                let two = model.constant(2.0f32.to_bits());
                [x, y, z].map(|value| {
                    let doubled = model.f32_binary(op::FMUL, value, two);
                    (doubled, doubled)
                })
            }
            "v_cubesc_f32" => {
                let (minus_z, minus_x) = (flip(model, z), flip(model, x));
                [(minus_z, z), (x, x), (x, minus_x)]
            }
            _ => {
                let (minus_y, minus_z) = (flip(model, y), flip(model, z));
                [(minus_y, minus_y), (z, minus_z), (minus_y, minus_y)]
            }
        }
        .into_iter()
        .zip(negative)
        .map(|((when_positive, when_negative), below)| {
            pick(model, below, when_negative, when_positive)
        })
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|_| TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a cube-map face instruction did not produce one answer per axis",
        })?;
        let not_z = pick(model, y_major, on_y, on_x);
        let value = pick(model, z_major, on_z, not_z);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The names [`half_float_arithmetic`] translates.
const HALF_FLOATS: [&str; 7] = [
    "v_add_f16_e32",
    "v_sub_f16_e32",
    "v_subrev_f16_e32",
    "v_mul_f16_e32",
    "v_max_f16_e32",
    "v_min_f16_e32",
    "v_fmac_f16_e32",
];

/// The sixteen-bit float arithmetic of the short form: `v_add_f16`, `v_sub_f16`, `v_subrev_f16`,
/// `v_mul_f16`, `v_max_f16`, `v_min_f16`, and `v_fmac_f16`, which adds the product to the
/// destination's half. Each reads its sources' low halves, computes in single precision - a
/// product of two halves is exact there - and rounds the result back to a half into the
/// destination's low sixteen bits, the high sixteen kept: on GFX10 these are partial register
/// writes (Mesa `aco_ir.cpp:683-742`, `instr_is_16bit`, and `:956`, which preserves the rest).
fn half_float_arithmetic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, first, second) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit float instruction without a vector destination",
        });
    };
    let register = u32::from(*register);
    let low = model.constant(0xffff);
    let high = model.constant(0xffff_0000);
    for lane in running_lanes(model) {
        let a = model.read_source(instruction, first, lane)?;
        let b = model.read_source(instruction, second, lane)?;
        let held = Operand::Vector(u16::try_from(register).unwrap_or(u16::MAX));
        let old = model.read_source(instruction, &held, lane)?;
        let widen = |model: &mut M, value: Id| {
            let half = model.binary(op::BITWISE_AND, value, low);
            model.half_to_float_bits(half)
        };
        let (x, y) = (widen(model, a), widen(model, b));
        let result = match name {
            "v_add_f16_e32" => model.f32_binary(op::FADD, x, y),
            "v_sub_f16_e32" => model.f32_binary(op::FSUB, x, y),
            "v_subrev_f16_e32" => model.f32_binary(op::FSUB, y, x),
            "v_mul_f16_e32" => model.f32_binary(op::FMUL, x, y),
            "v_max_f16_e32" => model.f32_ext_binary(GLSL_FMAX, x, y),
            "v_min_f16_e32" => model.f32_ext_binary(GLSL_FMIN, x, y),
            // `v_fmac_f16`, fused: rounded once to the half it writes.
            _ => {
                let accumulated = widen(model, old);
                fused_to_half(model, x, y, accumulated)
            }
        };
        let half = model.float_bits_to_half(result);
        let kept = model.binary(op::BITWISE_AND, old, high);
        let written = model.binary(op::BITWISE_OR, kept, half);
        model.write_vector_lane(register, lane, written);
    }
    model.count();
    Ok(())
}

/// Whether `name` is one of the sixteen-bit forms [`sixteen_bit`] carries out.
fn is_sixteen_bit(name: &str) -> bool {
    (name.starts_with("v_cmp_") && name.ends_with("_f16_e32"))
        || PACKED_HALF_FLOATS.contains(&name)
        || LONG_HALF_FLOATS.contains(&name)
        || matches!(
            name,
            "v_lshrrev_b16" | "v_sub_nc_i16" | "v_rcp_f16_e32" | "v_pk_fmac_f16"
        )
}

/// The sixteen-bit forms: packed and long-form float arithmetic, the sixteen-bit shift and
/// subtract, and the half-float reciprocal.
fn sixteen_bit<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    if PACKED_HALF_FLOATS.contains(&name) {
        packed_half_float(model, instruction, name)
    } else if LONG_HALF_FLOATS.contains(&name) {
        long_half_float(model, instruction, name)
    } else if name == "v_rcp_f16_e32" {
        half_float_reciprocal(model, instruction)
    } else if name == "v_pk_fmac_f16" {
        packed_half_accumulate(model, instruction)
    } else if name.starts_with("v_cmp_") {
        half_compare(model, instruction, name, None)
    } else {
        long_sixteen_bit(model, instruction, name)
    }
}

/// The packed sixteen-bit float arithmetic [`packed_half_float`] carries out.
const PACKED_HALF_FLOATS: [&str; 5] = [
    "v_pk_add_f16",
    "v_pk_mul_f16",
    "v_pk_fma_f16",
    "v_pk_min_f16",
    "v_pk_max_f16",
];

/// The long-form sixteen-bit float arithmetic [`long_half_float`] carries out.
const LONG_HALF_FLOATS: [&str; 4] = ["v_fma_f16", "v_min3_f16", "v_max3_f16", "v_med3_f16"];

/// The bits of an inline float as a sixteen-bit float instruction reads it: the half-precision
/// value of the constant the operand table names (LLVM encodes `v_add_f16 v0, 1.0, v0` with
/// operand 242, and `0.15915494` as 248, a half `0x3118`). `None` for a name that is not one.
fn inline_half_bits(name: &str) -> Option<u32> {
    let magnitude = match name.trim_start_matches('-') {
        "0.5" => 0x3800,
        "1.0" => 0x3C00,
        "2.0" => 0x4000,
        "4.0" => 0x4400,
        INVERSE_TWO_PI => 0x3118,
        _ => return None,
    };
    Some(if name.starts_with('-') {
        magnitude | 0x8000
    } else {
        magnitude
    })
}

/// One sixteen-bit float source as its instruction reads it, zero above: [`read_half`], or the
/// half-precision value of an inline float in the low half.
fn read_half_float<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    operand: &Operand,
    lane: u32,
    high: bool,
) -> Result<Id, TranslateError> {
    // An inline float is the same half in either half: a packed instruction's `op_sel_hi` reads the
    // constant (ACO treats a packed operand's constant as it does a register's,
    // `aco_optimizer.cpp:1315-1320`, and takes `op_sel_hi` as in force for it,
    // `aco_register_allocation.cpp:2998`).
    if let Operand::Named(name) = operand
        && let Some(bits) = inline_half_bits(name)
    {
        return Ok(model.constant(bits));
    }
    read_half(model, instruction, operand, lane, high)
}

/// A sixteen-bit float compare into the condition mask (AMD's published RDNA instruction set,
/// `VOPC`): each source's low half - or, in an SDWA form, the half its select names, negated and
/// made absolute as the word says - compared as single precision, which orders halves exactly.
/// Grand Theft Auto V's prologue does `v_cmp_lt_f16_sdwa vcc, v10, s? src1_sel:WORD_1` (word
/// `0x8506000a`). An SDWA form naming a destination of its own, or clamping, is refused.
fn half_compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
    word: Option<u32>,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let float_name = name.replacen("_f16_", "_f32_", 1);
    let (opcode, _) = op_for_compare(instruction, &float_name)?;
    let Some(Operand::Vector(vsrc1)) = instruction.operands.get(2) else {
        return Err(refuse(
            "a sixteen-bit float compare's second source is not its register field",
        ));
    };
    // Each source, the half it reads, and its negate and absolute value.
    let sources: [(Operand, bool, [bool; 2]); 2] =
        match word {
            None => [
                (
                    instruction.operands.get(1).cloned().ok_or_else(|| {
                        refuse("a sixteen-bit float compare without a first source")
                    })?,
                    false,
                    [false; 2],
                ),
                (Operand::Vector(*vsrc1), false, [false; 2]),
            ],
            Some(word) => {
                if word >> 15 & 1 != 0 || word >> 13 & 1 != 0 {
                    return Err(refuse(concat!(
                        "a sixteen-bit float SDWA compare naming a destination of its own, or ",
                        "clamping, which is not translated"
                    )));
                }
                let fields = SdwaFields::read(word);
                let half_of = |select: u32| match select {
                    SDWA_WORD_0 | SDWA_DWORD => Ok(false),
                    SDWA_WORD_1 => Ok(true),
                    _ => Err(refuse("a sixteen-bit float SDWA compare selecting a byte")),
                };
                [
                    (
                        sdwa_operand(instruction, fields.src0, fields.src0_scalar)?,
                        half_of(fields.select[0].0)?,
                        [word >> 20 & 1 != 0, word >> 21 & 1 != 0],
                    ),
                    (
                        sdwa_operand(instruction, u32::from(*vsrc1), fields.src1_scalar)?,
                        half_of(fields.select[1].0)?,
                        [word >> 28 & 1 != 0, word >> 29 & 1 != 0],
                    ),
                ]
            }
        };
    let (sign, magnitude) = (model.constant(0x8000), model.constant(0x7FFF));
    let zero = model.constant(0);
    let mut halves = (zero, zero);
    for lane in 0..model.modelled_lanes() {
        if model.lane_known_idle(lane) {
            continue;
        }
        let mut sides = [zero, zero];
        for (side, (operand, high, [negate, absolute])) in sides.iter_mut().zip(&sources) {
            let mut bits = read_half_float(model, instruction, operand, lane, *high)?;
            if *absolute {
                bits = model.binary(op::BITWISE_AND, bits, magnitude);
            }
            if *negate {
                bits = model.binary(op::BITWISE_XOR, bits, sign);
            }
            let widened = model.half_to_float_bits(bits);
            *side = model.as_float(widened);
        }
        let condition = lane_compare(model, opcode, sides[0], sides[1]);
        halves = model.set_lane_bit(halves, lane, condition);
    }
    let halves = running_bits(model, halves)?;
    write_wave_mask(model, VCC_LOW_HALF, halves)?;
    model.count();
    Ok(())
}

/// Rounds `operands`, already single-precision bits, through the sixteen-bit float operation `name`
/// names: a sum, product, fused multiply-add, minimum, maximum or median. A product of two halves
/// is exact in single precision, so a multiply-add rounds once there and once to a half.
fn half_float_operation<M: Model + ?Sized>(model: &mut M, name: &str, operands: &[Id]) -> Id {
    let at = |index: usize| operands.get(index).copied().unwrap_or(operands[0]);
    let (a, b, c) = (at(0), at(1), at(2));
    match name {
        "v_pk_add_f16" => model.f32_binary(op::FADD, a, b),
        "v_pk_mul_f16" => model.f32_binary(op::FMUL, a, b),
        "v_pk_min_f16" => model.f32_ext_binary(GLSL_FMIN, a, b),
        "v_pk_max_f16" => model.f32_ext_binary(GLSL_FMAX, a, b),
        "v_min3_f16" => {
            let less = model.f32_ext_binary(GLSL_FMIN, a, b);
            model.f32_ext_binary(GLSL_FMIN, less, c)
        }
        "v_max3_f16" => {
            let more = model.f32_ext_binary(GLSL_FMAX, a, b);
            model.f32_ext_binary(GLSL_FMAX, more, c)
        }
        "v_med3_f16" => {
            let (less, more) = (
                model.f32_ext_binary(GLSL_FMIN, a, b),
                model.f32_ext_binary(GLSL_FMAX, a, b),
            );
            let capped = model.f32_ext_binary(GLSL_FMIN, more, c);
            model.f32_ext_binary(GLSL_FMAX, less, capped)
        }
        // `v_pk_fma_f16` and `v_fma_f16`, fused: rounded once to the half the caller narrows to.
        _ => fused_to_half(model, a, b, c),
    }
}

/// The packed sixteen-bit float arithmetic (AMD's published RDNA instruction set, `VOP3P`):
/// `v_pk_add_f16`, `v_pk_mul_f16`, `v_pk_fma_f16`, `v_pk_min_f16` and `v_pk_max_f16`. Each half of
/// the result is the operation on the halves [`packed_selects`]'s bits pick - `op_sel` for the low
/// result, `op_sel_hi` for the high - each negated where `neg_lo` (the second word's 31:29) or
/// `neg_hi` (the first word's 10:8) says, computed in single precision and rounded back to a half;
/// the whole register is written. Clamp (bit 15) is refused.
fn packed_half_float<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [Operand::Vector(destination), sources @ ..] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a packed sixteen-bit float instruction without a vector destination",
        });
    };
    let second = instruction.second_word.unwrap_or(0);
    if instruction.word & 1 << 15 != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a packed sixteen-bit float instruction's clamp, which is not translated",
        });
    }
    let low_select = [11, 12, 13].map(|bit| instruction.word >> bit & 1 != 0);
    let high_select = [
        second >> 27 & 1 != 0,
        second >> 28 & 1 != 0,
        instruction.word >> 14 & 1 != 0,
    ];
    let negated = [
        [29, 30, 31].map(|bit| second >> bit & 1 != 0),
        [8, 9, 10].map(|bit| instruction.word >> bit & 1 != 0),
    ];
    let sign = model.constant(0x8000);
    let sixteen = model.constant(16);
    for lane in running_lanes(model) {
        let mut halves = [model.constant(0); 2];
        for (half, selects) in [low_select, high_select].iter().enumerate() {
            let mut operands = Vec::with_capacity(sources.len());
            for (index, source) in sources.iter().enumerate() {
                let bits = read_half_float(model, instruction, source, lane, selects[index])?;
                let bits = if negated[half][index] {
                    model.binary(op::BITWISE_XOR, bits, sign)
                } else {
                    bits
                };
                operands.push(model.half_to_float_bits(bits));
            }
            let result = half_float_operation(model, name, &operands);
            halves[half] = model.float_bits_to_half(result);
        }
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, halves[1], sixteen);
        let word = model.binary(op::BITWISE_OR, halves[0], high);
        model.write_vector_lane(u32::from(*destination), lane, word);
    }
    model.count();
    Ok(())
}

/// The packed sixteen-bit fused multiply-accumulate in its short form (`v_pk_fmac_f16`, `VOP2`
/// opcode 60, ACO's `aco_opcodes.py:1009`): each half of the destination becomes the first
/// source's half times the second's plus its own, low with low and high with high - no selects and
/// no modifiers in this form - computed in single precision and rounded back to a half. Grand Theft
/// Auto V's scene compute program does `v_pk_fmac_f16 v12, v46, v6` (word `0x78180d2e`).
fn packed_half_accumulate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [Operand::Vector(destination), first, second] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a packed multiply-accumulate without a vector destination and two sources",
        });
    };
    let accumulator = Operand::Vector(*destination);
    let sixteen = model.constant(16);
    for lane in running_lanes(model) {
        let mut halves = [model.constant(0); 2];
        for (half, high) in [false, true].into_iter().enumerate() {
            let mut operands = Vec::with_capacity(3);
            for source in [first, second, &accumulator] {
                let bits = read_half_float(model, instruction, source, lane, high)?;
                operands.push(model.half_to_float_bits(bits));
            }
            let result = half_float_operation(model, "v_pk_fma_f16", &operands);
            halves[half] = model.float_bits_to_half(result);
        }
        let high = model.binary(op::SHIFT_LEFT_LOGICAL, halves[1], sixteen);
        let word = model.binary(op::BITWISE_OR, halves[0], high);
        model.write_vector_lane(u32::from(*destination), lane, word);
    }
    model.count();
    Ok(())
}

/// The long-form sixteen-bit float arithmetic (AMD's published RDNA instruction set, `VOP3`):
/// `v_fma_f16`, `v_min3_f16`, `v_max3_f16` and `v_med3_f16`. Each source is the half `op_sel`
/// (bits 13:11) picks, its absolute value where `abs` (bits 10:8) says and negated where `neg` (the
/// second word's 31:29) does; the result, computed in single precision and rounded to a half, goes
/// to the destination's half `op_sel` bit 14 picks, the other kept. Clamp (bit 15) is refused.
fn long_half_float<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [Operand::Vector(destination), sources @ ..] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit float instruction without a vector destination",
        });
    };
    if instruction.word & 1 << 15 != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit float instruction's clamp, which is not translated",
        });
    }
    let second = instruction.second_word.unwrap_or(0);
    let select = [11, 12, 13].map(|bit| instruction.word >> bit & 1 != 0);
    let absolute = [8, 9, 10].map(|bit| instruction.word >> bit & 1 != 0);
    let negated = [29, 30, 31].map(|bit| second >> bit & 1 != 0);
    let high_destination = instruction.word >> 14 & 1 != 0;
    let (sign, magnitude) = (model.constant(0x8000), model.constant(0x7FFF));
    for lane in running_lanes(model) {
        let mut operands = Vec::with_capacity(sources.len());
        for (index, source) in sources.iter().enumerate() {
            let mut bits = read_half_float(model, instruction, source, lane, select[index])?;
            if absolute[index] {
                bits = model.binary(op::BITWISE_AND, bits, magnitude);
            }
            if negated[index] {
                bits = model.binary(op::BITWISE_XOR, bits, sign);
            }
            operands.push(model.half_to_float_bits(bits));
        }
        let result = half_float_operation(model, name, &operands);
        let half = model.float_bits_to_half(result);
        write_half(
            model,
            instruction,
            (u32::from(*destination), lane),
            half,
            high_destination,
        )?;
    }
    model.count();
    Ok(())
}

/// `v_rcp_f16_e32 d, a`: one over `a`'s low half, computed in single precision and rounded to a half
/// into the destination's low half, the high kept (AMD's published RDNA instruction set; a
/// sixteen-bit result keeps the rest of its register on GFX10, `aco_ir.cpp:956`).
fn half_float_reciprocal<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [Operand::Vector(destination), source] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_rcp_f16 does not name a vector register and a source",
        });
    };
    let one = model.constant(1.0f32.to_bits());
    for lane in running_lanes(model) {
        let bits = read_half_float(model, instruction, source, lane, false)?;
        let value = model.half_to_float_bits(bits);
        let reciprocal = model.f32_binary(op::FDIV, one, value);
        let half = model.float_bits_to_half(reciprocal);
        write_half(
            model,
            instruction,
            (u32::from(*destination), lane),
            half,
            false,
        )?;
    }
    model.count();
    Ok(())
}

/// `v_lshrrev_b16 d, a, b` - `b`'s half shifted right by `a`'s low four bits - and
/// `v_sub_nc_i16 d, a, b` - `a`'s half less `b`'s, wrapping - (AMD's published RDNA instruction
/// set): each source the half `op_sel` (bits 12:11) picks, the result into the destination's half
/// bit 14 picks, the other kept. Clamp (bit 15) is refused.
fn long_sixteen_bit<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let [Operand::Vector(destination), first, second] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit integer instruction without a vector destination and two sources",
        });
    };
    if instruction.word & 1 << 15 != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit integer instruction's clamp, which is not translated",
        });
    }
    let select = [11, 12].map(|bit| instruction.word >> bit & 1 != 0);
    let high_destination = instruction.word >> 14 & 1 != 0;
    let (low, fifteen) = (model.constant(0xFFFF), model.constant(0xF));
    for lane in running_lanes(model) {
        let a = read_half(model, instruction, first, lane, select[0])?;
        let b = read_half(model, instruction, second, lane, select[1])?;
        let result = if name == "v_lshrrev_b16" {
            let amount = model.binary(op::BITWISE_AND, a, fifteen);
            model.binary(op::SHIFT_RIGHT_LOGICAL, b, amount)
        } else {
            let difference = model.binary(op::ISUB, a, b);
            model.binary(op::BITWISE_AND, difference, low)
        };
        write_half(
            model,
            instruction,
            (u32::from(*destination), lane),
            result,
            high_destination,
        )?;
    }
    model.count();
    Ok(())
}

/// `v_writelane_b32 v, s, lane`: `s` into `v` at the lane the second source names, its low six bits
/// in a wave of sixty-four and five in one of thirty-two, every other lane kept (AMD's published RDNA
/// instruction set). The instruction ignores the execution mask: it writes that lane whether it runs
/// or not (D978). Grand Theft Auto V's ray traversal pushes its node stack so, at the lane its depth
/// names, under a mask of the lanes the last box test sent on; a push the mask kept out was lost, and
/// the pop read an older entry back, so the walk revisited nodes for ever.
fn write_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [Operand::Vector(destination), value, lane] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_writelane_b32 does not name a vector register, a value and a lane",
        });
    };
    let destination = u32::from(*destination);
    let value = model.read_source(instruction, value, 0)?;
    let chosen = model.read_source(instruction, lane, 0)?;
    let low_bits = model.constant(model.lanes() - 1);
    let chosen = model.binary(op::BITWISE_AND, chosen, low_bits);
    // Every lane, running or not: a lane known not to run is still read by `v_readlane_b32`.
    let lanes: Vec<u32> = if model.in_lane_loop() {
        vec![0]
    } else {
        (0..model.modelled_lanes()).collect()
    };
    for lane in lanes {
        let held = Operand::Vector(u16::try_from(destination).unwrap_or(u16::MAX));
        let old = model.read_source(instruction, &held, lane)?;
        // The wave's lane, at run time where an invocation's lanes are (D760, D973).
        let this = crate::scratch::lane_id(model, lane);
        let here = model.compare(op::IEQUAL, chosen, this);
        let written = model.select(here, value, old);
        model.write_vector_lane_unmasked(destination, lane, written);
    }
    model.count();
    Ok(())
}

/// `v_readlane_b32 s, v, lane`: the scalar takes `v` at the lane the second source names, its low
/// six bits in a wave of sixty-four and five in one of thirty-two (AMD's published RDNA instruction
/// set), whether that lane runs or not. The scalar may be a lane mask's half, as Grand Theft Auto
/// V's `v_readlane_b32 vcc_lo, v17, 63` reads the last lane's sum.
fn read_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [destination, Operand::Vector(source), lane] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_readlane_b32 does not name a scalar, a vector register and a lane",
        });
    };
    let lane = model.read_source(instruction, lane, 0)?;
    let low_bits = model.constant(model.lanes() - 1);
    let lane = model.binary(op::BITWISE_AND, lane, low_bits);
    let value =
        model
            .read_vector_at(u32::from(*source), lane)
            .ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a lane read at a lane known only when the program runs needs every lane",
            })?;
    write_scalar_operand(model, instruction, destination, value)?;
    model.count();
    Ok(())
}

/// `v_readfirstlane_b32 s, v`: the scalar takes `v` at the lowest running lane, and at lane 0 where
/// no lane runs (AMD's published RDNA instruction set).
fn read_first_lane<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [destination, Operand::Vector(source)] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_readfirstlane_b32 does not name a scalar and a vector register",
        });
    };
    let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    // The lowest set bit's place, as the count of the clear bits below it: 32 for a half with none.
    let lowest = |model: &mut M, half: Id| {
        let one = model.constant(1);
        let below = model.binary(op::ISUB, half, one);
        let clear = model.not(half);
        let under = model.binary(op::BITWISE_AND, below, clear);
        model.bit_count(under)
    };
    let thirty_two = model.constant(32);
    let mut first = lowest(model, running_low);
    let lanes = model.lanes();
    if lanes > 32 {
        let high = lowest(model, running_high);
        let above = model.add(high, thirty_two);
        let none_low = model.compare(op::IEQUAL, first, thirty_two);
        first = model.select(none_low, above, first);
    }
    let every = model.constant(lanes);
    let none = model.compare(op::IEQUAL, first, every);
    let zero = model.constant(0);
    let lane = model.select(none, zero, first);
    let value =
        model
            .read_vector_at(u32::from(*source), lane)
            .ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a lane read at a lane known only when the program runs needs every lane",
            })?;
    write_scalar_operand(model, instruction, destination, value)?;
    model.count();
    Ok(())
}

/// Writes a scalar result where `destination` names: a scalar register, a lane mask's low or high
/// half used as a scalar (`vcc_lo`, `vcc_hi`, `exec_lo`, `exec_hi`), or `m0`.
fn write_scalar_operand<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    destination: &Operand,
    value: Id,
) -> Result<(), TranslateError> {
    if let Some(mask) = mask_destination(destination) {
        return write_mask_low(model, mask, value);
    }
    if let Operand::Named(named) = destination {
        if let Some(mask) = lane_mask_high_name(named) {
            return write_mask_high(model, mask, value);
        }
        if named == M0 {
            model.write_m0(value);
            return Ok(());
        }
    }
    let register = scalar_destination(instruction, destination)?;
    model.write_scalar(register, value);
    Ok(())
}

/// `v_permlane16_b32 d, v, select_low, select_high`: within each row of sixteen lanes, lane `i`
/// takes `v` at the row's lane named by the four-bit selector `i` - lanes 0 to 7 from the first
/// scalar's nibbles, 8 to 15 from the second's (AMD's published RDNA instruction set; ACO's
/// `lane_permute_16_amd`). Without `FETCH_INACTIVE` an inactive source lane is not read; without
/// `BOUND_CTRL` the destination lane then keeps its value. Either flag set (`op_sel`) is refused.
fn permute_lanes_16<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [
        Operand::Vector(register),
        Operand::Vector(source),
        low,
        high,
    ] = instruction.operands.as_slice()
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_permlane16_b32 does not name a vector destination, a vector and two selects",
        });
    };
    if (instruction.word >> 11) & 0xf != 0 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_permlane16_b32 with FETCH_INACTIVE or BOUND_CTRL is not translated",
        });
    }
    let (register, source) = (u32::from(*register), u32::from(*source));
    let selects = [
        model.read_source(instruction, low, 0)?,
        model.read_source(instruction, high, 0)?,
    ];
    let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    let nibble = model.constant(0xf);
    // Every lane's value first: each reads the source before any lane writes the destination.
    let mut values = Vec::new();
    for lane in running_lanes(model) {
        let (select, row) = if let Some(index) = model.lane_number(lane) {
            // One invocation per lane (D760): its place in its row, at run time.
            let (fifteen, seven, eight, four) = (
                model.constant(15),
                model.constant(7),
                model.constant(8),
                model.constant(4),
            );
            let within = model.binary(op::BITWISE_AND, index, fifteen);
            let upper = model.compare(op::UGREATER_THAN_EQUAL, within, eight);
            let word = model.select(upper, selects[1], selects[0]);
            let place = model.binary(op::BITWISE_AND, within, seven);
            let shift = model.binary(op::IMUL, place, four);
            let select = model.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
            let row = model.binary(op::ISUB, index, within);
            (select, row)
        } else {
            let within = lane % 16;
            let shift = model.constant(4 * (within % 8));
            let select = model.binary(
                op::SHIFT_RIGHT_LOGICAL,
                selects[usize::from(within >= 8)],
                shift,
            );
            (select, model.constant(lane - within))
        };
        let select = model.binary(op::BITWISE_AND, select, nibble);
        let from = model.add(row, select);
        let fetched = model
            .read_vector_at(source, from)
            .ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a lane permute needs every lane",
            })?;
        // The source lane's execution bit: rows below thirty-two in the low half.
        let half = if model.lane_number(lane).is_some() {
            let thirty_two = model.constant(32);
            let upper = model.compare(op::UGREATER_THAN_EQUAL, from, thirty_two);
            model.select(upper, running_high, running_low)
        } else if lane < 32 {
            running_low
        } else {
            running_high
        };
        let thirty_one = model.constant(31);
        let bit = model.binary(op::BITWISE_AND, from, thirty_one);
        let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, half, bit);
        let one = model.constant(1);
        let active = model.binary(op::BITWISE_AND, shifted, one);
        let zero = model.constant(0);
        let active = model.compare(op::INOT_EQUAL, active, zero);
        let kept = model.read_source(instruction, &instruction.operands[0], lane)?;
        values.push((lane, model.select(active, fetched, kept)));
    }
    for (lane, value) in values {
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// `ds_bpermute_b32 d, address, data offset:o`: each running lane takes `data` from the lane
/// `(address + o) / 4` names, or zero where that lane does not run (AMD's published RDNA 2
/// instruction set, `DS_BPERMUTE_B32`: the execution mask applies to the source reads). The lane
/// is taken modulo 32 within the reading lane's own half of a wave of 64: on this generation the
/// permute works on half waves, which is why ACO builds a whole-wave one from two
/// (`src/amd/compiler/aco_lower_to_hw_instr.cpp:1054-1059`). No local memory is read or written.
fn backward_permute<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let [
        Operand::Vector(register),
        address,
        Operand::Vector(data),
        rest @ ..,
    ] = instruction.operands.as_slice()
    else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "ds_bpermute_b32 does not name a vector destination, an address and a vector",
        });
    };
    let offset = match rest.first() {
        Some(Operand::Immediate(offset)) => u32::try_from(*offset).unwrap_or(0),
        _ => 0,
    };
    let (register, data) = (u32::from(*register), u32::from(*data));
    let (running_low, running_high) = model.read_lane_mask(EXEC_LOW_HALF)?;
    let (two, thirty_one, thirty_two) = (model.constant(2), model.constant(31), model.constant(32));
    let (offset, one, zero) = (model.constant(offset), model.constant(1), model.constant(0));
    // Every lane's value first: each reads the data before any lane writes the destination. Where
    // the model runs the whole wave in one invocation, a loop over the lanes stages them, and a
    // second writes them.
    let looped = model.open_lane_loop();
    let mut values = Vec::new();
    for lane in running_lanes(model) {
        let byte = model.read_source(instruction, address, lane)?;
        let byte = model.add(byte, offset);
        let word = model.binary(op::SHIFT_RIGHT_LOGICAL, byte, two);
        let within = model.binary(op::BITWISE_AND, word, thirty_one);
        // The reading lane's half: the high one from lane thirty-two up.
        let index = model.lane_number(lane).or(model.looped_lane());
        let (from, half) = if let Some(index) = index {
            let upper = model.compare(op::UGREATER_THAN_EQUAL, index, thirty_two);
            let base = model.select(upper, thirty_two, zero);
            let half = model.select(upper, running_high, running_low);
            (model.add(base, within), half)
        } else if lane < 32 {
            (within, running_low)
        } else {
            (model.add(within, thirty_two), running_high)
        };
        let fetched = model
            .read_vector_at(data, from)
            .ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a lane permute needs every lane",
            })?;
        let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, half, within);
        let active = model.binary(op::BITWISE_AND, shifted, one);
        let active = model.compare(op::INOT_EQUAL, active, zero);
        values.push((lane, model.select(active, fetched, zero)));
    }
    if looped {
        for (lane, value) in values {
            model.write_vector_lane(DPP_STAGING_REGISTER, lane, value);
        }
        model.close_lane_loop();
        let staged = Operand::Vector(DPP_STAGING_REGISTER as u16);
        let again = model.open_lane_loop();
        for lane in running_lanes(model) {
            let value = model.read_source(instruction, &staged, lane)?;
            model.write_vector_lane(register, lane, value);
        }
        if again {
            model.close_lane_loop();
        }
    } else {
        for (lane, value) in values {
            model.write_vector_lane(register, lane, value);
        }
    }
    model.count();
    Ok(())
}

/// `v_bfi_b32 d, mask, a, b`: `(mask & a) | (!mask & b)` per lane (AMD's published RDNA instruction
/// set, `V_BFI_B32`; NIR's `bitfield_select`).
fn bitfield_insert<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 3)?;
    let [destination, mask, inserted, base] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_bfi_b32 does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_bfi_b32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let mask = model.read_source(instruction, mask, lane)?;
        let inserted = model.read_source(instruction, inserted, lane)?;
        let base = model.read_source(instruction, base, lane)?;
        let kept = model.binary(op::BITWISE_AND, mask, inserted);
        let inverse = model.not(mask);
        let rest = model.binary(op::BITWISE_AND, inverse, base);
        let value = model.binary(op::BITWISE_OR, kept, rest);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The two halves of a 64-bit mask with bits `offset..offset + width` set (both below sixty-four),
/// the ones past bit sixty-three dropped: each half is the ones below its end minus the ones below
/// its start, clamped to the half.
fn ones_between<M: Model + ?Sized>(model: &mut M, offset: Id, width: Id) -> (Id, Id) {
    let end = model.binary(op::IADD, offset, width);
    let half = |model: &mut M, base: u32| {
        let base_id = model.constant(base);
        let thirty_two = model.constant(32);
        let zero = model.constant(0);
        // How many bits of this half lie below `edge`: clamp(edge - base, 0, 32).
        let below = |model: &mut M, edge: Id| {
            let above_base = model.compare(op::UGREATER_THAN_EQUAL, edge, base_id);
            let raw = model.binary(op::ISUB, edge, base_id);
            let raw = model.select(above_base, raw, zero);
            let full = model.compare(op::UGREATER_THAN_EQUAL, raw, thirty_two);
            (raw, full)
        };
        // Ones below a count, where a count of thirty-two is every bit.
        let ones = |model: &mut M, (count, full): (Id, Id)| {
            let one = model.constant(1);
            let low_five = model.constant(31);
            let count = model.binary(op::BITWISE_AND, count, low_five);
            let bit = model.binary(op::SHIFT_LEFT_LOGICAL, one, count);
            let mask = model.binary(op::ISUB, bit, one);
            let all = model.constant(u32::MAX);
            model.select(full, all, mask)
        };
        let to_end = below(model, end);
        let to_end = ones(model, to_end);
        let to_start = below(model, offset);
        let to_start = ones(model, to_start);
        let inverse = model.not(to_start);
        model.binary(op::BITWISE_AND, to_end, inverse)
    };
    let low = half(model, 0);
    let high = half(model, 32);
    (low, high)
}

/// `v_cvt_f32_u32`, `v_cvt_f32_i32`, `v_cvt_i32_f32` and `v_cvt_u32_f32`, per lane, and
/// `v_cvt_f32_ubyte0` to `3`: byte `n` of the source as an unsigned float.
///
/// Unsigned or signed to float rounds to nearest, as SPIR-V's conversion does. Float to signed truncates
/// toward zero and saturates: below `-2^31` (and `-inf`) reads `0x80000000`, `2^31` and above (and
/// `+inf`) `0x7fffffff`, and a NaN zero - the published instruction set's conversion rules, which
/// SPIR-V leaves undefined out of range, so the edges are selected rather than converted. Float to
/// unsigned truncates the same way: below zero (and `-inf`) and a NaN read zero, `2^32` and above
/// (and `+inf`) `0xffffffff`.
fn integer_conversion<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a conversion's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let bits = model.read_source(instruction, source, lane)?;
        let value = if let Some(byte) = name
            .strip_prefix("v_cvt_f32_ubyte")
            .and_then(|rest| rest.strip_suffix("_e32"))
            .and_then(|byte| byte.parse::<u32>().ok())
        {
            let shift = model.constant(8 * byte);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, bits, shift);
            let low = model.constant(0xff);
            let byte = model.binary(op::BITWISE_AND, shifted, low);
            integer_to_float(model, byte, false)
        } else if name == "v_cvt_f32_u32_e32" || name == "v_cvt_f32_i32_e32" {
            integer_to_float(model, bits, name == "v_cvt_f32_i32_e32")
        } else if name == "v_cvt_u32_f32_e32" {
            float_to_unsigned_saturated(model, bits)
        } else if name == "v_cvt_f16_f32_e32" {
            float_to_half(model, bits)
        } else if name == "v_cvt_flr_i32_f32_e32" {
            // Toward negative infinity (`V_CVT_FLR_I32_F32`).
            let floored = model.f32_ext_unary(GLSL_FLOOR, bits);
            float_to_signed_saturated(model, floored)
        } else if name == "v_cvt_rpi_i32_f32_e32" {
            // To nearest, a half rounding up: the floor of the value plus a half
            // (`V_CVT_RPI_I32_F32`).
            let half = model.constant(0.5_f32.to_bits());
            let raised = model.f32_binary(op::FADD, bits, half);
            let floored = model.f32_ext_unary(GLSL_FLOOR, raised);
            float_to_signed_saturated(model, floored)
        } else {
            float_to_signed_saturated(model, bits)
        };
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// The conversions [`integer_conversion`] translates, between integers and floats.
const INTEGER_CONVERSIONS: [&str; 11] = [
    "v_cvt_f32_u32_e32",
    "v_cvt_f32_i32_e32",
    "v_cvt_i32_f32_e32",
    "v_cvt_u32_f32_e32",
    "v_cvt_f32_ubyte0_e32",
    "v_cvt_f32_ubyte1_e32",
    "v_cvt_f32_ubyte2_e32",
    "v_cvt_f32_ubyte3_e32",
    "v_cvt_rpi_i32_f32_e32",
    "v_cvt_flr_i32_f32_e32",
    "v_cvt_f16_f32_e32",
];

/// A float's bits as a half rounded to nearest, in the low sixteen bits with the high ones zero
/// (`V_CVT_F16_F32`): `GLSL.std.450` `PackHalf2x16` of the value and zero.
fn float_to_half<M: Model + ?Sized>(model: &mut M, bits: Id) -> Id {
    let (u32_type, f32_type, set) = (model.u32_type(), model.f32_type(), model.glsl_set());
    let zero = model.constant(0);
    let b = model.builder();
    let vec2 = b.declare_type(op::TYPE_VECTOR, &[f32_type.0, 2]);
    let (value, nought) = (b.id(), b.id());
    b.function(op::BITCAST, &[f32_type.0, value.0, bits.0]);
    b.function(op::BITCAST, &[f32_type.0, nought.0, zero.0]);
    let pair = b.id();
    b.function(
        op::COMPOSITE_CONSTRUCT,
        &[vec2.0, pair.0, value.0, nought.0],
    );
    b.ext_inst(u32_type, set, GLSL_PACK_HALF_2X16, &[pair])
}

/// GLSL.std.450 instruction number for `PackHalf2x16`.
const GLSL_PACK_HALF_2X16: u32 = 58;

/// An integer to a float's bits, signed or unsigned, rounding to nearest as SPIR-V's conversion does.
fn integer_to_float<M: Model + ?Sized>(model: &mut M, bits: Id, signed: bool) -> Id {
    let conversion = if signed {
        op::CONVERT_S_TO_F
    } else {
        op::CONVERT_U_TO_F
    };
    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let b = model.builder();
    let float = b.id();
    b.function(conversion, &[f32_type.0, float.0, bits.0]);
    let out = b.id();
    b.function(op::BITCAST, &[u32_type.0, out.0, float.0]);
    out
}

/// A float's bits to an unsigned integer, truncated, saturated and NaN to zero.
fn float_to_unsigned_saturated<M: Model + ?Sized>(model: &mut M, bits: Id) -> Id {
    let float = model.as_float(bits);
    // 2^32, exactly representable; zero.
    let top = model.constant(0x4f80_0000);
    let top = model.as_float(top);
    let zero_bits = model.constant(0);
    let zero = model.as_float(zero_bits);
    let too_high = model.compare(op::FORD_GREATER_THAN_EQUAL, float, top);
    // Below zero, or a NaN: neither is ordered at or above zero.
    let in_range_low = model.compare(op::FORD_GREATER_THAN_EQUAL, float, zero);
    // Clamped into range so the conversion is always defined: chosen as bits, since a select is a
    // word's, and only then read as a float.
    let safe = model.select(in_range_low, bits, zero_bits);
    let safe = model.select(too_high, zero_bits, safe);
    let safe = model.as_float(safe);
    let u32_type = model.u32_type();
    let b = model.builder();
    let converted = b.id();
    b.function(op::CONVERT_F_TO_U, &[u32_type.0, converted.0, safe.0]);
    let maximum = model.constant(0xffff_ffff);
    let converted = model.select(too_high, maximum, converted);
    model.select(in_range_low, converted, zero_bits)
}

/// A float's bits to a signed integer, truncated, saturated and NaN to zero.
fn float_to_signed_saturated<M: Model + ?Sized>(model: &mut M, bits: Id) -> Id {
    let float = model.as_float(bits);
    // 2^31 and -2^31, exactly representable.
    let top = model.constant(0x4f00_0000);
    let top = model.as_float(top);
    let bottom = model.constant(0xcf00_0000);
    let bottom = model.as_float(bottom);
    let too_high = model.compare(op::FORD_GREATER_THAN_EQUAL, float, top);
    let too_low = model.compare(op::FORD_LESS_THAN, float, bottom);
    let nan = {
        let bool_type = model.bool_type();
        let b = model.builder();
        let nan = b.id();
        b.function(op::IS_NAN, &[bool_type.0, nan.0, float.0]);
        nan
    };
    // In range, or clamped into range so the conversion is always defined: chosen as bits, since a
    // select is a word's, and only then read as a float.
    let bottom_bits = model.constant(0xcf00_0000);
    let clamped_high = model.select(too_high, bottom_bits, bits);
    let safe = model.select(too_low, bottom_bits, clamped_high);
    let zero_bits = model.constant(0);
    let safe = model.select(nan, zero_bits, safe);
    let safe = model.as_float(safe);
    let u32_type = model.u32_type();
    let b = model.builder();
    let converted = b.id();
    b.function(op::CONVERT_F_TO_S, &[u32_type.0, converted.0, safe.0]);
    let maximum = model.constant(0x7fff_ffff);
    let minimum = model.constant(0x8000_0000);
    let converted = model.select(too_high, maximum, converted);
    let converted = model.select(too_low, minimum, converted);
    model.select(nan, zero_bits, converted)
}

/// A long-form compare: every lane's answer into the destination's lane mask - a register pair, or
/// in a thirty-two-lane wave one register - or into a named mask. A float compare takes each
/// source's absolute and negate flags; an integer compare refuses them. The third source field is
/// unused.
fn compare_long<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let (opcode, floats) = op_for_compare(instruction, name)?;
    let modifiers = if floats || opcode == CLASS_TEST {
        Modifiers::read(instruction, false)?
    } else {
        no_integer_modifiers(instruction, 2)?;
        Modifiers::default()
    };
    let (destination, first, second) = three_operands(instruction)?;
    let zero = model.constant(0);
    let halves = every_lane_bit(model, true, |model, lane| {
        let mut sides = [zero, zero];
        for (source, (side, operand)) in sides.iter_mut().zip([first, second]).enumerate() {
            let value = model.read_source(instruction, operand, lane)?;
            let value = apply_modifiers(model, value, modifiers, source);
            *side = if floats { model.as_float(value) } else { value };
        }
        Ok(lane_compare(model, opcode, sides[0], sides[1]))
    })?;
    match destination {
        Operand::Named(named) if lane_mask_name(named).is_some() => {
            let mask = lane_mask_name(named).unwrap_or(EXEC_LOW_HALF);
            write_wave_mask(model, mask, halves)?;
        }
        Operand::Scalar(register) => {
            let register = u32::from(*register);
            // A thirty-two-lane wave's mask is one register (the 32-bit scalar forms ACO uses
            // there); the next is not written.
            let words = if model.lanes() > 32 { 2 } else { 1 };
            if register + words > SCALAR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a compare's register pair runs past the end of the register file",
                });
            }
            model.write_scalar(register, halves.0);
            if words == 2 {
                model.write_scalar(register + 1, halves.1);
            }
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a compare's destination is neither a register pair nor a lane mask",
            });
        }
    }
    model.count();
    Ok(())
}

/// `v_lshl_add_u32 d, a, b, c`: `(a << b[4:0]) + c` per lane; `v_add3_u32 d, a, b, c`: `a + b + c`,
/// as a streamout pass forms its store address (D861); and `v_add_lshl_u32 d, a, b, c`:
/// `(a + b) << c[4:0]`. All wrap (AMD's published RDNA instruction set, `V_LSHL_ADD_U32`,
/// `V_ADD3_U32`, `V_ADD_LSHL_U32`).
fn shift_add<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 3)?;
    let [destination, value, amount, addend] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a three-source add does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a three-source add's destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let low_five = model.constant(31);
    for lane in running_lanes(model) {
        let value = model.read_source(instruction, value, lane)?;
        let amount = model.read_source(instruction, amount, lane)?;
        let addend = model.read_source(instruction, addend, lane)?;
        let sum = match name {
            "v_add3_u32" => {
                let partial = model.add(value, amount);
                model.add(partial, addend)
            }
            // `(a + b) << c[4:0]`: the third source is the shift.
            "v_add_lshl_u32" => {
                let partial = model.add(value, amount);
                let shift = model.binary(op::BITWISE_AND, addend, low_five);
                model.binary(op::SHIFT_LEFT_LOGICAL, partial, shift)
            }
            _ => {
                let amount = model.binary(op::BITWISE_AND, amount, low_five);
                let shifted = model.binary(op::SHIFT_LEFT_LOGICAL, value, amount);
                model.add(shifted, addend)
            }
        };
        model.write_vector_lane(register, lane, sum);
    }
    model.count();
    Ok(())
}

/// The two halves of a 64-bit source operand for one lane: a vector or scalar register pair, or an
/// inline integer sign-extended as the hardware does.
fn pair_source<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    operand: &Operand,
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    let next = match operand {
        Operand::Vector(n) => {
            Operand::Vector(n.checked_add(1).ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a 64-bit vector source runs past the register file",
            })?)
        }
        Operand::Scalar(n) => {
            Operand::Scalar(n.checked_add(1).ok_or(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a 64-bit scalar source runs past the register file",
            })?)
        }
        Operand::Integer(value) => {
            let low = model.read_source(instruction, operand, lane)?;
            let high = model.constant(if *value < 0 { u32::MAX } else { 0 });
            return Ok((low, high));
        }
        // A lane mask is the pair it names: `exec` is scalar codes 126-127, `vcc` 106-107.
        Operand::Named(named) if lane_mask_name(named).is_some() => {
            let mask = lane_mask_name(named).expect("checked immediately above");
            return model.read_lane_mask(mask);
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: concat!(
                    "a 64-bit source that is neither a register pair, a lane mask nor an ",
                    "inline integer"
                ),
            });
        }
    };
    let low = model.read_source(instruction, operand, lane)?;
    let high = model.read_source(instruction, &next, lane)?;
    Ok((low, high))
}

/// `v_lshrrev_b64 d, amount, value`: the 64-bit `value` shifted right by `amount[5:0]`, into the
/// destination pair (`V_LSHRREV_B64`). SPIR-V leaves a shift by the operand's width or more
/// undefined, so the three ranges - zero, below 32, 32 and above - are computed apart and selected.
fn shift_right_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 2)?;
    let (destination, amount, value) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_lshrrev_b64 destination is not a vector register pair",
        });
    };
    let register = u32::from(*register);
    if register + 2 > VECTOR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_lshrrev_b64 destination runs past the vector register file",
        });
    }
    let (six_bits, thirty_one, thirty_two, zero) = (
        model.constant(63),
        model.constant(31),
        model.constant(32),
        model.constant(0),
    );
    for lane in running_lanes(model) {
        let amount = model.read_source(instruction, amount, lane)?;
        let (low, high) = pair_source(model, instruction, value, lane)?;
        let k = model.binary(op::BITWISE_AND, amount, six_bits);
        // Below 32: each half shifts, and the high half's low bits move into the low half.
        let small = model.binary(op::BITWISE_AND, k, thirty_one);
        let reverse = model.binary(op::ISUB, thirty_two, small);
        let reverse = model.binary(op::BITWISE_AND, reverse, thirty_one);
        let low_shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, low, small);
        let carried = model.binary(op::SHIFT_LEFT_LOGICAL, high, reverse);
        let no_carry = model.compare(op::IEQUAL, small, zero);
        let carried = pick(model, no_carry, zero, carried);
        let low_small = model.binary(op::BITWISE_OR, low_shifted, carried);
        let high_small = model.binary(op::SHIFT_RIGHT_LOGICAL, high, small);
        // 32 and above: the high half, shifted by the rest, becomes the low half.
        let low_large = model.binary(op::SHIFT_RIGHT_LOGICAL, high, small);
        let large = model.compare(op::UGREATER_THAN_EQUAL, k, thirty_two);
        let new_low = pick(model, large, low_large, low_small);
        let new_high = pick(model, large, zero, high_small);
        model.write_vector_lane(register, lane, new_low);
        model.write_vector_lane(register + 1, lane, new_high);
    }
    model.count();
    Ok(())
}

/// `v_med3_u32 d, a, b, c`: the median of three unsigned words per lane (`V_MED3_U32`, AMD's
/// published RDNA 2 instruction set, VOP3): the greater of the least two and the least of the
/// rest, `max(min(a, b), min(max(a, b), c))`.
fn unsigned_median_of_three<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 3)?;
    let [destination, first, second, third] = instruction.operands.as_slice() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_med3_u32 does not have four operands",
        });
    };
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_med3_u32 destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    for lane in running_lanes(model) {
        let a = model.read_source(instruction, first, lane)?;
        let b = model.read_source(instruction, second, lane)?;
        let c = model.read_source(instruction, third, lane)?;
        let a_below = model.compare(op::ULESS_THAN, a, b);
        let low = model.select(a_below, a, b);
        let high = model.select(a_below, b, a);
        let c_below = model.compare(op::ULESS_THAN, c, high);
        let upper = model.select(c_below, c, high);
        let low_below = model.compare(op::ULESS_THAN, low, upper);
        let median = model.select(low_below, upper, low);
        model.write_vector_lane(register, lane, median);
    }
    model.count();
    Ok(())
}

/// `v_cmp_eq_u64_e64` and `v_cmp_ne_u64_e64`: whether two 64-bit sources - register pairs, or an
/// inline integer sign-extended - are equal, a bit a lane into the destination's lane mask, as
/// [`compare_long`] writes a 32-bit compare's (AMD's published RDNA 2 instruction set, VOPC in its
/// long form).
fn compare_long_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 2)?;
    let (destination, first, second) = three_operands(instruction)?;
    let equal = name == "v_cmp_eq_u64_e64";
    let halves = every_lane_bit(model, true, |model, lane| {
        let (a_low, a_high) = pair_source(model, instruction, first, lane)?;
        let (b_low, b_high) = pair_source(model, instruction, second, lane)?;
        let low = model.compare(op::IEQUAL, a_low, b_low);
        let high = model.compare(op::IEQUAL, a_high, b_high);
        let same = model.both(low, high);
        Ok(if equal {
            same
        } else {
            let bool_type = model.bool_type();
            let b = model.builder();
            let differ = b.id();
            b.function(op::LOGICAL_NOT, &[bool_type.0, differ.0, same.0]);
            differ
        })
    })?;
    match destination {
        Operand::Named(named) if lane_mask_name(named).is_some() => {
            let mask = lane_mask_name(named).unwrap_or(EXEC_LOW_HALF);
            write_wave_mask(model, mask, halves)?;
        }
        Operand::Scalar(register) if u32::from(*register) + 2 <= SCALAR_REGISTERS => {
            let register = u32::from(*register);
            model.write_scalar(register, halves.0);
            if model.lanes() > 32 {
                model.write_scalar(register + 1, halves.1);
            }
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a 64-bit compare's destination is neither a register pair nor a lane mask",
            });
        }
    }
    model.count();
    Ok(())
}

/// `s_bitreplicate_b64_b32 d, s`: each bit of `s` twice, bit `n` into bits `2n` and `2n + 1` of the
/// pair `d` (`S_BITREPLICATE_B64_B32`, AMD's published RDNA 2 instruction set, SOP1); the condition
/// code is untouched. Each half's bits are spread apart by the shifts and masks of a Morton code,
/// then doubled.
fn bit_replicate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_bitreplicate_b64_b32 destination is not a scalar register pair",
        });
    };
    let register = u32::from(*register);
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_bitreplicate_b64_b32 destination runs past the end of the register file",
        });
    }
    let value = model.read_source(instruction, source, 0)?;
    let (low_mask, sixteen) = (model.constant(0xffff), model.constant(16));
    let low = model.binary(op::BITWISE_AND, value, low_mask);
    let high = model.binary(op::SHIFT_RIGHT_LOGICAL, value, sixteen);
    let mut halves = [low, high];
    for half in &mut halves {
        for (shift, mask) in [
            (8, 0x00ff_00ff),
            (4, 0x0f0f_0f0f),
            (2, 0x3333_3333),
            (1, 0x5555_5555),
        ] {
            let by = model.constant(shift);
            let moved = model.binary(op::SHIFT_LEFT_LOGICAL, *half, by);
            let joined = model.binary(op::BITWISE_OR, *half, moved);
            let mask = model.constant(mask);
            *half = model.binary(op::BITWISE_AND, joined, mask);
        }
        let one = model.constant(1);
        let doubled = model.binary(op::SHIFT_LEFT_LOGICAL, *half, one);
        *half = model.binary(op::BITWISE_OR, *half, doubled);
    }
    model.write_scalar(register, halves[0]);
    model.write_scalar(register + 1, halves[1]);
    model.count();
    Ok(())
}

/// `v_lshlrev_b64 d, amount, value`: the 64-bit `value` shifted left by `amount[5:0]`, into the
/// register pair from `d` (`V_LSHLREV_B64`, AMD's published RDNA 2 instruction set, VOP3): the
/// mirror of [`shift_right_64`].
fn shift_left_64<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    no_integer_modifiers(instruction, 2)?;
    let (destination, amount, value) = three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_lshlrev_b64 destination is not a vector register pair",
        });
    };
    let register = u32::from(*register);
    if register + 2 > VECTOR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "v_lshlrev_b64 destination runs past the vector register file",
        });
    }
    let (six_bits, thirty_one, thirty_two, zero) = (
        model.constant(63),
        model.constant(31),
        model.constant(32),
        model.constant(0),
    );
    for lane in running_lanes(model) {
        let amount = model.read_source(instruction, amount, lane)?;
        let (low, high) = pair_source(model, instruction, value, lane)?;
        let k = model.binary(op::BITWISE_AND, amount, six_bits);
        // Below 32: each half shifts, and the low half's high bits move into the high half.
        let small = model.binary(op::BITWISE_AND, k, thirty_one);
        let reverse = model.binary(op::ISUB, thirty_two, small);
        let reverse = model.binary(op::BITWISE_AND, reverse, thirty_one);
        let high_shifted = model.binary(op::SHIFT_LEFT_LOGICAL, high, small);
        let carried = model.binary(op::SHIFT_RIGHT_LOGICAL, low, reverse);
        let no_carry = model.compare(op::IEQUAL, small, zero);
        let carried = pick(model, no_carry, zero, carried);
        let high_small = model.binary(op::BITWISE_OR, high_shifted, carried);
        let low_small = model.binary(op::SHIFT_LEFT_LOGICAL, low, small);
        // 32 and above: the low half, shifted by the rest, becomes the high half.
        let high_large = model.binary(op::SHIFT_LEFT_LOGICAL, low, small);
        let large = model.compare(op::UGREATER_THAN_EQUAL, k, thirty_two);
        let new_low = pick(model, large, zero, low_small);
        let new_high = pick(model, large, high_large, high_small);
        model.write_vector_lane(register, lane, new_low);
        model.write_vector_lane(register + 1, lane, new_high);
    }
    model.count();
    Ok(())
}

/// The `component`th register of a multi-register operand.
///
/// Only a vector register can be stepped; stepping a scalar or an inline constant would read a
/// neighbouring value as data.
fn step_operand(operand: &Operand, component: u32) -> Result<Operand, TranslateError> {
    match operand {
        Operand::Vector(base) => Ok(Operand::Vector(
            base.checked_add(u16::try_from(component).unwrap_or(u16::MAX))
                .ok_or(TranslateError::Unsupported {
                    offset: 0,
                    detail: "a multi-channel store runs past the register file",
                })?,
        )),
        _ if component == 0 => Ok(operand.clone()),
        _ => Err(TranslateError::Unsupported {
            offset: 0,
            detail: "a multi-channel store does not name a vector register",
        }),
    }
}

/// Reads guest memory, answering zero for an address outside the window. With [`write_guarded`],
/// this stops an out-of-window access aliasing a real one; see [`Model::address_within_window`].
fn read_guarded<M: Model + ?Sized>(model: &mut M, address: Id) -> Id {
    let inside = model.address_within_window(address);
    let index = model.word_index(address);
    let value = model.read_memory(index);
    let zero = model.constant(0);
    pick(model, inside, value, zero)
}

/// Writes guest memory, dropping a write to an address outside the window, by writing back what was
/// there rather than branching around the store.
fn write_guarded<M: Model + ?Sized>(model: &mut M, address: Id, value: Id, lane: u32) {
    let inside = model.address_within_window(address);
    let index = model.word_index(address);
    let previous = model.read_memory(index);
    let kept = pick(model, inside, value, previous);
    model.write_memory(index, kept, lane);
}

/// What a texture access's operands say, once they have been checked.
///
/// Register numbers rather than [`Operand`]s, because each has been bounds-checked by the time this
/// exists.
struct ImageAccess {
    /// First vector register the components land in.
    destination: u32,
    /// The vector register holding each address register's worth of the access, in order:
    /// consecutive from the first, or as an `NSA` encoding lists them.
    addresses: [u32; MAX_ADDRESS_REGISTERS],
    /// First scalar register of the image descriptor.
    descriptor: u32,
    /// First scalar register of the sampler descriptor, when the instruction names one.
    sampler: Option<u32>,
    /// Which components come back, one bit each.
    mask: u32,
    /// Whether this reads a texel by its index rather than sampling a position.
    fetches: bool,
    /// Whether the level of detail comes from an address register rather than being named.
    levelled: bool,
    /// Which of the address and the components are sixteen bits wide.
    halves: Halves,
    /// What shape of image it samples.
    shape: ImageShape,
    /// What the address registers hold ahead of the coordinates.
    ahead: Ahead,
}

/// The shape of image an access samples, beyond a flat image's.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ImageShape {
    /// What its coordinates address beyond a flat image's two.
    pub dimension: Dimension,
    /// A depth texture compared against a reference, the first address element (D816).
    pub compare: bool,
}

/// What an image access's coordinates address beyond a flat image's two: one of these at most.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Dimension {
    /// Nothing: two coordinates.
    #[default]
    Flat,
    /// A cube, its address a face's two coordinates and the face (D811).
    Cube,
    /// A three-dimensional image, its address three coordinates (D823).
    Volume,
    /// A 2D array addressed with its layer, the address element after the two coordinates, of a
    /// one-layer image: layer zero is the image, any other out of bounds (D895). Fetches and stores
    /// only.
    Layered,
}

impl ImageShape {
    /// Whether it is a cube.
    #[must_use]
    pub const fn cube(self) -> bool {
        matches!(self.dimension, Dimension::Cube)
    }

    /// Whether it is a three-dimensional image.
    #[must_use]
    pub const fn volume(self) -> bool {
        matches!(self.dimension, Dimension::Volume)
    }

    /// Whether it is a 2D array of one layer, addressed with its layer.
    #[must_use]
    pub const fn layered(self) -> bool {
        matches!(self.dimension, Dimension::Layered)
    }
}

/// `A16` and `D16`: whether an image access's address elements, and its components, are sixteen
/// bits, two to a register, the first in the low half.
#[derive(Clone, Copy)]
struct Halves {
    address: bool,
    data: bool,
}

/// `A16` and `D16`, bits 30 and 31 of an image instruction's second word (AMD's published RDNA
/// instruction set, `MIMG`; the reference assembler sets exactly these for `a16` and `d16`).
const IMAGE_A16: u32 = 1 << 30;
const IMAGE_D16: u32 = 1 << 31;
/// Fields of an image instruction's first word that change which registers it reads or writes:
/// `R128` (15) takes a four-register descriptor, and `TFE` (16) and `LWE` (17) return a status
/// word beyond the components.
const IMAGE_LAYOUT_FIELDS: u32 = 1 << 15 | 1 << 16 | 1 << 17;

/// `NSA` (2:1): how many dwords after the instruction's fixed eight bytes list its address
/// registers apart, one byte each - the second address register in the first byte of the first,
/// and so on (`aco_assembler.cpp:744-777`, `:813-821`). The first is the second word's `VADDR`.
const IMAGE_NSA: (u32, u32) = (1, 0x3);

/// The most address registers a translated access reads: two coordinates, a cube's face or a
/// volume's depth, and a level - or a gradient sample's two derivative pairs ahead of its two
/// coordinates.
const MAX_ADDRESS_REGISTERS: usize = 6;

/// Address registers an access uses, by instruction.
///
/// Two elements for a coordinate, a third for a cube's face or a volume's depth, and one more where
/// the level is one of them: the assembler refuses an address operand whose register count does not
/// match the instruction's dimensionality field. Sixteen-bit elements pack two to a register.
const fn address_registers(levelled: bool, a16: bool, shape: ImageShape) -> u32 {
    let elements: u32 = 2
        + (shape.cube() || shape.volume() || shape.layered()) as u32
        + levelled as u32
        + shape.compare as u32;
    if a16 { elements.div_ceil(2) } else { elements }
}

/// The vector register of each of an access's `used` address registers, the first `first`:
/// consecutive, or for an `NSA` encoding the rest listed a byte each in the dwords after its fixed
/// part. `None` when one would lie past the register file or a listed one is not in the
/// instruction.
fn address_list(
    instruction: &Instruction,
    first: u32,
    used: u32,
) -> Option<[u32; MAX_ADDRESS_REGISTERS]> {
    let (shift, mask) = IMAGE_NSA;
    let nsa_dwords = (instruction.word >> shift) & mask;
    let mut addresses = [first; MAX_ADDRESS_REGISTERS];
    for (index, slot) in (0..used).zip(addresses.iter_mut()).skip(1) {
        *slot = if nsa_dwords == 0 {
            first + index
        } else {
            let at = index - 1;
            if at / 4 >= nsa_dwords {
                return None;
            }
            let word = instruction.words.get(2 + (at / 4) as usize)?;
            (word >> (8 * (at % 4))) & 0xff
        };
    }
    addresses[..used as usize]
        .iter()
        .all(|&register| register < VECTOR_REGISTERS)
        .then_some(addresses)
}

/// Registers `components` occupy: one each, or two to a register under `D16`.
const fn data_registers(components: u32, d16: bool) -> u32 {
    if d16 {
        components.div_ceil(2)
    } else {
        components
    }
}

/// Address element `index` of an access: a whole register, or under `A16` a half of one, zero
/// above.
fn address_element<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (addresses, a16): (&[u32; MAX_ADDRESS_REGISTERS], bool),
    index: u32,
    lane: u32,
) -> Result<Id, TranslateError> {
    let register = |at: u32| addresses[at as usize];
    if !a16 {
        return model.read_source(
            instruction,
            &Operand::Vector(address_of(register(index))),
            lane,
        );
    }
    let word = model.read_source(
        instruction,
        &Operand::Vector(address_of(register(index / 2))),
        lane,
    )?;
    let shift = model.constant(16 * (index % 2));
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
    let low = model.constant(0xFFFF);
    Ok(model.binary(op::BITWISE_AND, shifted, low))
}

/// What an access's address registers hold ahead of its coordinates.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Ahead {
    /// Nothing: the coordinates come first, after any compare reference.
    Nothing,
    /// The texel offsets, in one register: the `_o` forms.
    Offsets,
    /// The position's derivatives across and down a pixel, in four: `image_sample_d`, as
    /// `llvm.amdgcn.image.sample.d.2d` orders them.
    Gradients,
}

impl Ahead {
    /// The address registers it takes.
    const fn registers(self) -> u32 {
        match self {
            Self::Nothing => 0,
            Self::Offsets => 1,
            Self::Gradients => 4,
        }
    }
}

/// How an access's address is laid out, refusing what is not translated: a sample's position in
/// sixteen-bit halves, and texel offsets anywhere but a flat, uncompared, full-width sample. Whether
/// the first address register holds texel offsets, ahead of everything else (ACO's MIMG address
/// order: offset, bias, compare, coordinates): the `_o` forms. A gradient sample's derivatives are
/// ahead of its coordinates, and translated for a flat, uncompared sample.
fn address_form(
    name: &str,
    shape: ImageShape,
    (a16, fetches): (bool, bool),
) -> Result<Ahead, &'static str> {
    if a16 && !fetches && name != "image_store" {
        return Err("a sample whose position is sixteen-bit halves, which is not translated");
    }
    let offset = name.ends_with("_o");
    if offset && (shape.cube() || shape.volume() || shape.compare || a16 || fetches) {
        return Err(concat!(
            "a sample with texel offsets of a cube, a volume, a compare or sixteen-bit positions ",
            "is not translated"
        ));
    }
    if name == "image_sample_d" {
        if shape.cube() || shape.volume() || shape.compare {
            return Err("a gradient sample of a cube, a volume or a compare is not translated");
        }
        return Ok(Ahead::Gradients);
    }
    Ok(if offset {
        Ahead::Offsets
    } else {
        Ahead::Nothing
    })
}

/// Whether an image instruction takes its level from an address register, after the coordinates:
/// a levelled sample, a fetch from a level, and a store to one.
fn takes_its_level(name: &str) -> bool {
    matches!(
        name,
        "image_sample_l" | "image_load_mip" | "image_store_mip"
    )
}

/// What an access of dimensionality code `dimension` addresses, where it is translated: a cube or a
/// volume sampled, a volume stored to, a 2D array fetched or stored (D895); flat otherwise, which the
/// caller refuses for any code but 2D.
const fn shape_dimension(dimension: u32, (is_sample, name): (bool, &str)) -> Dimension {
    let stores = matches!(name.as_bytes(), b"image_store");
    match dimension {
        IMAGE_DIMENSION_CUBE if is_sample => Dimension::Cube,
        IMAGE_DIMENSION_3D if is_sample || stores => Dimension::Volume,
        IMAGE_DIMENSION_2D_ARRAY if !is_sample => Dimension::Layered,
        _ => Dimension::Flat,
    }
}

/// Reads a texture access's operands, refusing anything it cannot use.
///
/// The operand at index three differs: a sample names a sampler there and a fetch its mask. The
/// mnemonic says which, and the solved layouts agree (five operands for a sample, four for a
/// fetch).
fn image_access(instruction: &Instruction, name: &str) -> Result<ImageAccess, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    // Two dimensions, checked: every image translation here reads two coordinate registers, and an
    // image of another dimensionality would sample somewhere else.
    let (shift, mask) = IMAGE_DIMENSION;
    let dimension = (instruction.word >> shift) & mask;
    let is_sample = name.starts_with("image_sample");
    // A compare's reference comes first, as ACO places it ahead of the coordinates
    // (`aco_select_nir.cpp`, the MIMG address after any offset and bias).
    let shape = ImageShape {
        dimension: shape_dimension(dimension, (is_sample, name)),
        compare: matches!(name, "image_sample_c_lz" | "image_sample_c"),
    };
    let (volume, compare) = (shape.volume(), shape.compare);
    if dimension != IMAGE_DIMENSION_2D && shape.dimension == Dimension::Flat {
        return Err(TranslateError::ImageDimension {
            offset: instruction.offset,
            mnemonic: name.to_owned(),
            dimension: IMAGE_DIMENSION_NAMES
                .get(dimension as usize)
                .copied()
                .unwrap_or("unknown"),
        });
    }

    // A load reads a texel by integer coordinate and names no sampler; a sample takes a position
    // across the image and names one.
    let fetches = matches!(name, "image_load" | "image_load_mip");
    let second = instruction.second_word.unwrap_or(0);
    let a16 = second & IMAGE_A16 != 0;
    let d16 = second & IMAGE_D16 != 0;
    if instruction.word & IMAGE_LAYOUT_FIELDS != 0 {
        return Err(refuse(concat!(
            "an image access with a four-register descriptor, or a status word returned - which ",
            "registers it uses is not modelled"
        )));
    }
    // The level from a register rather than named: the last address element, after the coordinates,
    // as compiled levelled samples place it, and as `image_load_mip` takes its integer level.
    let levelled = takes_its_level(name);
    let ahead = address_form(name, shape, (a16, fetches)).map_err(refuse)?;

    let (Some(Operand::Vector(destination)), Some(Operand::Vector(address))) =
        (instruction.operands.first(), instruction.operands.get(1))
    else {
        return Err(refuse(
            "a texture access's destination and coordinate are not vector registers",
        ));
    };
    let Some(Operand::Scalar(descriptor)) = instruction.operands.get(2) else {
        return Err(refuse(concat!(
            "a texture access's image descriptor is not a scalar register - it names the first ",
            "register of a group, and a group has to start somewhere"
        )));
    };
    let sampler = match instruction.operands.get(3) {
        Some(Operand::Scalar(sampler)) if !fetches => Some(u32::from(*sampler)),
        _ => None,
    };
    let mask_at = if sampler.is_some() { 4 } else { 3 };
    let Some(Operand::Immediate(mask)) = instruction.operands.get(mask_at) else {
        return Err(refuse(
            "a texture access carries no component mask, so what it returns is unknown",
        ));
    };
    let Ok(mask) = u32::try_from(*mask) else {
        return Err(refuse("a texture access's component mask is negative"));
    };
    if mask == 0 || mask >= 1 << IMAGE_COMPONENTS {
        return Err(refuse(concat!(
            "a texture access's component mask selects nothing, or selects a component the ",
            "instruction does not have"
        )));
    }
    // A compared cube is sampled at its direction against the reference, as a flat one is (D933).
    if compare && (volume || mask != 1) {
        return Err(refuse(
            "a depth-compare sample of a volume or of more than one channel is not translated",
        ));
    }

    // The address registers and the components coming back both have to fit.
    let used = address_registers(levelled, a16, shape) + ahead.registers();
    let addresses = address_list(instruction, u32::from(*address), used).ok_or_else(|| {
        refuse(concat!(
            "an image access whose address registers run past the register file, or whose ",
            "listed address registers are not all in the instruction"
        ))
    })?;
    let destination = u32::from(*destination);
    if destination + data_registers(mask.count_ones(), d16) > VECTOR_REGISTERS {
        return Err(refuse(
            "a texture access runs past the end of the vector register file",
        ));
    }

    Ok(ImageAccess {
        destination,
        addresses,
        descriptor: u32::from(*descriptor),
        sampler,
        mask,
        fetches,
        levelled,
        halves: Halves {
            address: a16,
            data: d16,
        },
        shape,
        ahead,
    })
}

/// Writes a texel's components back: the ones the mask selects, into consecutive registers, so a
/// hole in the mask leaves no hole in the destination. Under `D16` each is narrowed to a half and two
/// share a register, the first in the low half.
fn write_components<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (texel, integer): (Id, bool),
    (mask, destination, d16): (u32, u32, bool),
    lane: u32,
) -> Result<(), TranslateError> {
    if integer && d16 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sixteen-bit fetch from an integer image is not translated",
        });
    }
    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let mut returned = Vec::new();
    for component in 0..IMAGE_COMPONENTS {
        if mask & (1 << component) == 0 {
            continue;
        }
        let builder = model.builder();
        let extracted = builder.id();
        // An integer image's components are the words themselves.
        let element = if integer { u32_type } else { f32_type };
        builder.function(
            op::COMPOSITE_EXTRACT,
            &[element.0, extracted.0, texel.0, component],
        );
        if integer {
            returned.push(extracted);
            continue;
        }
        let bits = builder.id();
        builder.function(op::BITCAST, &[u32_type.0, bits.0, extracted.0]);
        returned.push(bits);
    }
    if !d16 {
        for (register, bits) in (destination..).zip(returned) {
            model.write_vector_lane(register, lane, bits);
        }
        return Ok(());
    }
    let sixteen = model.constant(16);
    for (register, pair) in (destination..).zip(returned.chunks(2)) {
        let low = model.float_bits_to_half(pair[0]);
        let packed = if let Some(&second) = pair.get(1) {
            let high = model.float_bits_to_half(second);
            let high = model.binary(op::SHIFT_LEFT_LOGICAL, high, sixteen);
            model.binary(op::BITWISE_OR, low, high)
        } else {
            // An odd count's last half writes only its own sixteen bits: ACO keeps other values in
            // the register's high half across a D16 image load (`aco_ir.cpp:1001-1004`, a D16
            // load's result overwrites its own bytes), so the hardware leaves that half alone.
            let old =
                model.read_source(instruction, &Operand::Vector(address_of(register)), lane)?;
            let high_half = model.constant(0xFFFF_0000);
            let kept = model.binary(op::BITWISE_AND, old, high_half);
            model.binary(op::BITWISE_OR, kept, low)
        };
        model.write_vector_lane(register, lane, packed);
    }
    Ok(())
}

fn image_sample<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    let ImageAccess {
        destination,
        addresses,
        descriptor,
        sampler,
        mask,
        fetches,
        levelled,
        halves,
        shape,
        ahead,
    } = image_access(instruction, name)?;
    let (offset, gradients) = (ahead == Ahead::Offsets, ahead == Ahead::Gradients);
    let (cube, compare, a16) = (shape.cube(), shape.compare, halves.address);
    let gather = gather_component(name, (mask, shape), (offset, halves.data, destination))
        .map_err(refuse)?;
    let mask = if gather.is_some() { 0b1111 } else { mask };

    let named_level =
        name.ends_with("_lz") || name.ends_with("_lz_o") || levelled || gather.is_some();
    if !fetches && !named_level && !gradients && !model.has_derivatives() {
        return Err(refuse(
            "a sample whose level derivatives choose, which a compute dispatch has none of (D904)",
        ));
    }
    let texture = model
        .sampled_image(descriptor, sampler, (instruction.offset, shape))
        .map_err(refuse)?;
    note_unselected(model, &texture);
    if (cube || shape.volume()) && (texture.saturate != [false; 2] || texture.mirror != [false; 2])
    {
        return Err(refuse(
            "a cube or volume through a half-border clamp or a mirror-once mode is not translated",
        ));
    }

    // Ahead of the coordinates sit the texel offsets, where the instruction has them, or the two
    // derivative pairs of a gradient sample.
    let skip = ahead.registers();
    // Which sampling instruction this is: `lz` is level zero named, `_l` takes the level from a
    // register, and the plain form lets the implementation choose from derivatives, a different
    // SPIR-V instruction with different operands.

    let zero = model.constant(0);
    let constant_level = model.as_float(zero);

    for lane in running_lanes(model) {
        // The level, where the instruction takes one from a register: the last address element. A
        // fetch's level is an integer, a sample's a float.
        let level = if levelled {
            let at = 2 + u32::from(cube || shape.volume() || shape.layered());
            let bits = address_element(model, instruction, (&addresses, a16), at, lane)?;
            if fetches { bits } else { model.as_float(bits) }
        } else if fetches {
            zero
        } else {
            constant_level
        };
        let coordinate = sample_coordinate(
            model,
            instruction,
            (&addresses, a16),
            (&texture, fetches, cube),
            (skip, lane),
        )?;
        if compare {
            compare_sample(
                model,
                instruction,
                (&addresses, a16),
                (&texture, coordinate, name == "image_sample_c"),
                lane,
            )?;
            continue;
        }

        let builder = model.builder();
        // Loaded per lane rather than hoisted, so the translation does not reorder the guest's
        // program on an unchecked aliasing assumption.
        let bound = bound_texture(builder, &texture);
        let texel = lane_texel(
            model,
            instruction,
            (&addresses, a16, &texture),
            (bound, coordinate, level),
            (ahead, fetches, named_level, gather),
            lane,
        )?;

        let texel = layer_in_bounds(
            model,
            instruction,
            (&addresses, a16),
            (&texture, bound, texel),
            lane,
        )?;

        write_components(
            model,
            instruction,
            (texel, texture.integer),
            (mask, destination, halves.data),
            lane,
        )?;
    }

    model.count();
    Ok(())
}

/// One lane's texel from the bound image at `coordinate`: moved by the address's texel offsets where
/// the form has them, then a gradient sample, or a fetch, gather or sample ([`sample_or_fetch`]).
fn lane_texel<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (addresses, a16, texture): (&[u32; MAX_ADDRESS_REGISTERS], bool, &Texture),
    (bound, coordinate, level): (Id, Id, Id),
    (ahead, fetches, named_level, gather): (Ahead, bool, bool, Option<u32>),
    lane: u32,
) -> Result<Id, TranslateError> {
    let coordinate = if ahead == Ahead::Offsets {
        let offsets = address_element(model, instruction, (addresses, a16), 0, lane)?;
        offset_coordinate(model, texture, bound, (coordinate, offsets))
    } else {
        coordinate
    };
    if ahead == Ahead::Gradients {
        return gradient_sample(
            model,
            instruction,
            (addresses, texture),
            (bound, coordinate),
            lane,
        );
    }
    Ok(sample_or_fetch(
        model,
        texture,
        (bound, coordinate),
        (fetches, named_level, gather),
        level,
    ))
}

/// The component a gather returns of four texels - the one its mask's one bit names - where `name`
/// is one; all four registers are written (D913), or with sixteen-bit data two, the four halves packed
/// two a register as a sample's are (Grand Theft Auto V's prologue gathers so). Of a flat image only:
/// more than one mask bit, other shapes, offsets or a destination past the register file are refused.
fn gather_component(
    name: &str,
    (mask, shape): (u32, ImageShape),
    (offset, d16, destination): (bool, bool, u32),
) -> Result<Option<u32>, &'static str> {
    if !name.starts_with("image_gather4") {
        return Ok(None);
    }
    let registers = data_registers(IMAGE_COMPONENTS, d16);
    if mask.count_ones() != 1
        || shape.dimension != Dimension::Flat
        || offset
        || destination + registers > VECTOR_REGISTERS
    {
        return Err(concat!(
            "a gather of more than one component, of other than a flat image, with offsets, or ",
            "past the register file is not translated"
        ));
    }
    Ok(Some(mask.trailing_zeros()))
}

/// `texel` as a fetch from `texture` answers it: as it is, or - fetched from a 2D array at the layer
/// the address's third element names - itself where the array has that layer and zero past its
/// last, which is out of bounds (D895, D897). The layers are the bound image's own, asked of it.
fn layer_in_bounds<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (addresses, a16): (&[u32; MAX_ADDRESS_REGISTERS], bool),
    (texture, bound, texel): (&Texture, Id, Id),
    lane: u32,
) -> Result<Id, TranslateError> {
    if !texture.shape.layered() {
        return Ok(texel);
    }
    let layer = address_element(model, instruction, (addresses, a16), 2, lane)?;
    let zero = model.constant(0);
    let u32_type = model.u32_type();
    let builder = model.builder();
    builder.header(op::CAPABILITY, &[orbistoun_spirv::capability::IMAGE_QUERY]);
    let image = image_of(builder, texture, bound);
    let size = builder.id();
    builder.function(
        op::IMAGE_QUERY_SIZE_LOD,
        &[texture.texel.0, size.0, image.0, zero.0],
    );
    let layers = builder.id();
    builder.function(op::COMPOSITE_EXTRACT, &[u32_type.0, layers.0, size.0, 2]);
    let none = builder.id();
    builder.declare(op::CONSTANT_NULL, &[texture.result.0, none.0]);
    let held = model.compare(op::ULESS_THAN, layer, layers);
    // The texel is a four-component vector, not a word: below SPIR-V 1.4 a select between vectors
    // takes a condition as wide as they are, so the one answer is spread across four.
    let bool_type = model.bool_type();
    let builder = model.builder();
    let wide = builder.declare_type(op::TYPE_VECTOR, &[bool_type.0, IMAGE_COMPONENTS]);
    let each = builder.id();
    let mut spread = vec![wide.0, each.0];
    spread.extend(std::iter::repeat_n(held.0, IMAGE_COMPONENTS as usize));
    builder.function(op::COMPOSITE_CONSTRUCT, &spread);
    let chosen = builder.id();
    builder.function(
        op::SELECT,
        &[texture.result.0, chosen.0, each.0, texel.0, none.0],
    );
    Ok(chosen)
}

/// Whether `layer` is one the storage image `image` holds, its texel coordinate type `texel`: below
/// the layers the image is asked for (D897).
fn layer_held<M: Model + ?Sized>(model: &mut M, (texel, image): (Id, Id), layer: Id) -> Id {
    let u32_type = model.u32_type();
    let builder = model.builder();
    builder.header(op::CAPABILITY, &[orbistoun_spirv::capability::IMAGE_QUERY]);
    let size = builder.id();
    builder.function(op::IMAGE_QUERY_SIZE, &[texel.0, size.0, image.0]);
    let layers = builder.id();
    builder.function(op::COMPOSITE_EXTRACT, &[u32_type.0, layers.0, size.0, 2]);
    model.compare(op::ULESS_THAN, layer, layers)
}

/// `image_get_resinfo`: the size of a two-dimensional image's level, the level an integer in the one
/// address register - its width in the first channel and its height in the second, as ACO reads a
/// mask of `0x3` (`aco_select_nir.cpp` 334-343). The other two channels have no published meaning
/// here and are refused. The image is the one a fetch through the same descriptor reads.
fn image_size<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    let (shift, field) = IMAGE_DIMENSION;
    let dimension = (instruction.word >> shift) & field;
    if dimension != IMAGE_DIMENSION_2D {
        return Err(TranslateError::ImageDimension {
            offset: instruction.offset,
            mnemonic: name.to_owned(),
            dimension: IMAGE_DIMENSION_NAMES
                .get(dimension as usize)
                .copied()
                .unwrap_or("unknown"),
        });
    }
    let second = instruction.second_word.unwrap_or(0);
    if instruction.word & IMAGE_LAYOUT_FIELDS != 0 || second & (IMAGE_A16 | IMAGE_D16) != 0 {
        return Err(refuse(concat!(
            "a size query with a four-register descriptor, a status word, or sixteen-bit ",
            "halves is not translated"
        )));
    }
    let (
        Some(Operand::Vector(destination)),
        Some(Operand::Vector(level)),
        Some(Operand::Scalar(descriptor)),
        Some(Operand::Immediate(mask)),
    ) = (
        instruction.operands.first(),
        instruction.operands.get(1),
        instruction.operands.get(2),
        instruction.operands.get(3),
    )
    else {
        return Err(refuse(
            "a size query is not a destination, a level, a descriptor and a mask",
        ));
    };
    let mask = u32::try_from(*mask).unwrap_or(0);
    if mask == 0 || mask & !0b11 != 0 {
        return Err(refuse(concat!(
            "a size query's mask selects nothing, or a channel past the width and height, whose ",
            "meaning is not published"
        )));
    }
    let destination = u32::from(*destination);
    if destination + mask.count_ones() > VECTOR_REGISTERS {
        return Err(refuse(
            "a size query runs past the end of the vector register file",
        ));
    }
    let texture = model
        .sampled_image(
            u32::from(*descriptor),
            None,
            (instruction.offset, ImageShape::default()),
        )
        .map_err(refuse)?;
    note_unselected(model, &texture);
    let level_register = Operand::Vector(*level);
    for lane in running_lanes(model) {
        let level = model.read_source(instruction, &level_register, lane)?;
        let u32_type = model.u32_type();
        let builder = model.builder();
        builder.header(op::CAPABILITY, &[orbistoun_spirv::capability::IMAGE_QUERY]);
        let bound = bound_texture(builder, &texture);
        let image = image_of(builder, &texture, bound);
        let size = builder.id();
        builder.function(
            op::IMAGE_QUERY_SIZE_LOD,
            &[texture.texel.0, size.0, image.0, level.0],
        );
        let mut channels = Vec::new();
        for component in 0..2 {
            if mask & (1 << component) != 0 {
                let extracted = builder.id();
                builder.function(
                    op::COMPOSITE_EXTRACT,
                    &[u32_type.0, extracted.0, size.0, component],
                );
                channels.push(extracted);
            }
        }
        for (register, value) in (destination..).zip(channels) {
            model.write_vector_lane(register, lane, value);
        }
    }
    model.count();
    Ok(())
}

/// A depth-compare sample, for one lane (D816): the reference, the first address element, compared
/// against the depth texture at the coordinate, its result - the filtered comparison, one float -
/// written to the one channel the mask names. At level zero for `image_sample_c_lz`, and where
/// `implicit`, for `image_sample_c`, at the level the pixel's derivatives choose.
fn compare_sample<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (addresses, a16): (&[u32; MAX_ADDRESS_REGISTERS], bool),
    (texture, coordinate, implicit): (&Texture, Id, bool),
    lane: u32,
) -> Result<(), TranslateError> {
    let Some(Operand::Vector(destination)) = instruction.operands.first() else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a depth-compare sample's data is not a vector register",
        });
    };
    let reference = address_element(model, instruction, (addresses, a16), 0, lane)?;
    if let Some(compare) = texture.shader_compare {
        let compare = running_compare(model, instruction, compare)?;
        let result = shader_compare(model, reference, (texture, coordinate, implicit), compare);
        model.write_vector_lane(u32::from(*destination), lane, result);
        return Ok(());
    }
    let reference = model.as_float(reference);
    let zero = model.constant(0);
    let level = model.as_float(zero);
    let f32_type = model.f32_type();
    let u32_type = model.u32_type();
    let builder = model.builder();
    let bound = bound_texture(builder, texture);
    let compared = builder.id();
    if implicit {
        builder.function(
            op::IMAGE_SAMPLE_DREF_IMPLICIT_LOD,
            &[f32_type.0, compared.0, bound.0, coordinate.0, reference.0],
        );
    } else {
        builder.function(
            op::IMAGE_SAMPLE_DREF_EXPLICIT_LOD,
            &[
                f32_type.0,
                compared.0,
                bound.0,
                coordinate.0,
                reference.0,
                image_operands::LOD,
                level.0,
            ],
        );
    }
    let bits = builder.id();
    builder.function(op::BITCAST, &[u32_type.0, bits.0, compared.0]);
    model.write_vector_lane(u32::from(*destination), lane, bits);
    Ok(())
}

/// How a compare the shader makes itself chooses its function and filter: compiled in, as the
/// sampler the host read names them, or read from the sampler's registers as the module runs.
#[derive(Debug, Clone, Copy)]
enum CompareBy {
    /// `SQ_TEX_DEPTH_COMPARE` and whether the sampler filters bilinearly.
    Known(u8, bool),
    /// The function, `0..=7`, and one where the sampler filters bilinearly, else zero, as ids.
    Running(Id, Id),
}

/// A slot's compare as the module makes it: the one compiled in, or - for
/// [`crate::wavefront::RUNNING_COMPARE`] - the function and filter read from the instruction's
/// sampler registers as the module runs (D995), `DEPTH_COMPARE_FUNC` (word 0, bits 14:12) and
/// `XY_MAG_FILTER`'s bilinear bit (word 2, bit 20; `gfx10-rsrc.json`).
fn running_compare<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (function, linear): (u8, bool),
) -> Result<CompareBy, TranslateError> {
    if function != crate::wavefront::RUNNING_COMPARE {
        return Ok(CompareBy::Known(function, linear));
    }
    let Some(Operand::Scalar(sampler)) = instruction.operands.get(3) else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a depth-compare sample whose sampler is not in scalar registers",
        });
    };
    let sampler = u32::from(*sampler);
    if sampler + 4 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a sampler descriptor runs past the end of the register file",
        });
    }
    let (word0, word2) = (model.read_scalar(sampler), model.read_scalar(sampler + 2));
    let (twelve, twenty, seven, one) = (
        model.constant(12),
        model.constant(20),
        model.constant(7),
        model.constant(1),
    );
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word0, twelve);
    let function = model.binary(op::BITWISE_AND, shifted, seven);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word2, twenty);
    let linear = model.binary(op::BITWISE_AND, shifted, one);
    Ok(CompareBy::Running(function, linear))
}

/// A compare the shader makes itself, of an image that is not a depth target, which no host
/// compare sampler reads: the image's first channel compared with the reference by the sampler's
/// `SQ_TEX_DEPTH_COMPARE` function - 1.0 where the reference passes, 0.0 where it does not - as a
/// compare sampler compares a depth (`reference OP texel`, Vulkan's `Dref` order). A bilinear
/// filter compares the four texels it would blend, gathered, and blends the answers with the
/// filter's weights, as a filtered compare does; a point filter compares the one texel. A compare
/// read as the module runs makes every function's answer and both filters', and keeps the ones its
/// sampler names. Answers the result's bits.
fn shader_compare<M: Model + ?Sized>(
    model: &mut M,
    reference: Id,
    (texture, coordinate, implicit): (&Texture, Id, bool),
    compare: CompareBy,
) -> Id {
    let (one, nothing) = (model.constant(1.0f32.to_bits()), model.constant(0));
    let reference_f = model.as_float(reference);
    let passes = |model: &mut M, texel_bits: Id| -> Id {
        let texel = model.as_float(texel_bits);
        let function = match compare {
            CompareBy::Known(function, _) => {
                let Some(opcode) = compare_opcode(function) else {
                    // Never (0) and always (7) answer alike whatever the texel holds.
                    return if function == 0 { nothing } else { one };
                };
                let passed = model.compare(opcode, reference_f, texel);
                return model.select(passed, one, nothing);
            }
            CompareBy::Running(function, _) => function,
        };
        let seven = model.constant(7);
        let always = model.compare(op::IEQUAL, function, seven);
        let mut answer = model.select(always, one, nothing);
        for code in 1..=6 {
            let Some(opcode) = compare_opcode(code) else {
                continue;
            };
            let passed = model.compare(opcode, reference_f, texel);
            let held = model.select(passed, one, nothing);
            let named = model.constant(u32::from(code));
            let chosen = model.compare(op::IEQUAL, function, named);
            answer = model.select(chosen, held, answer);
        }
        answer
    };
    match compare {
        CompareBy::Known(_, false) => {
            compare_point(model, (texture, coordinate, implicit), &passes)
        }
        CompareBy::Known(_, true) => compare_bilinear(model, (texture, coordinate), &passes),
        CompareBy::Running(_, linear) => {
            let point = compare_point(model, (texture, coordinate, implicit), &passes);
            let blended = compare_bilinear(model, (texture, coordinate), &passes);
            let zero = model.constant(0);
            let filtered = model.compare(op::INOT_EQUAL, linear, zero);
            model.select(filtered, blended, point)
        }
    }
}

/// The comparison `SQ_TEX_DEPTH_COMPARE` names, of the reference with the texel; `None` for never
/// (0) and always (7).
const fn compare_opcode(function: u8) -> Option<u16> {
    Some(match function {
        1 => op::FORD_LESS_THAN,
        2 => op::FORD_EQUAL,
        3 => op::FORD_LESS_THAN_EQUAL,
        4 => op::FORD_GREATER_THAN,
        5 => op::FUNORD_NOT_EQUAL,
        6 => op::FORD_GREATER_THAN_EQUAL,
        _ => return None,
    })
}

/// Channel `at` of the float vector `vector`, as bits.
fn channel_bits<M: Model + ?Sized>(model: &mut M, vector: Id, at: u32) -> Id {
    let (f32_type, u32_type) = (model.f32_type(), model.u32_type());
    let b = model.builder();
    let float = b.id();
    b.function(op::COMPOSITE_EXTRACT, &[f32_type.0, float.0, vector.0, at]);
    let bits = b.id();
    b.function(op::BITCAST, &[u32_type.0, bits.0, float.0]);
    bits
}

/// A point-filtered compare: the one texel at `coordinate`, compared by `passes`.
fn compare_point<M: Model + ?Sized>(
    model: &mut M,
    (texture, coordinate, implicit): (&Texture, Id, bool),
    passes: &dyn Fn(&mut M, Id) -> Id,
) -> Id {
    let f32_type = model.f32_type();
    let zero_level = model.constant(0);
    let b = model.builder();
    let bound = bound_texture(b, texture);
    let sampled = b.id();
    if implicit {
        b.function(
            op::IMAGE_SAMPLE_IMPLICIT_LOD,
            &[texture.result.0, sampled.0, bound.0, coordinate.0],
        );
    } else {
        let level = b.id();
        b.function(op::BITCAST, &[f32_type.0, level.0, zero_level.0]);
        b.function(
            op::IMAGE_SAMPLE_EXPLICIT_LOD,
            &[
                texture.result.0,
                sampled.0,
                bound.0,
                coordinate.0,
                image_operands::LOD,
                level.0,
            ],
        );
    }
    let texel = channel_bits(model, sampled, 0);
    passes(model, texel)
}

/// A bilinear compare: four texels gathered from the first channel - (i0, j1), (i1, j1), (i1, j0),
/// (i0, j0), in the order `OpImageGather` answers them - each compared by `passes`, the answers
/// blended with the filter's weights.
fn compare_bilinear<M: Model + ?Sized>(
    model: &mut M,
    (texture, coordinate): (&Texture, Id),
    passes: &dyn Fn(&mut M, Id) -> Id,
) -> Id {
    let zero_level = model.constant(0);
    let b = model.builder();
    let bound = bound_texture(b, texture);
    b.header(op::CAPABILITY, &[orbistoun_spirv::capability::IMAGE_QUERY]);
    let gathered = b.id();
    b.function(
        op::IMAGE_GATHER,
        &[
            texture.result.0,
            gathered.0,
            bound.0,
            coordinate.0,
            zero_level.0,
        ],
    );
    let image = image_of(b, texture, bound);
    let size = b.id();
    b.function(
        op::IMAGE_QUERY_SIZE_LOD,
        &[texture.texel.0, size.0, image.0, zero_level.0],
    );
    let answers: Vec<Id> = (0..4)
        .map(|at| {
            let texel = channel_bits(model, gathered, at);
            passes(model, texel)
        })
        .collect();
    let across = [0, 1].map(|axis| channel_bits(model, coordinate, axis));
    bilinear_blend(
        model,
        [answers[0], answers[1], answers[2], answers[3]],
        (across, size),
    )
}

/// Four answers gathered at `(i0, j1)`, `(i1, j1)`, `(i1, j0)`, `(i0, j0)` blended as a bilinear
/// filter weighs them at `coordinate` (its two channels, as bits) over an image `size` texels
/// across and down: by the fraction of the way past the texel centre below the coordinate.
fn bilinear_blend<M: Model + ?Sized>(
    model: &mut M,
    answers: [Id; 4],
    (coordinate, size): ([Id; 2], Id),
) -> Id {
    let u32_type = model.u32_type();
    let half = model.constant(0.5f32.to_bits());
    let mut fractions = [half; 2];
    for (axis, fraction) in (0..2_u32).zip(&mut fractions) {
        let b = model.builder();
        let extent = b.id();
        b.function(op::COMPOSITE_EXTRACT, &[u32_type.0, extent.0, size.0, axis]);
        let extent = model.unsigned_to_float_bits(extent);
        let scaled = model.f32_binary(op::FMUL, coordinate[axis as usize], extent);
        let centred = model.f32_binary(op::FSUB, scaled, half);
        let below = model.f32_ext_unary(GLSL_FLOOR, centred);
        *fraction = model.f32_binary(op::FSUB, centred, below);
    }
    let blend = |model: &mut M, from: Id, to: Id, by: Id| -> Id {
        let span = model.f32_binary(op::FSUB, to, from);
        let part = model.f32_binary(op::FMUL, span, by);
        model.f32_binary(op::FADD, from, part)
    };
    let below = blend(model, answers[3], answers[2], fractions[0]);
    let above = blend(model, answers[0], answers[1], fractions[0]);
    blend(model, below, above, fractions[1])
}

/// The coordinate a texture access reads at, built for one lane from its address registers. A
/// fetch's is a texel index, an integer, so the words go straight in; a sample's is a position, so
/// the words are reinterpreted as float bits; a cube's is the direction its face's coordinates and
/// the face name (D811).
fn sample_coordinate<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (addresses, a16): (&[u32; MAX_ADDRESS_REGISTERS], bool),
    (texture, fetches, cube): (&Texture, bool, bool),
    (skip, lane): (u32, u32),
) -> Result<Id, TranslateError> {
    // Two dimensions from consecutive elements; the dimensionality field was checked. A compare's
    // reference is ahead of them (D816), and texel offsets ahead of that (`skip`).
    let first = skip + u32::from(texture.shape.compare);
    let across = address_element(model, instruction, (addresses, a16), first, lane)?;
    let down = address_element(model, instruction, (addresses, a16), first + 1, lane)?;
    let components = if cube {
        // The face follows the face's two coordinates, behind a compare's reference.
        let face = address_element(model, instruction, (addresses, a16), first + 2, lane)?;
        cube_direction(model, across, down, face)
            .map(|bits| model.as_float(bits))
            .to_vec()
    } else if texture.shape.volume() && !fetches {
        // A volume's position, across, down and deep, each as the guest gave it (D823).
        let deep = address_element(model, instruction, (addresses, a16), 2, lane)?;
        [across, down, deep].map(|at| model.as_float(at)).to_vec()
    } else if fetches && texture.shape.layered() {
        // A 2D array's texel and layer (D897).
        let layer = address_element(model, instruction, (addresses, a16), 2, lane)?;
        vec![across, down, layer]
    } else if fetches {
        vec![across, down]
    } else {
        // A mirror-once mode samples the coordinate's absolute value, its reflection about zero; a
        // half-border clamp then samples it saturated, a NaN to zero as Mesa's `fsat` (D743).
        [
            (across, texture.saturate[0], texture.mirror[0]),
            (down, texture.saturate[1], texture.mirror[1]),
        ]
        .map(|(at, saturated, mirrored)| {
            let at = if mirrored {
                model.f32_ext_unary(GLSL_FABS, at)
            } else {
                at
            };
            let at = if saturated {
                clamp_unit(model, at, true)
            } else {
                at
            };
            model.as_float(at)
        })
        .to_vec()
    };
    let kind = if fetches {
        texture.texel
    } else {
        texture.coordinate
    };
    let builder = model.builder();
    let coordinate = builder.id();
    let mut operands = vec![kind.0, coordinate.0];
    operands.extend(components.iter().map(|component| component.0));
    builder.function(op::COMPOSITE_CONSTRUCT, &operands);
    Ok(coordinate)
}

/// The texel a gradient sample answers, from the bound image at `coordinate`: the level its
/// derivatives choose, the position's change across a pixel in the first two address elements and
/// down one in the next two (`image_sample_d`, as `llvm.amdgcn.image.sample.d.2d` orders them).
fn gradient_sample<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (addresses, texture): (&[u32; MAX_ADDRESS_REGISTERS], &Texture),
    (bound, coordinate): (Id, Id),
    lane: u32,
) -> Result<Id, TranslateError> {
    let mut pairs = [Id(0); 2];
    for (pair, first) in pairs.iter_mut().zip([0, 2]) {
        let mut components = [Id(0); 2];
        for (component, at) in components.iter_mut().zip([first, first + 1]) {
            let bits = address_element(model, instruction, (addresses, false), at, lane)?;
            *component = model.as_float(bits);
        }
        let builder = model.builder();
        let vector = builder.id();
        builder.function(
            op::COMPOSITE_CONSTRUCT,
            &[
                texture.coordinate.0,
                vector.0,
                components[0].0,
                components[1].0,
            ],
        );
        *pair = vector;
    }
    let builder = model.builder();
    let texel = builder.id();
    builder.function(
        op::IMAGE_SAMPLE_EXPLICIT_LOD,
        &[
            texture.result.0,
            texel.0,
            bound.0,
            coordinate.0,
            image_operands::GRAD,
            pairs[0].0,
            pairs[1].0,
        ],
    );
    Ok(texel)
}

/// The texel a sample or a fetch answers, from the bound image at `coordinate`: a fetch at an
/// integer level, a gather of one component, a sample at a named level, or one whose level the
/// implementation chooses.
fn sample_or_fetch<M: Model + ?Sized>(
    model: &mut M,
    texture: &Texture,
    (bound, coordinate): (Id, Id),
    (fetches, named_level, gather): (bool, bool, Option<u32>),
    level: Id,
) -> Id {
    let component = gather.map(|component| model.constant(component));
    let builder = model.builder();
    let texel = builder.id();
    if let Some(component) = component {
        builder.function(
            op::IMAGE_GATHER,
            &[
                texture.result.0,
                texel.0,
                bound.0,
                coordinate.0,
                component.0,
            ],
        );
    } else if fetches {
        // A fetch takes an image, so the bound pair is unwrapped first. Vulkan requires a level
        // for a non-multisampled image: zero for `image_load`, and the guest's own for
        // `image_load_mip`, counted from the bound image's first level as the descriptor
        // counts from its base level.
        let image = image_of(builder, texture, bound);
        builder.function(
            op::IMAGE_FETCH,
            &[
                texture.result.0,
                texel.0,
                image.0,
                coordinate.0,
                image_operands::LOD,
                level.0,
            ],
        );
    } else if named_level {
        builder.function(
            op::IMAGE_SAMPLE_EXPLICIT_LOD,
            &[
                texture.result.0,
                texel.0,
                bound.0,
                coordinate.0,
                image_operands::LOD,
                level.0,
            ],
        );
    } else {
        builder.function(
            op::IMAGE_SAMPLE_IMPLICIT_LOD,
            &[texture.result.0, texel.0, bound.0, coordinate.0],
        );
    }
    texel
}

/// A flat sample's normalised position moved by the texel offsets an `_o` form carries: the low
/// six bits across and bits 13:8 down, each signed, in texels of the level sampled - level zero,
/// the only one an offset form is translated at - so each is divided by that level's size before it
/// is added. A host sample takes only a constant offset, and the guest's is a register.
fn offset_coordinate<M: Model + ?Sized>(
    model: &mut M,
    texture: &Texture,
    bound: Id,
    (coordinate, offsets): (Id, Id),
) -> Id {
    let twenty_six = model.constant(26);
    let eight = model.constant(8);
    let signed_six = |model: &mut M, value| {
        let raised = model.binary(op::SHIFT_LEFT_LOGICAL, value, twenty_six);
        model.binary(op::SHIFT_RIGHT_ARITHMETIC, raised, twenty_six)
    };
    let across = signed_six(model, offsets);
    let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, offsets, eight);
    let down = signed_six(model, shifted);
    let zero = model.constant(0);
    let f32_type = model.f32_type();
    let builder = model.builder();
    builder.header(op::CAPABILITY, &[orbistoun_spirv::capability::IMAGE_QUERY]);
    let image = image_of(builder, texture, bound);
    let size = builder.id();
    builder.function(
        op::IMAGE_QUERY_SIZE_LOD,
        &[texture.texel.0, size.0, image.0, zero.0],
    );
    let size_f = builder.id();
    builder.function(
        op::CONVERT_U_TO_F,
        &[texture.coordinate.0, size_f.0, size.0],
    );
    let mut texels = [Id(0); 2];
    for (slot, value) in texels.iter_mut().zip([across, down]) {
        let float = builder.id();
        builder.function(op::CONVERT_S_TO_F, &[f32_type.0, float.0, value.0]);
        *slot = float;
    }
    let moved = builder.id();
    builder.function(
        op::COMPOSITE_CONSTRUCT,
        &[texture.coordinate.0, moved.0, texels[0].0, texels[1].0],
    );
    let fraction = builder.id();
    builder.function(
        op::FDIV,
        &[texture.coordinate.0, fraction.0, moved.0, size_f.0],
    );
    let shifted = builder.id();
    builder.function(
        op::FADD,
        &[texture.coordinate.0, shifted.0, coordinate.0, fraction.0],
    );
    shifted
}

/// The direction a cube sample's address names (D811). The hardware takes a face's two coordinates,
/// each `sc / |ma| + 1.5` as ACO forms them (`ac_nir_lower_tex_coords.c`, `prepare_cube_coords`),
/// and the face, `cube_amd`'s id. Inverting `cube_amd` with the major component one half gives the
/// direction a host cube samples at the same place: each coordinate less 1.5 is then that face's
/// `sc` or `tc` itself.
///
/// The face is the register's, whatever the coordinates hold: each is clamped to just inside the
/// face, so a coordinate outside ACO's range - oops-sdk's GL biases by a half instead, landing in
/// `[0, 1]` - cannot tip the major axis onto another face. Where the hardware samples within a
/// face for such a coordinate is unmeasured (obSCEne `REQ-20261013T1930Z-cb01`).
fn cube_direction<M: Model + ?Sized>(model: &mut M, across: Id, down: Id, face: Id) -> [Id; 3] {
    let middle = model.constant(1.5f32.to_bits());
    // The largest float below a half, so a clamped coordinate never ties the major axis.
    let inside = model.constant(0x3EFF_FFFF);
    let outside = model.constant(0x3EFF_FFFF | 0x8000_0000);
    let [along, upward] = [across, down].map(|at| {
        let centred = model.f32_binary(op::FSUB, at, middle);
        let low = model.f32_ext_binary(GLSL_FMAX, centred, outside);
        model.f32_ext_binary(GLSL_FMIN, low, inside)
    });
    let sign = model.constant(0x8000_0000);
    let [against, downward] =
        [along, upward].map(|value| model.binary(op::BITWISE_XOR, value, sign));
    let half = model.constant(0.5f32.to_bits());
    let minus_half = model.constant((-0.5f32).to_bits());
    let face = model.as_float(face);
    // Below each boundary between faces: the id is a whole number, so the half-way points split the
    // faces whatever rounding produced it.
    let below = [0.5f32, 1.5, 2.5, 3.5, 4.5].map(|boundary| {
        let boundary = model.constant(boundary.to_bits());
        let boundary = model.as_float(boundary);
        model.compare(op::FORD_LESS_THAN, face, boundary)
    });
    // Per face: +x, -x, +y, -y, +z, -z.
    let choose = |model: &mut M, per_face: [Id; 6]| {
        let mut value = per_face[5];
        for (at, &under) in below.iter().enumerate().rev() {
            value = pick(model, under, per_face[at], value);
        }
        value
    };
    let x = choose(model, [half, minus_half, along, along, along, against]);
    let y = choose(
        model,
        [downward, downward, half, minus_half, downward, downward],
    );
    let z = choose(model, [against, along, upward, downward, half, minus_half]);
    [x, y, z]
}

/// Translates a texel store.
///
/// Four operands, shaped like a fetch: the data registers, the coordinate registers, the image
/// descriptor and the component mask, which decides how many consecutive registers hold the texel.
/// Components the mask does not select are written as zero: the format that would define them is in
/// a descriptor that is not decoded (D692).
fn image_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };

    let ImageAccess {
        destination: data,
        addresses,
        descriptor,
        mask,
        halves,
        shape,
        ..
    } = image_access(instruction, name)?;
    let Halves {
        address: a16,
        data: d16,
    } = halves;

    // A store to a level writes the level its address names after the coordinates. The host binds
    // that level's own surface, so the level has to be one it can read for the dispatch: a
    // user-data word the program copied into the register, as PPSA04263's copy into a level does.
    let level = if name == "image_store_mip" {
        let register = addresses[2 + usize::from(shape.layered())];
        Some(model.vector_user_data(register).ok_or_else(|| {
            refuse(concat!(
                "a store to a level the program computes rather than takes from its user data - ",
                "the host binds one level's surface, and which one has to be known per dispatch"
            ))
        })?)
    } else {
        None
    };
    let stored = model
        .storage_image(descriptor, (shape, level, instruction.offset))
        .map_err(refuse)?;
    if stored.integer && d16 {
        return Err(refuse(
            "a sixteen-bit store to an integer image is not translated",
        ));
    }
    let zero = model.constant(0);

    for lane in running_lanes(model) {
        let u = address_element(model, instruction, (&addresses, a16), 0, lane)?;
        let v = address_element(model, instruction, (&addresses, a16), 1, lane)?;

        // The texel's components, taken in mask order from consecutive registers, so a hole in the
        // mask reads no register for that component. Under `D16` two halves share a register, the
        // first in the low half, each widened back to a float.
        let zero_component = if stored.integer {
            zero
        } else {
            model.as_float(zero)
        };
        let mut components = [zero_component; IMAGE_COMPONENTS as usize];
        let mut read = 0;
        for (component, slot) in components.iter_mut().enumerate() {
            let selected = u32::try_from(component).unwrap_or(IMAGE_COMPONENTS);
            if mask & (1 << selected) == 0 {
                continue;
            }
            let register = if d16 { read / 2 } else { read };
            let source = Operand::Vector(address_of(data + register));
            let mut bits = model.read_source(instruction, &source, lane)?;
            if d16 {
                let shift = model.constant(16 * (read % 2));
                let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, bits, shift);
                let low = model.constant(0xFFFF);
                let half = model.binary(op::BITWISE_AND, shifted, low);
                bits = model.half_to_float_bits(half);
            }
            *slot = if stored.integer {
                bits
            } else {
                model.as_float(bits)
            };
            read += 1;
        }

        // A volume's third coordinate is its slice (D823), and an array's its layer (D897).
        let w = (stored.volume || stored.layered)
            .then(|| address_element(model, instruction, (&addresses, a16), 2, lane))
            .transpose()?;
        let builder = model.builder();
        let coordinate = builder.id();
        let mut construct = vec![stored.texel.0, coordinate.0, u.0, v.0];
        construct.extend(w.map(|w| w.0));
        builder.function(op::COMPOSITE_CONSTRUCT, &construct);
        let value = builder.id();
        let mut construct = vec![stored.value.0, value.0];
        construct.extend(components.iter().map(|id| id.0));
        builder.function(op::COMPOSITE_CONSTRUCT, &construct);
        // An inactive lane stores nothing. Its registers hold whatever an earlier write left there,
        // often nothing at all: radeonsi's one-texel readback copy runs a single thread of 64, and
        // the other 63, whose coordinates were never computed, would store zero over its texel.
        let builder = model.builder();
        let bound = builder.id();
        builder.function(op::LOAD, &[stored.image.0, bound.0, stored.variable.0]);
        // A layer past an array's last is out of bounds, and stores nothing (D895, D897).
        let in_layer = w
            .filter(|_| stored.layered)
            .map(|layer| layer_held(model, (stored.texel, bound), layer));
        let merge = open_if_active(model, lane, in_layer);
        model
            .builder()
            .function(op::IMAGE_WRITE, &[bound.0, coordinate.0, value.0]);
        close_if_active(model, merge);
    }

    model.count();
    Ok(())
}

/// Opens a block that runs only while `lane` is active in the execution mask and `and` holds, where
/// given, returning the merge [`close_if_active`] closes it at; `None`, opening nothing, for a model
/// with no execution mask, whose every lane runs, and no condition.
fn open_if_active<M: Model + ?Sized>(model: &mut M, lane: u32, and: Option<Id>) -> Option<Id> {
    let active = match (model.read_lane_mask(EXEC_LOW_HALF).ok(), and) {
        (Some((low, high)), and) => {
            let active = model.lane_bit(low, high, lane);
            and.map_or(active, |and| model.both(active, and))
        }
        (None, Some(and)) => and,
        (None, None) => return None,
    };
    let b = model.builder();
    let (body, merge) = (b.id(), b.id());
    b.function(op::SELECTION_MERGE, &[merge.0, 0]);
    b.function(op::BRANCH_CONDITIONAL, &[active.0, body.0, merge.0]);
    b.function(op::LABEL, &[body.0]);
    Some(merge)
}

/// Closes a block [`open_if_active`] opened.
fn close_if_active<M: Model + ?Sized>(model: &mut M, merge: Option<Id>) {
    if let Some(merge) = merge {
        let b = model.builder();
        b.function(op::BRANCH, &[merge.0]);
        b.function(op::LABEL, &[merge.0]);
    }
}

/// Whether an access reaches a buffer the tracer can give a slot: a scalar load, or a global load
/// or a dispatch's global store, which reach the buffer of the pages its lanes' addresses fall in
/// (D883, D961).
fn traced_load(name: &str) -> bool {
    name.starts_with("s_buffer_load_dword")
        || name.starts_with("s_load_dword")
        || name.starts_with("global_load_")
        || name.starts_with("global_store_dword")
}

/// A traced global load's draw buffer and its guest base: the buffer its lanes' addresses span
/// (D758, D883); `None` for a load read through the window.
fn traced_global_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<Option<(u32, Id)>, TranslateError> {
    let Some(slot) = model.draw_buffer_slot(instruction) else {
        return Ok(None);
    };
    let base = model
        .draw_buffer_base(slot)
        .ok_or(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a global load's draw buffer in a module whose draw data carries no base (D758)",
        })?;
    Ok(Some((slot, base)))
}

/// The word at `address` a global load reads, its low and high halves: from its traced draw
/// buffer, relative to the buffer's guest base, or through the window. With the word, for a traced
/// load in an exact module, whether the buffer does not hold it: the range bound was placed from a
/// sample of the dispatch's waves (D950), and a lane outside it is an escape, not a read of the
/// wrong word. A dispatch's buffer holds only the pages its waves reach ([`global_page_word`]).
fn global_word<M: Model + ?Sized>(
    model: &mut M,
    bound: Option<(u32, Id)>,
    (address, high): (Id, Id),
) -> Result<(Id, Option<Id>), TranslateError> {
    Ok(match bound {
        Some((slot, _)) if model.global_pages() => {
            let (value, outside) = global_page_word(model, slot, (address, high))?;
            (value, model.exact_memory().then_some(outside))
        }
        Some((slot, base)) => {
            let offset = model.binary(op::ISUB, address, base);
            let two = model.constant(2);
            let index = model.binary(op::SHIFT_RIGHT_LOGICAL, offset, two);
            let outside = if model.exact_memory() {
                model.draw_buffer_words(slot).map(|words| {
                    // Unsigned, so an address below the base wraps past the end and is outside too.
                    model.compare(op::UGREATER_THAN_EQUAL, index, words)
                })
            } else {
                None
            };
            (model.read_draw_buffer(slot, index)?, outside)
        }
        None => (read_guarded(model, address), None),
    })
}

/// The word at `address`, its low and high halves, from a dispatch's global buffer of pages
/// ([`crate::wavefront::GLOBAL_PAGE_BYTES`]), with whether the buffer does not hold it. A word not
/// held reads zero, never another page's.
fn global_page_word<M: Model + ?Sized>(
    model: &mut M,
    slot: u32,
    address: (Id, Id),
) -> Result<(Id, Id), TranslateError> {
    let (index, held) = global_page_index(model, slot, address)?;
    let word = model.read_draw_buffer(slot, index)?;
    let zero = model.constant(0);
    let value = model.select(held, word, zero);
    let bool_type = model.bool_type();
    let b = model.builder();
    let outside = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, outside.0, held.0]);
    Ok((value, outside))
}

/// Where the word at `low`, `high` lies in a dispatch's global buffer of pages
/// ([`crate::wavefront::GLOBAL_PAGE_BYTES`]), and whether the buffer holds it (D961): its page's
/// number, the address's top 52 bits as a low and a high half, hashed to its slot of the table
/// ([`crate::wavefront::global_page_slot`]), and the slots after it searched for that number. A
/// word not held lies at index zero, the table's size, which nothing stores to.
fn global_page_index<M: Model + ?Sized>(
    model: &mut M,
    slot: u32,
    (low, high): (Id, Id),
) -> Result<(Id, Id), TranslateError> {
    let page_bytes = crate::wavefront::GLOBAL_PAGE_BYTES;
    let page_shift = page_bytes.trailing_zeros();
    let (zero, one, three) = (model.constant(0), model.constant(1), model.constant(3));
    let (down, up) = (model.constant(page_shift), model.constant(32 - page_shift));
    let low_part = model.binary(op::SHIFT_RIGHT_LOGICAL, low, down);
    let high_part = model.binary(op::SHIFT_LEFT_LOGICAL, high, up);
    let number_low = model.binary(op::BITWISE_OR, low_part, high_part);
    let number_high = model.binary(op::SHIFT_RIGHT_LOGICAL, high, down);
    // The slot it hashes to, in a table of `1 << bits` slots.
    let bits = model.read_draw_buffer(slot, zero)?;
    let mixed = model.binary(op::BITWISE_XOR, number_low, number_high);
    let hash = model.constant(crate::wavefront::GLOBAL_PAGE_HASH);
    let hashed = model.binary(op::IMUL, mixed, hash);
    let thirty_two = model.constant(32);
    let keep = model.binary(op::ISUB, thirty_two, bits);
    let home = model.binary(op::SHIFT_RIGHT_LOGICAL, hashed, keep);
    let size = model.binary(op::SHIFT_LEFT_LOGICAL, one, bits);
    let mask = model.binary(op::ISUB, size, one);
    let (mut start, mut held) = (zero, model.compare(op::INOT_EQUAL, zero, zero));
    for probe in 0..crate::wavefront::GLOBAL_PAGE_PROBES {
        let step = model.constant(probe);
        let at = model.add(home, step);
        let at = model.binary(op::BITWISE_AND, at, mask);
        let at = model.binary(op::IMUL, at, three);
        let at = model.add(at, one);
        let held_low = model.read_draw_buffer(slot, at)?;
        let at = model.add(at, one);
        let held_high = model.read_draw_buffer(slot, at)?;
        let at = model.add(at, one);
        let begins = model.read_draw_buffer(slot, at)?;
        let same_low = model.compare(op::IEQUAL, held_low, number_low);
        let same_high = model.compare(op::IEQUAL, held_high, number_high);
        let occupied = model.compare(op::INOT_EQUAL, begins, zero);
        let same = model.both(same_low, same_high);
        let found = model.both(same, occupied);
        start = model.select(found, begins, start);
        held = model.either(held, found);
    }
    let within_mask = model.constant(page_bytes - 1);
    let within = model.binary(op::BITWISE_AND, low, within_mask);
    let two = model.constant(2);
    let within = model.binary(op::SHIFT_RIGHT_LOGICAL, within, two);
    let index = model.add(start, within);
    Ok((model.select(held, index, zero), held))
}

/// A flat access's address for one lane, its low and high halves: the low as
/// [`Model::flat_address`] forms it, and the high half of the vector pair or scalar base it adds
/// to, with the carry out of the low half.
fn wide_flat_address<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (vaddr, base): (&Operand, &Operand),
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    let low = model.flat_address(instruction, vaddr, base, lane)?;
    let (from, high) = match (base, vaddr) {
        (Operand::Scalar(register), _) => {
            let register = u32::from(*register);
            (model.read_scalar(register), model.read_scalar(register + 1))
        }
        (_, Operand::Vector(register)) => {
            let next = Operand::Vector(register.saturating_add(1));
            (
                model.read_source(instruction, vaddr, lane)?,
                model.read_source(instruction, &next, lane)?,
            )
        }
        _ => {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "a flat access whose address is not a register pair",
            });
        }
    };
    Ok((low, carried(model, (from, low), high)))
}

/// A global access's address for one lane, its low and high halves: both where a dispatch's
/// global buffer of pages looks its page up by its full number (D961), and elsewhere the low half
/// alone, which is all a window or a range from a base reads, standing for both.
fn global_access_address<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (vaddr, base): (&Operand, &Operand),
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    if model.global_pages() && model.draw_buffer_slot(instruction).is_some() {
        return wide_flat_address(model, instruction, (vaddr, base), lane);
    }
    let low = model.flat_address(instruction, vaddr, base, lane)?;
    Ok((low, low))
}

/// `high` with one added where `low`, a sum from `from`, carried out of thirty-two bits.
fn carried<M: Model + ?Sized>(model: &mut M, (from, low): (Id, Id), high: Id) -> Id {
    let (zero, one) = (model.constant(0), model.constant(1));
    let carry = model.compare(op::ULESS_THAN, low, from);
    let carry = model.select(carry, one, zero);
    model.add(high, carry)
}

/// The address `words` words past `address`, its low and high halves, with the carry.
fn step_wide<M: Model + ?Sized>(model: &mut M, (low, high): (Id, Id), words: u32) -> (Id, Id) {
    if words == 0 {
        return (low, high);
    }
    let stepped = step_address(model, low, words);
    // A low half standing for both ([`global_access_address`]) carries nothing.
    if high == low {
        return (stepped, stepped);
    }
    (stepped, carried(model, (low, stepped), high))
}

/// One lane's word of a dispatch's global store through its global buffer of pages (D961), at
/// `address`, its low and high halves: stored where the buffer holds its page, and an escape,
/// storing nothing, where it does not.
fn global_page_store<M: Model + ?Sized>(
    model: &mut M,
    slot: u32,
    (address, value): ((Id, Id), Id),
    lane: u32,
) -> Result<(), TranslateError> {
    let (index, held) = global_page_index(model, slot, address)?;
    let bool_type = model.bool_type();
    let b = model.builder();
    let outside = b.id();
    b.function(op::LOGICAL_NOT, &[bool_type.0, outside.0, held.0]);
    if model.exact_memory() {
        model.note_escape(outside, Some(lane));
    }
    let previous = model.read_draw_buffer(slot, index)?;
    let kept = pick(model, outside, previous, value);
    model.write_draw_buffer(slot, (index, kept), outside, lane)
}

/// `global_store_byte` and `global_store_short`: a lane's data register's low byte or half stored
/// at its address, merged into the word holding it and, for a half at a word's last byte, the
/// next, as one step a part (D977) - through the window, or a dispatch's global buffer of pages
/// (D961).
fn global_sub_word_store<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    bytes: u32,
) -> Result<(), TranslateError> {
    let (vaddr, data, base) = first_three_operands(instruction)?;
    if !matches!(data, Operand::Vector(_)) {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a flat store's data is not a vector register",
        });
    }
    let bound = traced_global_load(model, instruction)?;
    if bound.is_some() && !model.global_pages() {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a global store through a bound buffer that is not a dispatch's pages (D961)",
        });
    }
    for lane in running_lanes(model) {
        let (low, high) = global_access_address(model, instruction, (vaddr, base), lane)?;
        let value = model.read_source(instruction, data, lane)?;
        let (parts, spans) = sub_word_parts(model, (low, value), 8 * bytes);
        let aligned = model.constant(!3);
        let first = model.binary(op::BITWISE_AND, low, aligned);
        let first = (first, if high == low { first } else { high });
        let always = model.compare(op::IEQUAL, aligned, aligned);
        for (word, parts) in [0u32, 1].into_iter().zip(parts) {
            let at = step_wide(model, first, word);
            let reaches = if word == 0 { always } else { spans };
            if let Some((slot, _)) = bound {
                let (index, held) = global_page_index(model, slot, at)?;
                let bool_type = model.bool_type();
                let b = model.builder();
                let unheld = b.id();
                b.function(op::LOGICAL_NOT, &[bool_type.0, unheld.0, held.0]);
                if model.exact_memory() {
                    let escaped = model.both(unheld, reaches);
                    model.note_escape(escaped, Some(lane));
                }
                let landed = model.both(held, reaches);
                merge_draw_buffer(model, slot, (index, parts), landed, lane)?;
            } else {
                let inside = model.address_within_window(at.0);
                let landed = model.both(inside, reaches);
                let index = model.word_index(at.0);
                model.merge_memory(index, parts, landed, lane);
            }
        }
    }
    model.count();
    Ok(())
}

/// Records a traced global load's word outside its buffer as an escape in `lane` (D950), where
/// `needed` holds when given: the word's bytes are part of what the load answers.
fn note_global_escape<M: Model + ?Sized>(
    model: &mut M,
    outside: Option<Id>,
    needed: Option<Id>,
    lane: u32,
) {
    if let Some(outside) = outside {
        let escaped = match needed {
            Some(needed) => model.both(outside, needed),
            None => outside,
        };
        model.note_escape(escaped, Some(lane));
    }
}

/// The width in bits and signedness of a sub-word load: a byte or a half, zero- or sign-extended.
fn sub_word(name: &str) -> Option<(u32, bool)> {
    match name {
        "global_load_ubyte" => Some((8, false)),
        "global_load_sbyte" => Some((8, true)),
        "global_load_ushort" => Some((16, false)),
        "global_load_sshort" => Some((16, true)),
        _ => None,
    }
}

/// A byte or half load: the word holding its address read, and the bytes at its place in it
/// widened into the destination - zero-extended, or `signed` sign-extended (AMD's published RDNA
/// instruction set). A half at a word's last byte takes its high byte from the next word.
fn sub_word_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (width, signed): (u32, bool),
) -> Result<(), TranslateError> {
    let (destination, vaddr, base) = first_three_operands(instruction)?;
    let Operand::Vector(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a flat load destination is not a vector register",
        });
    };
    let register = u32::from(*register);
    let bound = traced_global_load(model, instruction)?;
    for lane in running_lanes(model) {
        let (address, top) = global_access_address(model, instruction, (vaddr, base), lane)?;
        let aligned = model.constant(!3);
        let word_address = model.binary(op::BITWISE_AND, address, aligned);
        let (low, low_outside) = global_word(model, bound, (word_address, top))?;
        let next = step_wide(model, (word_address, top), 1);
        let (high, high_outside) = global_word(model, bound, next)?;
        // The bytes from the address on: the low word shifted down, the next word's above it.
        let three = model.constant(3);
        let place = model.binary(op::BITWISE_AND, address, three);
        // The next word is part of the answer only for a half at a word's last byte (D950).
        note_global_escape(model, low_outside, None, lane);
        if width > 8 {
            let crosses = model.compare(op::IEQUAL, place, three);
            note_global_escape(model, high_outside, Some(crosses), lane);
        }
        let eight = model.constant(8);
        let shift = model.binary(op::IMUL, place, eight);
        let down = model.binary(op::SHIFT_RIGHT_LOGICAL, low, shift);
        // `high << (32 - shift)`, as two shifts so a shift of zero moves it out entirely.
        let one = model.constant(1);
        let doubled = model.binary(op::SHIFT_LEFT_LOGICAL, high, one);
        let thirty_one = model.constant(31);
        let rest = model.binary(op::ISUB, thirty_one, shift);
        let up = model.binary(op::SHIFT_LEFT_LOGICAL, doubled, rest);
        let bytes = model.binary(op::BITWISE_OR, down, up);
        let unused = model.constant(32 - width);
        let top = model.binary(op::SHIFT_LEFT_LOGICAL, bytes, unused);
        let widen = if signed {
            op::SHIFT_RIGHT_ARITHMETIC
        } else {
            op::SHIFT_RIGHT_LOGICAL
        };
        let value = model.binary(widen, top, unused);
        model.write_vector_lane(register, lane, value);
    }
    model.count();
    Ok(())
}

/// One vector register number as the decoder spells it.
///
/// Saturating, and unreachable either way: the caller has checked the pair fits the register file.
fn address_of(register: u32) -> u16 {
    u16::try_from(register).unwrap_or(u16::MAX)
}

/// Translates the flat memory instructions.
///
/// Loads and stores of one, two or four consecutive words. A load's destination and a store's data
/// sit in different fields, so each opcode has its own operand layout.
fn flat_memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    // The segment, bits 15:14 of the first word: flat (0) and global (2) both reach global memory
    // here (D878); scratch (1) is a lane's private memory, which no window holds.
    if (instruction.word >> 14) & 3 == 1 {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "a scratch-segment access reaches a lane's private memory, which is not modelled (D878)",
        });
    }
    if let Some((width, signed)) = sub_word(name) {
        return sub_word_load(model, instruction, (width, signed));
    }
    match name {
        // The flat loads, which differ only in how many consecutive registers they fill; each
        // layout is probed rather than assumed shared.
        "global_load_dword"
        | "global_load_dwordx2"
        | "global_load_dwordx3"
        | "global_load_dwordx4" => {
            let words = access_words(name);
            let (destination, vaddr, base) = first_three_operands(instruction)?;
            let Operand::Vector(register) = destination else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat load destination is not a vector register",
                });
            };
            let register = u32::from(*register);
            if register + words > VECTOR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat load runs past the end of the vector register file",
                });
            }
            let bound = traced_global_load(model, instruction)?;
            for lane in running_lanes(model) {
                let address = global_access_address(model, instruction, (vaddr, base), lane)?;
                for word in 0..words {
                    // Consecutive words, stepped by address so each word is bounds-checked: a
                    // multi-word access can start inside the window and end outside it.
                    let stepped = step_wide(model, address, word);
                    let (value, outside) = global_word(model, bound, stepped)?;
                    note_global_escape(model, outside, None, lane);
                    model.write_vector_lane(register + word, lane, value);
                }
            }
            model.count();
            Ok(())
        }

        // The flat stores: per lane and masked, because another lane may read what an inactive lane
        // would have written.
        "global_store_byte" => global_sub_word_store(model, instruction, 1),
        "global_store_short" => global_sub_word_store(model, instruction, 2),
        "global_store_dword"
        | "global_store_dwordx2"
        | "global_store_dwordx3"
        | "global_store_dwordx4" => {
            let words = access_words(name);
            let (vaddr, data, base) = first_three_operands(instruction)?;
            let Operand::Vector(first_register) = data else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat store's data is not a vector register",
                });
            };
            let first_register = u32::from(*first_register);
            if first_register + words > VECTOR_REGISTERS {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a flat store reads past the end of the vector register file",
                });
            }
            // A dispatch's traced store goes to its global buffer of pages (D961).
            let bound = traced_global_load(model, instruction)?;
            if bound.is_some() && !model.global_pages() {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a global store through a bound buffer that is not a dispatch's pages (D961)",
                });
            }
            for lane in running_lanes(model) {
                let address = global_access_address(model, instruction, (vaddr, base), lane)?;
                for word in 0..words {
                    let stepped = step_wide(model, address, word);
                    let source =
                        Operand::Vector(u16::try_from(first_register + word).unwrap_or(u16::MAX));
                    let value = model.read_source(instruction, &source, lane)?;
                    match bound {
                        Some((slot, _)) => global_page_store(model, slot, (stepped, value), lane)?,
                        None => write_guarded(model, stepped.0, value, lane),
                    }
                }
            }
            model.count();
            Ok(())
        }
        _ => Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "no translation for this flat memory instruction",
        }),
    }
}

/// The bits of one half of a mask below lane `index`, a run-time value (D760): `v_mbcnt_lo`'s for
/// the low half and `v_mbcnt_hi`'s for the high one - all of a half below the lane, none above it,
/// and in the lane's own half the bits before it.
fn lanes_below<M: Model + ?Sized>(model: &mut M, index: Id, high_half: bool) -> Id {
    let (u32_type, bool_type) = (model.u32_type(), model.bool_type());
    let (zero, one, all, thirty_two) = (
        model.constant(0),
        model.constant(1),
        model.constant(u32::MAX),
        model.constant(32),
    );
    let b = model.builder();
    // The lane's position counted from this half's first bit, and whether it is past this half.
    let within = if high_half {
        let within = b.id();
        b.function(op::ISUB, &[u32_type.0, within.0, index.0, thirty_two.0]);
        within
    } else {
        index
    };
    let before = b.id();
    b.function(
        op::ULESS_THAN,
        &[
            bool_type.0,
            before.0,
            index.0,
            if high_half { thirty_two.0 } else { zero.0 },
        ],
    );
    let past = b.id();
    b.function(
        op::UGREATER_THAN_EQUAL,
        &[bool_type.0, past.0, within.0, thirty_two.0],
    );
    // `1 << within` is defined for a within below thirty-two; the other cases are selected away.
    let shift = b.id();
    b.function(
        op::SHIFT_LEFT_LOGICAL,
        &[u32_type.0, shift.0, one.0, within.0],
    );
    let mask = b.id();
    b.function(op::ISUB, &[u32_type.0, mask.0, shift.0, one.0]);
    let capped = b.id();
    b.function(op::SELECT, &[u32_type.0, capped.0, past.0, all.0, mask.0]);
    let result = b.id();
    b.function(
        op::SELECT,
        &[u32_type.0, result.0, before.0, zero.0, capped.0],
    );
    result
}

/// [`Model::set_lane_bit`] for a lane known as a constant: bit `lane % 32` of the half it is in.
pub(crate) fn constant_lane_bit<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
    lane: u32,
    condition: Id,
) -> (Id, Id) {
    let bit = model.constant(1u32 << (lane % 32));
    let zero = model.constant(0);
    let u32_type = model.u32_type();

    let b = model.builder();
    let contribution = b.id();
    b.function(
        op::SELECT,
        &[u32_type.0, contribution.0, condition.0, bit.0, zero.0],
    );
    let updated = model.binary(
        op::BITWISE_OR,
        if lane < 32 { low } else { high },
        contribution,
    );
    if lane < 32 {
        (updated, high)
    } else {
        (low, updated)
    }
}

/// [`Model::lane_bit`] for a lane known as a constant.
pub(crate) fn constant_lane_test<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
    lane: u32,
) -> Id {
    let half = if lane < 32 { low } else { high };
    let shift = model.constant(lane % 32);
    let one = model.constant(1);
    let zero = model.constant(0);
    let (u32_type, bool_type) = (model.u32_type(), model.bool_type());

    let b = model.builder();
    let shifted = b.id();
    b.function(
        op::SHIFT_RIGHT_LOGICAL,
        &[u32_type.0, shifted.0, half.0, shift.0],
    );
    let bit = b.id();
    b.function(op::BITWISE_AND, &[u32_type.0, bit.0, shifted.0, one.0]);
    let result = b.id();
    b.function(op::INOT_EQUAL, &[bool_type.0, result.0, bit.0, zero.0]);
    result
}

/// Vector registers the guest has.
///
/// The wide flat forms reach the end: `global_load_dwordx4` into v253 would write past the last
/// register.
pub const VECTOR_REGISTERS: u32 = 256;

/// The byte address `offset` words past `address`.
///
/// Stepped by address rather than index so each word is checked against the window; stepping a
/// masked index would wrap the tail onto the front of the buffer.
fn step_address<M: Model + ?Sized>(model: &mut M, address: Id, offset: u32) -> Id {
    if offset == 0 {
        return address;
    }
    let step = model.constant(offset * 4);
    model.add(address, step)
}

/// Translates `s_wqm_b64`: whole quad mode.
///
/// Each group of four result bits is set if any of the source's four is: fold the group with two
/// shifts and two ors, then spread the answer across all four positions.
fn whole_quad_mode<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let (low, high) = sixty_four_bit_source(model, instruction, source)?;

    let low = quad_expand(model, low);
    let high = quad_expand(model, high);

    if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_name(name)
    {
        model.write_lane_mask(mask, low, high)?;
        model.count();
        return Ok(());
    }
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_wqm_b64 destination is neither a register pair nor a lane mask",
        });
    };
    let register = u32::from(*register);
    if register + 2 > SCALAR_REGISTERS {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_wqm_b64 runs past the end of the register file",
        });
    }
    model.write_scalar(register, low);
    model.write_scalar(register + 1, high);
    model.count();
    Ok(())
}

/// Translates `s_quadmask_b64`: bit `n` of the result set where any of bits `4n` to `4n + 3` of the
/// source is, the low word's sixteen groups in the result's low sixteen bits and nothing above, and
/// the condition code whether the result is non-zero (RDNA ISA, `S_QUADMASK_B64`).
fn quad_mask<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let (low, high) = sixty_four_bit_source(model, instruction, source)?;
    let zero = model.constant(0);
    let nibble = model.constant(0xf);
    let mut result = zero;
    for (word, first) in [(low, 0), (high, 8)] {
        for group in 0..8 {
            let shift = model.constant(4 * group);
            let shifted = model.binary(op::SHIFT_RIGHT_LOGICAL, word, shift);
            let bits = model.binary(op::BITWISE_AND, shifted, nibble);
            let any = model.compare(op::INOT_EQUAL, bits, zero);
            let bit = model.constant(1 << (first + group));
            let set = model.select(any, bit, zero);
            result = model.binary(op::BITWISE_OR, result, set);
        }
    }
    if let Operand::Named(name) = destination
        && let Some(mask) = lane_mask_name(name)
    {
        model.write_lane_mask(mask, result, zero)?;
    } else {
        let Operand::Scalar(register) = destination else {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "s_quadmask_b64 destination is neither a register pair nor a lane mask",
            });
        };
        let register = u32::from(*register);
        if register + 2 > SCALAR_REGISTERS {
            return Err(TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "s_quadmask_b64 runs past the end of the register file",
            });
        }
        model.write_scalar(register, result);
        model.write_scalar(register + 1, zero);
    }
    let condition = model.is_not_zero(result);
    model.set_condition_code(condition);
    model.count();
    Ok(())
}

/// Translates `s_wqm_b32`: whole quad mode on a 32-lane mask.
///
/// One register, so one [`quad_expand`], with `s_mov_b32`'s destination rules: `exec_lo` or
/// `vcc_lo` sets that mask's low half, anything else must be a scalar register. The GL context's
/// textured pixel shader runs it on `exec_lo` so every covered quad's helper pixels execute the
/// sample (oops-sdk `tools/shader/tex-prolog.s`).
fn whole_quad_mode_32<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
) -> Result<(), TranslateError> {
    let (destination, source) = two_operands(instruction)?;
    let value = model.read_source(instruction, source, 0)?;
    let expanded = quad_expand(model, value);
    if let Some(mask) = mask_destination(destination) {
        write_mask_low(model, mask, expanded)?;
        model.count();
        return Ok(());
    }
    let Operand::Scalar(register) = destination else {
        return Err(TranslateError::Unsupported {
            offset: instruction.offset,
            detail: "s_wqm_b32 destination is neither a scalar register nor a lane mask",
        });
    };
    model.write_scalar(u32::from(*register), expanded);
    model.count();
    Ok(())
}

/// Sets every bit of each four-bit group whose group contained any set bit.
///
/// Fold the group down to its lowest bit with two shift-and-or steps, mask to the group
/// starts, then multiply by 0b1111 to spread it back. The multiply cannot carry between
/// groups because only the low bit of each group survives the mask.
fn quad_expand<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let two = model.constant(2);
    let one = model.constant(1);
    let starts = model.constant(0x1111_1111);
    let spread = model.constant(0b1111);

    let shifted_two = model.binary(op::SHIFT_RIGHT_LOGICAL, value, two);
    let folded = model.binary(op::BITWISE_OR, value, shifted_two);
    let shifted_one = model.binary(op::SHIFT_RIGHT_LOGICAL, folded, one);
    let folded = model.binary(op::BITWISE_OR, folded, shifted_one);
    let lowest = model.binary(op::BITWISE_AND, folded, starts);
    model.binary(op::IMUL, lowest, spread)
}

/// Translates an instruction that reaches guest memory.
///
/// Reached only for admitted families, so the fallthrough is a translator bug; it still returns an
/// error rather than panicking.
fn memory<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    match name {
        // The scalar loads, which differ only in how many consecutive registers they fill.
        "s_load_dword" | "s_load_dwordx2" | "s_load_dwordx4" | "s_load_dwordx8"
        | "s_load_dwordx16" => {
            // The suffix says how many consecutive registers are filled; opcodes are consecutive
            // only on the generation they were numbered for.
            let words = access_words(name);
            let (destination, base, offset) = three_operands(instruction)?;
            let destinations = load_destinations(instruction, destination, words)?;
            let Operand::Scalar(base_register) = base else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a scalar load needs a scalar destination and a scalar base",
                });
            };
            let Operand::Immediate(bytes) = offset else {
                return Err(TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "a scalar load offset is not an immediate",
                });
            };
            let bytes = u32::try_from(*bytes).map_err(|_| TranslateError::Unsupported {
                offset: instruction.offset,
                detail: "negative load offset",
            })?;

            // A traced base is a draw buffer that starts at it (D733): the words are the offset's.
            if let Some(slot) = model.draw_buffer_slot(instruction) {
                // A scalar load's base pair is not placed from a sample (D951): it binds where
                // every wave reaches it with the one pair.
                if model.sampled_buffer(slot) {
                    return Err(TranslateError::Unsupported {
                        offset: instruction.offset,
                        detail: "a scalar load through a buffer placed from a sample (D951)",
                    });
                }
                for (word, destination) in (0..words).zip(&destinations) {
                    let index = model.constant(bytes / 4 + word);
                    let loaded = model.read_draw_buffer(slot, index)?;
                    write_scalar_operand(model, instruction, destination, loaded)?;
                }
                model.count();
                return Ok(());
            }
            let base_value = model.read_scalar(u32::from(*base_register));
            let offset_value = model.constant(bytes);
            let address = model.add(base_value, offset_value);
            let first = model.word_index(address);
            for (word, destination) in (0..words).zip(&destinations) {
                // Consecutive words, so consecutive indices: the address arithmetic is done once
                // and stepped.
                let index = if word == 0 {
                    first
                } else {
                    let step = model.constant(word);
                    model.add(first, step)
                };
                let loaded = model.read_memory(index);
                write_scalar_operand(model, instruction, destination, loaded)?;
            }
            model.count();
            Ok(())
        }

        "s_buffer_load_dword"
        | "s_buffer_load_dwordx2"
        | "s_buffer_load_dwordx4"
        | "s_buffer_load_dwordx8"
        | "s_buffer_load_dwordx16" => scalar_buffer_load(model, instruction, name),

        // Flat memory: a per-lane address rather than a uniform one.
        "global_load_dword"
        | "global_load_dwordx2"
        | "global_load_dwordx3"
        | "global_load_dwordx4"
        | "global_load_ubyte"
        | "global_load_sbyte"
        | "global_load_ushort"
        | "global_load_sshort"
        | "global_store_dword"
        | "global_store_dwordx2"
        | "global_store_dwordx3"
        | "global_store_dwordx4"
        | "global_store_byte"
        | "global_store_short" => flat_memory(model, instruction, name),

        _ => Err(TranslateError::NotTranslated {
            offset: instruction.offset,
            mnemonic: name.to_owned(),
            detail: NO_TRANSLATION,
        }),
    }
}

/// Where a scalar load's `words` land: consecutive registers from its destination, or a lane mask's
/// low half and then its high one (`s_load_dwordx2 vcc, ...`). A destination running off the end of
/// the register file is refused rather than truncated: the registers stop at 105 and the wide forms
/// take up to sixteen.
fn load_destinations(
    instruction: &Instruction,
    destination: &Operand,
    words: u32,
) -> Result<Vec<Operand>, TranslateError> {
    let refuse = |detail| TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    };
    match destination {
        Operand::Scalar(register) => {
            if u32::from(*register) + words > SCALAR_REGISTERS {
                return Err(refuse(
                    "a scalar load runs past the end of the register file",
                ));
            }
            Ok((0..words)
                .map(|word| Operand::Scalar(*register + u16::try_from(word).unwrap_or(0)))
                .collect())
        }
        Operand::Named(name) if words <= 2 && lane_mask_name(name).is_some() => {
            let high = name.replace("_lo", "_hi");
            Ok([Operand::Named(name.clone()), Operand::Named(high)]
                .into_iter()
                .take(words as usize)
                .collect())
        }
        // `vcc_hi` is scalar register 107 (`src/amd/compiler/aco_ir.h:441`), a destination like any other
        // for one word: `s_load_dword vcc_hi, s[2:3], 0x84`. A wider load would run past it into
        // the trap registers.
        Operand::Named(name) if words == 1 && lane_mask_high_name(name) == Some(VCC_LOW_HALF) => {
            Ok(vec![Operand::Named(name.clone())])
        }
        _ => Err(refuse(
            "a scalar load needs a scalar destination and a scalar base",
        )),
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_shader::{EncodingTable, OperandTable, decode};

    /// An image access's address registers: consecutive from the first, or listed a byte each
    /// after the fixed part. The listed form is the reference assembler's `image_sample_l v0,
    /// [v2, v1, v0]` (`aco_assembler.cpp:813-821` lays them out so); the consecutive one its
    /// `v[0:2]`.
    #[test]
    fn an_access_reads_its_listed_address_registers() {
        let encodings = EncodingTable::builtin().expect("the shipped encoding table");
        let operands = OperandTable::builtin().expect("the shipped operand table");
        let listed = [
            0x0a, 0x01, 0x90, 0xf0, 0x02, 0x00, 0x61, 0x00, 0x01, 0x00, 0x00, 0x00,
        ];
        let consecutive = [0x08, 0x01, 0x90, 0xf0, 0x00, 0x00, 0x61, 0x00];
        let first = |bytes: &[u8]| decode(bytes, &encodings, &operands).instructions[0].clone();
        assert_eq!(
            super::address_list(&first(&listed), 2, 3),
            Some([2, 1, 0, 2, 2, 2])
        );
        assert_eq!(
            super::address_list(&first(&consecutive), 0, 3),
            Some([0, 1, 2, 0, 0, 0])
        );
        assert_eq!(
            super::address_list(&first(&listed[..8]), 2, 3),
            None,
            "a listed register the instruction does not hold"
        );
    }
}
