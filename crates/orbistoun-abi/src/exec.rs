//! Executable memory for emitted code.
//!
//! Deliberately *not* fixed-address: placement is `orbistoun-mem`'s problem and is
//! already solved. This asks only "can emitted bytes run", so it takes whatever
//! address the OS offers.
//!
//! Write permission and execute permission are never held at the same time. The bytes
//! are written while the page is writable, then it is flipped to read-execute. That is
//! not ceremony - W^X is enforced by default on some platforms, and code that assumes
//! RWX works will fail there in a way that looks like a corrupt instruction stream.

use std::io;

/// A page of executable memory holding emitted code, released on drop.
#[derive(Debug)]
pub struct ExecutableBuffer {
    ptr: *mut u8,
    len: usize,
}

// SAFETY: the buffer owns its mapping exclusively and hands out only a raw pointer;
// there is no interior mutability and no shared state to race on.
unsafe impl Send for ExecutableBuffer {}

impl ExecutableBuffer {
    /// Copies `code` into fresh memory and makes it executable.
    ///
    /// # Errors
    ///
    /// Fails if the allocation or the protection change is refused.
    pub fn new(code: &[u8]) -> io::Result<Self> {
        imp::allocate(code)
    }

    /// Address of the first instruction.
    pub const fn as_ptr(&self) -> *const u8 {
        self.ptr.cast_const()
    }

    /// Address of the first instruction, as a guest-visible integer.
    ///
    /// The mapping is identity, so a host address and a guest address are the same
    /// number - which is what makes this meaningful rather than a cast for convenience.
    pub fn address(&self) -> u64 {
        self.ptr as usize as u64
    }

    /// Length of the emitted code.
    pub const fn len(&self) -> usize {
        self.len
    }

    /// Whether the buffer holds no code.
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }
}

impl Drop for ExecutableBuffer {
    fn drop(&mut self) {
        imp::release(self.ptr, self.len);
    }
}

#[cfg(windows)]
mod imp {
    use super::ExecutableBuffer;
    use std::io;
    use windows_sys::Win32::System::Memory::{
        MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_EXECUTE_READ, PAGE_READWRITE, VirtualAlloc,
        VirtualFree, VirtualProtect,
    };

    pub(super) fn allocate(code: &[u8]) -> io::Result<ExecutableBuffer> {
        if code.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no code to execute",
            ));
        }
        // SAFETY: a null base lets the OS choose the address; the call validates its
        // own arguments and returns null on failure.
        let ptr = unsafe {
            VirtualAlloc(
                std::ptr::null(),
                code.len(),
                MEM_RESERVE | MEM_COMMIT,
                PAGE_READWRITE,
            )
        };
        if ptr.is_null() {
            return Err(io::Error::last_os_error());
        }
        let ptr = ptr.cast::<u8>();

        // SAFETY: the allocation is at least `code.len()` bytes, writable, freshly
        // obtained, and cannot overlap the source slice.
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, code.len()) };

        let mut previous = 0_u32;
        // SAFETY: the range is exactly the allocation made above, and `previous` is a
        // valid out-parameter the call is required to write.
        let ok = unsafe {
            VirtualProtect(
                ptr.cast(),
                code.len(),
                PAGE_EXECUTE_READ,
                std::ptr::addr_of_mut!(previous),
            )
        };
        if ok == 0 {
            let err = io::Error::last_os_error();
            release(ptr, code.len());
            return Err(err);
        }
        Ok(ExecutableBuffer {
            ptr,
            len: code.len(),
        })
    }

    pub(super) fn release(ptr: *mut u8, _len: usize) {
        // SAFETY: the pointer came from VirtualAlloc above and is released once, from
        // Drop. MEM_RELEASE requires a size of zero.
        unsafe {
            VirtualFree(ptr.cast(), 0, MEM_RELEASE);
        }
    }
}

#[cfg(unix)]
mod imp {
    use super::ExecutableBuffer;
    use rustix::mm::{MapFlags, MprotectFlags, ProtFlags, mmap_anonymous, mprotect, munmap};
    use std::io;

    pub(super) fn allocate(code: &[u8]) -> io::Result<ExecutableBuffer> {
        if code.is_empty() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "no code to execute",
            ));
        }
        // SAFETY: a null hint lets the kernel choose; an anonymous private mapping
        // backs no file and aliases nothing.
        let ptr = unsafe {
            mmap_anonymous(
                std::ptr::null_mut(),
                code.len(),
                ProtFlags::READ.union(ProtFlags::WRITE),
                MapFlags::PRIVATE,
            )
        }
        .map_err(|e| io::Error::from_raw_os_error(e.raw_os_error()))?;
        let ptr = ptr.cast::<u8>();

        // SAFETY: the mapping is at least `code.len()` bytes, writable, freshly
        // obtained, and cannot overlap the source slice.
        unsafe { std::ptr::copy_nonoverlapping(code.as_ptr(), ptr, code.len()) };

        // W^X: drop write before adding execute, rather than ever holding both.
        // SAFETY: the range is exactly the mapping made above.
        let flipped = unsafe {
            mprotect(
                ptr.cast(),
                code.len(),
                MprotectFlags::READ.union(MprotectFlags::EXEC),
            )
        };
        if let Err(e) = flipped {
            release(ptr, code.len());
            return Err(io::Error::from_raw_os_error(e.raw_os_error()));
        }
        Ok(ExecutableBuffer {
            ptr,
            len: code.len(),
        })
    }

    pub(super) fn release(ptr: *mut u8, len: usize) {
        // SAFETY: the pointer and length come from the mapping above and are unmapped
        // once, from Drop.
        unsafe {
            let _ = munmap(ptr.cast(), len);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ExecutableBuffer;

    #[test]
    fn empty_code_is_refused_rather_than_producing_an_unusable_buffer() {
        assert!(ExecutableBuffer::new(&[]).is_err());
    }

    #[test]
    fn a_buffer_reports_the_length_it_was_given() {
        // 0x48 0xB8 .. is `movabs rax, 0`, then `ret` - a valid, harmless sequence, so
        // this stays a test about the buffer rather than about instruction encoding.
        let code = [0x48_u8, 0xB8, 0, 0, 0, 0, 0, 0, 0, 0, 0xC3];
        let buf = ExecutableBuffer::new(&code).expect("allocate");
        assert_eq!(buf.len(), code.len());
        assert!(!buf.is_empty());
        assert!(!buf.as_ptr().is_null());
    }
}
