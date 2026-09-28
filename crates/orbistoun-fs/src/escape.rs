//! Simulated kernel memory for the kernel read interface open-toolchain payloads use.
//!
//! The interface is built over a socket and pipe pair; payloads use it to walk kernel
//! structures (`allproc` -> `struct proc` -> `dynlib_obj`) and resolve library symbols such
//! as `sceKernelDlsym`. This module answers those reads from fixed synthetic tables.

use std::collections::BTreeMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Mutex, OnceLock};

static KERNEL_READ_ADDR: AtomicU64 = AtomicU64::new(0);
static KERNEL_WRITES: Mutex<Option<BTreeMap<u64, u8>>> = Mutex::new(None);

/// Sets the kernel address the next read through the socket option interface targets.
pub fn set_kernel_read_address(addr: u64) {
    KERNEL_READ_ADDR.store(addr, Ordering::SeqCst);
}

/// The kernel address the next read targets.
pub fn get_kernel_read_address() -> u64 {
    KERNEL_READ_ADDR.load(Ordering::SeqCst)
}

/// Clears the kernel read address and any written kernel memory.
pub fn clear() {
    set_kernel_read_address(0);
    if let Ok(mut guard) = KERNEL_WRITES.lock() {
        *guard = None;
    }
}

/// Writes `bytes.len()` bytes into simulated kernel memory starting at the targeted address.
pub fn write_kernel_pipe(bytes: &[u8]) -> usize {
    let addr = get_kernel_read_address();
    let len = bytes.len();
    if let Ok(mut guard) = KERNEL_WRITES.lock() {
        let map = guard.get_or_insert_with(BTreeMap::new);
        for (i, &byte) in bytes.iter().enumerate() {
            map.insert(addr.wrapping_add(i as u64), byte);
        }
    }
    KERNEL_READ_ADDR.fetch_add(len as u64, Ordering::SeqCst);
    len
}

static ACTIVE_CLIENT_PID: std::sync::atomic::AtomicI32 = std::sync::atomic::AtomicI32::new(100);

/// Sets the PID of the active client connecting for sandbox elevation.
pub fn set_active_client_pid(pid: i32) {
    ACTIVE_CLIENT_PID.store(pid, Ordering::SeqCst);
}

/// The PID of the active client.
pub fn get_active_client_pid() -> i32 {
    ACTIVE_CLIENT_PID.load(Ordering::SeqCst)
}

/// Canonical kernel data base address.
pub const KERNEL_DATA_BASE: u64 = 0xffff_ffff_8c29_0000;
/// Canonical kernel proc struct address for PID 1 (mini-syscore).
pub const KPROC_PID1_ADDR: u64 = 0xffff_cd61_5000_0000;
/// Canonical kernel proc struct address for self (daemon).
pub const KPROC_ADDR: u64 = 0xffff_cd61_5000_0800;
/// Canonical kernel proc struct address for client (target app).
pub const KPROC_CLIENT_ADDR: u64 = 0xffff_cd61_5000_1000;
/// Canonical kernel proc struct address for SceShellCore.
pub const KPROC_SHELLCORE_ADDR: u64 = 0xffff_cd61_5000_1800;
/// Canonical kernel ucred struct address.
pub const KUCRED_ADDR: u64 = 0xffff_cd61_5000_2000;
/// Canonical kernel filedesc struct address.
pub const KFILEDESC_ADDR: u64 = 0xffff_cd61_5000_2800;
/// Canonical root vnode address.
pub const ROOTVNODE_ADDR: u64 = 0xffff_cd61_6000_0000;
/// Canonical kernel dynlib linked list head address.
pub const DYNLIB_HEAD_ADDR: u64 = 0xffff_cd61_5000_3000;
/// Canonical kernel dynlib object for libkernel.
pub const DYNLIB_LIBKERNEL_ADDR: u64 = 0xffff_cd61_5000_3200;
/// Canonical kernel dynlib object for libc.
pub const DYNLIB_LIBC_ADDR: u64 = 0xffff_cd61_5000_3400;
/// Canonical kernel dynlib object for main executable.
pub const DYNLIB_MAIN_ADDR: u64 = 0xffff_cd61_5000_3600;
/// Canonical kernel dynlib path string address.
pub const DYNLIB_PATH_ADDR: u64 = 0xffff_cd61_5000_3800;
/// Canonical kernel dynlib linked list head address for SceShellCore.
pub const DYNLIB_SHELLCORE_HEAD_ADDR: u64 = 0xffff_cd61_5000_3a00;
/// Canonical kernel dynlib object for SceShellCore.
pub const DYNLIB_SHELLCORE_OBJ_ADDR: u64 = 0xffff_cd61_5000_3c00;
/// Canonical kernel dynlib path string address for SceShellCore.
pub const DYNLIB_SHELLCORE_PATH_ADDR: u64 = 0xffff_cd61_5000_3e00;
/// Canonical kernel RTLD meta address.
pub const RTLD_META_ADDR: u64 = 0xffff_cd61_5000_4000;
/// Canonical kernel dynlib object for libSceSysmodule.
pub const DYNLIB_SYSMODULE_ADDR: u64 = 0xffff_cd61_5000_4200;
/// Canonical kernel dynlib object for libSceNet.
pub const DYNLIB_SCENET_ADDR: u64 = 0xffff_cd61_5000_4400;
/// Canonical kernel dynlib object for libSceLibcInternal.
pub const DYNLIB_LIBCINT_ADDR: u64 = 0xffff_cd61_5000_4600;
/// Canonical kernel dynlib object for libkernel_web.
pub const DYNLIB_LIBKERNEL_WEB_ADDR: u64 = 0xffff_cd61_5000_4800;
/// Canonical kernel dynlib object for libkernel_sys.
pub const DYNLIB_LIBKERNEL_SYS_ADDR: u64 = 0xffff_cd61_5000_4a00;
/// Canonical kernel dynlib path string address for libc.
pub const DYNLIB_LIBC_PATH_ADDR: u64 = 0xffff_cd61_5000_4c00;
/// Canonical kernel dynlib path string address for main executable.
pub const DYNLIB_MAIN_PATH_ADDR: u64 = 0xffff_cd61_5000_4c40;
/// Canonical kernel dynlib path string address for libSceSysmodule.
pub const DYNLIB_SYSMODULE_PATH_ADDR: u64 = 0xffff_cd61_5000_4c80;
/// Canonical kernel dynlib path string address for libSceNet.
pub const DYNLIB_SCENET_PATH_ADDR: u64 = 0xffff_cd61_5000_4cc0;
/// Canonical kernel dynlib path string address for libSceLibcInternal.
pub const DYNLIB_LIBCINT_PATH_ADDR: u64 = 0xffff_cd61_5000_4d00;
/// Canonical kernel dynlib path string address for libkernel_web.
pub const DYNLIB_LIBKERNEL_WEB_PATH_ADDR: u64 = 0xffff_cd61_5000_4d40;
/// Canonical kernel dynlib path string address for libkernel_sys.
pub const DYNLIB_LIBKERNEL_SYS_PATH_ADDR: u64 = 0xffff_cd61_5000_4d80;
/// Canonical kernel dynlib object for libSceNetCtl.
pub const DYNLIB_NETCTL_ADDR: u64 = 0xffff_cd61_5000_8000;
/// Canonical kernel dynlib object for libSceUserService.
pub const DYNLIB_USERSERVICE_ADDR: u64 = 0xffff_cd61_5000_8200;
/// Canonical kernel dynlib object for libSceSystemService.
pub const DYNLIB_SYSTEMSERVICE_ADDR: u64 = 0xffff_cd61_5000_8400;
/// Canonical kernel dynlib object for libSceAppInstUtil.
pub const DYNLIB_APPINSTUTIL_ADDR: u64 = 0xffff_cd61_5000_8600;
/// Canonical kernel dynlib object for libSceHttp2.
pub const DYNLIB_HTTP2_ADDR: u64 = 0xffff_cd61_5000_8800;
/// Canonical kernel dynlib object for libSceSsl.
pub const DYNLIB_SSL_ADDR: u64 = 0xffff_cd61_5000_8a00;
/// Canonical kernel dynlib path string address for libSceNetCtl.
pub const DYNLIB_NETCTL_PATH_ADDR: u64 = 0xffff_cd61_5000_8c00;
/// Canonical kernel dynlib path string address for libSceUserService.
pub const DYNLIB_USERSERVICE_PATH_ADDR: u64 = 0xffff_cd61_5000_8c40;
/// Canonical kernel dynlib path string address for libSceSystemService.
pub const DYNLIB_SYSTEMSERVICE_PATH_ADDR: u64 = 0xffff_cd61_5000_8c80;
/// Canonical kernel dynlib path string address for libSceAppInstUtil.
pub const DYNLIB_APPINSTUTIL_PATH_ADDR: u64 = 0xffff_cd61_5000_8cc0;
/// Canonical kernel dynlib path string address for libSceHttp2.
pub const DYNLIB_HTTP2_PATH_ADDR: u64 = 0xffff_cd61_5000_8d00;
/// Canonical kernel dynlib path string address for libSceSsl.
pub const DYNLIB_SSL_PATH_ADDR: u64 = 0xffff_cd61_5000_8d40;
/// Canonical kernel symtab address.
pub const SYMTAB_ADDR: u64 = 0xffff_cd61_5001_0000;
/// Canonical kernel vmspace struct address.
pub const KVMSPACE_ADDR: u64 = 0xffff_cd61_5000_6000;
/// Canonical kernel IOMMU softc struct address.
pub const KIOMMU_SOFTC_ADDR: u64 = 0xffff_cd61_5000_7000;
/// Canonical kernel strtab address.
pub const STRTAB_ADDR: u64 = 0xffff_cd61_5002_0000;

