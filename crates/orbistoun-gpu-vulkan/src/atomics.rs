//! Which of a compute module's bindings it changes atomically, read from the module itself.
//!
//! Each atomic that writes - a store, an exchange, a compare-exchange, an integer or float
//! read-modify-write, a flag - is followed from its pointer back through access chains, copies
//! and bitcasts to the variable it reaches, and named by that variable's descriptor set and
//! binding. For the dispatch's buffer array (`DRAW_BUFFERS_SET`, `COMPUTE_BUFFERS_BINDING`) the
//! first index into the variable is the buffer's slot, known where it is a constant.

use std::collections::HashMap;

/// A binding a module changes atomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub(crate) struct AtomicTarget {
    /// The variable's descriptor set.
    pub(crate) set: u32,
    /// Its binding.
    pub(crate) binding: u32,
    /// The constant first index into it - an array's element - where the chain begins with one.
    pub(crate) element: Option<u32>,
}

/// The atomic instructions that write through their pointer, with the operand index of the
/// pointer after the opcode word (SPIR-V 1.6, 3.37.18; `SPV_EXT_shader_atomic_float_min_max`,
/// `SPV_EXT_shader_atomic_float_add`).
const WRITING_ATOMICS: [(u32, usize); 20] = [
    (228, 0),  // OpAtomicStore
    (229, 2),  // OpAtomicExchange
    (230, 2),  // OpAtomicCompareExchange
    (231, 2),  // OpAtomicCompareExchangeWeak
    (232, 2),  // OpAtomicIIncrement
    (233, 2),  // OpAtomicIDecrement
    (234, 2),  // OpAtomicIAdd
    (235, 2),  // OpAtomicISub
    (236, 2),  // OpAtomicSMin
    (237, 2),  // OpAtomicUMin
    (238, 2),  // OpAtomicSMax
    (239, 2),  // OpAtomicUMax
    (240, 2),  // OpAtomicAnd
    (241, 2),  // OpAtomicOr
    (242, 2),  // OpAtomicXor
    (318, 2),  // OpAtomicFlagTestAndSet
    (319, 0),  // OpAtomicFlagClear
    (5614, 2), // OpAtomicFMinEXT
    (5615, 2), // OpAtomicFMaxEXT
    (6035, 2), // OpAtomicFAddEXT
];

const OP_CONSTANT: u32 = 43;
const OP_VARIABLE: u32 = 59;
const OP_ACCESS_CHAIN: u32 = 65;
const OP_IN_BOUNDS_ACCESS_CHAIN: u32 = 66;
const OP_PTR_ACCESS_CHAIN: u32 = 67;
const OP_DECORATE: u32 = 71;
const OP_COPY_OBJECT: u32 = 83;
const OP_BITCAST: u32 = 124;
const DECORATION_BINDING: u32 = 33;
const DECORATION_DESCRIPTOR_SET: u32 = 34;

/// How a pointer id was made, as far as following it back needs.
#[derive(Debug, Clone, Copy)]
enum Made {
    /// A variable.
    Variable,
    /// An access chain from this base, with its first index id.
    Chain(u32, Option<u32>),
    /// A copy or bitcast of this id.
    Copy(u32),
}

/// Every binding `module` changes atomically, each once.
pub(crate) fn atomic_targets(module: &[u32]) -> Vec<AtomicTarget> {
    let mut made: HashMap<u32, Made> = HashMap::new();
    let mut constants: HashMap<u32, u32> = HashMap::new();
    let mut sets: HashMap<u32, u32> = HashMap::new();
    let mut bindings: HashMap<u32, u32> = HashMap::new();
    let mut pointers = Vec::new();
    let mut at = 5;
    while at < module.len() {
        let count = (module[at] >> 16) as usize;
        if count == 0 || at + count > module.len() {
            break;
        }
        let opcode = module[at] & 0xffff;
        let operands = &module[at + 1..at + count];
        match (opcode, operands) {
            (OP_CONSTANT, [_, id, value, ..]) => {
                constants.insert(*id, *value);
            }
            (OP_VARIABLE, [_, id, ..]) => {
                made.insert(*id, Made::Variable);
            }
            (
                OP_ACCESS_CHAIN | OP_IN_BOUNDS_ACCESS_CHAIN | OP_PTR_ACCESS_CHAIN,
                [_, id, base, indices @ ..],
            ) => {
                made.insert(*id, Made::Chain(*base, indices.first().copied()));
            }
            (OP_COPY_OBJECT | OP_BITCAST, [_, id, source]) => {
                made.insert(*id, Made::Copy(*source));
            }
            (OP_DECORATE, [target, DECORATION_DESCRIPTOR_SET, value]) => {
                sets.insert(*target, *value);
            }
            (OP_DECORATE, [target, DECORATION_BINDING, value]) => {
                bindings.insert(*target, *value);
            }
            _ => {
                if let Some(&(_, pointer)) = WRITING_ATOMICS.iter().find(|(op, _)| *op == opcode)
                    && let Some(&pointer) = operands.get(pointer)
                {
                    pointers.push(pointer);
                }
            }
        }
        at += count;
    }
    let mut targets: Vec<AtomicTarget> = Vec::new();
    for pointer in pointers {
        let Some((variable, first)) = variable_of(pointer, &made) else {
            continue;
        };
        let (Some(&set), Some(&binding)) = (sets.get(&variable), bindings.get(&variable)) else {
            continue;
        };
        let target = AtomicTarget {
            set,
            binding,
            element: first.and_then(|index| constants.get(&index).copied()),
        };
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    targets
}

/// The variable `pointer` reaches, and the first index of the chain that starts at it.
fn variable_of(mut pointer: u32, made: &HashMap<u32, Made>) -> Option<(u32, Option<u32>)> {
    let mut first = None;
    // Bounded: a malformed module could make a cycle.
    for _ in 0..64 {
        match made.get(&pointer)? {
            Made::Variable => return Some((pointer, first)),
            Made::Chain(base, index) => {
                first = *index;
                pointer = *base;
            }
            Made::Copy(source) => pointer = *source,
        }
    }
    None
}

/// What a dispatch's atomics reach of the binding a capture or a watch names: the window at set 0
/// binding 1, or buffer `slot` of the dispatch's buffer array.
pub(crate) fn changes_atomically(targets: &[AtomicTarget], slot: Option<usize>) -> Atomics {
    let buffers = |target: &&AtomicTarget| {
        target.set == orbistoun_spirv::DRAW_BUFFERS_SET
            && target.binding == orbistoun_spirv::COMPUTE_BUFFERS_BINDING
    };
    match slot {
        None => {
            if targets.iter().any(|t| t.set == 0 && t.binding == 1) {
                Atomics::Yes
            } else {
                Atomics::No
            }
        }
        Some(slot) => {
            let on_buffers: Vec<&AtomicTarget> = targets.iter().filter(buffers).collect();
            if on_buffers
                .iter()
                .any(|t| t.element.is_some_and(|e| e as usize == slot))
            {
                Atomics::Yes
            } else if on_buffers.iter().any(|t| t.element.is_none()) {
                Atomics::Maybe
            } else {
                Atomics::No
            }
        }
    }
}

/// Whether a dispatch changes a binding atomically.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Atomics {
    /// It does.
    Yes,
    /// It changes some buffer atomically through an index that is not a constant.
    Maybe,
    /// It does not.
    No,
}

