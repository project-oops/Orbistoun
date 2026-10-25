//! Guest memory a device-addressed dispatch reaches by device address (D964).
//!
//! A guest address is turned into a device address through the table the host keeps
//! ([`crate::device_address`]): the address's chunk is hashed to a slot, the next [`PROBES`] slots
//! are searched for it, and the slot's delta added. An access the table does not hold reads zero,
//! stores nothing and is an escape; it is pointed at the table's header instead, so no access
//! needs a branch to stay off memory nothing maps. A store branches only on whether it lands.
//!
//! A flat access routes by aperture first: its address's high half [`LOCAL_APERTURE_HIGH`] reaches
//! the local data share, [`PRIVATE_APERTURE_HIGH`] the lane's private memory, and any other value
//! guest memory. A global access always reaches guest memory.

use orbistoun_shader::{Instruction, Operand};
use orbistoun_spirv::{Id, MEMORY_ALIGNED, MEMORY_VOLATILE, op, scope};

use super::{
    BufferResource, EXEC_LOW_HALF, FLAT_NO_BASE, Model, SCALAR_REGISTERS, VECTOR_REGISTERS,
    access_words, atomic_index_and_offset, buffer_out_of_bounds_placed, close_if_active,
    first_three_operands, float_atomic_parts, load_destinations, open_if_active, pick,
    read_buffer_resource, running_format, running_lanes, scalar_offset_register, step_operand,
    three_operands, typed_channels, write_scalar_operand,
};
use crate::TranslateError;
use crate::device_address::{
    CHUNK_HASH, DeviceTable, HEADER_WORDS, LOCAL_APERTURE_HIGH, MARK_SINK_WORDS,
    PRIVATE_APERTURE_HIGH, PROBES, SLOT_WORDS,
};

fn refuse(instruction: &Instruction, detail: &'static str) -> TranslateError {
    TranslateError::Unsupported {
        offset: instruction.offset,
        detail,
    }
}

/// Carries out `instruction` by device address where the module is device-addressed and the access
/// is one that reaches guest memory so, answering whether it did: a global or flat access, and a
/// scalar, untyped or format buffer access no bound buffer serves. Anything else - an access
/// through a bound buffer, the local data share, an image - is left to the module's other paths.
///
/// # Errors
///
/// An access of a form not translated by device address.
pub(super) fn access<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<bool, TranslateError> {
    if !model.device_addressed() {
        return Ok(false);
    }
    if name.starts_with("global_load_") || name.starts_with("global_store_") {
        flat(model, instruction, name)?;
        return Ok(true);
    }
    // An image descriptor loaded from a table still names the image the host binds from it: an
    // image instruction binds by the load reaching it, not by its registers' values. Its words are
    // read too, since they are the guest's and the program may read them as values - a ray
    // traversal tests a word of a sampled image's descriptor, and takes a 4-word hierarchy
    // descriptor and a table base from the 8 or 16 words a load fills (D970). Through the buffer
    // bound over its table where it has one, as any scalar load; where they lie otherwise, below.
    if model.draw_buffer_slot(instruction).is_some() {
        return Ok(false);
    }
    if name.starts_with("s_load_dword") {
        scalar_load(model, instruction, name)?;
    } else if name.starts_with("s_buffer_load_dword") {
        // Reads by device address where it has no bound buffer ([`read_from_base`]).
        super::scalar_buffer_load(model, instruction, name)?;
    } else if name.starts_with("buffer_atomic_") {
        buffer_atomic(model, instruction, name)?;
    } else if matches!(
        name,
        "buffer_load_ushort" | "buffer_store_byte" | "buffer_store_short"
    ) || name.starts_with("buffer_load_dword")
        || name.starts_with("buffer_store_dword")
    {
        buffer(model, instruction, name)?;
    } else if name.starts_with("buffer_load_format_") {
        // Converted by the format its descriptor names as the module runs (D967).
        format_load(model, instruction, name)?;
    } else {
        return Ok(false);
    }
    Ok(true)
}

fn table<M: Model + ?Sized>(model: &mut M, offset: u32) -> Result<DeviceTable, TranslateError> {
    model.device_table().ok_or(TranslateError::Unsupported {
        offset,
        detail: "a device-addressed access in a module with no device-address table (D964)",
    })
}

/// A 64-bit operation.
fn wide_op<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, opcode: u16, a: Id, b: Id) -> Id {
    let builder = model.builder();
    let result = builder.id();
    builder.function(opcode, &[t.u64_type.0, result.0, a.0, b.0]);
    result
}

/// A 64-bit constant.
fn wide_constant<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, value: u64) -> Id {
    #[expect(
        clippy::cast_possible_truncation,
        reason = "the halves of a 64-bit word"
    )]
    let halves = [value as u32, (value >> 32) as u32];
    model.builder().declare_constant(t.u64_type, &halves)
}

/// A 32-bit value zero-extended to 64 bits.
fn widen<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, value: Id) -> Id {
    let builder = model.builder();
    let result = builder.id();
    builder.function(op::UCONVERT, &[t.u64_type.0, result.0, value.0]);
    result
}

/// A 64-bit value's low 32 bits.
fn narrow<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let u32_type = model.u32_type();
    let builder = model.builder();
    let result = builder.id();
    builder.function(op::UCONVERT, &[u32_type.0, result.0, value.0]);
    result
}

/// A 64-bit value's high 32 bits.
fn high_half<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, value: Id) -> Id {
    let thirty_two = model.constant(32);
    let shifted = wide_op(model, t, op::SHIFT_RIGHT_LOGICAL, value, thirty_two);
    narrow(model, shifted)
}

/// The 64-bit value whose halves are `low` and `high`.
fn wide<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, (low, high): (Id, Id)) -> Id {
    let low = widen(model, t, low);
    let high = widen(model, t, high);
    let thirty_two = model.constant(32);
    let up = wide_op(model, t, op::SHIFT_LEFT_LOGICAL, high, thirty_two);
    wide_op(model, t, op::BITWISE_OR, up, low)
}

/// `condition ? when_true : when_false` on 64-bit values.
fn wide_select<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    condition: Id,
    when_true: Id,
    when_false: Id,
) -> Id {
    let builder = model.builder();
    let result = builder.id();
    builder.function(
        op::SELECT,
        &[
            t.u64_type.0,
            result.0,
            condition.0,
            when_true.0,
            when_false.0,
        ],
    );
    result
}

fn logical_not<M: Model + ?Sized>(model: &mut M, value: Id) -> Id {
    let bool_type = model.bool_type();
    let builder = model.builder();
    let result = builder.id();
    builder.function(op::LOGICAL_NOT, &[bool_type.0, result.0, value.0]);
    result
}

/// A pointer to the word at device address `address`.
fn pointer<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> Id {
    let builder = model.builder();
    let result = builder.id();
    builder.function(
        op::CONVERT_U_TO_PTR,
        &[t.word_pointer.0, result.0, address.0],
    );
    result
}

