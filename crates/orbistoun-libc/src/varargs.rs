//! Reading a guest's `va_list`.
//!
//! # Why this is the most-wanted function in the payload library
//!
//! `vsnprintf` is the single most imported name nothing here implements - twenty-two of
//! the twenty-five open-toolchain payloads measured ask for it, more than any other. It is
//! not decoration: it is what every one of their logging helpers is built out of, so a
//! payload that cannot render a message cannot say why it stopped.
//!
//! # What a `va_list` actually is here
//!
//! On this architecture it is not a pointer walking a stack. The System V AMD64 psABI
//! defines a four-field structure, and the arguments live in **two** places at once: the
//! first six integer arguments were spilled by the *caller's own prologue* into a register
//! save area, and everything past them sits on the stack.
//!
//! ```text
//! offset  0  gp_offset          u32   bytes already taken from the register save area
//! offset  4  fp_offset          u32   the same, for the floating-point half
//! offset  8  overflow_arg_area  ptr   the next stack argument
//! offset 16  reg_save_area      ptr   where the caller spilled its registers
//! ```
//!
//! Fetching an integer argument reads from the save area while `gp_offset` is below the
//! end of the integer half, and from the overflow area afterwards.
//!
//! # The capability this adds, stated plainly
//!
//! The register-based forms - `printf`, `snprintf` - see only what the trampoline caught
//! in registers, so a format with more conversions than that **cannot be rendered at all**
//! and is refused (`OutOfArguments`). A `va_list` has no such limit: it is a cursor over
//! the whole argument list, spill area and stack alike. The `v` forms are therefore not a
//! convenience wrapper over the others - they are the only ones that can render a long
//! format correctly (D364).
//!
//! # Why nothing is written back
//!
//! `va_arg` advances the caller's list, and a real `vsnprintf` does too. The C standard
//! says the value of `ap` is indeterminate after the call for exactly that reason, so a
//! conforming caller may not read it again - which makes advancing it unobservable, and
//! makes *not* advancing it the safer of two permitted behaviours. The cursor lives here
//! instead, and nothing writes into guest memory that did not have to be written.
//!
//! Reference: System V Application Binary Interface, AMD64 Architecture Processor
//! Supplement, section 3.5.7 (`va_list` and the register save area).

/// Bytes of the register save area belonging to integer arguments.
///
/// Six integer registers at eight bytes each. `gp_offset` counts up through this and
/// switches to the stack when it reaches the end, which is the whole of the rule.
const INTEGER_SAVE_AREA: u32 = 48;

/// Whether an area pointer could be one.
///
/// # Refusing beats faulting inside the renderer
///
/// A `va_list` names two areas, and a guest whose format asks for more arguments than it
/// passed walks off the end of the register half into the overflow pointer - which, in a list
/// that was never meant to be walked that far, holds whatever was in that stack slot.
///
/// The rule everywhere else here is that a guest passing a bad pointer faults exactly as it
/// would have on the machine this imitates. **This is the exception, and narrowly:** null and
/// all-ones are not addresses any program computed, they are what uninitialised and
/// error-returning code leaves behind, and dereferencing one crashes *inside the renderer* -
/// where the report says `vsnprintf` and means "the format wanted an eighth argument" (D378).
///
/// So those two are refused, the format is reported as out of arguments, and the guest gets
/// an empty string - which is bounded and wrong rather than a crash in this emulator's own
/// code. Every other value is dereferenced, exactly as before.
fn plausible(area: u64) -> bool {
    area != 0 && area != u64::MAX
}

/// A guest's `va_list`, read as a cursor rather than mutated in place.
///
/// Holds its own copy of the two moving fields, so a caller's structure is untouched -
/// see the module note on why that is permitted and preferred.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct VaList {
    /// Bytes already taken from the integer register save area.
    gp_offset: u32,
    /// The next stack argument.
    overflow: u64,
    /// Where the caller spilled its integer registers.
    save_area: u64,
}