/// Canonical kernel CR3 (physical page directory root) address.
pub const CR3_ADDR: u64 = 0x1000_0000;
/// Canonical kernel Direct Physical Memory Map (DMAP) base address.
pub const DMAP_ADDR: u64 = 0xffff_8000_0000_0000;
/// Canonical kernel PML4U address.
pub const PML4U_ADDR: u64 = DMAP_ADDR + CR3_ADDR;
/// Canonical kernel PDPT page table address.
pub const PDPT_PAGE_ADDR: u64 = DMAP_ADDR + 0x1000_1000;
/// Canonical DMAP address for SceShellCore text segment (1 GB superpage aligned).
pub const DMAP_SHELLCORE_ADDR: u64 = DMAP_ADDR + 0x4000_0000;
/// Size of DMAP mapping for SceShellCore text segment (64 MB).
pub const DMAP_SHELLCORE_SIZE: u64 = 0x0400_0000;

/// Known kernel allproc offsets across Prospero firmware releases.
pub const ALLPROC_OFFSETS: &[u64] = &[
    0x26D_1BF8, // FW 1.00 - 1.02
    0x26D_1C18, // FW 1.05 - 1.14
    0x270_1C28, // FW 2.00 - 2.70
    0x276_DC58, // FW 3.00 - 3.21
    0x27E_DCB8, // FW 4.00 - 4.51
    0x291_DD00, // FW 5.00 - 5.50
    0x286_9D20, // FW 6.00 - 6.50
    0x285_9D50, // FW 7.00 - 7.61
    0x287_5D50, // FW 8.00 - 8.60
    0x288_5E00, // FW 9.00 - 13.00+
];

struct KernelTables {
    symtab: Vec<u8>,
    strtab: Vec<u8>,
}