/// The guest's word at device address `address`, read each time the load runs: another workgroup,
/// or the CPU, may write it meanwhile, and a program polling it - as a single-pass scan waits on
/// the group before it - waits forever on a read the driver keeps (D964). A bound guest buffer is
/// decorated `Volatile` for the same reason.
fn load_guest_word<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> Id {
    load_with(model, t, address, MEMORY_ALIGNED | MEMORY_VOLATILE)
}

/// The word at device address `address`, loaded with the memory operands `access`, aligned to 4.
fn load_with<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id, access: u32) -> Id {
    let at = pointer(model, t, address);
    let u32_type = model.u32_type();
    let builder = model.builder();
    let value = builder.id();
    builder.function(op::LOAD, &[u32_type.0, value.0, at.0, access, 4]);
    value
}

/// The four words of the table slot at device address `address`, one load of sixteen bytes. The
/// table changes only between dispatches, so the load may be cached and kept.
fn load_slot<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> [Id; 4] {
    let builder = model.builder();
    let at = builder.id();
    builder.function(op::CONVERT_U_TO_PTR, &[t.slot_pointer.0, at.0, address.0]);
    let slot = builder.id();
    builder.function(
        op::LOAD,
        &[t.slot_type.0, slot.0, at.0, MEMORY_ALIGNED, SLOT_WORDS * 4],
    );
    let u32_type = model.u32_type();
    [0, 1, 2, 3].map(|component| {
        let builder = model.builder();
        let word = builder.id();
        builder.function(
            op::COMPOSITE_EXTRACT,
            &[u32_type.0, word.0, slot.0, component],
        );
        word
    })
}

/// `FunctionControl` `DontInline`: a hint that the lookup stays one function the module calls, as
/// written, rather than copied into every call site.
const DONT_INLINE: u32 = 0x2;

/// The device address of guest address `address`, whether the table holds it, and the device
/// address of its slot's written mark ([`MARK_SINK_WORDS`]) - the marks' sink where none holds it.
///
/// Every access of every lane looks up, so the lookup is one helper function the module calls
/// (D969): written in place at each, a whole-wave module repeats it for each of 64 lanes, and the
/// largest of Grand Theft Auto V's traversal programs came out at millions of words the driver did
/// not finish compiling. The helper answers the device address - zero where the table holds none,
/// which no mapping's byte has - and the mark's. A model with no helper writes it in place.
fn lookup<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> (Id, Id, Id) {
    let Some((function, written)) = model.device_lookup_function() else {
        return lookup_in_place(model, t, address);
    };
    // The answer is four 32-bit words, the two addresses' halves, not two 64-bit ones: answered as
    // a vector of two 64-bit words, a four-word store lost its second word on an RTX 5070 Ti, the
    // module valid (D969, `every_word_of_a_wide_access_lands`).
    let pair = t.slot_type;
    if !written {
        define_lookup(model, t, function, pair);
    }
    let builder = model.builder();
    let answer = builder.id();
    builder.function(
        op::FUNCTION_CALL,
        &[
            pair.0,
            answer.0,
            function.0,
            address.0,
            t.address.0,
            t.bits.0,
            t.shift.0,
            t.marks.0,
            t.recording.0,
        ],
    );
    let u32_type = model.u32_type();
    let [device_low, device_high, mark_low, mark_high] = [0, 1, 2, 3].map(|component| {
        let builder = model.builder();
        let part = builder.id();
        builder.function(
            op::COMPOSITE_EXTRACT,
            &[u32_type.0, part.0, answer.0, component],
        );
        part
    });
    let device = wide(model, t, (device_low, device_high));
    let mark = wide(model, t, (mark_low, mark_high));
    let none = wide_constant(model, t, 0);
    let bool_type = model.bool_type();
    let builder = model.builder();
    let found = builder.id();
    builder.function(op::INOT_EQUAL, &[bool_type.0, found.0, device.0, none.0]);
    (device, found, mark)
}

/// Writes the lookup helper `function` (D969): from a guest address and the table's device address,
/// size, chunk shift, marks' device address and whether the dispatch records - passed in, so the
/// helper reads nothing of the calling function's - to the device address, zero where none, and
/// the mark's device address, as four 32-bit halves of the `pair` type, low half first. Written
/// whole among the helpers while the calling function
/// stays open.
///
/// In a recording dispatch the helper also sets the touched mark of the chunk it found, the sink's
/// where it found none: the host reads them back to learn what the dispatch reached. Whether it
/// records is one value for the whole dispatch, so the branch around the store never diverges.
fn define_lookup<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, function: Id, pair: Id) {
    let u32_type = model.u32_type();
    let previous = model.builder().into_helpers(true);
    let builder = model.builder();
    let u64_type = t.u64_type;
    let signature = builder.declare_type(
        op::TYPE_FUNCTION,
        &[
            pair.0, u64_type.0, u64_type.0, u32_type.0, u32_type.0, u64_type.0, u32_type.0,
        ],
    );
    builder.function(
        op::FUNCTION,
        &[pair.0, function.0, DONT_INLINE, signature.0],
    );
    let [address, table, bits, shift, marks, recording] =
        [u64_type, u64_type, u32_type, u32_type, u64_type, u32_type].map(|kind| {
            let parameter = builder.id();
            builder.function(op::FUNCTION_PARAMETER, &[kind.0, parameter.0]);
            parameter
        });
    let entry = builder.id();
    builder.function(op::LABEL, &[entry.0]);
    let inner = DeviceTable {
        address: table,
        bits,
        shift,
        marks,
        recording,
        ..*t
    };
    let (device, found, mark) = lookup_in_place(model, &inner, address);
    mark_touched(model, &inner, (found, mark));
    let none = wide_constant(model, &inner, 0);
    let device = wide_select(model, &inner, found, device, none);
    let halves = [
        narrow(model, device),
        high_half(model, &inner, device),
        narrow(model, mark),
        high_half(model, &inner, mark),
    ];
    let builder = model.builder();
    let answer = builder.id();
    builder.function(
        op::COMPOSITE_CONSTRUCT,
        &[
            pair.0,
            answer.0,
            halves[0].0,
            halves[1].0,
            halves[2].0,
            halves[3].0,
        ],
    );
    builder.function(op::RETURN_VALUE, &[answer.0]);
    builder.function(op::FUNCTION_END, &[]);
    builder.into_helpers(previous);
}

