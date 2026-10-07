//! The scalar registers a program prefix leaves, computed on the host for one draw (D753).
//!
//! A descriptor a program computes from words it reads from memory has no static source, but every
//! input is uniform for the draw. The prefix up to the access runs here over the draw's user data and
//! guest memory. Each scalar instruction goes through the translator's own semantics
//! ([`model::instruction`]), driven by a model whose values are numbers. The SPIR-V those semantics
//! emit is read back an instruction at a time and evaluated, so nothing about an instruction is
//! restated here. A value the host cannot know is unknown, and anything computed from it is
//! unknown: a vector result written to a scalar, the execution mask at entry (which differs from
//! wave to wave), and an operation this reader does not evaluate.

use std::collections::HashMap;

use orbistoun_shader::{Decode, EncodingTable, Instruction, Operand};
use orbistoun_spirv::{Builder, Id, op};

use crate::TranslateError;
use crate::model::{self, Model};

/// Scalar register codes as an operand field numbers them: `s0`-`s105`, the condition mask at 106,
/// `m0` at 124 and the execution mask at 126.
pub const SCALAR_CODES: usize = 128;
const VCC: usize = 106;
const M0: usize = 124;
const EXEC: usize = 126;
/// The scalar offset field's "no offset" code.
const SOFFSET_NULL: u32 = 0x7d;

/// `OpIAddCarry`, `OpISubBorrow` and `OpUMulExtended` (SPIR-V 3.42.13): two-member results.
const IADD_CARRY: u16 = 149;
const ISUB_BORROW: u16 = 150;
const UMUL_EXTENDED: u16 = 151;
/// `OpBitFieldInsert`, `OpBitFieldSExtract` and `OpBitFieldUExtract` (SPIR-V 3.42.14).
const BIT_FIELD_INSERT: u16 = 201;
const BIT_FIELD_SEXTRACT: u16 = 202;
const BIT_FIELD_UEXTRACT: u16 = 203;
/// `OpULessThanEqual` and `OpLogicalEqual`/`OpLogicalNotEqual` (SPIR-V 3.42.15).
const ULESS_THAN_EQUAL: u16 = 178;
const LOGICAL_EQUAL: u16 = 164;
const LOGICAL_NOT_EQUAL: u16 = 165;

/// What each scalar register code holds after a prefix, where the host can know it.
pub type Scalars = [Option<u32>; SCALAR_CODES];

/// Runs `decode`'s instructions before byte `until` over a stage whose user-data words land from
/// scalar register `first_register`, reading guest memory word by word through `read`.
///
/// # Errors
///
/// A reason, where the prefix branches: control flow past a branch is not followed.
pub fn scalar_prefix(
    decode: &Decode,
    encodings: &EncodingTable,
    until: u32,
    (first_register, user_data): (u32, &[u32]),
    read: &mut dyn FnMut(u64) -> Option<u32>,
) -> Result<Scalars, &'static str> {
    let mut model = Concrete::new(encodings, decode.instructions.len());
    for (register, word) in (first_register as usize..).zip(user_data) {
        if register < model::SCALAR_REGISTERS as usize {
            model.scalars[register] = Some(*word);
        }
    }
    for instruction in decode
        .instructions
        .iter()
        .take_while(|instruction| instruction.offset < until)
    {
        let Some((family, name)) = named(encodings, instruction) else {
            return Err("an instruction before the access has no name (D753)");
        };
        match family {
            "SOPP" if branches(name) => {
                return Err("the program branches before the access, which is not followed (D753)");
            }
            "SOPP" => {}
            "SMEM" => model.scalar_load(instruction, name, read),
            "SOP1" | "SOP2" | "SOPK" | "SOPC" => {
                if name.starts_with("s_setpc")
                    || name.starts_with("s_swappc")
                    || name.starts_with("s_cbranch")
                {
                    return Err(
                        "the program branches before the access, which is not followed (D753)",
                    );
                }
                if model::instruction(&mut model, instruction).is_err() {
                    model.forget_written(instruction, name);
                }
                model.catch_up();
            }
            _ => model.forget_vector_writes(instruction, name),
        }
    }
    Ok(model.scalars)
}