impl Atomics {
    /// As a log line says it.
    pub(crate) const fn said(self) -> &'static str {
        match self {
            Self::Yes => "atomics on it",
            Self::Maybe => "atomics on a buffer chosen at run time",
            Self::No => "no atomics on it",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{AtomicTarget, Atomics, atomic_targets, changes_atomically};

    /// One instruction's words: its count and opcode, then its operands.
    fn instruction(opcode: u32, operands: &[u32]) -> Vec<u32> {
        let mut words = vec![((operands.len() as u32 + 1) << 16) | opcode];
        words.extend_from_slice(operands);
        words
    }

    /// A module with the buffer array at set 1 binding 2 (id 10) and the window at set 0 binding 1
    /// (id 11): a float max into buffer slot 3 through a chain and a bitcast, a compare-exchange
    /// into a slot chosen at run time, an atomic load of the window (which writes nothing), and an
    /// integer add into the window.
    fn module() -> Vec<u32> {
        let mut words = vec![0x0723_0203, 0x0001_0600, 0, 100, 0];
        for (op, operands) in [
            (71, vec![10, 34, 1]),
            (71, vec![10, 33, 2]),
            (71, vec![11, 34, 0]),
            (71, vec![11, 33, 1]),
            (43, vec![1, 20, 3]),
            (43, vec![1, 21, 0]),
            (59, vec![2, 10, 12]),
            (59, vec![2, 11, 12]),
            (65, vec![3, 30, 10, 20, 21, 21]),
            (124, vec![3, 31, 30]),
            (5615, vec![4, 40, 31, 21, 21, 50]),
            (65, vec![3, 32, 10, 60, 21]),
            (230, vec![4, 41, 32, 21, 21, 21, 50, 51]),
            (65, vec![3, 33, 11, 21, 21]),
            (227, vec![4, 42, 33, 21, 21]),
            (234, vec![4, 43, 33, 21, 21, 50]),
        ] {
            words.extend(instruction(op, &operands));
        }
        words
    }

    /// Each writing atomic is followed back to its variable's set and binding, a constant first
    /// index naming the element; an atomic load is not a write.
    #[test]
    fn each_atomic_is_followed_to_its_binding() {
        assert_eq!(
            atomic_targets(&module()),
            [
                AtomicTarget {
                    set: 1,
                    binding: 2,
                    element: Some(3)
                },
                AtomicTarget {
                    set: 1,
                    binding: 2,
                    element: None
                },
                AtomicTarget {
                    set: 0,
                    binding: 1,
                    element: Some(0)
                },
            ]
        );
    }

    /// A buffer slot a constant index reaches has atomics; any other slot may, where an index is
    /// chosen at run time; the window has them where an atomic reaches it.
    #[test]
    fn a_binding_is_told_whether_atomics_reach_it() {
        let targets = atomic_targets(&module());
        assert_eq!(changes_atomically(&targets, Some(3)), Atomics::Yes);
        assert_eq!(changes_atomically(&targets, Some(1)), Atomics::Maybe);
        assert_eq!(changes_atomically(&targets, None), Atomics::Yes);
        let fixed = &targets[..1];
        assert_eq!(changes_atomically(fixed, Some(1)), Atomics::No);
        assert_eq!(changes_atomically(fixed, None), Atomics::No);
        assert_eq!(atomic_targets(&[]), []);
    }
}
