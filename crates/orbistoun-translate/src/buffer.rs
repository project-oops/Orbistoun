//! Declaring the storage buffers a translated module binds.
//!
//! Both models declare two: an observation window the epilogue copies registers into, and
//! guest memory that loads and stores reach. They are separate bindings so no guest address
//! can reach the observation window and rewrite the registers a test asserts on (D101).

use orbistoun_spirv::{Builder, Id, decoration, op, storage};

/// A storage buffer of words, and the pointer type for reaching one of them.
#[derive(Debug, Clone, Copy)]
pub(crate) struct StorageBuffer {
    /// The variable to bind.
    pub(crate) buffer: Id,
    /// Pointer to a single word within it.
    pub(crate) element_ptr: Id,
}

/// Declares a storage buffer of `count` words at `binding` in descriptor set zero.
///
/// The shape is a struct containing an array, as a shader interface block is, so reaching a
/// word takes two indices: the member (always zero), then the element. One index yields a
/// module that validates and faults the driver.
pub(crate) fn declare(b: &mut Builder, u32_type: Id, count: Id, binding: u32) -> StorageBuffer {
    let array = b.id();
    let block = b.id();
    let block_ptr = b.id();
    let element_ptr = b.id();
    let buffer = b.id();

    b.annotate(op::DECORATE, &[array.0, decoration::ARRAY_STRIDE, 4]);
    b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);
    b.annotate(op::DECORATE, &[buffer.0, decoration::DESCRIPTOR_SET, 0]);
    b.annotate(op::DECORATE, &[buffer.0, decoration::BINDING, binding]);
    if binding == GUEST_MEMORY {
        // Guest memory is shared with every other workgroup, as the console's is: a load the
        // guest marks `glc` re-reads it, and a spin on a flag another workgroup sets (a ticket
        // taken with an atomic, then a wait on the one before) must see the store. Without these
        // the host compiler may read the flag once and hoist it out of the loop, and the spin
        // never ends - GTA V's ray-tracing set-up lost the device that way.
        b.annotate(op::DECORATE, &[buffer.0, decoration::COHERENT]);
        b.annotate(op::DECORATE, &[buffer.0, decoration::VOLATILE]);
    }

    b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, count.0]);
    b.declare(op::TYPE_STRUCT, &[block.0, array.0]);
    b.declare(
        op::TYPE_POINTER,
        &[block_ptr.0, storage::STORAGE_BUFFER, block.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[element_ptr.0, storage::STORAGE_BUFFER, u32_type.0],
    );
    b.declare(
        op::VARIABLE,
        &[block_ptr.0, buffer.0, storage::STORAGE_BUFFER],
    );

    StorageBuffer {
        buffer,
        element_ptr,
    }
}

/// Declares the push-constant block of `count` words a module's user data is read from.
///
/// The same struct-of-array shape as [`declare`], reached with two indices; a push constant
/// has no descriptor set or binding, only its offset within the block.
pub(crate) fn declare_push_constants(b: &mut Builder, u32_type: Id, count: Id) -> StorageBuffer {
    let array = b.id();
    let block = b.id();
    let block_ptr = b.id();
    let element_ptr = b.id();
    let buffer = b.id();

    b.annotate(op::DECORATE, &[array.0, decoration::ARRAY_STRIDE, 4]);
    b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);

    b.declare(op::TYPE_ARRAY, &[array.0, u32_type.0, count.0]);
    b.declare(op::TYPE_STRUCT, &[block.0, array.0]);
    b.declare(
        op::TYPE_POINTER,
        &[block_ptr.0, storage::PUSH_CONSTANT, block.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[element_ptr.0, storage::PUSH_CONSTANT, u32_type.0],
    );
    b.declare(
        op::VARIABLE,
        &[block_ptr.0, buffer.0, storage::PUSH_CONSTANT],
    );

    StorageBuffer {
        buffer,
        element_ptr,
    }
}

/// A stage's bound draw buffers (D733): an array of storage blocks, each a runtime array of words
/// as long as the buffer bound there.
#[derive(Debug, Clone, Copy)]
pub(crate) struct DrawBufferArray {
    /// The variable to bind.
    pub(crate) variable: Id,
    /// Pointer to one buffer's block, for its length.
    pub(crate) block_ptr: Id,
    /// Pointer to a single word of one buffer.
    pub(crate) element_ptr: Id,
}

/// Declares a stage's draw buffers at `binding` of the draw-buffer set: read-only for a draw's, and
/// writable for a dispatch's, whose stores are written back (D746).
///
/// The array always has [`orbistoun_spirv::DRAW_BUFFERS_PER_STAGE`] elements, so every module that
/// binds any shares one layout; a word is reached with three indices: the buffer, the member, the
/// word.
pub(crate) fn declare_draw_buffers(
    b: &mut Builder,
    u32_type: Id,
    (binding, writable): (u32, bool),
) -> DrawBufferArray {
    declare_buffer_array(b, u32_type, (u32_type, 4), (binding, writable))
}

