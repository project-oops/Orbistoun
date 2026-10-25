// ---- hardware registers ----------------------------------------------------------
//
// `s_setreg_b32`, `s_setreg_imm32_b32` and `s_getreg_b32` name a hardware register, a bit offset
// in it and a width as one sixteen-bit field (`hwreg(id, offset, size)`), with the scalar register
// read or written beside it. Grand Theft Auto V's ray-traversal programs set the flat scratch base
// with `s_setreg_b32` at entry, which no probe had named, so the instruction had no operand layout.
//
// The field's values spread across its sixteen bits - the id in 5:0, the offset in 10:6, the
// size less one in 15:11 - and the scalar registers reach the top of the file, so neither width
// can be guessed short.
s_setreg_b32 hwreg(HW_REG_FLAT_SCR_LO), s96
s_setreg_b32 hwreg(HW_REG_FLAT_SCR_HI), s97
s_setreg_b32 hwreg(HW_REG_MODE, 0, 4), s5
s_setreg_b32 hwreg(63, 31, 32), s101
s_setreg_b32 hwreg(42, 17, 9), vcc_hi
s_setreg_b32 hwreg(1, 23, 1), s0
s_getreg_b32 s5, hwreg(HW_REG_MODE, 0, 4)
s_getreg_b32 s101, hwreg(63, 31, 32)
s_getreg_b32 vcc_hi, hwreg(42, 17, 9)
s_getreg_b32 s64, hwreg(HW_REG_FLAT_SCR_LO)
//
// The value `s_setreg_imm32_b32` writes is the word after it. The reference prints a value with
// its top bit set as a negative number, so the samples keep it clear.
s_setreg_imm32_b32 hwreg(HW_REG_MODE, 0, 4), 0x5a5a5a5b
s_setreg_imm32_b32 hwreg(63, 31, 32), 0x7fffffff
s_setreg_imm32_b32 hwreg(42, 17, 9), 0x12345678
