//! What orbistoun says when a driver asks it the conformance probe's questions.
//!
//! Here rather than in a shim because both shims serve it, the CLI over a socket and the window
//! from a menu. The first `part` written after negotiation says `kind=emulator`, unprompted:
//! a driver comparing this with a probe must know which end is the reference, and the one thing
//! this end can certify is that it is not the platform (D225).

use std::collections::hash_map::RandomState;
use std::hash::{BuildHasher as _, Hasher as _};

use orbistoun_probe::respond::Answers;
use orbistoun_probe::{Capability, Record, Refusal};

use crate::Service;

/// Answers the command protocol out of a [`Service`].
///
/// Only [`Capability::Report`] is announced. `call` and `read` need a loaded, running guest, and
/// this borrows a service rather than a run, so announcing them would offer in `hello` what every
/// later command refuses.
#[derive(Debug)]
pub struct ServiceAnswers<'a> {
    service: &'a Service,
    session: String,
    secret: Option<String>,
}

impl<'a> ServiceAnswers<'a> {
    /// Answers from this service, with a freshly generated secret.
    ///
    /// For a single session. A listener that accepts more than one wants [`Self::generate_secret`]
    /// and [`Self::with_secret`], because the secret has to exist before anybody can present it. A
    /// secret is never compiled in, since everyone holding the binary would share it.
    pub fn new(service: &'a Service) -> Self {
        Self {
            service,
            session: token(),
            secret: Some(token()),
        }
    }

    /// Answers with a secret somebody else already generated and displayed.
    ///
    /// Per startup, not per connection: the listener generates it once, shows it once, and every
    /// session checks against it. The session identifier is fresh each time, so a driver can tell a
    /// reconnection from a continuation.
    pub fn with_secret(service: &'a Service, secret: Option<String>) -> Self {
        Self {
            service,
            session: token(),
            secret,
        }
    }

    /// A secret to display once and then hand to [`Self::with_secret`].
    #[must_use]
    pub fn generate_secret() -> String {
        token()
    }

    /// Answers with no secret required.
    ///
    /// For a responder bound to the loopback interface by somebody who started it deliberately.
    /// Anything reachable from a network wants [`Self::new`].
    pub fn unauthenticated(service: &'a Service) -> Self {
        Self {
            service,
            session: token(),
            secret: None,
        }
    }

    /// The secret a caller must present, for whoever has to display it.
    pub fn key(&self) -> Option<&str> {
        self.secret.as_deref()
    }
}

impl Answers for ServiceAnswers<'_> {
    fn capabilities(&self) -> Vec<Capability> {
        vec![Capability::Report]
    }

    fn session(&self) -> String {
        self.session.clone()
    }

    fn secret(&self) -> Option<String> {
        self.secret.clone()
    }

    fn describe(&self) -> Vec<(String, String)> {
        vec![
            // First, and unprompted (D225).
            ("kind".to_owned(), "emulator".to_owned()),
            ("name".to_owned(), "orbistoun".to_owned()),
            ("build".to_owned(), orbistoun_env::build::line()),
        ]
    }

    fn report(&mut self) -> Result<Vec<Record>, Refusal> {
        let declared = self.service.declared_symbols();
        let implemented = declared.iter().filter(|s| s.implemented).count();
        let mut records = vec![
            Record::Build {
                build: orbistoun_env::build::line(),
                // `host` rather than `module` or `payload`: this is not running on the target.
                kind: "host".to_owned(),
            },
            // A `sym` record has no field for whether a handler is attached behind a declared name,
            // so the counts are stated here rather than smuggled into a field that means linkage.
            Record::SysInfo {
                field: "symbols-declared".to_owned(),
                state: "known".to_owned(),
                value: declared.len().to_string(),
            },
            Record::SysInfo {
                field: "symbols-implemented".to_owned(),
                state: "known".to_owned(),
                value: implemented.to_string(),
            },
        ];
        records.extend(declared.into_iter().map(|symbol| Record::Sym {
            library: symbol.library,
            symbol: symbol.symbol,
            // The name is one orbistoun declares. Whether the platform has it is the other end's
            // answer.
            presence: "present".to_owned(),
            // Linkage, not implementation status, which the counts above carry.
            availability: "shared".to_owned(),
        }));
        Ok(records)
    }
}

/// A token that differs between runs.
///
/// [`RandomState`] is seeded by the operating system. Best-effort, not cryptographic: the socket is
/// cleartext and the secret is readable in the `hello` that presents it. It stops an unattended
/// scan from finding an open responder, nothing more.
fn token() -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u32(std::process::id());
    hasher.write_u64(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos() as u64),
    );
    format!("{:016x}", hasher.finish())
}

#[cfg(test)]
mod tests {
    use super::{Answers as _, ServiceAnswers};
    use crate::{Service, ServiceConfig};

    fn service() -> Service {
        Service::new(ServiceConfig::default())
    }

    /// The first thing a driver is told is that this is not the platform.
    #[test]
    fn it_announces_itself_as_an_emulator_before_anything_else() {
        let service = service();
        let answers = ServiceAnswers::unauthenticated(&service);
        let described = answers.describe();
        assert_eq!(
            described.first().map(|(k, v)| (k.as_str(), v.as_str())),
            Some(("kind", "emulator"))
        );
    }

    /// Nothing is offered that would then be refused.
    #[test]
    fn every_announced_capability_is_one_it_can_serve() {
        let service = service();
        let mut answers = ServiceAnswers::unauthenticated(&service);
        for capability in answers.capabilities() {
            match capability {
                orbistoun_probe::Capability::Report => {
                    assert!(answers.report().is_ok(), "report was announced and refused");
                }
                other => panic!("announced {} with nothing behind it", other.token()),
            }
        }
    }

    /// A report carries every declared name, and says how many have a handler.
    #[test]
    fn the_report_carries_the_symbols_and_counts_what_is_implemented() {
        let service = service();
        let declared = service.declared_symbols();
        let mut answers = ServiceAnswers::unauthenticated(&service);
        let records = answers.report().expect("reports");

        let symbols = records
            .iter()
            .filter(|r| matches!(r, orbistoun_probe::Record::Sym { .. }))
            .count();
        assert_eq!(symbols, declared.len(), "not every name was reported");

        let counted = records.iter().find_map(|r| match r {
            orbistoun_probe::Record::SysInfo { field, value, .. }
                if field == "symbols-implemented" =>
            {
                value.parse::<usize>().ok()
            }
            _ => None,
        });
        assert_eq!(
            counted,
            Some(declared.iter().filter(|s| s.implemented).count()),
            "the implemented count disagrees with the registry"
        );
    }

    /// Two responders never share a secret, and never share a session identifier.
    #[test]
    fn a_secret_is_generated_per_responder_and_never_compiled_in() {
        let service = service();
        let one = ServiceAnswers::new(&service);
        let two = ServiceAnswers::new(&service);
        assert!(one.key().is_some(), "a secret was expected");
        assert_ne!(
            one.key(),
            two.key(),
            "the secret is fixed across responders"
        );
        assert_ne!(
            one.session(),
            two.session(),
            "two responders claimed the same session"
        );
    }
}