/// Where `t.recording` is set, sets the touched mark of the slot whose written mark is at `mark` -
/// the table's slots' count of words past it - or the sink's where `found` does not hold.
fn mark_touched<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, (found, mark): (Id, Id)) {
    let (zero, one) = (model.constant(0), model.constant(1));
    let homes = model.binary(op::SHIFT_LEFT_LOGICAL, one, t.bits);
    let past = model.constant(PROBES - 1);
    let slots = model.add(homes, past);
    let two = model.constant(2);
    let bytes = model.binary(op::SHIFT_LEFT_LOGICAL, slots, two);
    let bytes = widen(model, t, bytes);
    let touched = wide_op(model, t, op::IADD, mark, bytes);
    let touched = wide_select(model, t, found, touched, t.marks);
    let on = model.compare(op::INOT_EQUAL, t.recording, zero);
    let at = pointer(model, t, touched);
    let builder = model.builder();
    let (record, merge) = (builder.id(), builder.id());
    builder.function(op::SELECTION_MERGE, &[merge.0, 0]);
    builder.function(op::BRANCH_CONDITIONAL, &[on.0, record.0, merge.0]);
    builder.function(op::LABEL, &[record.0]);
    builder.function(op::STORE, &[at.0, one.0, MEMORY_ALIGNED, 4]);
    builder.function(op::BRANCH, &[merge.0]);
    builder.function(op::LABEL, &[merge.0]);
}

/// [`lookup`] written in place: the chunk's [`PROBES`] slots are consecutive from its home and
/// never wrap, so they are four loads from one base, each of a whole slot.
fn lookup_in_place<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> (Id, Id, Id) {
    let chunk = wide_op(model, t, op::SHIFT_RIGHT_LOGICAL, address, t.shift);
    let low = narrow(model, chunk);
    let high = high_half(model, t, chunk);
    let mixed = model.binary(op::BITWISE_XOR, low, high);
    let hash = model.constant(CHUNK_HASH);
    let hashed = model.binary(op::IMUL, mixed, hash);
    let thirty_two = model.constant(32);
    let keep = model.binary(op::ISUB, thirty_two, t.bits);
    let home = model.binary(op::SHIFT_RIGHT_LOGICAL, hashed, keep);
    let zero = model.constant(0);
    let home = widen(model, t, home);
    let slot_bytes = wide_constant(model, t, u64::from(SLOT_WORDS) * 4);
    let word_bytes = wide_constant(model, t, 4);
    let slots_at = wide_op(model, t, op::IMUL, home, slot_bytes);
    let header = wide_constant(model, t, u64::from(HEADER_WORDS) * 4);
    let first = wide_op(model, t, op::IADD, t.address, header);
    let first = wide_op(model, t, op::IADD, first, slots_at);
    let marks_at = wide_op(model, t, op::IMUL, home, word_bytes);
    let mark_sink = wide_constant(model, t, u64::from(MARK_SINK_WORDS) * 4);
    let first_mark = wide_op(model, t, op::IADD, t.marks, mark_sink);
    let first_mark = wide_op(model, t, op::IADD, first_mark, marks_at);
    let mut delta = wide_constant(model, t, 0);
    let mut found = model.compare(op::INOT_EQUAL, zero, zero);
    let mut written_at = t.marks;
    for probe in 0..PROBES {
        let step = wide_constant(model, t, u64::from(probe * SLOT_WORDS * 4));
        let entry = wide_op(model, t, op::IADD, first, step);
        let held = load_slot(model, t, entry);
        let same_low = model.compare(op::IEQUAL, held[0], low);
        let same_high = model.compare(op::IEQUAL, held[1], high);
        let same = model.both(same_low, same_high);
        let slot_delta = wide(model, t, (held[2], held[3]));
        delta = wide_select(model, t, same, slot_delta, delta);
        let step = wide_constant(model, t, u64::from(probe) * 4);
        let slot_mark = wide_op(model, t, op::IADD, first_mark, step);
        written_at = wide_select(model, t, same, slot_mark, written_at);
        found = model.either(found, same);
    }
    (
        wide_op(model, t, op::IADD, address, delta),
        found,
        written_at,
    )
}

/// Marks the chunk whose written mark is at device address `mark` written (one), where the store
/// landed: the caller is inside the block that runs only then.
fn mark_written<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, mark: Id) {
    let at = pointer(model, t, mark);
    let one = model.constant(1);
    model
        .builder()
        .function(op::STORE, &[at.0, one.0, MEMORY_ALIGNED, 4]);
}

/// One word's lookup ([`lookup`]): its device address, whether the table holds it, its written
/// mark's device address, and its guest page ([`page_of`]), which an escape names (D971).
#[derive(Debug, Clone, Copy)]
struct Looked {
    device: Id,
    found: Id,
    mark: Id,
    page: Id,
}

/// The guest page holding guest address `address`, as an escape names it (D971): bits 47:16.
fn page_of<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> Id {
    let shift = model.constant(orbistoun_spirv::DISPATCH_ESCAPE_PAGE_SHIFT);
    let page = wide_op(model, t, op::SHIFT_RIGHT_LOGICAL, address, shift);
    narrow(model, page)
}

/// `condition ? a : b` on booleans.
fn pick_bool<M: Model + ?Sized>(model: &mut M, condition: Id, a: Id, b: Id) -> Id {
    let otherwise = logical_not(model, condition);
    let first = model.both(condition, a);
    let second = model.both(otherwise, b);
    model.either(first, second)
}

/// The lookups of `words` consecutive words from guest address `address`, its low two bits dropped:
/// one access's words, a scalar load's sixteen at the most. A chunk is at least a page and the words
/// at most 64 bytes, so they lie in one chunk or two - the first word's and the last's - and two
/// lookups serve them all: each word takes the first's where it shares its chunk, the last's
/// otherwise.
fn lookup_words<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    address: Id,
    words: u32,
) -> Vec<Looked> {
    let aligned = wide_constant(model, t, !3);
    let first_address = wide_op(model, t, op::BITWISE_AND, address, aligned);
    let (device, found, mark) = lookup(model, t, first_address);
    let page = page_of(model, t, first_address);
    let first = Looked {
        device,
        found,
        mark,
        page,
    };
    if words <= 1 {
        return vec![first];
    }
    let span = u64::from(words - 1) * 4;
    let step = wide_constant(model, t, span);
    let last_address = wide_op(model, t, op::IADD, first_address, step);
    let (device, found, mark) = lookup(model, t, last_address);
    let page = page_of(model, t, last_address);
    let last = Looked {
        device,
        found,
        mark,
        page,
    };
    let first_chunk = wide_op(model, t, op::SHIFT_RIGHT_LOGICAL, first_address, t.shift);
    (0..words)
        .map(|word| {
            let offset = u64::from(word) * 4;
            let ahead = wide_constant(model, t, offset);
            let at = wide_op(model, t, op::IADD, first_address, ahead);
            let chunk = wide_op(model, t, op::SHIFT_RIGHT_LOGICAL, at, t.shift);
            let same = {
                let bool_type = model.bool_type();
                let builder = model.builder();
                let result = builder.id();
                builder.function(op::IEQUAL, &[bool_type.0, result.0, chunk.0, first_chunk.0]);
                result
            };
            let from_first = wide_op(model, t, op::IADD, first.device, ahead);
            let behind = wide_constant(model, t, span - offset);
            let from_last = wide_op(model, t, op::ISUB, last.device, behind);
            Looked {
                device: wide_select(model, t, same, from_first, from_last),
                found: pick_bool(model, same, first.found, last.found),
                mark: wide_select(model, t, same, first.mark, last.mark),
                page: model.select(same, first.page, last.page),
            }
        })
        .collect()
}

