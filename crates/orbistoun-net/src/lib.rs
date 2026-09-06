//! Networking HLE - the libraries a title imports and nothing answers yet.
//!
//! # Why a crate with no implementations
//!
//! A title in the corpus imports **sixty-eight** functions across seven networking
//! libraries - HTTP, sockets, TLS, and the platform's account services - and orbistoun
//! declared none of them. Every one was reported as an unresolved import or, where the
//! library id did not map, as `unknown::`, which meant a guest reaching the network could
//! not be named or counted.
//!
//! This crate is the honest home for those names: it is where an implementation would go,
//! so putting the declarations anywhere else would have to be undone later. Nothing here is
//! implemented and every library is listed in `SERVES_NOTHING` with that reason - see
//! `orbistoun-gpu`'s `agc` module for the argument in full (D504).
//!
//! # Names confirmed, arities not
//!
//! Every name is read out of a real module's import table. The arities are `6`, the
//! trampoline's full capture, which is not a claim that these take six arguments: with
//! nothing established, recording every argument register loses no information where
//! guessing low discards it.
//!
//! # What networking is not going to be
//!
//! Worth saying now, because a networking stack is the kind of thing that grows by
//! accident. `docs/SCOPE.md` puts online services out of scope; what these declarations
//! buy is that a title asking for the network is **visible in a report** rather than dying
//! on an unresolved import, and that a guest which tolerates a refused connection can carry
//! on. Answering a socket call with success it can act on is a different project.

pub mod http;
pub mod http2;
pub mod netctl;
pub mod npmanager;
pub mod npwebapi2;
pub mod socket;
pub mod ssl;
