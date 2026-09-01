//! Declaring the storage buffers a translated module binds.
//!
//! Two of them, and both models declare both: an observation window the epilogue copies
//! registers into, and guest memory that loads and stores reach.
//!
//! # Why they are separate bindings
//!
//! One buffer split in half would be fewer descriptors and less code. It would also let
//! a guest address reach the observation window - and a store landing there would
//! rewrite the registers a test is about to assert on, so the failure would present as
//! a register bug. The addresses in these tests are chosen and would not do that today,
//! but the first real shader's addresses are not chosen by anyone here.

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
/// The shape is a struct containing an array, which is what a shader interface block
/// is. That extra level matters at the point of use: reaching a word takes **two**
/// indices - the member, always zero, then the element. Passing one index produces a
/// module that validates and faults the driver, which cost an afternoon to find once
/// already.
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

/// Binding of the observation window.
pub(crate) const OBSERVATION: u32 = 0;

/// Binding of guest memory.
pub(crate) const GUEST_MEMORY: u32 = 1;