/// The word holding guest address `address`, its low two bits dropped, and whether a mapping holds
/// it: zero where none does, read from the table's header instead.
fn guarded_load<M: Model + ?Sized>(model: &mut M, t: &DeviceTable, address: Id) -> (Id, Id) {
    let looked = lookup_words(model, t, address, 1)[0];
    guarded_load_looked(model, t, looked)
}

/// The word `looked` names and whether a mapping holds it: zero where none does, read from the
/// table's header instead.
fn guarded_load_looked<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    looked: Looked,
) -> (Id, Id) {
    let target = wide_select(model, t, looked.found, looked.device, t.address);
    let value = load_guest_word(model, t, target);
    let zero = model.constant(0);
    (model.select(looked.found, value, zero), looked.found)
}

/// Stores `value` at the word holding guest address `address` in `lane`, where the lane runs, a
/// mapping holds the word and `condition` holds when given; a store `condition` admits that no
/// mapping holds is an escape.
fn store_word<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (address, value): (Id, Id),
    condition: Option<Id>,
    lane: u32,
) {
    let looked = lookup_words(model, t, address, 1)[0];
    store_looked(model, t, (looked, value), condition, lane);
}

/// [`store_word`] at the word `looked` names.
fn store_looked<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (looked, value): (Looked, Id),
    condition: Option<Id>,
    lane: u32,
) {
    let Looked {
        device,
        found,
        mark,
        page,
    } = looked;
    let lost = logical_not(model, found);
    let (lands, escaped) = match condition {
        Some(condition) => (model.both(found, condition), model.both(lost, condition)),
        None => (found, lost),
    };
    model.note_escape_at(escaped, Some(lane), page);
    let merge = open_if_active(model, lane, Some(lands));
    let at = pointer(model, t, device);
    model.builder().function(
        op::STORE,
        &[at.0, value.0, MEMORY_ALIGNED | MEMORY_VOLATILE, 4],
    );
    mark_written(model, t, mark);
    close_if_active(model, merge);
}

/// Whether `lane` runs, where the model holds an execution mask.
fn lane_runs<M: Model + ?Sized>(model: &mut M, lane: u32) -> Option<Id> {
    let (low, high) = model.read_lane_mask(EXEC_LOW_HALF).ok()?;
    Some(model.lane_bit(low, high, lane))
}

/// Combines `value` into the word holding guest address `address` by the atomic `opcode`, in `lane`
/// where `landed` holds, answering what it held; zero, and the word untouched, where the lane does
/// not run, does not land or no mapping holds the word - the last an escape. Such a lane's atomic,
/// and its mark, are pointed at the marks' sink, which the host clears and reads nothing from: the
/// table itself the device only reads.
fn atomic_word<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (address, opcode, value): (Id, u16, Id),
    landed: Id,
    lane: u32,
) -> Id {
    let aligned = wide_constant(model, t, !3);
    let address = wide_op(model, t, op::BITWISE_AND, address, aligned);
    let (device, found, mark) = lookup(model, t, address);
    let lost = logical_not(model, found);
    let escaped = model.both(lost, landed);
    let page = page_of(model, t, address);
    model.note_escape_at(escaped, Some(lane), page);
    let mut lands = model.both(found, landed);
    if let Some(runs) = lane_runs(model, lane) {
        lands = model.both(lands, runs);
    }
    let marked = wide_select(model, t, lands, mark, t.marks);
    mark_written(model, t, marked);
    let target = wide_select(model, t, lands, device, t.marks);
    let at = pointer(model, t, target);
    let (device_scope, relaxed) = (model.constant(scope::DEVICE), model.constant(0));
    let u32_type = model.u32_type();
    let builder = model.builder();
    let held = builder.id();
    builder.function(
        opcode,
        &[u32_type.0, held.0, at.0, device_scope.0, relaxed.0, value.0],
    );
    let zero = model.constant(0);
    model.select(lands, held, zero)
}

/// The word at `at` bytes past the guest base whose low half is `base` and bits 47:32 `high`, as a
/// scalar buffer load with no bound buffer reads it in a device-addressed module: zero where
/// `outside`, and an escape where the descriptor admits it and no mapping holds it.
///
/// # Errors
///
/// A model with no device-address table.
pub(super) fn read_from_base<M: Model + ?Sized>(
    model: &mut M,
    (base, high): (Id, Id),
    at: Id,
    outside: Id,
) -> Result<Id, TranslateError> {
    let t = table(model, 0)?;
    let base = wide(model, &t, (base, high));
    let at = widen(model, &t, at);
    let address = wide_op(model, &t, op::IADD, base, at);
    let (value, found) = guarded_load(model, &t, address);
    let lost = logical_not(model, found);
    let admitted = logical_not(model, outside);
    let escaped = model.both(lost, admitted);
    let page = page_of(model, &t, address);
    model.note_escape_at(escaped, None, page);
    Ok(value)
}

/// The word `byte` bytes past the guest address whose halves are `low` and `high`, and whether a
/// mapping holds it: what a ray intersection reads its node through in a device-addressed module.
///
/// # Errors
///
/// A model with no device-address table.
pub(super) fn read_word_found<M: Model + ?Sized>(
    model: &mut M,
    (low, high): (Id, Id),
    byte: u64,
) -> Result<(Id, Id), TranslateError> {
    let t = table(model, 0)?;
    let base = wide(model, &t, (low, high));
    let step = wide_constant(model, &t, byte);
    let address = wide_op(model, &t, op::IADD, base, step);
    Ok(guarded_load(model, &t, address))
}

/// What a flat access moves: whether it loads, how many words, and for a byte or half load its
/// width in bits and whether it sign-extends.
type FlatShape = (bool, u32, Option<(u32, bool)>);

/// A flat access's [`FlatShape`], by name.
fn flat_shape(name: &str) -> Option<FlatShape> {
    Some(match name {
        "global_load_dword" => (true, 1, None),
        "global_load_dwordx2" => (true, 2, None),
        "global_load_dwordx3" => (true, 3, None),
        "global_load_dwordx4" => (true, 4, None),
        "global_load_ubyte" => (true, 1, Some((8, false))),
        "global_load_sbyte" => (true, 1, Some((8, true))),
        "global_load_ushort" => (true, 1, Some((16, false))),
        "global_load_sshort" => (true, 1, Some((16, true))),
        "global_store_dword" => (false, 1, None),
        "global_store_dwordx2" => (false, 2, None),
        "global_store_dwordx3" => (false, 3, None),
        "global_store_dwordx4" => (false, 4, None),
        "global_store_byte" => (false, 1, Some((8, false))),
        "global_store_short" => (false, 1, Some((16, false))),
        _ => return None,
    })
}