#[allow(clippy::too_many_lines)]
fn kernel_tables() -> &'static KernelTables {
    static TABLES: OnceLock<KernelTables> = OnceLock::new();
    TABLES.get_or_init(|| {
        let mut symtab = Vec::new();
        let mut strtab = Vec::new();

        // The string table starts with a NUL byte.
        strtab.push(0);

        // Symbols and their addresses.
        let symbols: &[(&str, u64)] = &[
            ("sceKernelDlsym", 0x135f0),
            ("sceKernelLoadStartModule", 0x16d90),
            ("sceKernelStopUnloadModule", 0x16d80),
            ("sceSysmoduleLoadModuleInternal", 0x16d70),
            ("getpid", 0x5b0),
            ("exit", 0x16dc0),
            ("sceKernelWrite", 0x16e00),
            ("sceKernelRead", 0x16dd0),
            ("sceKernelOpen", 0x16da0),
            ("sceKernelClose", 0x16db0),
            ("mmap", 0x135f0),
            ("munmap", 0x13600),
            ("mlock", 0x13610),
            ("setsockopt", 0xcb0),
            ("getsockopt", 0xcb8),
            ("getsockname", 0xcc8),
            ("socket", 0xc80),
            ("bind", 0xc90),
            ("listen", 0xca0),
            ("accept", 0xcc0),
            ("connect", 0xcd8),
            ("send", 0xcd0),
            ("recv", 0xce0),
            ("sendto", 0xce8),
            ("recvfrom", 0xcf8),
            ("sendfile", 0xd00),
            ("select", 0xcf0),
            ("poll", 0xd10),
            ("kevent", 0xd20),
            ("kqueue", 0xd30),
            ("freeifaddrs", 0xd40),
            ("getifaddrs", 0xd50),
            ("__inet_ntop", 0xd60),
            ("__inet_pton", 0xd70),
            ("sceNetInit", 0xd80),
            ("sceNetTerm", 0xd90),
            ("sceNetPoolCreate", 0xda0),
            ("sceNetPoolDestroy", 0xdb0),
            ("close", 0x16db0),
            ("read", 0x16dd0),
            ("write", 0x16e00),
            ("open", 0x16da0),
            ("pread", 0x16e10),
            ("pwrite", 0x16e20),
            ("_close", 0x16db0),
            ("_read", 0x16dd0),
            ("_open", 0x16da0),
            ("lseek", 0x16e30),
            ("stat", 0x16e40),
            ("lstat", 0x16e50),
            ("fstat", 0x16e60),
            ("mkdir", 0x16e70),
            ("rmdir", 0x16e80),
            ("unlink", 0x16e90),
            ("remove", 0x16ea0),
            ("rename", 0x16eb0),
            ("rewind", 0x16ec0),
            ("opendir", 0x16ed0),
            ("readdir", 0x16ee0),
            ("closedir", 0x16ef0),
            ("getcwd", 0x16f00),
            ("ftruncate", 0x16f10),
            ("chmod", 0x16f20),
            ("fileno", 0x16f30),
            ("fopen", 0x16f40),
            ("fclose", 0x16f50),
            ("fdopen", 0x16f60),
            ("fflush", 0x16f70),
            ("fgets", 0x16f80),
            ("fputs", 0x16f90),
            ("setvbuf", 0x16fa0),
            ("strcpy", 0x1000),
            ("strncpy", 0x1010),
            ("strcat", 0x1020),
            ("strncat", 0x1030),
            ("strcmp", 0x1040),
            ("strncmp", 0x1060),
            ("strcasecmp", 0x1068),
            ("strncasecmp", 0x1070),
            ("strlen", 0x1080),
            ("strnlen", 0x1088),
            ("strchr", 0x1090),
            ("strrchr", 0x1098),
            ("strstr", 0x109c),
            ("sprintf", 0x10a0),
            ("snprintf", 0x10c0),
            ("vsnprintf", 0x10d0),
            ("calloc", 0x10e0),
            ("malloc", 0x1100),
            ("realloc", 0x1110),
            ("free", 0x1120),
            ("memcpy", 0x1128),
            ("memset", 0x1130),
            ("memchr", 0x1138),
            ("getenv", 0x1140),
            ("setenv", 0x1148),
            ("unsetenv", 0x1150),
            ("getopt", 0x1160),
            ("optarg", 0x1340),
            ("atoi", 0x1180),
            ("atol", 0x1188),
            ("strtol", 0x1190),
            ("printf", 0x11a0),
            ("fprintf", 0x11b0),
            ("puts", 0x11c0),
            ("putchar", 0x11c8),
            ("sscanf", 0x11d0),
            ("isspace", 0x11d8),
            ("tolower", 0x11dc),
            ("kill", 0x11e0),
            ("signal", 0x1220),
            ("_sigaction", 0x1228),
            ("sigaction", 0x1228),
            ("sleep", 0x1230),
            ("usleep", 0x1238),
            ("waitpid", 0x123c),
            ("execve", 0x123e),
            ("dup2", 0x1242),
            ("rfork_thread", 0x1244),
            ("pthread_create", 0x1248),
            ("pthread_detach", 0x124c),
            ("pthread_exit", 0x1250),
            ("pthread_mutex_lock", 0x1254),
            ("pthread_mutex_unlock", 0x1258),
            ("gettimeofday", 0x125c),
            ("strerror", 0x1200),
            ("_Strerror", 0x1202),
            ("strerror_r", 0x1208),
            ("strftime", 0x1210),
            ("perror", 0x1218),
            ("__error", 0x1240),
            ("__stderrp", 0x1260),
            ("__stdoutp", 0x1280),
            ("__stdinp", 0x12a0),
            ("__isthreaded", 0x12c0),
            ("environ", 0x12e0),
            ("__progname", 0x1300),
            ("getargc", 0x1310),
            ("getargv", 0x1320),
            ("sysctl", 0x1330),
            ("sysctlbyname", 0x1338),
            ("sceKernelSendNotificationRequest", 0x17000),
            ("sceSysUtilSendSystemNotificationWithText", 0x17010),
            ("sceKernelUsleep", 0x17020),
            ("sceSysmoduleIsLoaded", 0x17030),
            ("sceSysmoduleLoadModule", 0x17040),
            ("sceSysmoduleUnloadModule", 0x17050),
            ("sceKernelAllocateDirectMemory", 0x17060),
            ("sceKernelAllocateMainDirectMemory", 0x17070),
            ("sceKernelReleaseDirectMemory", 0x17080),
            ("sceKernelMapDirectMemory", 0x17090),
            ("sceKernelMunmap", 0x170a0),
            ("sceKernelBatchMap", 0x170b0),
            ("sceKernelReserveVirtualRange", 0x170c0),
            ("sceKernelGetDirectMemorySize", 0x170d0),
            ("sceKernelGetProcessTime", 0x170e0),
            ("sceKernelGetProcessTimeCounter", 0x170f0),
            ("sceKernelGetProcessTimeCounterFrequency", 0x17100),
            ("sceKernelGetTscFrequency", 0x17110),
            ("sceKernelGetCpuFrequency", 0x17120),
            ("sceKernelGetCpuTemperature", 0x17130),
            ("sceKernelGetCurrentFanDuty", 0x17140),
            ("sceKernelGetSocSensorTemperature", 0x17150),
            ("sceKernelGetHwModelName", 0x17160),
            ("sceKernelGetHwSerialNumber", 0x17170),
            ("sceSystemServiceHideSplashScreen", 0x17180),
            ("sceSystemServiceLaunchApp", 0x17190),
            ("sceSystemServiceKillApp", 0x171a0),
            ("sceSystemServiceIsAppSuspended", 0x171b0),
            ("sceSystemServiceNavigateToGoHome", 0x171c0),
            ("sceSystemServicePowerTick", 0x171d0),
            ("sceSystemServiceParamGetInt", 0x171e0),
            ("sceSystemServiceGetAppIdOfBigApp", 0x171f0),
            ("sceSystemServiceGetMainAppTitleId", 0x17200),
            ("sceUserServiceInitialize", 0x17210),
            ("sceUserServiceGetInitialUser", 0x17220),
            ("sceUserServiceGetUserName", 0x17230),
            ("sceUserServiceGetLoginUserIdList", 0x17240),
            ("sceAgcDriverGetDefaultOwner", 0x17250),
            ("sceGnmSubmitDone", 0x17260),
            ("sceNetCtlInit", 0x17270),
            ("sceNetCtlGetInfo", 0x17280),
            ("sceNetCtlTerm", 0x17290),
            ("sceSystemServiceLaunchWebBrowser", 0x172a0),
            ("sceAppInstUtilInitialize", 0x172b0),
            ("sceAppInstUtilTerminate", 0x172c0),
            ("sceAppInstUtilAppInstallAll", 0x172d0),
            ("Wudg3Xe3heE", 0x172d8),
            ("sceLncUtilGetAppIdOfRunningBigApp", 0x172e0),
            ("sceLncUtilGetAppTitleId", 0x172f0),
            ("sceLncUtilKillApp", 0x17300),
            ("sceLncUtilSuspendApp", 0x17310),
            ("fread", 0x16fb0),
            ("fwrite", 0x16fc0),
            ("fseek", 0x16fd0),
            ("ftell", 0x16fe0),
            ("fseeko", 0x16ff0),
            ("feof", 0x16ff8),
            ("fgetc", 0x16d10),
            ("fputc", 0x16d20),
            ("fscanf", 0x16d30),
            ("setbuf", 0x16d40),
            ("access", 0x16d50),
            ("clock_gettime", 0x1350),
            ("fcntl", 0x16d60),
            ("localtime_s", 0x1360),
            ("memcmp", 0x112c),
            ("memmove", 0x1124),
            ("memrchr", 0x113c),
            ("pipe", 0x16d68),
            ("pthread_attr_destroy", 0x1260),
            ("pthread_attr_init", 0x1264),
            ("pthread_attr_setstacksize", 0x1268),
            ("pthread_cond_broadcast", 0x126c),
            ("pthread_cond_timedwait", 0x1270),
            ("pthread_equal", 0x1274),
            ("pthread_join", 0x1278),
            ("pthread_mutex_destroy", 0x127c),
            ("pthread_mutex_init", 0x1280),
            ("pthread_self", 0x1284),
            ("pthread_set_name_np", 0x1288),
            ("pthread_sigmask", 0x128c),
            ("qsort", 0x1370),
            ("realpath", 0x16f18),
            ("sceKernelGetAppInfo", 0x17320),
            ("sceNetErrnoLoc", 0xdc0),
            ("sceNetResolverCreate", 0xdd0),
            ("sceNetResolverDestroy", 0xde0),
            ("sceNetResolverStartAton", 0xdf0),
            ("sceNetResolverStartNtoa", 0xe00),
            ("sendmsg", 0xce4),
            ("recvmsg", 0xce5),
            ("shutdown", 0xce6),
            ("sigaddset", 0x122a),
            ("sigemptyset", 0x122c),
            ("strcspn", 0x10b0),
            ("strdup", 0x10b4),
            ("strlcpy", 0x1014),
            ("strspn", 0x10b8),
            ("strtok", 0x10bc),
            ("sysconf", 0x1380),
            ("time", 0x1390),
            ("vfprintf", 0x11b8),
            ("abort", 0x1290),
            ("ioctl", 0x16d78),
            ("_ioctl", 0x16d78),
            ("geteuid", 0x5c0),
            ("basename", 0x16f28),
            ("__inet_addr", 0xd64),
            ("__inet_aton", 0xd68),
            ("__udivti3", 0x13a0),
        ];

        let hasher = orbistoun_nid::NidHasher::default();

        let mut resolved_symbols = Vec::with_capacity(symbols.len());
        for &(name, fallback_vaddr) in symbols {
            let (vaddr, size) = if let Some(thunk) = orbistoun_thunk::name_thunk(name) {
                (thunk.wrapping_sub(0x8_0000_0000), 0x20_u64)
            } else if let Some(thunk) = orbistoun_thunk::declared_thunk(name) {
                (thunk.wrapping_sub(0x8_0000_0000), 0x20_u64)
            } else if let Some(data) = orbistoun_thunk::data_symbol(name) {
                (data.wrapping_sub(0x8_0000_0000), 0x20_u64)
            } else {
                (fallback_vaddr, 0x20_u64)
            };
            resolved_symbols.push((name, vaddr, size));
        }

        // 1. Sony NID encoded form (11 characters and a NUL). Placed first so that
        // loaders checking NIDs first find their exact NID match and do not false-match
        // an earlier raw symbol name that happens to share an 11-character prefix.
        for &(name, vaddr, size) in &resolved_symbols {
            let nid = hasher.hash(name);
            let encoded = orbistoun_nid::encode_nid(nid);
            let str_offset = strtab.len() as u32;
            strtab.extend_from_slice(encoded.as_bytes());
            strtab.push(0);

            let mut entry = [0u8; 24];
            entry[0..4].copy_from_slice(&str_offset.to_le_bytes());
            entry[8..16].copy_from_slice(&vaddr.to_le_bytes());
            entry[16..24].copy_from_slice(&size.to_le_bytes());
            symtab.extend_from_slice(&entry);
        }

        // 2. Raw name form (string and a NUL) for open-toolchain loaders resolving by name.
        for &(name, vaddr, size) in &resolved_symbols {
            let raw_offset = strtab.len() as u32;
            strtab.extend_from_slice(name.as_bytes());
            strtab.push(0);

            let mut raw_entry = [0u8; 24];
            raw_entry[0..4].copy_from_slice(&raw_offset.to_le_bytes());
            raw_entry[8..16].copy_from_slice(&vaddr.to_le_bytes());
            raw_entry[16..24].copy_from_slice(&size.to_le_bytes());
            symtab.extend_from_slice(&raw_entry);
        }

        KernelTables { symtab, strtab }
    })
}

