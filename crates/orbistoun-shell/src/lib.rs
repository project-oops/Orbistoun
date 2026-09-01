//! The system software: what runs when a title is not the whole machine.
//!
//! # Why a crate rather than a screen
//!
//! A console's shell looks like a user interface, and treating it as one gets the
//! architecture wrong immediately. Two thirds of what it does is invisible: a title that
//! is interrupted has to be *told*, an input the shell consumed must not reach the title,
//! and the settings a title reads are the ones a person chose in a menu. None of that is
//! drawing.
//!
//! So the model lives here, below the shims that expose it to the guest and below the
//! window that drives it. The window is a front-end onto this crate, and could be replaced
//! by a command line without any of the semantics moving (principle 12).
//!
//! # What is ours and what is the vendor's
//!
//! **The behaviour is ours; the identifiers are not.** That a backgrounded title should be
//! told it was backgrounded is a fact about operating systems. The *number* the vendor's
//! event carries is a fact about the vendor, and this repository has no lawful source for
//! it.
//!
//! That split runs through the whole crate. [`event::ShellEvent`] names events in our own
//! vocabulary and carries no codes. [`settings::Settings`] holds values a person chose, and
//! the parameter identifiers a guest asks by are a separate, empty, data-driven mapping.
//! Nothing here can deliver a guest an event or a setting it has not been *told* the code
//! for, and what it withholds it counts (principle 3).
//!
//! Which is the useful shape rather than a limitation: measured codes drop in as data
//! (principle 5), and until then the guest is told nothing rather than told something
//! invented.
//!
//! # Naming
//!
//! Principle 2: this holds no vendor product name. "The shell" is what the component is,
//! and the front-end built on it should be named after this project rather than after
//! anything it resembles - imitating a console's presentation is the one way to take a
//! clean-room design and give back the position it was built to hold.

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