impl VaList {
    /// Reads the structure a guest passed by address.
    ///
    /// Answers [`None`] for a list that cannot be one - null or all-ones - which is what a
    /// caller passing a register nothing set arrives with, and the case where reading on
    /// would fault inside formatting rather than reporting anything.
    ///
    /// # Safety
    ///
    /// `address`, when non-null, must point at a readable 24-byte `va_list` in guest
    /// memory - the same contract the real function has, under the identity mapping
    /// (D014).
    pub(crate) unsafe fn read(address: u64) -> Option<Self> {
        // The same guard the areas get, and for the same reason: the list *itself* arrives as
        // all-ones when a caller passes a register nothing set, and reading through it crashes
        // inside the renderer rather than reporting anything (D378).
        if !plausible(address) {
            return None;
        }
        let at = crate::ptr(address).cast_const();
        // SAFETY: the caller guarantees 24 readable bytes at `address`, and every field
        // below is read from inside that range at its psABI offset.
        let gp_offset = unsafe { std::ptr::read_unaligned(at.cast::<u32>()) };
        // SAFETY: the overflow pointer sits at offset 8, inside the same 24 bytes.
        let overflow_at = unsafe { at.add(8) };
        // SAFETY: `overflow_at` is in bounds by the line above, and eight bytes there are
        // readable by the caller's guarantee.
        let overflow = unsafe { std::ptr::read_unaligned(overflow_at.cast::<u64>()) };
        // SAFETY: the register save area pointer sits at offset 16, inside the same bytes.
        let save_area_at = unsafe { at.add(16) };
        // SAFETY: `save_area_at` is in bounds by the line above, and eight bytes there are
        // readable by the caller's guarantee.
        let save_area = unsafe { std::ptr::read_unaligned(save_area_at.cast::<u64>()) };
        Some(Self {
            gp_offset,
            overflow,
            save_area,
        })
    }

    /// Builds a list directly, for tests and for callers that already hold the fields.
    #[cfg(test)]
    pub(crate) const fn new(gp_offset: u32, overflow: u64, save_area: u64) -> Self {
        Self {
            gp_offset,
            overflow,
            save_area,
        }
    }

    /// The next integer-class argument.
    ///
    /// Answers [`None`] only when the area it would read from is null, which is a
    /// malformed list rather than an exhausted one: a real `va_list` has no end marker and
    /// a format asking for more than was passed is the caller's defect. Refusing there is
    /// what turns that defect into a reported fault instead of a read of whatever follows.
    pub(crate) fn next_integer(&mut self) -> Option<u64> {
        let from = if self.gp_offset < INTEGER_SAVE_AREA {
            if !plausible(self.save_area) {
                return None;
            }
            let at = self.save_area.checked_add(u64::from(self.gp_offset))?;
            self.gp_offset += 8;
            at
        } else {
            if !plausible(self.overflow) {
                return None;
            }
            let at = self.overflow;
            self.overflow = self.overflow.checked_add(8)?;
            at
        };
        // SAFETY: `from` is inside one of the two areas the guest's own list points at,
        // both of which it wrote and both of which hold at least the arguments it passed.
        // A guest whose format asks for more than it passed reads past them, exactly as it
        // would have on the machine this imitates.
        Some(unsafe { std::ptr::read_unaligned(crate::ptr(from).cast_const().cast::<u64>()) })
    }
}

#[cfg(test)]
mod tests {
    use super::{INTEGER_SAVE_AREA, VaList};

    /// A register save area holding six known words, and a stack area holding two more.
    ///
    /// Boxed so the addresses are real and stable, which is what the walk is about.
    fn areas() -> (Box<[u64; 6]>, Box<[u64; 2]>) {
        (Box::new([10, 11, 12, 13, 14, 15]), Box::new([100, 101]))
    }

    #[test]
    fn the_register_half_comes_first_and_in_order() {
        let (save, overflow) = areas();
        let mut list = VaList::new(0, overflow.as_ptr() as u64, save.as_ptr() as u64);
        let taken: Vec<u64> = (0..6).filter_map(|_| list.next_integer()).collect();
        assert_eq!(taken, vec![10, 11, 12, 13, 14, 15]);
    }