/// Copies `buf` from `offset` into `out`, as far as either reaches: the tail every region of
/// the simulated kernel space shares.
fn copy_from(out: &mut [u8], buf: &[u8], offset: usize) {
    if offset < buf.len() {
        let copy_len = out.len().min(buf.len() - offset);
        out[..copy_len].copy_from_slice(&buf[offset..offset + copy_len]);
    }
}

/// One `struct dynlib_obj` the resolver walks: its `next` link and its handle over the fixed
/// fields the entries share: a path pointer, an image base, and the pointer to the shared
/// metadata block.
fn dynlib_obj(next: u64, handle: i32, path: u64) -> [u8; 0x200] {
    let mut buf = [0u8; 0x200];
    buf[0x00..0x08].copy_from_slice(&next.to_le_bytes());
    buf[0x08..0x10].copy_from_slice(&path.to_le_bytes());
    buf[0x28..0x2c].copy_from_slice(&handle.to_le_bytes());
    buf[0x30..0x38].copy_from_slice(&0x8_0000_0000_u64.to_le_bytes());
    buf[0x38..0x40].copy_from_slice(&0x1_0000_0000_u64.to_le_bytes());
    buf[0x148..0x150].copy_from_slice(&RTLD_META_ADDR.to_le_bytes());
    buf
}

/// Generates a `struct proc` buffer populated with credentials, vmspace, pid and candidate comm
/// names.
fn proc_struct(next: u64, pid: i32, comm: &[u8], dynlib: u64) -> [u8; 0x800] {
    let mut buf = [0u8; 0x800];
    buf[0x00..0x08].copy_from_slice(&next.to_le_bytes());
    buf[0x08..0x10].copy_from_slice(&KUCRED_ADDR.to_le_bytes());
    buf[0x40..0x48].copy_from_slice(&KUCRED_ADDR.to_le_bytes());
    buf[0x48..0x50].copy_from_slice(&KFILEDESC_ADDR.to_le_bytes());
    buf[0xbc..0xc0].copy_from_slice(&pid.to_le_bytes());
    buf[0x200..0x208].copy_from_slice(&KVMSPACE_ADDR.to_le_bytes());
    if dynlib != 0 {
        buf[0x3e8..0x3f0].copy_from_slice(&dynlib.to_le_bytes());
    }
    // Candidate p_comm offsets across FreeBSD / Prospero firmwares:
    // 0x274, 0x5dc, 0x5e4, 0x604, 0x61e
    for &off in &[0x274, 0x5dc, 0x5e4, 0x604, 0x61e] {
        let copy_len = comm.len().min(32);
        buf[off..off + copy_len].copy_from_slice(&comm[..copy_len]);
    }
    buf
}