/// An instruction's family and mnemonic.
fn named<'a>(
    encodings: &'a EncodingTable,
    instruction: &Instruction,
) -> Option<(&'a str, &'a str)> {
    let family = encodings
        .encodings()
        .get(usize::from(instruction.encoding?))?;
    Some((
        family.name.as_str(),
        encodings.mnemonic_for(&family.name, instruction.opcode)?,
    ))
}

/// Whether a program-control instruction leaves the straight line.
fn branches(name: &str) -> bool {
    name.starts_with("s_branch")
        || name.starts_with("s_cbranch")
        || name.starts_with("s_setpc")
        || name.starts_with("s_endpgm")
}

/// A value the SPIR-V reader holds for an id.
#[derive(Debug, Clone)]
enum Value {
    Word(u32),
    Bool(bool),
    Pair(u32, u32),
}

/// The model the prefix runs through: registers that hold numbers, and a builder whose output is
/// read back and evaluated.
struct Concrete<'a> {
    encodings: &'a EncodingTable,
    builder: Builder,
    /// Values by id; an id with none is unknown.
    values: HashMap<u32, Value>,
    /// How far into the builder's function section has been evaluated.
    read: usize,
    scalars: Scalars,
    condition: Option<u32>,
    instructions: usize,
    types: [Id; 6],
    pointers: [Id; 4],
}

/// Indices into [`Concrete::types`].
const U32: usize = 0;
const F32: usize = 1;
const BOOL: usize = 2;
const F16: usize = 3;
const U16: usize = 4;
const GLSL: usize = 5;
/// Indices into [`Concrete::pointers`].
const CONDITION: usize = 0;
const COUNTER: usize = 1;
const MEMORY: usize = 2;
const ELEMENT: usize = 3;

impl<'a> Concrete<'a> {
    fn new(encodings: &'a EncodingTable, instructions: usize) -> Self {
        let mut builder = Builder::new();
        let types = std::array::from_fn(|_| builder.id());
        let pointers = std::array::from_fn(|_| builder.id());
        Self {
            encodings,
            builder,
            values: HashMap::new(),
            read: 0,
            scalars: [None; SCALAR_CODES],
            condition: None,
            instructions,
            types,
            pointers,
        }
    }

    /// A fresh id holding `value`, or unknown for `None`.
    fn hold(&mut self, value: Option<u32>) -> Id {
        let id = self.builder.id();
        if let Some(value) = value {
            self.values.insert(id.0, Value::Word(value));
        }
        id
    }

    /// The number an id holds, evaluating what was emitted since the last look.
    fn word(&mut self, id: Id) -> Option<u32> {
        self.catch_up();
        match self.values.get(&id.0)? {
            Value::Word(value) => Some(*value),
            Value::Bool(value) => Some(u32::from(*value)),
            Value::Pair(..) => None,
        }
    }

    /// Evaluates every instruction emitted since the last call, in order.
    fn catch_up(&mut self) {
        while self.read < self.builder.function_words().len() {
            let words = self.builder.function_words();
            let head = words[self.read];
            let count = (head >> 16) as usize;
            if count == 0 || self.read + count > words.len() {
                self.read = words.len();
                return;
            }
            let operands: Vec<u32> = words[self.read + 1..self.read + count].to_vec();
            self.read += count;
            self.evaluate((head & 0xffff) as u16, &operands);
        }
    }

    /// Evaluates one emitted instruction. One this does not know leaves its result unknown.
    fn evaluate(&mut self, opcode: u16, operands: &[u32]) {
        if opcode == op::STORE {
            if let [pointer, value] = operands
                && *pointer == self.pointers[CONDITION].0
            {
                self.condition = self.number(*value);
            }
            return;
        }
        let [_, result, arguments @ ..] = operands else {
            return;
        };
        if let Some(value) = self.compute(opcode, arguments) {
            self.values.insert(*result, value);
        }
    }

    fn number(&self, id: u32) -> Option<u32> {
        match self.values.get(&id)? {
            Value::Word(value) => Some(*value),
            Value::Bool(value) => Some(u32::from(*value)),
            Value::Pair(..) => None,
        }
    }

    fn truth(&self, id: u32) -> Option<bool> {
        match self.values.get(&id)? {
            Value::Bool(value) => Some(*value),
            Value::Word(value) => Some(*value != 0),
            Value::Pair(..) => None,
        }
    }