    /// **The reason this exists.** Six is where the register forms stop; a `va_list` keeps
    /// going onto the stack, and that is the capability the `v` forms add.
    #[test]
    fn the_seventh_argument_comes_from_the_stack() {
        let (save, overflow) = areas();
        let mut list = VaList::new(0, overflow.as_ptr() as u64, save.as_ptr() as u64);
        for _ in 0..6 {
            list.next_integer().expect("the register half");
        }
        assert_eq!(list.next_integer(), Some(100));
        assert_eq!(list.next_integer(), Some(101));
    }

    /// A caller that already spent some registers on its own fixed parameters starts
    /// partway in, which is the ordinary case: `vsnprintf` itself has three.
    #[test]
    fn a_partly_spent_list_resumes_where_it_left_off() {
        let (save, overflow) = areas();
        let mut list = VaList::new(24, overflow.as_ptr() as u64, save.as_ptr() as u64);
        assert_eq!(
            list.next_integer(),
            Some(13),
            "three registers already gone"
        );
    }

    /// The boundary itself, spelled out rather than left to the arithmetic.
    #[test]
    fn the_switch_to_the_stack_happens_exactly_at_the_end_of_the_registers() {
        let (save, overflow) = areas();
        let mut list = VaList::new(
            INTEGER_SAVE_AREA - 8,
            overflow.as_ptr() as u64,
            save.as_ptr() as u64,
        );
        assert_eq!(list.next_integer(), Some(15), "the last register");
        assert_eq!(list.next_integer(), Some(100), "then the stack");
    }

    /// A list that cannot be one is refused rather than read through.
    #[test]
    fn an_impossible_list_address_is_refused() {
        // SAFETY: neither address is read - both are the cases this answers without reading.
        assert_eq!(unsafe { VaList::read(0) }, None);
        // SAFETY: as above.
        // SAFETY: as above - all-ones is answered without being read.
        let impossible = unsafe { VaList::read(u64::MAX) };
        assert_eq!(
            impossible, None,
            "all-ones is what a caller passing a register nothing set arrives with"
        );
    }

    /// A malformed list - one whose areas are null - refuses rather than reading from zero.
    #[test]
    fn a_null_area_refuses_rather_than_dereferencing_it() {
        let mut no_registers = VaList::new(0, 0, 0);
        assert_eq!(no_registers.next_integer(), None);
        let mut no_stack = VaList::new(INTEGER_SAVE_AREA, 0, 0);
        assert_eq!(no_stack.next_integer(), None);
    }

    /// **All-ones is refused too**, and that one was found the hard way (D378).
    ///
    /// A guest whose format asks for more arguments than it passed walks past the register
    /// half into an overflow pointer nothing ever set. Dereferencing it crashes inside the
    /// renderer, where the report says `vsnprintf` and means something else entirely.
    #[test]
    fn an_all_ones_area_is_refused_rather_than_dereferenced() {
        let mut walked_off = VaList::new(INTEGER_SAVE_AREA, u64::MAX, u64::MAX);
        assert_eq!(walked_off.next_integer(), None);
        let mut from_the_start = VaList::new(0, 0, u64::MAX);
        assert_eq!(from_the_start.next_integer(), None);
    }

    /// Reading the structure back gives the fields the psABI puts at those offsets.
    #[test]
    fn the_structure_is_read_at_the_offsets_the_abi_states() {
        // gp_offset, fp_offset, overflow_arg_area, reg_save_area - 4, 4, 8, 8.
        let block: Box<[u64; 3]> = Box::new([
            u64::from(16_u32) | (u64::from(48_u32) << 32),
            0xDEAD_0000,
            0xBEEF_0000,
        ]);
        // SAFETY: `block` is 24 readable bytes laid out as a `va_list`.
        let list = unsafe { VaList::read(block.as_ptr() as u64) }.expect("non-null");
        assert_eq!(list, VaList::new(16, 0xDEAD_0000, 0xBEEF_0000));
    }
}
