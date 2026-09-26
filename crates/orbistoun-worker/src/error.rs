//! What can go wrong in worker mode, and the words each failure is reported in.
//!
//! One enum for the crate. The worker hands most of these to the shim as the reason a run
//! halted, so each variant renders exactly the sentence the run report carries.

use crate::watchpoint::MAX_WATCHPOINTS;

/// A worker-mode failure.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    /// The title's own modules could not be linked behind the import table.
    #[error("could not link the title: {0}")]
    Link(#[source] orbistoun_service::ServiceError),

    /// The process description (arguments, environment, auxiliary vector) is larger than the
    /// guest stack it has to be written onto.
    #[error("the description does not fit in the guest stack")]
    StackTooSmall,

    /// The built stack pointer is not an address this host can write through.
    #[error("stack pointer {0:#x} is not addressable")]
    StackPointer(u64),

    /// The run configuration file is malformed.
    #[error("reading the run configuration: {0}")]
    Configuration(#[source] orbistoun_service::ServiceError),

    /// The file of measurements this machine made could not be read.
    #[error("reading what was learned: {0}")]
    Learned(#[source] orbistoun_hle::HleError),

    /// The system settings file could not be read.
    #[error("reading what the console is set to: {0}")]
    ConsoleSettings(#[source] orbistoun_shell::ShellError),

    /// The protocol stream to the shim failed.
    #[error("worker loop: {0}")]
    WorkerLoop(#[source] std::io::Error),

    /// The watchpoints a run asked for could not be armed.
    #[error("the watchpoints could not be armed: {0}")]
    Watchpoints(#[source] Box<Error>),

    /// A watchpoint kind that is not `w`, `rw` or `x`.
    #[error("{0}: a watchpoint is w, rw or x")]
    WatchpointKind(String),

    /// A watchpoint address or length that is not a number.
    #[error("{0}: not a number")]
    NotANumber(String),

    /// A watchpoint length the debug registers cannot encode.
    #[error("{address:#x}+{length}: a watchpoint covers one, two, four or eight bytes")]
    WatchpointLength {
        /// The address asked for.
        address: u64,
        /// The length asked for.
        length: u64,
    },

    /// A watchpoint address not aligned to its length, which the hardware refuses.
    #[error(
        "{address:#x}+{length}: an {length}-byte watchpoint needs an {length}-byte-aligned address"
    )]
    WatchpointAlignment {
        /// The address asked for.
        address: u64,
        /// The length asked for.
        length: u64,
    },

    /// More watchpoints than the hardware has debug registers.
    #[error("{0} requested; the hardware has {MAX_WATCHPOINTS}")]
    TooManyWatchpoints(usize),

    /// No handle could be taken to the thread the watchpoints are for.
    #[error("could not take a handle to the thread about to run the guest")]
    ThreadHandle,

    /// The helper thread that writes the debug registers panicked.
    #[error("the arming thread died")]
    ArmingThreadDied,

    /// The thread could not be suspended to have its debug registers written.
    #[error("could not suspend the thread to set its debug registers")]
    SuspendThread,

    /// The thread's debug registers could not be read.
    #[error("could not read the thread's debug registers")]
    ReadDebugRegisters,

    /// The thread's debug registers could not be written.
    #[error("could not set the thread's debug registers")]
    SetDebugRegisters,

    /// The thread stayed suspended after its debug registers were written.
    #[error("the thread could not be resumed after arming")]
    ResumeThread,

    /// Watchpoints on a host where debug registers are not wired up.
    #[error("watchpoints need debug registers, which are only wired up on Windows")]
    WatchpointsUnsupported,

    /// A thread-local block larger than this host's address width.
    #[error("thread-local block does not fit a pointer")]
    TlsTooLarge,

    /// The memory for a thread-local block could not be reserved.
    #[error("could not reserve a thread-local block: {0}")]
    TlsReserve(#[source] orbistoun_mem::MemError),

    /// The thread pointer could not be installed on this host.
    #[error("could not install the thread pointer: {0}")]
    ThreadPointerInstall(orbistoun_abi::thread_pointer::Unsupported),

    /// The thread pointer read back differently from what was written.
    #[error("the thread pointer read back as {read:x?}, not the {written:#x} that was written")]
    ThreadPointerMismatch {
        /// What the thread pointer read back as, if it could be read at all.
        read: Option<u64>,
        /// What was written.
        written: u64,
    },

    /// The image's thread-local layout could not be read.
    #[error("could not read the thread-local layout: {0}")]
    TlsLayout(#[source] orbistoun_loader::LoadError),

    /// The stack fill diagnostic could not write the guest stack.
    #[error("could not fill the guest stack with {byte:#04x}: {source}")]
    StackFill {
        /// The byte the stack was to be filled with.
        byte: u8,
        /// Why it could not be.
        source: orbistoun_mem::MemError,
    },

    /// The pad script a run or its configuration names could not be read or validated.
    #[error(transparent)]
    PadScript(orbistoun_service::ServiceError),
}