    fn compute(&self, opcode: u16, arguments: &[u32]) -> Option<Value> {
        let word = |at: usize| arguments.get(at).and_then(|id| self.number(*id));
        let truth = |at: usize| arguments.get(at).and_then(|id| self.truth(*id));
        let signed = |value: u32| i32::from_ne_bytes(value.to_ne_bytes());
        Some(match opcode {
            op::LOAD => {
                if arguments.first() == Some(&self.pointers[CONDITION].0) {
                    Value::Word(self.condition?)
                } else {
                    return None;
                }
            }
            op::BITCAST => self.values.get(arguments.first()?)?.clone(),
            op::IADD => Value::Word(word(0)?.wrapping_add(word(1)?)),
            op::ISUB => Value::Word(word(0)?.wrapping_sub(word(1)?)),
            op::IMUL => Value::Word(word(0)?.wrapping_mul(word(1)?)),
            op::UDIV => Value::Word(word(0)?.checked_div(word(1)?)?),
            op::SHIFT_LEFT_LOGICAL => Value::Word(word(0)?.checked_shl(word(1)?)?),
            op::SHIFT_RIGHT_LOGICAL => Value::Word(word(0)?.checked_shr(word(1)?)?),
            op::SHIFT_RIGHT_ARITHMETIC => Value::Word(u32::from_ne_bytes(
                signed(word(0)?).checked_shr(word(1)?)?.to_ne_bytes(),
            )),
            op::BITWISE_AND => Value::Word(word(0)? & word(1)?),
            op::BITWISE_OR => Value::Word(word(0)? | word(1)?),
            op::BITWISE_XOR => Value::Word(word(0)? ^ word(1)?),
            op::NOT => Value::Word(!word(0)?),
            op::BIT_COUNT => Value::Word(word(0)?.count_ones()),
            op::BIT_REVERSE => Value::Word(word(0)?.reverse_bits()),
            BIT_FIELD_UEXTRACT | BIT_FIELD_SEXTRACT => {
                let (value, offset, count) = (word(0)?, word(1)?, word(2)?);
                if count == 0 {
                    Value::Word(0)
                } else if offset.checked_add(count)? > 32 {
                    return None;
                } else {
                    let shifted = value >> offset;
                    let field = shifted & (u32::MAX >> (32 - count));
                    let top = 1u32 << (count - 1);
                    if opcode == BIT_FIELD_SEXTRACT && field & top != 0 {
                        Value::Word(field | !(u32::MAX >> (32 - count)))
                    } else {
                        Value::Word(field)
                    }
                }
            }
            BIT_FIELD_INSERT => {
                let (base, insert, offset, count) = (word(0)?, word(1)?, word(2)?, word(3)?);
                if count == 0 {
                    Value::Word(base)
                } else if offset.checked_add(count)? > 32 {
                    return None;
                } else {
                    let mask = (u32::MAX >> (32 - count)) << offset;
                    Value::Word((base & !mask) | ((insert << offset) & mask))
                }
            }
            op::SELECT => {
                let chosen = if truth(0)? { 1 } else { 2 };
                self.values.get(arguments.get(chosen)?)?.clone()
            }
            op::IEQUAL => Value::Bool(word(0)? == word(1)?),
            op::INOT_EQUAL => Value::Bool(word(0)? != word(1)?),
            op::ULESS_THAN => Value::Bool(word(0)? < word(1)?),
            ULESS_THAN_EQUAL => Value::Bool(word(0)? <= word(1)?),
            op::UGREATER_THAN => Value::Bool(word(0)? > word(1)?),
            op::UGREATER_THAN_EQUAL => Value::Bool(word(0)? >= word(1)?),
            op::SLESS_THAN => Value::Bool(signed(word(0)?) < signed(word(1)?)),
            op::SLESS_THAN_EQUAL => Value::Bool(signed(word(0)?) <= signed(word(1)?)),
            op::SGREATER_THAN => Value::Bool(signed(word(0)?) > signed(word(1)?)),
            op::SGREATER_THAN_EQUAL => Value::Bool(signed(word(0)?) >= signed(word(1)?)),
            op::LOGICAL_OR => Value::Bool(truth(0)? || truth(1)?),
            op::LOGICAL_AND => Value::Bool(truth(0)? && truth(1)?),
            op::LOGICAL_NOT => Value::Bool(!truth(0)?),
            LOGICAL_EQUAL => Value::Bool(truth(0)? == truth(1)?),
            LOGICAL_NOT_EQUAL => Value::Bool(truth(0)? != truth(1)?),
            IADD_CARRY => {
                let (sum, carried) = word(0)?.overflowing_add(word(1)?);
                Value::Pair(sum, u32::from(carried))
            }
            ISUB_BORROW => {
                let (difference, borrowed) = word(0)?.overflowing_sub(word(1)?);
                Value::Pair(difference, u32::from(borrowed))
            }
            UMUL_EXTENDED => {
                let product = u64::from(word(0)?) * u64::from(word(1)?);
                Value::Pair(product as u32, (product >> 32) as u32)
            }
            op::COMPOSITE_EXTRACT => match (self.values.get(arguments.first()?)?, arguments.get(1))
            {
                (Value::Pair(low, _), Some(0)) => Value::Word(*low),
                (Value::Pair(_, high), Some(1)) => Value::Word(*high),
                _ => return None,
            },
            _ => return None,
        })
    }