/// Where a flat access's address routes: the local data share, the lane's private memory, or
/// guest memory, one condition each.
#[derive(Clone, Copy)]
struct Routes {
    local: Id,
    private: Id,
    device: Id,
}

/// A flat access's address in `lane` as 64 bits: the vector pair alone with no base, or the scalar
/// pair plus the vector register's unsigned offset, then the instruction's signed offset.
fn flat_address<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    instruction: &Instruction,
    (vaddr, base): (&Operand, &Operand),
    lane: u32,
) -> Result<Id, TranslateError> {
    let Operand::Vector(register) = vaddr else {
        return Err(refuse(
            instruction,
            "a flat access whose address is not a vector register",
        ));
    };
    let address = match base {
        Operand::Named(name) if name == FLAT_NO_BASE => {
            let low = model.read_source(instruction, vaddr, lane)?;
            let next = Operand::Vector(register.saturating_add(1));
            let high = model.read_source(instruction, &next, lane)?;
            wide(model, t, (low, high))
        }
        Operand::Scalar(scalar) => {
            let scalar = u32::from(*scalar);
            if scalar + 1 >= SCALAR_REGISTERS {
                return Err(refuse(
                    instruction,
                    "a flat access's scalar base runs past the register file",
                ));
            }
            let (low, high) = (model.read_scalar(scalar), model.read_scalar(scalar + 1));
            let base = wide(model, t, (low, high));
            let offset = model.read_source(instruction, vaddr, lane)?;
            let offset = widen(model, t, offset);
            wide_op(model, t, op::IADD, base, offset)
        }
        _ => {
            return Err(refuse(
                instruction,
                "flat access has a base this translator does not understand",
            ));
        }
    };
    let immediate = match instruction.operands.get(3) {
        Some(Operand::Immediate(immediate)) => *immediate,
        _ => 0,
    };
    if immediate == 0 {
        return Ok(address);
    }
    let step = wide_constant(model, t, u64::from_ne_bytes(immediate.to_ne_bytes()));
    Ok(wide_op(model, t, op::IADD, address, step))
}

/// The word an address whose low half is `low` and whose lookup is `looked` holds, where `routes`
/// sends it - the local data share, the lane's private memory or guest memory, or guest memory alone
/// for a global access - recording a guest-memory word no mapping holds as an escape.
fn routed_word<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (looked, low): (Looked, Id),
    routes: Option<Routes>,
    lane: u32,
) -> Result<Id, TranslateError> {
    let (value, found) = guarded_load_looked(model, t, looked);
    let lost = logical_not(model, found);
    let Some(routes) = routes else {
        model.note_escape_at(lost, Some(lane), looked.page);
        return Ok(value);
    };
    let escaped = model.both(lost, routes.device);
    model.note_escape_at(escaped, Some(lane), looked.page);
    let index = model.local_word_index(low);
    let local = model.read_local(index)?;
    let private = crate::scratch::private_word(model, (low, routes.private), None, lane);
    let value = model.select(routes.private, private, value);
    Ok(model.select(routes.local, local, value))
}

/// Stores `value` at the word an address whose low half is `low` and whose lookup is `looked`
/// holds, where `routes` sends it.
fn routed_store<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (looked, low, value): (Looked, Id, Id),
    routes: Option<Routes>,
    lane: u32,
) -> Result<(), TranslateError> {
    let Some(routes) = routes else {
        store_looked(model, t, (looked, value), None, lane);
        return Ok(());
    };
    store_looked(model, t, (looked, value), Some(routes.device), lane);
    let index = model.local_word_index(low);
    let merge = open_if_active(model, lane, Some(routes.local));
    model.write_local(index, value, lane)?;
    close_if_active(model, merge);
    crate::scratch::private_word(model, (low, routes.private), Some(value), lane);
    Ok(())
}

/// A byte or half load's value from the word holding `address` and the next, at its place in them,
/// zero- or `signed` sign-extended from `bits`.
fn sub_word<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (address, routes): (Id, Option<Routes>),
    (bits, signed): (u32, bool),
    lane: u32,
) -> Result<Id, TranslateError> {
    let aligned = wide_constant(model, t, !3);
    let first = wide_op(model, t, op::BITWISE_AND, address, aligned);
    let four = wide_constant(model, t, 4);
    let second = wide_op(model, t, op::IADD, first, four);
    let (first_low, second_low) = (narrow(model, first), narrow(model, second));
    let looked = lookup_words(model, t, first, 2);
    let low = routed_word(model, t, (looked[0], first_low), routes, lane)?;
    let high = routed_word(model, t, (looked[1], second_low), routes, lane)?;
    let place = narrow(model, address);
    let three = model.constant(3);
    let place = model.binary(op::BITWISE_AND, place, three);
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
    let unused = model.constant(32 - bits);
    let top = model.binary(op::SHIFT_LEFT_LOGICAL, bytes, unused);
    let widen = if signed {
        op::SHIFT_RIGHT_ARITHMETIC
    } else {
        op::SHIFT_RIGHT_LOGICAL
    };
    Ok(model.binary(widen, top, unused))
}

