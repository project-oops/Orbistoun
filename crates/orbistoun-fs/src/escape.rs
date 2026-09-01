//! Kernel memory emulation for payload escape primitives.
//!
//! Open-toolchain payloads use a kernel read/write primitive (built over a socket/pipe pair)
//! to walk kernel structures (`allproc` -> `struct proc` -> `dynlib_obj`) and resolve library
//! symbols like `sceKernelDlsym`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;

static KERNEL_READ_ADDR: AtomicU64 = AtomicU64::new(0);

/// Sets the current kernel address targeted by the escape setsockopt primitive.
pub fn set_kernel_read_address(addr: u64) {
    KERNEL_READ_ADDR.store(addr, Ordering::SeqCst);
}

/// Gets the current kernel address targeted by the escape primitive.
pub fn get_kernel_read_address() -> u64 {
    KERNEL_READ_ADDR.load(Ordering::SeqCst)
}

/// Canonical kernel data base address.
pub const KERNEL_DATA_BASE: u64 = 0xffff_ffff_8c29_0000;
/// Canonical kernel proc struct address.
pub const KPROC_ADDR: u64 = 0xffff_8661_5000_0000;
/// Canonical kernel ucred struct address.
pub const KUCRED_ADDR: u64 = 0xffff_8661_5000_1000;
/// Canonical kernel dynlib linked list head address.
pub const DYNLIB_HEAD_ADDR: u64 = 0xffff_8661_5000_1f00;
/// Canonical kernel dynlib object for libkernel.
pub const DYNLIB_LIBKERNEL_ADDR: u64 = 0xffff_8661_5000_2000;
/// Canonical kernel dynlib object for libc.
pub const DYNLIB_LIBC_ADDR: u64 = 0xffff_8661_5000_2200;
/// Canonical kernel dynlib object for main executable.
pub const DYNLIB_MAIN_ADDR: u64 = 0xffff_8661_5000_2400;
/// Canonical kernel dynlib path string address.
pub const DYNLIB_PATH_ADDR: u64 = 0xffff_8661_5000_2800;
/// Canonical kernel RTLD meta address.
pub const RTLD_META_ADDR: u64 = 0xffff_8661_5000_3000;
/// Canonical kernel symtab address.
pub const SYMTAB_ADDR: u64 = 0xffff_8661_5000_4000;
/// Canonical kernel strtab address.
pub const STRTAB_ADDR: u64 = 0xffff_8661_5000_8000;

struct KernelTables {
    symtab: Vec<u8>,
    strtab: Vec<u8>,
}

fn kernel_tables() -> &'static KernelTables {
    static TABLES: OnceLock<KernelTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut symtab = Vec::new();
        let mut strtab = Vec::new();

        // Initial 0 byte in strtab
        strtab.push(0);

        // Symbols with their confirmed/measured vaddrs
        let symbols: &[(&str, u64)] = &[
            ("sceKernelDlsym", 0x135f0),
            ("sceKernelLoadStartModule", 0x16d90),
            ("sceKernelStopUnloadModule", 0x16d80),
            ("getpid", 0x5b0),
            ("exit", 0x16dc0),
            ("sceKernelWrite", 0x16e00),
            ("sceKernelRead", 0x16dd0),
            ("sceKernelOpen", 0x16da0),
            ("sceKernelClose", 0x16db0),
            ("mmap", 0x135f0),
            ("munmap", 0x13600),
            ("setsockopt", 0xcb0),
            ("getsockopt", 0xcb8),
            ("socket", 0xc80),
            ("bind", 0xc90),
            ("listen", 0xca0),
            ("accept", 0xcc0),
            ("send", 0xcd0),
            ("recv", 0xce0),
            ("select", 0xcf0),
            ("close", 0x16db0),
            ("read", 0x16dd0),
            ("write", 0x16e00),
            ("open", 0x16da0),
            ("strcpy", 0x1000),
            ("strcat", 0x1020),
            ("strcmp", 0x1040),
            ("strncmp", 0x1060),
            ("strlen", 0x1080),
            ("sprintf", 0x10a0),
            ("snprintf", 0x10c0),
            ("calloc", 0x10e0),
            ("malloc", 0x1100),
            ("free", 0x1120),
            ("getenv", 0x1140),
            ("getopt", 0x1160),
            ("atoi", 0x1180),
            ("printf", 0x11a0),
            ("puts", 0x11c0),
            ("kill", 0x11e0),
            ("strerror", 0x1200),
            ("signal", 0x1220),
            ("__error", 0x1240),
            ("__stderrp", 0x1260),
            ("__stdoutp", 0x1280),
            ("__stdinp", 0x12a0),
            ("__isthreaded", 0x12c0),
            ("environ", 0x12e0),
            ("getargc", 0x1300),
            ("getargv", 0x1320),
        ];

        let hasher = orbistoun_nid::NidHasher::default();

        for &(name, vaddr) in symbols {
            // 1. Sony NID encoded form (11 chars + null)
            let nid = hasher.hash(name);
            let encoded = orbistoun_nid::encode_nid(nid);
            let str_offset = strtab.len() as u32;
            strtab.extend_from_slice(encoded.as_bytes());
            strtab.push(0);

            let mut entry = [0u8; 24];
            entry[0..4].copy_from_slice(&str_offset.to_le_bytes());
            entry[8..16].copy_from_slice(&vaddr.to_le_bytes());
            symtab.extend_from_slice(&entry);

            // 2. Raw name form (string + null)
            let raw_offset = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);

            let mut raw_entry = [0u8; 24];
            raw_entry[0..4].copy_from_slice(&raw_offset.to_le_bytes());
            raw_entry[8..16].copy_from_slice(&vaddr.to_le_bytes());
            symtab.extend_from_slice(&raw_entry);
        }

        KernelTables { symtab, strtab }
    })
}

