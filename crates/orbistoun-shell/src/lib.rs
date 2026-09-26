//! The system software model: events a title is told, input the shell consumes, and the
//! settings a title reads.
//!
//! This sits below the shims that expose it to the guest and below the window that drives
//! it, so a command line could replace the window without moving any semantics.
//!
//! The behaviour is ours; the vendor's identifiers are not. [`event::ShellEvent`] names
//! events in our own vocabulary and carries no codes, and the parameter identifiers a guest
//! asks [`settings::Settings`] by are a separate data-driven mapping. A guest is never
//! delivered an event or setting whose code is unknown, and each one withheld is counted.

pub mod cross;
pub mod event;
pub mod profiles;
pub mod session;
pub mod settings;
pub mod startup;

pub use cross::{Cross, Move};
pub use event::{Delivery, EventQueue, ShellEvent, Withheld};
pub use session::{Execution, Focus, Lifecycle, Refused, Request, Taken, Video, WhenBackgrounded};
pub use settings::{Answer, ButtonAssignment, Parameters, Settings, User};
pub use startup::{Refusal, Start, View};

/// Why a file this crate owns could not be read or written.
#[derive(Debug, thiserror::Error)]
pub enum ShellError {
    /// The file exists and could not be read.
    #[error("reading {}: {source}", path.display())]
    Read {
        /// The file.
        path: std::path::PathBuf,
        /// What the filesystem said.
        source: std::io::Error,
    },
    /// The file was read and is not the format it should be.
    #[error("parsing {}: {source}", path.display())]
    Parse {
        /// The file.
        path: std::path::PathBuf,
        /// What the parser said, boxed to keep the error small.
        source: Box<toml::de::Error>,
    },
    /// The contents could not be rendered as TOML.
    #[error("serialising TOML: {0}")]
    Toml(#[from] toml::ser::Error),
}
