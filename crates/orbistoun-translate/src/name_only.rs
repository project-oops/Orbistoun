//! Which scalar registers hold only an image's name, along the program's paths (D970).
//!
//! A dispatch takes an image descriptor it loads from a table it has no buffer for as the image's
//! name alone: the load reads nothing, and the host binds the image the load names. A read of
//! those registers as a value would read nothing the guest wrote, so it is refused. Whether a read
//! is one is a question about the program's paths, not about the order the translation emits its
//! blocks in: a load on one path does not reach a read on another, and a load reaches a read
//! through a branch back however the blocks are laid out. So the translation records what each
//! instruction does to the registers - a name-only load, a write, a value read - and this runs a
//! forward dataflow over the program's blocks, joining at merges, to find a read some name-only
//! load reaches with no write between.

use std::collections::BTreeMap;

use orbistoun_shader::{Decode, EncodingTable, Instruction};

use crate::blocks::{self, Terminator};

/// One register a bit, for the registers a `u128` numbers: the scalar file and the named halves
/// below 128.
pub type Registers = u128;

/// The program's blocks as the dataflow reads them: each block's instruction offsets in order and
/// the blocks it can go to next.
#[derive(Debug, Clone, Default)]
pub struct Flow {
    blocks: Vec<(Vec<u32>, Vec<usize>)>,
}

impl Flow {
    /// The blocks of `decode`, as the translation splits them; `None` where they cannot be split,
    /// which the translation refuses on its own.
    #[must_use]
    pub fn of(decode: &Decode, encodings: &EncodingTable) -> Option<Self> {
        let family_of = |instruction: &Instruction| {
            instruction
                .encoding
                .and_then(|e| encodings.encodings().get(usize::from(e)))
                .map(|e| e.name.clone())
        };
        let split = blocks::split(decode, family_of).ok()?;
        let blocks = split
            .iter()
            .map(|block| {
                let offsets = decode.instructions[block.first..block.end]
                    .iter()
                    .map(|instruction| instruction.offset)
                    .collect();
                let targets: Vec<u32> = match block.terminator {
                    Terminator::End => vec![],
                    Terminator::Jump { target } | Terminator::Fallthrough { next: target } => {
                        vec![target]
                    }
                    Terminator::Branch {
                        target,
                        fallthrough,
                    } => vec![target, fallthrough],
                };
                let next = targets
                    .into_iter()
                    .filter_map(|target| blocks::block_at(&split, target))
                    .collect();
                (offsets, next)
            })
            .collect();
        Some(Self { blocks })
    }

    /// Blocks given directly: each one's instruction offsets and the blocks it goes to next.
    #[must_use]
    pub fn from_blocks(blocks: Vec<(Vec<u32>, Vec<usize>)>) -> Self {
        Self { blocks }
    }
}

/// What the translation did to the registers, instruction by instruction.
#[derive(Debug, Clone, Default)]
pub struct NameOnly {
    flow: Option<Flow>,
    /// The instruction being translated, and what it did so far.
    current: Option<(u32, Effects)>,
    effects: BTreeMap<u32, Effects>,
}

/// One instruction's registers: those it reads as values, those it writes, and those a load leaves
/// holding only an image's name.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Effects {
    /// Read as values.
    pub reads: Registers,
    /// Written.
    pub writes: Registers,
    /// Left holding only a name.
    pub names: Registers,
}

impl NameOnly {
    /// Over the program's blocks.
    #[must_use]
    pub fn over(flow: Option<Flow>) -> Self {
        Self {
            flow,
            ..Self::default()
        }
    }

    /// The translation begins the instruction at `offset`.
    pub fn at(&mut self, offset: u32) {
        self.flush();
        self.current = Some((offset, Effects::default()));
    }

    fn flush(&mut self) {
        if let Some((offset, effects)) = self.current.take()
            && effects != Effects::default()
        {
            let held = self.effects.entry(offset).or_default();
            held.reads |= effects.reads;
            held.writes |= effects.writes;
            held.names |= effects.names;
        }
    }

    fn bit(register: u32) -> Registers {
        if register < Registers::BITS {
            1 << register
        } else {
            0
        }
    }

    /// The current instruction reads `register` as a value.
    pub fn read(&mut self, register: u32) {
        if let Some((_, effects)) = &mut self.current {
            effects.reads |= Self::bit(register);
        }
    }

    /// The current instruction writes `register`.
    pub fn wrote(&mut self, register: u32) {
        if let Some((_, effects)) = &mut self.current {
            effects.writes |= Self::bit(register);
        }
    }

    /// The current instruction leaves `registers` holding only an image's name.
    pub fn named(&mut self, registers: Registers) {
        if let Some((_, effects)) = &mut self.current {
            effects.names |= registers;
        }
    }

    /// The first instruction, by offset, that reads as a value a register some name-only load
    /// reaches along a path with no write between. Over no blocks, the instructions are taken in
    /// offset order as one block.
    #[must_use]
    pub fn first_read(&mut self) -> Option<u32> {
        self.flush();
        if self.effects.values().all(|effects| effects.names == 0) {
            return None;
        }
        let flow = self.flow.clone().unwrap_or_else(|| {
            Flow::from_blocks(vec![(self.effects.keys().copied().collect(), vec![])])
        });
        first_read(&flow, &self.effects)
    }
}