/// A global or flat access by device address: a global one reaches guest memory, a flat one routes
/// by its address's aperture first.
fn flat<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let segment = (instruction.word >> 14) & 3;
    if segment == 1 {
        return Err(refuse(
            instruction,
            "a scratch-segment access in a module whose dispatch gives it no scratch (D878, D954)",
        ));
    }
    let flat_segment = segment == 0;
    let (loading, words, part) = flat_shape(name).ok_or_else(|| {
        refuse(
            instruction,
            "no translation by device address for this flat memory instruction (D964)",
        )
    })?;
    let (first, second, base) = first_three_operands(instruction)?;
    let (vaddr, register) = if loading {
        (second, first)
    } else {
        (first, second)
    };
    let Operand::Vector(register) = register else {
        return Err(refuse(
            instruction,
            "a flat access's data is not a vector register",
        ));
    };
    let register = u32::from(*register);
    if register + words > VECTOR_REGISTERS {
        return Err(refuse(
            instruction,
            "a flat access runs past the end of the vector register file",
        ));
    }
    let t = table(model, instruction.offset)?;
    let (local_high, private_high) = (
        model.constant(LOCAL_APERTURE_HIGH),
        model.constant(PRIVATE_APERTURE_HIGH),
    );
    for lane in running_lanes(model) {
        let address = flat_address(model, &t, instruction, (vaddr, base), lane)?;
        let routes = if flat_segment {
            let high = high_half(model, &t, address);
            let to_share = model.compare(op::IEQUAL, high, local_high);
            let to_scratch = model.compare(op::IEQUAL, high, private_high);
            let either = model.either(to_share, to_scratch);
            let device = logical_not(model, either);
            Some(Routes {
                local: to_share,
                private: to_scratch,
                device,
            })
        } else {
            None
        };
        let values = if loading {
            Vec::new()
        } else {
            (0..words)
                .map(|word| {
                    let source =
                        Operand::Vector(u16::try_from(register + word).unwrap_or(u16::MAX));
                    model.read_source(instruction, &source, lane)
                })
                .collect::<Result<Vec<_>, _>>()?
        };
        // The access's words share their lookups: two at the most, however many words.
        let looked = if part.is_none() {
            lookup_words(model, &t, address, words)
        } else {
            Vec::new()
        };
        for word in 0..words {
            let step = wide_constant(model, &t, u64::from(word) * 4);
            let at = wide_op(model, &t, op::IADD, address, step);
            if loading {
                let value = if let Some(shape) = part {
                    sub_word(model, &t, (at, routes), shape, lane)?
                } else {
                    let low = narrow(model, at);
                    routed_word(model, &t, (looked[word as usize], low), routes, lane)?
                };
                model.write_vector_lane(register + word, lane, value);
            } else if let Some((bits, _)) = part {
                // A byte or half merges into its word (D977); through the flat segment it may
                // reach the local share or private memory instead, which is not merged here.
                if routes.is_some() {
                    return Err(refuse(
                        instruction,
                        "a byte or half store through the flat segment, which may reach the local share or private memory (D977)",
                    ));
                }
                sub_word_store(model, &t, (at, values[word as usize]), bits, None, lane);
            } else {
                let low = narrow(model, at);
                let stored = (looked[word as usize], low, values[word as usize]);
                routed_store(model, &t, stored, routes, lane)?;
            }
        }
    }
    model.count();
    Ok(())
}

/// A scalar load by device address: consecutive words from its base pair, bits 47:0, plus its
/// immediate and any offset register.
fn scalar_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let words = access_words(name);
    let (destination, base, offset) = three_operands(instruction)?;
    let destinations = load_destinations(instruction, destination, words)?;
    let (Operand::Scalar(base_register), Operand::Immediate(bytes)) = (base, offset) else {
        return Err(refuse(
            instruction,
            "a scalar load needs a scalar base and an immediate offset",
        ));
    };
    let base_register = u32::from(*base_register);
    if base_register + 1 >= SCALAR_REGISTERS {
        return Err(refuse(
            instruction,
            "a scalar load's base runs past the register file",
        ));
    }
    let bytes = u32::try_from(*bytes).map_err(|_| refuse(instruction, "negative load offset"))?;
    let second = instruction.second_word.unwrap_or(0);
    let added = scalar_offset_register(model, instruction, (second >> 25) & 0x7f)?;
    let t = table(model, instruction.offset)?;
    let low = model.read_scalar(base_register);
    let high = model.read_scalar(base_register + 1);
    let address_bits = model.constant(0xFFFF);
    let high = model.binary(op::BITWISE_AND, high, address_bits);
    let base = wide(model, &t, (low, high));
    let immediate = wide_constant(model, &t, u64::from(bytes));
    let mut address = wide_op(model, &t, op::IADD, base, immediate);
    if let Some(added) = added {
        let added = widen(model, &t, added);
        address = wide_op(model, &t, op::IADD, address, added);
    }
    // Its words share two lookups, the first word's and the last's ([`lookup_words`]).
    let count = u32::try_from(destinations.len()).unwrap_or(u32::MAX);
    let looked = lookup_words(model, &t, address, count);
    for (looked, destination) in looked.into_iter().zip(&destinations) {
        let (value, found) = guarded_load_looked(model, &t, looked);
        let lost = logical_not(model, found);
        model.note_escape_at(lost, None, looked.page);
        write_scalar_operand(model, instruction, destination, value)?;
    }
    model.count();
    Ok(())
}

/// A buffer access's operands, checked: its data's first register, its address operand, its
/// descriptor's first register and its scalar offset.
fn buffer_operands(
    instruction: &Instruction,
) -> Result<(u32, &Operand, u32, &Operand), TranslateError> {
    let [data, vaddr, resource, soffset] = instruction.operands.as_slice() else {
        return Err(refuse(
            instruction,
            "a buffer access does not have four operands",
        ));
    };
    let (Operand::Vector(register), Operand::Scalar(resource)) = (data, resource) else {
        return Err(refuse(
            instruction,
            "a buffer access's data is not vector registers or its descriptor not scalar ones",
        ));
    };
    let resource = u32::from(*resource);
    if resource + 4 > SCALAR_REGISTERS {
        return Err(refuse(
            instruction,
            "a buffer resource constant runs past the end of the register file",
        ));
    }
    Ok((u32::from(*register), vaddr, resource, soffset))
}

/// A descriptor the program computes, read as the module runs (D967): its addressing fields, the
/// bounds and base [`read_buffer_resource`] reads with none of them refused, and its fourth word.
struct Computed {
    resource: BufferResource,
    flags: Id,
    /// Its stride, whether it swizzles, its index stride's power of two and whether it adds the
    /// lane's id to the index, as a scratch descriptor's are read.
    stride: Id,
    swizzled: Id,
    stride_shift: Id,
    adds_lane: Id,
}

/// The descriptor in the four scalar registers from `first`, read as the module runs: swizzled and
/// lane-indexed addressing are carried out rather than refused (D967).
fn computed<M: Model + ?Sized>(model: &mut M, first: u32) -> Computed {
    let resource = read_buffer_resource(model, first);
    let unsupported = model.constant(0);
    let flags = model.read_scalar(first + 3);
    let (stride, swizzled, stride_shift, adds_lane) =
        crate::scratch::descriptor_fields(model, first);
    Computed {
        resource: BufferResource {
            unsupported,
            ..resource
        },
        flags,
        stride,
        swizzled,
        stride_shift,
        adds_lane,
    }
}