/// Reads `out.len()` bytes of simulated kernel memory starting at the targeted address.
#[allow(clippy::too_many_lines)]
pub fn read_kernel_pipe(out: &mut [u8]) -> usize {
    let addr = get_kernel_read_address();
    let len = out.len();
    out.fill(0);

    // KERNEL_DATA_BASE region (`allproc` and `iommu_softc` lookup).
    if (KERNEL_DATA_BASE..KERNEL_DATA_BASE + 0x1000_0000).contains(&addr) {
        for (i, out_byte) in out.iter_mut().enumerate().take(len) {
            let cur_addr = addr.wrapping_add(i as u64);
            let cur_offset = cur_addr.wrapping_sub(KERNEL_DATA_BASE);
            let slot_offset = cur_offset & !7;
            let ptr = if slot_offset == 0x288_5df8
                || ALLPROC_OFFSETS
                    .iter()
                    .any(|&allproc| slot_offset == allproc.saturating_sub(8))
            {
                KIOMMU_SOFTC_ADDR
            } else {
                KPROC_PID1_ADDR
            };
            let byte_pos = (cur_addr % 8) as usize;
            *out_byte = ptr.to_le_bytes()[byte_pos];
        }
    }

    // KPROC_PID1_ADDR region (`struct proc` for PID 1, mini-syscore).
    if (KPROC_PID1_ADDR..KPROC_PID1_ADDR + 0x800).contains(&addr) {
        let proc = proc_struct(KPROC_ADDR, 1, b"mini-syscore\0", DYNLIB_HEAD_ADDR);
        copy_from(out, &proc, (addr - KPROC_PID1_ADDR) as usize);
    }

    // KPROC_ADDR region (`struct proc` for daemon).
    if (KPROC_ADDR..KPROC_ADDR + 0x800).contains(&addr) {
        let pid = std::process::id() as i32;
        let proc = proc_struct(KPROC_CLIENT_ADDR, pid, b"daemon\0", DYNLIB_HEAD_ADDR);
        copy_from(out, &proc, (addr - KPROC_ADDR) as usize);
    }

    // KPROC_CLIENT_ADDR region (`struct proc` for client app).
    if (KPROC_CLIENT_ADDR..KPROC_CLIENT_ADDR + 0x800).contains(&addr) {
        let pid = get_active_client_pid();
        let proc = proc_struct(KPROC_SHELLCORE_ADDR, pid, b"app\0", DYNLIB_HEAD_ADDR);
        copy_from(out, &proc, (addr - KPROC_CLIENT_ADDR) as usize);
    }

    // KPROC_SHELLCORE_ADDR region (`struct proc` for SceShellCore).
    if (KPROC_SHELLCORE_ADDR..KPROC_SHELLCORE_ADDR + 0x800).contains(&addr) {
        let proc = proc_struct(0, 2, b"SceShellCore\0", DYNLIB_SHELLCORE_HEAD_ADDR);
        copy_from(out, &proc, (addr - KPROC_SHELLCORE_ADDR) as usize);
    }

    // KFILEDESC_ADDR region (`struct filedesc`).
    if (KFILEDESC_ADDR..KFILEDESC_ADDR + 0x100).contains(&addr) {
        let offset = (addr - KFILEDESC_ADDR) as usize;
        let mut fd_buf = [0u8; 0x40];
        fd_buf[0x08..0x10].copy_from_slice(&ROOTVNODE_ADDR.to_le_bytes());
        fd_buf[0x10..0x18].copy_from_slice(&ROOTVNODE_ADDR.to_le_bytes());
        fd_buf[0x18..0x20].copy_from_slice(&ROOTVNODE_ADDR.to_le_bytes());
        copy_from(out, &fd_buf, offset);
    }

    // KUCRED_ADDR region (`struct ucred`).
    if (KUCRED_ADDR..KUCRED_ADDR + 0x100).contains(&addr) {
        let offset = (addr - KUCRED_ADDR) as usize;
        let mut ucred_buf = [0u8; 0x80];
        ucred_buf[0x30..0x38].copy_from_slice(&(KUCRED_ADDR + 0x80).to_le_bytes());
        ucred_buf[0x58..0x60].copy_from_slice(&0x4801_0000_0000_0013_u64.to_le_bytes());
        ucred_buf[0x60..0x70].copy_from_slice(&[0xff; 16]);
        copy_from(out, &ucred_buf, offset);
    }

    // ROOTVNODE_ADDR region (`struct vnode`).
    // Handled by 0-fill.

    // DYNLIB_HEAD_ADDR region.
    if (DYNLIB_HEAD_ADDR..DYNLIB_HEAD_ADDR + 0x100).contains(&addr) {
        copy_from(
            out,
            &DYNLIB_LIBKERNEL_ADDR.to_le_bytes(),
            (addr - DYNLIB_HEAD_ADDR) as usize,
        );
    }

    // The linked `dynlib_obj`s:
    // libkernel (0x2001) -> libc (2) -> main (1) -> libSceSysmodule (0x3001) ->
    // libSceNet (0x3002) -> libSceLibcInternal (0x3003) -> libkernel_web (0x3004) ->
    // libkernel_sys (0x3005) -> end (0).
    if (DYNLIB_LIBKERNEL_ADDR..DYNLIB_LIBKERNEL_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_LIBC_ADDR, 0x2001, DYNLIB_PATH_ADDR),
            (addr - DYNLIB_LIBKERNEL_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBC_ADDR..DYNLIB_LIBC_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_MAIN_ADDR, 2, DYNLIB_LIBC_PATH_ADDR),
            (addr - DYNLIB_LIBC_ADDR) as usize,
        );
    }
    if (DYNLIB_MAIN_ADDR..DYNLIB_MAIN_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_SYSMODULE_ADDR, 1, DYNLIB_MAIN_PATH_ADDR),
            (addr - DYNLIB_MAIN_ADDR) as usize,
        );
    }
    if (DYNLIB_SYSMODULE_ADDR..DYNLIB_SYSMODULE_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_SCENET_ADDR, 0x3001, DYNLIB_SYSMODULE_PATH_ADDR),
            (addr - DYNLIB_SYSMODULE_ADDR) as usize,
        );
    }
    if (DYNLIB_SCENET_ADDR..DYNLIB_SCENET_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_LIBCINT_ADDR, 0x3002, DYNLIB_SCENET_PATH_ADDR),
            (addr - DYNLIB_SCENET_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBCINT_ADDR..DYNLIB_LIBCINT_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_LIBKERNEL_WEB_ADDR, 0x3003, DYNLIB_LIBCINT_PATH_ADDR),
            (addr - DYNLIB_LIBCINT_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBKERNEL_WEB_ADDR..DYNLIB_LIBKERNEL_WEB_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(
                DYNLIB_LIBKERNEL_SYS_ADDR,
                0x3004,
                DYNLIB_LIBKERNEL_WEB_PATH_ADDR,
            ),
            (addr - DYNLIB_LIBKERNEL_WEB_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBKERNEL_SYS_ADDR..DYNLIB_LIBKERNEL_SYS_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_NETCTL_ADDR, 0x3005, DYNLIB_LIBKERNEL_SYS_PATH_ADDR),
            (addr - DYNLIB_LIBKERNEL_SYS_ADDR) as usize,
        );
    }
    if (DYNLIB_NETCTL_ADDR..DYNLIB_NETCTL_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_USERSERVICE_ADDR, 0x3006, DYNLIB_NETCTL_PATH_ADDR),
            (addr - DYNLIB_NETCTL_ADDR) as usize,
        );
    }
    if (DYNLIB_USERSERVICE_ADDR..DYNLIB_USERSERVICE_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(
                DYNLIB_SYSTEMSERVICE_ADDR,
                0x3007,
                DYNLIB_USERSERVICE_PATH_ADDR,
            ),
            (addr - DYNLIB_USERSERVICE_ADDR) as usize,
        );
    }
    if (DYNLIB_SYSTEMSERVICE_ADDR..DYNLIB_SYSTEMSERVICE_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(
                DYNLIB_APPINSTUTIL_ADDR,
                0x3008,
                DYNLIB_SYSTEMSERVICE_PATH_ADDR,
            ),
            (addr - DYNLIB_SYSTEMSERVICE_ADDR) as usize,
        );
    }
    if (DYNLIB_APPINSTUTIL_ADDR..DYNLIB_APPINSTUTIL_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_HTTP2_ADDR, 0x3009, DYNLIB_APPINSTUTIL_PATH_ADDR),
            (addr - DYNLIB_APPINSTUTIL_ADDR) as usize,
        );
    }
    if (DYNLIB_HTTP2_ADDR..DYNLIB_HTTP2_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(DYNLIB_SSL_ADDR, 0x300a, DYNLIB_HTTP2_PATH_ADDR),
            (addr - DYNLIB_HTTP2_ADDR) as usize,
        );
    }
    if (DYNLIB_SSL_ADDR..DYNLIB_SSL_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(0, 0x300b, DYNLIB_SSL_PATH_ADDR),
            (addr - DYNLIB_SSL_ADDR) as usize,
        );
    }

    // Path regions for each loaded dynlib object:
    if (DYNLIB_PATH_ADDR..DYNLIB_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libkernel.sprx\0",
            (addr - DYNLIB_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBC_PATH_ADDR..DYNLIB_LIBC_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libc.sprx\0",
            (addr - DYNLIB_LIBC_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_MAIN_PATH_ADDR..DYNLIB_MAIN_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/eboot.bin\0",
            (addr - DYNLIB_MAIN_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_SYSMODULE_PATH_ADDR..DYNLIB_SYSMODULE_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceSysmodule.sprx\0",
            (addr - DYNLIB_SYSMODULE_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_SCENET_PATH_ADDR..DYNLIB_SCENET_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceNet.sprx\0",
            (addr - DYNLIB_SCENET_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBCINT_PATH_ADDR..DYNLIB_LIBCINT_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceLibcInternal.sprx\0",
            (addr - DYNLIB_LIBCINT_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBKERNEL_WEB_PATH_ADDR..DYNLIB_LIBKERNEL_WEB_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libkernel_web.sprx\0",
            (addr - DYNLIB_LIBKERNEL_WEB_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_LIBKERNEL_SYS_PATH_ADDR..DYNLIB_LIBKERNEL_SYS_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libkernel_sys.sprx\0",
            (addr - DYNLIB_LIBKERNEL_SYS_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_NETCTL_PATH_ADDR..DYNLIB_NETCTL_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceNetCtl.sprx\0",
            (addr - DYNLIB_NETCTL_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_USERSERVICE_PATH_ADDR..DYNLIB_USERSERVICE_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceUserService.sprx\0",
            (addr - DYNLIB_USERSERVICE_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_SYSTEMSERVICE_PATH_ADDR..DYNLIB_SYSTEMSERVICE_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceSystemService.sprx\0",
            (addr - DYNLIB_SYSTEMSERVICE_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_APPINSTUTIL_PATH_ADDR..DYNLIB_APPINSTUTIL_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceAppInstUtil.sprx\0",
            (addr - DYNLIB_APPINSTUTIL_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_HTTP2_PATH_ADDR..DYNLIB_HTTP2_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceHttp2.sprx\0",
            (addr - DYNLIB_HTTP2_PATH_ADDR) as usize,
        );
    }
    if (DYNLIB_SSL_PATH_ADDR..DYNLIB_SSL_PATH_ADDR + 0x40).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/libSceSsl.sprx\0",
            (addr - DYNLIB_SSL_PATH_ADDR) as usize,
        );
    }

    // DYNLIB_SHELLCORE_HEAD_ADDR region.
    if (DYNLIB_SHELLCORE_HEAD_ADDR..DYNLIB_SHELLCORE_HEAD_ADDR + 0x100).contains(&addr) {
        copy_from(
            out,
            &DYNLIB_SHELLCORE_OBJ_ADDR.to_le_bytes(),
            (addr - DYNLIB_SHELLCORE_HEAD_ADDR) as usize,
        );
    }

    // DYNLIB_SHELLCORE_OBJ_ADDR region.
    if (DYNLIB_SHELLCORE_OBJ_ADDR..DYNLIB_SHELLCORE_OBJ_ADDR + 0x200).contains(&addr) {
        copy_from(
            out,
            &dynlib_obj(0, 1, DYNLIB_SHELLCORE_PATH_ADDR),
            (addr - DYNLIB_SHELLCORE_OBJ_ADDR) as usize,
        );
    }

    // DYNLIB_SHELLCORE_PATH_ADDR region.
    if (DYNLIB_SHELLCORE_PATH_ADDR..DYNLIB_SHELLCORE_PATH_ADDR + 0x100).contains(&addr) {
        copy_from(
            out,
            b"/system/common/lib/SceShellCore.elf\0",
            (addr - DYNLIB_SHELLCORE_PATH_ADDR) as usize,
        );
    }

    // RTLD_META_ADDR region.
    if (RTLD_META_ADDR..RTLD_META_ADDR + 0x200).contains(&addr) {
        let tables = kernel_tables();
        let offset = (addr - RTLD_META_ADDR) as usize;
        let mut meta_buf = vec![0u8; 0x120];
        meta_buf[0x28..0x30].copy_from_slice(&SYMTAB_ADDR.to_le_bytes());
        let symtab_size = tables.symtab.len() as u64;
        meta_buf[0x30..0x38].copy_from_slice(&symtab_size.to_le_bytes());
        meta_buf[0x38..0x40].copy_from_slice(&STRTAB_ADDR.to_le_bytes());
        let strtab_size = tables.strtab.len() as u64;
        meta_buf[0x40..0x48].copy_from_slice(&strtab_size.to_le_bytes());
        copy_from(out, &meta_buf, offset);
    }

    // SYMTAB_ADDR region.
    if (SYMTAB_ADDR..SYMTAB_ADDR + 0x1_0000).contains(&addr) {
        let tables = kernel_tables();
        copy_from(out, &tables.symtab, (addr - SYMTAB_ADDR) as usize);
    }

    // STRTAB_ADDR region.
    if (STRTAB_ADDR..STRTAB_ADDR + 0x2_0000).contains(&addr) {
        let tables = kernel_tables();
        copy_from(out, &tables.strtab, (addr - STRTAB_ADDR) as usize);
    }

    // KVMSPACE_ADDR region (`struct vmspace`).
    if (KVMSPACE_ADDR..KVMSPACE_ADDR + 0x500).contains(&addr) {
        let mut vmspace_buf = vec![0u8; 0x500];
        vmspace_buf[0x1c8..0x1d0].copy_from_slice(&ROOTVNODE_ADDR.to_le_bytes());
        vmspace_buf[0x1d0..0x1d8].copy_from_slice(&ROOTVNODE_ADDR.to_le_bytes());
        // pmap_off = 0x2e8: pml4u at +0x20, cr3 at +0x28
        vmspace_buf[0x308..0x310].copy_from_slice(&PML4U_ADDR.to_le_bytes());
        vmspace_buf[0x310..0x318].copy_from_slice(&CR3_ADDR.to_le_bytes());
        copy_from(out, &vmspace_buf, (addr - KVMSPACE_ADDR) as usize);
    }

    // KIOMMU_SOFTC_ADDR region (`struct amdvi_softc`).
    if (KIOMMU_SOFTC_ADDR..KIOMMU_SOFTC_ADDR + 0x100).contains(&addr) {
        let mut softc_buf = [0u8; 0x80];
        softc_buf[0x48..0x50].copy_from_slice(&0x0000_0000_fdd8_0000_u64.to_le_bytes());
        copy_from(out, &softc_buf, (addr - KIOMMU_SOFTC_ADDR) as usize);
    }

    // PML4 page table region (CR3).
    if (PML4U_ADDR..PML4U_ADDR + 0x1000).contains(&addr) {
        for (i, out_byte) in out.iter_mut().enumerate().take(len) {
            let byte_pos = ((addr + i as u64) % 8) as usize;
            *out_byte = 0x1000_1003_u64.to_le_bytes()[byte_pos];
        }
    }

    // PDPT page table region.
    // 0x4000_0083 has physical base 0x4000_0000 (1GB superpage aligned) with bit 7 (1GB),
    // bit 1 (writable), and bit 0 (present).
    if (PDPT_PAGE_ADDR..PDPT_PAGE_ADDR + 0x1000).contains(&addr) {
        for (i, out_byte) in out.iter_mut().enumerate().take(len) {
            let byte_pos = ((addr + i as u64) % 8) as usize;
            *out_byte = 0x4000_0083_u64.to_le_bytes()[byte_pos];
        }
    }

    // DMAP memory region (excluding PML4/PDPT page table pages).
    // Defaults to 0x90 (NOP opcodes) so blank-memory checks pass.
    if (DMAP_ADDR..DMAP_ADDR + 0x1_0000_0000).contains(&addr)
        && !(PML4U_ADDR..PML4U_ADDR + 0x1000).contains(&addr)
        && !(PDPT_PAGE_ADDR..PDPT_PAGE_ADDR + 0x1000).contains(&addr)
    {
        out.fill(0x90);
    }

    // Overlay any written bytes from guest write_kernel_pipe calls.
    if let Ok(guard) = KERNEL_WRITES.lock()
        && let Some(map) = guard.as_ref()
    {
        for (i, slot) in out.iter_mut().enumerate() {
            let target = addr.wrapping_add(i as u64);
            if let Some(&b) = map.get(&target) {
                *slot = b;
            }
        }
    }

    KERNEL_READ_ADDR.fetch_add(len as u64, Ordering::SeqCst);
    len
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Reads through the kernel pipe answer the `allproc` and `struct proc` layout.
    #[test]
    fn kernel_read_pipe_answers_allproc_and_proc() {
        let _guard = crate::exclusively();
        clear();
        set_kernel_read_address(KERNEL_DATA_BASE + 0x20000);
        let mut buf = [0u8; 8];
        assert_eq!(read_kernel_pipe(&mut buf), 8);
        let proc_ptr = u64::from_le_bytes(buf);
        assert_eq!(proc_ptr, KPROC_PID1_ADDR);

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

    /// Walking the dynlib linked list discovers all libraries and resolvable symbols.
    #[test]
    fn kernel_read_pipe_answers_dynlib_chain_and_symbols() {
        let _guard = crate::exclusively();
        clear();

        let expected = [
            (DYNLIB_LIBKERNEL_ADDR, 0x2001, "libkernel.sprx"),
            (DYNLIB_LIBC_ADDR, 2, "libc.sprx"),
            (DYNLIB_MAIN_ADDR, 1, "eboot.bin"),
            (DYNLIB_SYSMODULE_ADDR, 0x3001, "libSceSysmodule.sprx"),
            (DYNLIB_SCENET_ADDR, 0x3002, "libSceNet.sprx"),
            (DYNLIB_LIBCINT_ADDR, 0x3003, "libSceLibcInternal.sprx"),
            (DYNLIB_LIBKERNEL_WEB_ADDR, 0x3004, "libkernel_web.sprx"),
            (DYNLIB_LIBKERNEL_SYS_ADDR, 0x3005, "libkernel_sys.sprx"),
            (DYNLIB_NETCTL_ADDR, 0x3006, "libSceNetCtl.sprx"),
            (DYNLIB_USERSERVICE_ADDR, 0x3007, "libSceUserService.sprx"),
            (
                DYNLIB_SYSTEMSERVICE_ADDR,
                0x3008,
                "libSceSystemService.sprx",
            ),
            (DYNLIB_APPINSTUTIL_ADDR, 0x3009, "libSceAppInstUtil.sprx"),
            (DYNLIB_HTTP2_ADDR, 0x300a, "libSceHttp2.sprx"),
            (DYNLIB_SSL_ADDR, 0x300b, "libSceSsl.sprx"),
        ];

        set_kernel_read_address(DYNLIB_HEAD_ADDR);
        let mut ptr_buf = [0u8; 8];
        assert_eq!(read_kernel_pipe(&mut ptr_buf), 8);
        let mut cur = u64::from_le_bytes(ptr_buf);

        for (expected_addr, expected_handle, expected_name) in expected {
            assert_eq!(cur, expected_addr);

            set_kernel_read_address(cur);
            assert_eq!(read_kernel_pipe(&mut ptr_buf), 8);
            cur = u64::from_le_bytes(ptr_buf);

            set_kernel_read_address(expected_addr + 0x08);
            let mut path_ptr_buf = [0u8; 8];
            assert_eq!(read_kernel_pipe(&mut path_ptr_buf), 8);
            let path_ptr = u64::from_le_bytes(path_ptr_buf);

            set_kernel_read_address(path_ptr);
            let mut path_bytes = [0u8; 64];
            assert_eq!(read_kernel_pipe(&mut path_bytes), 64);
            let path_str = std::str::from_utf8(&path_bytes).unwrap();
            let nul_pos = path_str.find('\0').unwrap();
            assert!(path_str[..nul_pos].ends_with(expected_name));

            set_kernel_read_address(expected_addr + 0x28);
            let mut handle_buf = [0u8; 4];
            assert_eq!(read_kernel_pipe(&mut handle_buf), 4);
            assert_eq!(i32::from_le_bytes(handle_buf), expected_handle);

            set_kernel_read_address(expected_addr + 0x30);
            assert_eq!(read_kernel_pipe(&mut ptr_buf), 8);
            assert_eq!(u64::from_le_bytes(ptr_buf), 0x8_0000_0000);

            set_kernel_read_address(expected_addr + 0x38);
            assert_eq!(read_kernel_pipe(&mut ptr_buf), 8);
            assert_eq!(u64::from_le_bytes(ptr_buf), 0x1_0000_0000);

            set_kernel_read_address(expected_addr + 0x148);
            assert_eq!(read_kernel_pipe(&mut ptr_buf), 8);
            assert_eq!(u64::from_le_bytes(ptr_buf), RTLD_META_ADDR);
        }
        assert_eq!(cur, 0);

        set_kernel_read_address(RTLD_META_ADDR + 0x30);
        assert_eq!(read_kernel_pipe(&mut ptr_buf), 8);
        let symtab_size = u64::from_le_bytes(ptr_buf);
        assert!(symtab_size > 0);
        let num_syms = symtab_size / 24;
        assert!(num_syms >= 100);

        set_kernel_read_address(SYMTAB_ADDR + 16);
        let mut size_buf = [0u8; 8];
        assert_eq!(read_kernel_pipe(&mut size_buf), 8);
        assert_eq!(u64::from_le_bytes(size_buf), 0x20);
    }

    /// Reads and writes through simulated kernel memory for SceShellCore and DMAP page tables.
    #[test]
    fn kernel_read_pipe_answers_shellcore_and_dmap() {
        let _guard = crate::exclusively();
        clear();

        // 1. Process chain contains SceShellCore: PID 1 -> daemon -> client -> shellcore.
        set_kernel_read_address(KPROC_PID1_ADDR);
        let mut next_buf = [0u8; 8];
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), KPROC_ADDR);

        set_kernel_read_address(KPROC_ADDR);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), KPROC_CLIENT_ADDR);

        set_kernel_read_address(KPROC_CLIENT_ADDR);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), KPROC_SHELLCORE_ADDR);

        set_kernel_read_address(KPROC_SHELLCORE_ADDR);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), 0);

        // ShellCore comm name and PID.
        set_kernel_read_address(KPROC_SHELLCORE_ADDR + 0xbc);
        let mut pid_buf = [0u8; 4];
        read_kernel_pipe(&mut pid_buf);
        assert_eq!(i32::from_le_bytes(pid_buf), 2);

        set_kernel_read_address(KPROC_SHELLCORE_ADDR + 0x5e4);
        let mut comm_buf = [0u8; 13];
        read_kernel_pipe(&mut comm_buf);
        assert_eq!(&comm_buf, b"SceShellCore\0");

        // ShellCore dynlib points to SceShellCore module.
        set_kernel_read_address(KPROC_SHELLCORE_ADDR + 0x3e8);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), DYNLIB_SHELLCORE_HEAD_ADDR);

        set_kernel_read_address(DYNLIB_SHELLCORE_HEAD_ADDR);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), DYNLIB_SHELLCORE_OBJ_ADDR);

        set_kernel_read_address(DYNLIB_SHELLCORE_OBJ_ADDR + 0x08);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), DYNLIB_SHELLCORE_PATH_ADDR);

        set_kernel_read_address(DYNLIB_SHELLCORE_PATH_ADDR);
        let mut path_buf = [0u8; 35];
        read_kernel_pipe(&mut path_buf);
        assert_eq!(&path_buf, b"/system/common/lib/SceShellCore.elf");

        // 2. vmspace has pml4u and cr3 matching DMAP_BASE.
        set_kernel_read_address(KPROC_SHELLCORE_ADDR + 0x200);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), KVMSPACE_ADDR);

        set_kernel_read_address(KVMSPACE_ADDR + 0x308);
        read_kernel_pipe(&mut next_buf);
        let pml4u = u64::from_le_bytes(next_buf);
        assert_eq!(pml4u, PML4U_ADDR);

        set_kernel_read_address(KVMSPACE_ADDR + 0x310);
        read_kernel_pipe(&mut next_buf);
        let cr3 = u64::from_le_bytes(next_buf);
        assert_eq!(cr3, CR3_ADDR);
        assert_eq!(pml4u - cr3, DMAP_ADDR);

        // 3. IOMMU softc at allproc - 8 has marker at +0x48.
        set_kernel_read_address(KERNEL_DATA_BASE + 0x288_5df8);
        read_kernel_pipe(&mut next_buf);
        let softc_ptr = u64::from_le_bytes(next_buf);
        assert_eq!(softc_ptr, KIOMMU_SOFTC_ADDR);

        set_kernel_read_address(KIOMMU_SOFTC_ADDR + 0x48);
        read_kernel_pipe(&mut next_buf);
        assert_eq!(u64::from_le_bytes(next_buf), 0x0000_0000_fdd8_0000);

        // 4. Page table entries for 4-level translation.
        set_kernel_read_address(PML4U_ADDR + 16 * 8);
        read_kernel_pipe(&mut next_buf);
        let pml4_entry = u64::from_le_bytes(next_buf);
        assert_eq!(pml4_entry & 1, 1);

        set_kernel_read_address(PDPT_PAGE_ADDR + 32 * 8);
        read_kernel_pipe(&mut next_buf);
        let pdpte = u64::from_le_bytes(next_buf);
        assert_eq!(pdpte & 1, 1);
        assert_ne!(pdpte & 0x80, 0); // 1GB superpage bit
        assert_eq!(pdpte & 0x000f_ffff_c000_0000, 0x4000_0000); // 1GB-aligned physical base

        // 5. DMAP SceShellCore memory reading and patching.
        let target_pa = DMAP_SHELLCORE_ADDR + 0x00c8_70c3;
        set_kernel_read_address(target_pa);
        let mut cur_data = [0u8; 3];
        read_kernel_pipe(&mut cur_data);
        assert_eq!(cur_data, [0x90, 0x90, 0x90]);

        // Write patch bytes.
        set_kernel_read_address(target_pa);
        write_kernel_pipe(&[0x52, 0xeb, 0xe2]);

        // Read back and verify.
        set_kernel_read_address(target_pa);
        read_kernel_pipe(&mut cur_data);
        assert_eq!(cur_data, [0x52, 0xeb, 0xe2]);
    }
}