/// Copies `buf` from `offset` into `out`, as far as either reaches - the tail every region of the
/// simulated kernel space shares.
fn copy_from(out: &mut [u8], buf: &[u8], offset: usize) {
    if offset < buf.len() {
        let copy_len = out.len().min(buf.len() - offset);
        out[..copy_len].copy_from_slice(&buf[offset..offset + copy_len]);
    }
}

/// One `struct dynlib_obj` the resolver walks: its `next` link and its handle over the fixed fields
/// the three real entries (libkernel, libc, main) share - a path pointer, an image base, and the
/// pointer to the shared metadata block.
fn dynlib_obj(next: u64, handle: i32) -> [u8; 0x200] {
    let mut buf = [0u8; 0x200];
    buf[0x00..0x08].copy_from_slice(&next.to_le_bytes());
    buf[0x08..0x10].copy_from_slice(&DYNLIB_PATH_ADDR.to_le_bytes());
    buf[0x28..0x2c].copy_from_slice(&handle.to_le_bytes());
    buf[0x30..0x38].copy_from_slice(&0x8_0000_0000_u64.to_le_bytes());
    buf[0x148..0x150].copy_from_slice(&RTLD_META_ADDR.to_le_bytes());
    buf
}

/// Reads `out.len()` bytes from the simulated kernel memory space starting at `get_kernel_read_address()`.
pub fn read_kernel_pipe(out: &mut [u8]) -> usize {
    let addr = get_kernel_read_address();
    let len = out.len();
    out.fill(0);

    // KERNEL_DATA_BASE region (allproc lookup)
    if (KERNEL_DATA_BASE..KERNEL_DATA_BASE + 0x1000_0000).contains(&addr) {
        let ptr = KPROC_ADDR;
        let bytes = ptr.to_le_bytes();
        let copy_len = len.min(bytes.len());
        out[..copy_len].copy_from_slice(&bytes[..copy_len]);
        return len;
    }

    // KPROC_ADDR region (struct proc)
    if (KPROC_ADDR..KPROC_ADDR + 0x1000).contains(&addr) {
        let offset = (addr - KPROC_ADDR) as usize;
        let mut proc_buf = vec![0u8; 0x500];

        // proc + 0x08: p_ucred
        proc_buf[0x08..0x10].copy_from_slice(&KUCRED_ADDR.to_le_bytes());
        // proc + 0x40: ucred offset
        proc_buf[0x40..0x48].copy_from_slice(&KUCRED_ADDR.to_le_bytes());
        // proc + 0xbc: pid
        let pid = std::process::id() as i32;
        proc_buf[0xbc..0xc0].copy_from_slice(&pid.to_le_bytes());
        // proc + 0x3e8: p_dynlib (LIST_HEAD pointer)
        proc_buf[0x3e8..0x3f0].copy_from_slice(&DYNLIB_HEAD_ADDR.to_le_bytes());

        if offset < proc_buf.len() {
            let available = proc_buf.len() - offset;
            let copy_len = len.min(available);
            out[..copy_len].copy_from_slice(&proc_buf[offset..offset + copy_len]);
        }
        return len;
    }

    // DYNLIB_HEAD_ADDR region (LIST_HEAD pointing to first dynlib_obj)
    if (DYNLIB_HEAD_ADDR..DYNLIB_HEAD_ADDR + 0x100).contains(&addr) {
        copy_from(
            out,
            &DYNLIB_LIBKERNEL_ADDR.to_le_bytes(),
            (addr - DYNLIB_HEAD_ADDR) as usize,
        );
        return len;
    }

    // The three linked `dynlib_obj`s: libkernel (handle 0x2001) -> libc (2) -> main (1) -> end.
    if (DYNLIB_LIBKERNEL_ADDR..DYNLIB_LIBKERNEL_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_LIBC_ADDR, 0x2001),
            (addr - DYNLIB_LIBKERNEL_ADDR) as usize,
        );
        return len;
    }
    if (DYNLIB_LIBC_ADDR..DYNLIB_LIBC_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_MAIN_ADDR, 2),
            (addr - DYNLIB_LIBC_ADDR) as usize,
        );
        return len;
    }
    if (DYNLIB_MAIN_ADDR..DYNLIB_MAIN_ADDR + 0x200).contains(&addr) {
        copy_from(out, &dynlib_obj(0, 1), (addr - DYNLIB_MAIN_ADDR) as usize);
        return len;
    }

    // DYNLIB_PATH_ADDR region
    if (DYNLIB_PATH_ADDR..DYNLIB_PATH_ADDR + 0x100).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libkernel.sprx\0",
            (addr - DYNLIB_PATH_ADDR) as usize,
        );
        return len;
    }

    // RTLD_META_ADDR region
    if (RTLD_META_ADDR..RTLD_META_ADDR + 0x200).contains(&addr) {
        let tables = kernel_tables();
        let offset = (addr - RTLD_META_ADDR) as usize;
        let mut meta_buf = vec![0u8; 0x120];

        // meta + 0x28: symtab_addr
        meta_buf[0x28..0x30].copy_from_slice(&SYMTAB_ADDR.to_le_bytes());
        // meta + 0x30: symtab_size
        let symtab_size = tables.symtab.len() as u64;
        meta_buf[0x30..0x38].copy_from_slice(&symtab_size.to_le_bytes());
        // meta + 0x38: strtab_addr
        meta_buf[0x38..0x40].copy_from_slice(&STRTAB_ADDR.to_le_bytes());
        // meta + 0x40: strtab_size
        let strtab_size = tables.strtab.len() as u64;
        meta_buf[0x40..0x48].copy_from_slice(&strtab_size.to_le_bytes());

        if offset < meta_buf.len() {
            let available = meta_buf.len() - offset;
            let copy_len = len.min(available);
            out[..copy_len].copy_from_slice(&meta_buf[offset..offset + copy_len]);
        }
        return len;
    }

    // SYMTAB_ADDR region
    if (SYMTAB_ADDR..SYMTAB_ADDR + 0x4000).contains(&addr) {
        let tables = kernel_tables();
        let offset = (addr - SYMTAB_ADDR) as usize;
        if offset < tables.symtab.len() {
            let available = tables.symtab.len() - offset;
            let copy_len = len.min(available);
            out[..copy_len].copy_from_slice(&tables.symtab[offset..offset + copy_len]);
        }
        return len;
    }

    // STRTAB_ADDR region
    if (STRTAB_ADDR..STRTAB_ADDR + 0x8000).contains(&addr) {
        let tables = kernel_tables();
        let offset = (addr - STRTAB_ADDR) as usize;
        if offset < tables.strtab.len() {
            let available = tables.strtab.len() - offset;
            let copy_len = len.min(available);
            out[..copy_len].copy_from_slice(&tables.strtab[offset..offset + copy_len]);
        }
        return len;
    }

    len
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kernel_read_pipe_answers_allproc_and_proc() {
        set_kernel_read_address(KERNEL_DATA_BASE + 0x20000);
        let mut buf = [0u8; 8];
        assert_eq!(read_kernel_pipe(&mut buf), 8);
        let proc_ptr = u64::from_le_bytes(buf);
        assert_eq!(proc_ptr, KPROC_ADDR);

        set_kernel_read_address(KPROC_ADDR + 0x3e8);
        let mut head_buf = [0u8; 8];
        assert_eq!(read_kernel_pipe(&mut head_buf), 8);
        let head_ptr = u64::from_le_bytes(head_buf);
        assert_eq!(head_ptr, DYNLIB_HEAD_ADDR);

        set_kernel_read_address(DYNLIB_HEAD_ADDR);
        let mut dynlib_buf = [0u8; 8];
        assert_eq!(read_kernel_pipe(&mut dynlib_buf), 8);
        let dynlib_ptr = u64::from_le_bytes(dynlib_buf);
        assert_eq!(dynlib_ptr, DYNLIB_LIBKERNEL_ADDR);

        set_kernel_read_address(DYNLIB_LIBKERNEL_ADDR + 0x28);
        let mut handle_buf = [0u8; 4];
        assert_eq!(read_kernel_pipe(&mut handle_buf), 4);
        let handle = i32::from_le_bytes(handle_buf);
        assert_eq!(handle, 0x2001);
    }
}