/// A buffer access's place in `lane`, `extra` bytes past its offset, through a descriptor the
/// program computes: its byte from the descriptor's base, and whether it is out of bounds for a
/// payload of `payload` bytes (D967).
///
/// AMD's published RDNA 2 instruction set's buffer address less the base: the scalar offset, plus
/// `offset + stride * index`, or where the descriptor swizzles
///
/// ```text
/// ((index / IS) * stride + (offset / 4) * 4) * IS + (index % IS) * 4 + offset % 4
/// ```
///
/// for the index stride `IS` and the four-byte element GFX9 on fix (`aco_ir.cpp:1953-1954`), as a
/// scratch access is placed. `index` is the address register's index plus, where the descriptor
/// adds it, the lane's id; `offset` the address register's offset plus the immediate.
fn buffer_place<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    (vaddr, soffset): (&Operand, &Operand),
    computed: &Computed,
    (extra, payload): (u32, Id),
    lane: u32,
) -> Result<(Id, Id), TranslateError> {
    let word = instruction.word;
    let (literal, offen, idxen) = (word & 0xFFF, word & 1 << 12 != 0, word & 1 << 13 != 0);
    let scalar_offset = model.read_source(instruction, soffset, lane)?;
    let (vindex, voffset) =
        atomic_index_and_offset(model, (instruction, vaddr), (idxen, offen), lane)?;
    let zero = model.constant(0);
    let instruction_offset = model.constant(literal + extra);
    let offset = model.add(instruction_offset, voffset.unwrap_or(zero));
    let id = crate::scratch::lane_id(model, lane);
    let added = model.select(computed.adds_lane, id, zero);
    let index = model.add(vindex.unwrap_or(zero), added);
    let scaled = model.binary(op::IMUL, computed.stride, index);
    let linear = model.add(offset, scaled);
    let swizzled = crate::scratch::swizzled_place(
        model,
        (index, offset),
        (computed.stride, computed.stride_shift),
    );
    let place = model.select(computed.swizzled, swizzled, linear);
    let within = model.select(computed.swizzled, swizzled, offset);
    let outside = buffer_out_of_bounds_placed(
        model,
        &computed.resource,
        computed.flags,
        (offset, within),
        index,
        payload,
    );
    Ok((model.add(scalar_offset, place), outside))
}

/// An untyped buffer access no bound buffer serves, by device address: the descriptor's base plus
/// the buffer address equation, bounded as a bound buffer's access is.
fn buffer<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let loading = name.starts_with("buffer_load");
    let (components, narrow_bytes) = if matches!(name, "buffer_load_ushort" | "buffer_store_short")
    {
        (1, Some(2))
    } else if name == "buffer_store_byte" {
        (1, Some(1))
    } else {
        match name.rsplit_once("dword") {
            Some((_, "")) => (1, None),
            Some((_, "x2")) => (2, None),
            Some((_, "x3")) => (3, None),
            Some((_, "x4")) => (4, None),
            _ => {
                return Err(refuse(
                    instruction,
                    "an untyped buffer access names a width this does not translate",
                ));
            }
        }
    };
    let (register, vaddr, resource_base, soffset) = buffer_operands(instruction)?;
    if register + components > VECTOR_REGISTERS {
        return Err(refuse(
            instruction,
            "a buffer access runs past the end of the vector register file",
        ));
    }
    let t = table(model, instruction.offset)?;
    let computed = computed(model, resource_base);
    let resource = computed.resource;
    let base = wide(model, &t, (resource.base, resource.high));
    let zero = model.constant(0);
    let payload = model.constant(narrow_bytes.unwrap_or(4));
    for lane in running_lanes(model) {
        for component in 0..components {
            // Each word placed on its own: a swizzled buffer's words lie apart (D967).
            let (at, outside) = buffer_place(
                model,
                instruction,
                (vaddr, soffset),
                &computed,
                (component * 4, payload),
                lane,
            )?;
            let inside = logical_not(model, outside);
            let at = widen(model, &t, at);
            let address = wide_op(model, &t, op::IADD, base, at);
            if loading {
                let value = if let Some(bytes) = narrow_bytes {
                    sub_word(model, &t, (address, None), (8 * bytes, false), lane)?
                } else {
                    let (value, found) = guarded_load(model, &t, address);
                    let lost = logical_not(model, found);
                    let escaped = model.both(lost, inside);
                    let page = page_of(model, &t, address);
                    model.note_escape_at(escaped, Some(lane), page);
                    value
                };
                let kept = pick(model, outside, zero, value);
                model.write_vector_lane(register + component, lane, kept);
            } else {
                let source =
                    step_operand(&Operand::Vector(super::address_of(register)), component)?;
                let value = model.read_source(instruction, &source, lane)?;
                match narrow_bytes {
                    // A byte or half merges into its word (D977).
                    Some(bytes) => {
                        sub_word_store(model, &t, (address, value), 8 * bytes, Some(inside), lane);
                    }
                    None => store_word(model, &t, (address, value), Some(inside), lane),
                }
            }
        }
    }
    model.count();
    Ok(())
}

/// Stores `value`'s low `bits` bits at guest address `address` in `lane`, where `landed` holds when
/// given (D977): merged into the word holding the first byte and, where it reaches it, the next -
/// its other bits kept, then its own set, each one atomic step, so another lane's or wave's store
/// to the rest of the word is kept. A part no mapping holds is an escape ([`atomic_word`]).
fn sub_word_store<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    (address, value): (Id, Id),
    bits: u32,
    landed: Option<Id>,
    lane: u32,
) {
    let low = narrow(model, address);
    let (parts, spans) = super::sub_word_parts(model, (low, value), bits);
    let landed = landed.unwrap_or_else(|| {
        let zero = model.constant(0);
        model.compare(op::IEQUAL, zero, zero)
    });
    let aligned = wide_constant(model, t, !3);
    let first = wide_op(model, t, op::BITWISE_AND, address, aligned);
    for (word, (keep, set)) in [0u64, 1].into_iter().zip(parts) {
        let step = wide_constant(model, t, word * 4);
        let at = wide_op(model, t, op::IADD, first, step);
        let reaches = if word == 0 {
            landed
        } else {
            model.both(landed, spans)
        };
        // A part no mapping holds is an escape; one that lands is swapped into its word whole
        // ([`super::merge_bits`]), its chunk marked written.
        let (device, found, mark) = lookup(model, t, at);
        let lost = logical_not(model, found);
        let escaped = model.both(lost, reaches);
        let page = page_of(model, t, at);
        model.note_escape_at(escaped, Some(lane), page);
        let lands = model.both(found, reaches);
        let merge = open_if_active(model, lane, Some(lands));
        let at = pointer(model, t, device);
        let gave_up = super::merge_bits(model, at, (keep, set));
        model.note_escape_at(gave_up, Some(lane), page);
        mark_written(model, t, mark);
        close_if_active(model, merge);
    }
}