    /// A scalar load from memory: `s_load_dword*` reads the words at the base pair, its immediate
    /// and its scalar offset; any other scalar memory instruction leaves its destination unknown.
    fn scalar_load(
        &mut self,
        instruction: &Instruction,
        name: &str,
        read: &mut dyn FnMut(u64) -> Option<u32>,
    ) {
        let words = match name.rsplit_once("dwordx") {
            Some((_, count)) => count.parse().unwrap_or(16),
            None => 1,
        };
        let [
            Operand::Scalar(destination),
            Operand::Scalar(base),
            Operand::Immediate(offset),
            ..,
        ] = instruction.operands.as_slice()
        else {
            if let Some(Operand::Scalar(destination)) = instruction.operands.first() {
                self.forget(usize::from(*destination), 16);
            }
            return;
        };
        let destination = usize::from(*destination);
        let soffset = match instruction.second_word.map(|second| (second >> 25) & 0x7f) {
            Some(SOFFSET_NULL) | None => Some(0),
            Some(code) => self.scalars.get(code as usize).copied().flatten(),
        };
        let base = usize::from(*base);
        let address = name
            .starts_with("s_load_dword")
            .then_some(())
            .and(self.scalars[base].zip(self.scalars.get(base + 1).copied().flatten()))
            .zip(soffset)
            .map(|((low, high), soffset)| {
                let base = (u64::from(high & 0xffff) << 32) | u64::from(low);
                base.wrapping_add_signed(*offset)
                    .wrapping_add(u64::from(soffset))
                    & !3
            });
        for step in 0..words {
            let value = address.and_then(|address| read(address + 4 * step as u64));
            if let Some(slot) = self.scalars.get_mut(destination + step) {
                *slot = value;
            }
        }
    }

    /// Forgets `count` registers from `first`.
    fn forget(&mut self, first: usize, count: usize) {
        for slot in self.scalars.iter_mut().skip(first).take(count) {
            *slot = None;
        }
    }

    /// Forgets what an instruction the semantics could not run may have written: its destination,
    /// as wide as a pair, and the condition code.
    fn forget_written(&mut self, instruction: &Instruction, name: &str) {
        if name.starts_with("s_movrel") {
            self.scalars = [None; SCALAR_CODES];
        }
        self.forget_operand(instruction.operands.first());
        if model::writes_condition_code(name) || name.starts_with("s_cmp") {
            self.condition = None;
        }
    }

    /// Forgets the scalars a vector instruction writes: a scalar or mask destination in its first
    /// two operands (a compare's mask, a carry-out, a lane read), and the execution mask for a
    /// compare into it.
    fn forget_vector_writes(&mut self, instruction: &Instruction, name: &str) {
        for operand in instruction.operands.iter().take(2) {
            self.forget_operand(Some(operand));
        }
        if name.starts_with("v_cmpx") {
            self.forget(EXEC, 2);
        }
        if name.starts_with("v_cmp") {
            self.forget(VCC, 2);
        }
    }

    fn forget_operand(&mut self, operand: Option<&Operand>) {
        match operand {
            Some(Operand::Scalar(register)) => self.forget(usize::from(*register), 2),
            Some(Operand::Named(name)) => {
                if let Some(code) = mask_code(name) {
                    self.forget(code, 2);
                } else if name == model::M0 {
                    self.forget(M0, 1);
                } else if let Some(high) = model::lane_mask_high_name(name).and_then(mask_code) {
                    self.forget(high + 1, 1);
                }
            }
            _ => {}
        }
    }
}