/// A dispatch's draw buffers at `binding` again, as runtime arrays of 64-bit words of type
/// `u64_type` rather than of 32-bit ones: the view a 64-bit atomic reaches its pair through as one
/// word. Both views are the one buffer bound at the binding, which the variables alias.
pub(crate) fn declare_draw_buffer_pairs(
    b: &mut Builder,
    u32_type: Id,
    u64_type: Id,
    binding: u32,
) -> DrawBufferArray {
    declare_buffer_array(b, u32_type, (u64_type, 8), (binding, true))
}

/// [`declare_draw_buffers`] over runtime arrays of `element`, `stride` bytes apart.
fn declare_buffer_array(
    b: &mut Builder,
    u32_type: Id,
    (element, stride): (Id, u32),
    (binding, writable): (u32, bool),
) -> DrawBufferArray {
    let runtime = b.id();
    let block = b.id();
    let count = b.id();
    let array = b.id();
    let array_ptr = b.id();
    let block_ptr = b.id();
    let element_ptr = b.id();
    let variable = b.id();

    b.annotate(op::DECORATE, &[runtime.0, decoration::ARRAY_STRIDE, stride]);
    b.annotate(op::DECORATE, &[block.0, decoration::BLOCK]);
    b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::OFFSET, 0]);
    if writable {
        // A dispatch's buffers are shared by all its workgroups, as guest memory is (see
        // [`declare`]): an atomic hands out tickets and a spin waits on a flag another workgroup
        // stores, so every access must reach memory. GTA V's decoupled look-back scan
        // (`image+0x3e16400`) spun forever on one hoisted out of its loop and lost the device.
        b.annotate(op::DECORATE, &[variable.0, decoration::COHERENT]);
        b.annotate(op::DECORATE, &[variable.0, decoration::VOLATILE]);
    } else {
        b.annotate(op::MEMBER_DECORATE, &[block.0, 0, decoration::NON_WRITABLE]);
    }
    b.annotate(
        op::DECORATE,
        &[
            variable.0,
            decoration::DESCRIPTOR_SET,
            orbistoun_spirv::DRAW_BUFFERS_SET,
        ],
    );
    b.annotate(op::DECORATE, &[variable.0, decoration::BINDING, binding]);

    b.declare(op::TYPE_RUNTIME_ARRAY, &[runtime.0, element.0]);
    b.declare(op::TYPE_STRUCT, &[block.0, runtime.0]);
    b.declare(
        op::CONSTANT,
        &[u32_type.0, count.0, orbistoun_spirv::DRAW_BUFFERS_PER_STAGE],
    );
    b.declare(op::TYPE_ARRAY, &[array.0, block.0, count.0]);
    b.declare(
        op::TYPE_POINTER,
        &[array_ptr.0, storage::STORAGE_BUFFER, array.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[block_ptr.0, storage::STORAGE_BUFFER, block.0],
    );
    b.declare(
        op::TYPE_POINTER,
        &[element_ptr.0, storage::STORAGE_BUFFER, element.0],
    );
    b.declare(
        op::VARIABLE,
        &[array_ptr.0, variable.0, storage::STORAGE_BUFFER],
    );

    DrawBufferArray {
        variable,
        block_ptr,
        element_ptr,
    }
}

/// Binding of the observation window.
pub(crate) const OBSERVATION: u32 = 0;

/// Binding of guest memory.
pub(crate) const GUEST_MEMORY: u32 = 1;

#[cfg(test)]
mod tests {
    use orbistoun_spirv::{Builder, Id, decoration, op};

    /// Whether `words` decorate `target` with `which` (`OpDecorate`, three words).
    fn decorates(words: &[u32], target: Id, which: u32) -> bool {
        words
            .windows(3)
            .any(|w| w == [3 << 16 | u32::from(op::DECORATE), target.0, which])
    }

    /// Guest memory is coherent and volatile, so a spin on a flag another workgroup sets sees the
    /// store; the observation window, which only this invocation writes, is not.
    #[test]
    fn guest_memory_is_coherent_and_volatile() {
        let mut b = Builder::new();
        let u32_type = b.id();
        let count = b.id();
        let memory = super::declare(&mut b, u32_type, count, super::GUEST_MEMORY);
        let observed = super::declare(&mut b, u32_type, count, super::OBSERVATION);
        let words = b.finish();
        assert!(decorates(&words, memory.buffer, decoration::COHERENT));
        assert!(decorates(&words, memory.buffer, decoration::VOLATILE));
        assert!(!decorates(&words, observed.buffer, decoration::COHERENT));
        assert!(!decorates(&words, observed.buffer, decoration::VOLATILE));
    }

    /// A dispatch's writable buffers are coherent and volatile, as guest memory is; a draw's
    /// read-only ones are not.
    #[test]
    fn a_dispatch_s_buffers_are_coherent_and_volatile() {
        let mut b = Builder::new();
        let u32_type = b.id();
        let written = super::declare_draw_buffers(&mut b, u32_type, (0, true));
        let read = super::declare_draw_buffers(&mut b, u32_type, (1, false));
        let words = b.finish();
        assert!(decorates(&words, written.variable, decoration::COHERENT));
        assert!(decorates(&words, written.variable, decoration::VOLATILE));
        assert!(!decorates(&words, read.variable, decoration::COHERENT));
        assert!(!decorates(&words, read.variable, decoration::VOLATILE));
    }
}