/// A one-word buffer atomic no bound buffer serves, by device address (D919's forms): an integer
/// one as one atomic, a float minimum or maximum as two by the value's sign. A 64-bit one is
/// refused.
fn buffer_atomic<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    if name.ends_with("_x2") {
        return Err(refuse(
            instruction,
            "a 64-bit buffer atomic by device address, which is not translated (D964)",
        ));
    }
    let float = match name {
        "buffer_atomic_fmin" => Some(false),
        "buffer_atomic_fmax" => Some(true),
        _ => None,
    };
    let opcode = match name {
        "buffer_atomic_add" => op::ATOMIC_I_ADD,
        "buffer_atomic_sub" => op::ATOMIC_I_SUB,
        "buffer_atomic_and" => op::ATOMIC_AND,
        "buffer_atomic_or" => op::ATOMIC_OR,
        "buffer_atomic_xor" => op::ATOMIC_XOR,
        "buffer_atomic_umin" => op::ATOMIC_U_MIN,
        "buffer_atomic_umax" => op::ATOMIC_U_MAX,
        "buffer_atomic_smin" => op::ATOMIC_S_MIN,
        "buffer_atomic_smax" => op::ATOMIC_S_MAX,
        "buffer_atomic_swap" => op::ATOMIC_EXCHANGE,
        _ if float.is_some() => op::ATOMIC_U_MIN,
        _ => {
            return Err(refuse(
                instruction,
                "a buffer atomic not translated by device address (D964)",
            ));
        }
    };
    let (register, vaddr, resource_base, soffset) = buffer_operands(instruction)?;
    let returning = instruction.word & 1 << 14 != 0;
    let t = table(model, instruction.offset)?;
    let computed = computed(model, resource_base);
    let resource = computed.resource;
    let base = wide(model, &t, (resource.base, resource.high));
    let zero = model.constant(0);
    let payload = model.constant(4);
    for lane in running_lanes(model) {
        let (start, outside) = buffer_place(
            model,
            instruction,
            (vaddr, soffset),
            &computed,
            (0, payload),
            lane,
        )?;
        let landed = logical_not(model, outside);
        let at = widen(model, &t, start);
        let address = wide_op(model, &t, op::IADD, base, at);
        let value = model.read_source(
            instruction,
            &Operand::Vector(super::address_of(register)),
            lane,
        )?;
        let held = match float {
            None => atomic_word(model, &t, (address, opcode, value), landed, lane),
            Some(maximum) => {
                let [(first, kept), (second, other)] = float_atomic_parts(model, value, maximum);
                let by_sign = atomic_word(model, &t, (address, first, kept), landed, lane);
                let reversed = atomic_word(model, &t, (address, second, other), landed, lane);
                let sign_bit = model.constant(0x8000_0000);
                let sign = model.binary(op::BITWISE_AND, value, sign_bit);
                let positive = model.compare(op::IEQUAL, sign, zero);
                model.select(positive, by_sign, reversed)
            }
        };
        if returning {
            let kept = pick(model, outside, zero, held);
            model.write_vector_lane(register, lane, kept);
        }
    }
    model.count();
    Ok(())
}

/// The four words of a format load's element at guest address `address`, its first byte at bit 0
/// of the first, by device address (D967): the five aligned words over it, which share their
/// lookups ([`lookup_words`]). A word the element needs - one holding any of its `bytes`, where it
/// is not `outside` - that no mapping holds reads zero and is an escape in `lane`.
fn element_words<M: Model + ?Sized>(
    model: &mut M,
    t: &DeviceTable,
    address: Id,
    (bytes, outside): (Id, Id),
    lane: u32,
) -> [Id; 4] {
    let aligned = wide_constant(model, t, !3);
    let first = wide_op(model, t, op::BITWISE_AND, address, aligned);
    let looked = lookup_words(model, t, first, 5);
    let three = model.constant(3);
    let low = narrow(model, address);
    let byte = model.binary(op::BITWISE_AND, low, three);
    let reach = model.add(byte, bytes);
    let admitted = logical_not(model, outside);
    let zero = model.constant(0);
    let mut held = [zero; 5];
    for ((word, value), looked) in (0u32..).zip(held.iter_mut()).zip(&looked) {
        let target = wide_select(model, t, looked.found, looked.device, t.address);
        let loaded = load_guest_word(model, t, target);
        *value = model.select(looked.found, loaded, zero);
        let start = model.constant(word * 4);
        let needed = model.compare(op::ULESS_THAN, start, reach);
        let needed = model.both(needed, admitted);
        let lost = logical_not(model, looked.found);
        let escaped = model.both(needed, lost);
        model.note_escape_at(escaped, Some(lane), looked.page);
    }
    // Each word from its byte: the word holding it shifted down, the next word's low bytes above,
    // `next << (32 - shift)` as two shifts so a shift of zero moves it out entirely.
    let (eight, one, thirty_one) = (model.constant(8), model.constant(1), model.constant(31));
    let shift = model.binary(op::IMUL, byte, eight);
    let rest = model.binary(op::ISUB, thirty_one, shift);
    let mut words = [zero; 4];
    for (word, value) in words.iter_mut().enumerate() {
        let down = model.binary(op::SHIFT_RIGHT_LOGICAL, held[word], shift);
        let doubled = model.binary(op::SHIFT_LEFT_LOGICAL, held[word + 1], one);
        let up = model.binary(op::SHIFT_LEFT_LOGICAL, doubled, rest);
        *value = model.binary(op::BITWISE_OR, down, up);
    }
    words
}

/// `buffer_load_format_x` .. `_xyzw` through a descriptor no bound buffer serves, by device address
/// (D967): placed and bounded as an untyped access is, its element read from the guest's bytes and
/// converted by the format the descriptor's fourth word names, decoded as the module runs
/// ([`running_format`]). A format or select the module cannot answer, and an element wider than
/// four bytes in a swizzled buffer, whose words the swizzle parts, are escapes.
fn format_load<M: Model + ?Sized>(
    model: &mut M,
    instruction: &Instruction,
    name: &str,
) -> Result<(), TranslateError> {
    let channels = typed_channels(name)
        .ok_or_else(|| refuse(instruction, "a format load does not name its channels"))?;
    let (register, vaddr, resource_base, soffset) = buffer_operands(instruction)?;
    if register + channels > VECTOR_REGISTERS {
        return Err(refuse(
            instruction,
            "a buffer access runs past the end of the vector register file",
        ));
    }
    let t = table(model, instruction.offset)?;
    let computed = computed(model, resource_base);
    let layout = running_format::layout(model, instruction, computed.flags);
    let four = model.constant(4);
    let wide_element = model.compare(op::UGREATER_THAN, layout.bytes, four);
    let parted = model.both(computed.swizzled, wide_element);
    let resource = computed.resource;
    let base = wide(model, &t, (resource.base, resource.high));
    for lane in running_lanes(model) {
        model.note_escape(parted, Some(lane));
        let (at, outside) = buffer_place(
            model,
            instruction,
            (vaddr, soffset),
            &computed,
            (0, layout.bytes),
            lane,
        )?;
        let at = widen(model, &t, at);
        let address = wide_op(model, &t, op::IADD, base, at);
        let words = element_words(model, &t, address, (layout.bytes, outside), lane);
        let components = running_format::components(model, &layout, &words, outside);
        for channel in 0..channels {
            let (value, unanswered) =
                running_format::channel(model, &layout, computed.flags, channel, &components);
            model.note_escape(unanswered, Some(lane));
            model.write_vector_lane(register + channel, lane, value);
        }
    }
    model.count();
    Ok(())
}