/// The register code of a lane mask's low half, by any spelling of it.
fn mask_code(name: &str) -> Option<usize> {
    match model::lane_mask_name(name)? {
        model::EXEC_LOW_HALF => Some(EXEC),
        model::VCC_LOW_HALF => Some(VCC),
        _ => None,
    }
}

impl Model for Concrete<'_> {
    fn encodings(&self) -> &EncodingTable {
        self.encodings
    }

    fn lanes(&self) -> u32 {
        64
    }

    fn constant(&mut self, value: u32) -> Id {
        self.hold(Some(value))
    }

    fn read_source(
        &mut self,
        instruction: &Instruction,
        operand: &Operand,
        _lane: u32,
    ) -> Result<Id, TranslateError> {
        Ok(match operand {
            Operand::Integer(value) => {
                let value = i32::try_from(*value).map_err(|_| TranslateError::Unsupported {
                    offset: instruction.offset,
                    detail: "inline constant does not fit in a register",
                })?;
                self.hold(Some(u32::from_ne_bytes(value.to_ne_bytes())))
            }
            Operand::Literal(value) => self.hold(Some(*value)),
            Operand::Scalar(register) => self.read_scalar(u32::from(*register)),
            Operand::Named(name) => {
                if let Some(code) = mask_code(name) {
                    self.hold(self.scalars[code])
                } else if let Some(code) = model::lane_mask_high_name(name).and_then(mask_code) {
                    self.hold(self.scalars[code + 1])
                } else if name == model::M0 {
                    self.hold(self.scalars[M0])
                } else {
                    self.hold(name.parse::<f32>().ok().map(f32::to_bits))
                }
            }
            _ => self.hold(None),
        })
    }

    fn write_vector_lane(&mut self, _register: u32, _lane: u32, _value: Id) {}

    fn write_scalar(&mut self, register: u32, value: Id) {
        let value = self.word(value);
        if let Some(slot) = self.scalars.get_mut(register as usize) {
            *slot = value;
        }
    }

    fn count(&mut self) {}

    fn builder(&mut self) -> &mut Builder {
        &mut self.builder
    }

    fn u32_type(&self) -> Id {
        self.types[U32]
    }

    fn f32_type(&self) -> Id {
        self.types[F32]
    }

    fn glsl_set(&mut self) -> Id {
        self.types[GLSL]
    }

    fn f16_type(&mut self) -> Id {
        self.types[F16]
    }

    fn u16_type(&mut self) -> Id {
        self.types[U16]
    }

    fn read_local(&mut self, _word_index: Id) -> Result<Id, TranslateError> {
        Ok(self.hold(None))
    }

    fn write_local(
        &mut self,
        _word_index: Id,
        _value: Id,
        _lane: u32,
    ) -> Result<(), TranslateError> {
        Ok(())
    }

    fn memory_buffer(&self) -> Id {
        self.pointers[MEMORY]
    }

    fn memory_element_ptr(&self) -> Id {
        self.pointers[ELEMENT]
    }

    fn read_scalar(&mut self, register: u32) -> Id {
        let value = self.scalars.get(register as usize).copied().flatten();
        self.hold(value)
    }

    fn read_lane_mask(&mut self, name: &str) -> Result<(Id, Id), TranslateError> {
        let code = mask_code(name).unwrap_or(VCC);
        let (low, high) = (self.scalars[code], self.scalars[code + 1]);
        Ok((self.hold(low), self.hold(high)))
    }

    fn write_lane_mask(&mut self, name: &str, low: Id, high: Id) -> Result<(), TranslateError> {
        let code = mask_code(name).unwrap_or(VCC);
        self.scalars[code] = self.word(low);
        self.scalars[code + 1] = self.word(high);
        Ok(())
    }

    fn write_memory(&mut self, _word_index: Id, _value: Id, _lane: u32) {}

    fn memory_words(&self) -> u32 {
        0
    }

    fn bool_type(&mut self) -> Id {
        self.types[BOOL]
    }

    fn condition_code(&mut self) -> Id {
        // A read emits a load of this pointer, evaluated in order with the stores before it.
        self.catch_up();
        self.pointers[CONDITION]
    }

    fn read_m0(&mut self) -> Id {
        self.hold(self.scalars[M0])
    }

    fn write_m0(&mut self, value: Id) {
        self.scalars[M0] = self.word(value);
    }

    fn program_counter(&mut self) -> Id {
        self.pointers[COUNTER]
    }

    fn instructions(&self) -> usize {
        self.instructions
    }
}