/// The dataflow itself: what may hold only a name entering each block, from nothing at the first,
/// joined over every block that goes to it, to a fixed point; then the first read, by offset, of a
/// register that may.
#[must_use]
pub fn first_read(flow: &Flow, effects: &BTreeMap<u32, Effects>) -> Option<u32> {
    let blocks = &flow.blocks;
    if blocks.is_empty() {
        return None;
    }
    let through = |offsets: &[u32], mut held: Registers, mut on_read: Option<&mut Vec<u32>>| {
        for offset in offsets {
            let Some(effects) = effects.get(offset) else {
                continue;
            };
            if effects.reads & held != 0
                && let Some(found) = on_read.as_deref_mut()
            {
                found.push(*offset);
            }
            held = (held & !effects.writes) | effects.names;
        }
        held
    };
    let mut entering: Vec<Registers> = vec![0; blocks.len()];
    let mut reached = vec![false; blocks.len()];
    reached[0] = true;
    let mut pending = vec![0_usize];
    while let Some(index) = pending.pop() {
        let leaving = through(&blocks[index].0, entering[index], None);
        for &next in &blocks[index].1 {
            let joined = entering[next] | leaving;
            if !reached[next] || joined != entering[next] {
                reached[next] = true;
                entering[next] = joined;
                pending.push(next);
            }
        }
    }
    let mut found = Vec::new();
    for (index, (offsets, _)) in blocks.iter().enumerate() {
        if reached[index] {
            through(offsets, entering[index], Some(&mut found));
        }
    }
    found.into_iter().min()
}

#[cfg(test)]
mod tests {
    use super::*;

    const S20: Registers = 1 << 20;
    const DESCRIPTOR: Registers = 0xff << 16;

    fn effects(list: &[(u32, Effects)]) -> BTreeMap<u32, Effects> {
        list.iter().copied().collect()
    }

    fn reads(registers: Registers) -> Effects {
        Effects {
            reads: registers,
            ..Effects::default()
        }
    }

    fn writes(registers: Registers) -> Effects {
        Effects {
            writes: registers,
            ..Effects::default()
        }
    }

    fn names(registers: Registers) -> Effects {
        Effects {
            names: registers,
            ..Effects::default()
        }
    }

    /// A load on one path does not reach a read on the other, though the load's block lies between
    /// them in offset order: `0x0` branches to `0x10` (the load) or falls into `0x4` (a write of
    /// `s20`, then a jump to `0x20`, the read).
    #[test]
    fn a_load_on_another_path_does_not_reach_a_read() {
        let flow = Flow::from_blocks(vec![
            (vec![0x0], vec![2, 1]),
            (vec![0x4, 0x8], vec![3]),
            (vec![0x10, 0x18], vec![]),
            (vec![0x20], vec![]),
        ]);
        let effects = effects(&[
            (0x4, writes(S20)),
            (0x10, names(DESCRIPTOR)),
            (0x20, reads(S20)),
        ]);
        assert_eq!(first_read(&flow, &effects), None);
    }

    /// A load reaches a read through a branch back, though a write lies between them in offset
    /// order: `0x0` writes `s20` and branches to `0x20` (the load) or falls into `0x8`, the read;
    /// `0x20` jumps back to `0x8`.
    #[test]
    fn a_load_reaches_a_read_through_a_branch_back() {
        let flow = Flow::from_blocks(vec![
            (vec![0x0, 0x4], vec![2, 1]),
            (vec![0x8, 0xc], vec![]),
            (vec![0x20, 0x28], vec![1]),
        ]);
        let effects = effects(&[
            (0x0, writes(S20)),
            (0x20, names(DESCRIPTOR)),
            (0x8, reads(S20)),
        ]);
        assert_eq!(first_read(&flow, &effects), Some(0x8));
    }

    /// A write on every path to the read clears the name; a write on one path of two does not.
    #[test]
    fn a_merge_holds_a_name_any_way_in_holds() {
        let flow = Flow::from_blocks(vec![
            (vec![0x0, 0x4], vec![2, 1]),
            (vec![0x8], vec![3]),
            (vec![0x10], vec![3]),
            (vec![0x18], vec![]),
        ]);
        let one_way = effects(&[
            (0x0, names(DESCRIPTOR)),
            (0x8, writes(S20)),
            (0x18, reads(S20)),
        ]);
        assert_eq!(first_read(&flow, &one_way), Some(0x18));
        let both_ways = effects(&[
            (0x0, names(DESCRIPTOR)),
            (0x8, writes(S20)),
            (0x10, writes(S20)),
            (0x18, reads(S20)),
        ]);
        assert_eq!(first_read(&flow, &both_ways), None);
    }

    /// An instruction reads its sources before it writes: one that reads a name and writes the
    /// register again is still a value read.
    #[test]
    fn a_read_comes_before_the_same_instruction_s_write() {
        let flow = Flow::from_blocks(vec![(vec![0x0, 0x8], vec![])]);
        let effects = effects(&[
            (0x0, names(DESCRIPTOR)),
            (
                0x8,
                Effects {
                    reads: S20,
                    writes: S20,
                    names: 0,
                },
            ),
        ]);
        assert_eq!(first_read(&flow, &effects), Some(0x8));
    }

    /// Recorded instruction by instruction as the translation runs, in whatever order it emits.
    #[test]
    fn effects_are_recorded_by_the_instruction_they_belong_to() {
        let flow = Flow::from_blocks(vec![
            (vec![0x0], vec![2, 1]),
            (vec![0x4, 0x8], vec![3]),
            (vec![0x10, 0x18], vec![]),
            (vec![0x20], vec![]),
        ]);
        let mut name_only = NameOnly::over(Some(flow));
        // Emitted with the load's block after the read's, then before it: the answer is the same.
        name_only.at(0x0);
        name_only.at(0x4);
        name_only.wrote(20);
        name_only.at(0x10);
        name_only.named(DESCRIPTOR);
        name_only.at(0x20);
        name_only.read(20);
        assert_eq!(name_only.first_read(), None);
    }
}