#[cfg(test)]
mod tests {
    use super::{VCC, scalar_prefix};
    use orbistoun_shader::{EncodingTable, OperandTable, decode_program};
    use std::collections::BTreeMap;

    /// A fetch shader's prefix, as PPSA28061's vertex shader forms its vertex descriptor: a word
    /// from the table at `s[10:11]` picks the descriptor's place in the table at `s[8:9]`, and the
    /// descriptor's fourth word is rebuilt from a constant table under a condition.
    const FETCH: [u32; 20] = [
        0xf404_0105,
        0xfa00_0000, // s_load_dwordx2 s[4:5], s[10:11], 0x0
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0x93ea_ff04,
        0x0002_0005, // s_bfe_u32 vcc_lo, s4, 0x20005
        0x98eb_ff6a,
        0x000c_0000, // s_lshl4_add_u32 vcc_hi, vcc_lo, 0xc0000
        0xf408_0004,
        0xd600_0000, // s_load_dwordx4 s[0:3], s[8:9], vcc_hi
        0xbf8c_c07f, // s_waitcnt lgkmcnt(0)
        0xbe86_03ff,
        0x022c_0204, // s_mov_b32 s6, 0x022c0204
        0xbe87_03ff,
        0x0fac_03ac, // s_mov_b32 s7, 0x0fac03ac
        0x948c_ff06,
        0x0008_0004, // s_bfe_u64 s[12:13], s[6:7], 0x80004
        0xbf06_8004, // s_cmp_eq_u32 s4, 0
        0x8503_0c03, // s_cselect_b32 s3, s3, s12
        0xe00c_2000,
        0x6b00_0000, // buffer_load_format_xyzw v0, v0, s[0:3], vcc_hi idxen
    ];

    #[test]
    fn a_computed_descriptor_is_evaluated_from_the_draws_memory() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        let bytes: Vec<u8> = FETCH.iter().flat_map(|w| w.to_le_bytes()).collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let access = decode.instructions.last().expect("the access").offset;

        let (index_table, descriptors) = (0x1000_0000u64, 0x2000_0000u64);
        // Bits 6:5 of the first word say 2, so the descriptor is at 0xc0000 + 2 * 16.
        let mut memory = BTreeMap::from([(index_table, 0x40), (index_table + 4, 0x7)]);
        for (step, word) in (0..).zip([0x3000_0000, 0x0010_0000, 100, 0x0003_0fac]) {
            memory.insert(descriptors + 0xc_0020 + 4 * step, word);
        }
        let user_data = [
            descriptors as u32,
            (descriptors >> 32) as u32,
            index_table as u32,
            (index_table >> 32) as u32,
        ];
        let scalars = scalar_prefix(
            &decode,
            &encodings,
            access,
            (8, &user_data),
            &mut |address| memory.get(&address).copied(),
        )
        .expect("a straight-line prefix");
        assert_eq!(scalars[VCC + 1], Some(0xc_0020), "the descriptor's place");
        assert_eq!(
            scalars[0..3],
            [Some(0x3000_0000), Some(0x0010_0000), Some(100)]
        );
        // s4 is non-zero, so the condition is clear and the fourth word is bits 11:4 of the table.
        assert_eq!(scalars[3], Some(0x20));
    }

    /// A value the host cannot know stays unknown: the execution mask at entry, and so anything
    /// read from memory at an address made from it.
    #[test]
    fn what_depends_on_the_wave_is_unknown() {
        let encodings = EncodingTable::builtin().expect("encodings");
        let operands = OperandTable::builtin().expect("operands");
        // s_mov_b32 s4, exec_lo; s_endpgm
        let bytes: Vec<u8> = [0xbe84_037e_u32, 0xbf81_0000]
            .iter()
            .flat_map(|w| w.to_le_bytes())
            .collect();
        let decode = decode_program(&bytes, &encodings, &operands);
        let scalars = scalar_prefix(&decode, &encodings, 4, (8, &[]), &mut |_| Some(0))
            .expect("a straight-line prefix");
        assert_eq!(scalars[4], None);
    }
}
