//! Where a title stands relative to the shell, and what follows from it.
//!
//! The model is a multitasker, as the modelled system is: a title stays alive while the
//! shell has the screen, so every subsystem asks who a frame, a button press and a thread's
//! execution belong to. [`Lifecycle`] is one value rather than a set of flags, so states
//! that do not exist cannot be represented, and [`Lifecycle::on`] refuses a request that
//! does not apply. [`Focus`], [`Video`] and [`Execution`] are derived from it, never stored.

use serde::{Deserialize, Serialize};

use crate::event::ShellEvent;

/// Where the title stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The title owns the screen and the controller. The ordinary case.
    Foreground,
    /// The title is still running and still presenting; the shell is drawn over it.
    ///
    /// The title keeps executing, so the overlay is drawn over live frames.
    Overlaid,
    /// The shell owns the screen. The title still exists and can be returned to.
    ///
    /// Whether it is still *executing* is a policy question - see [`WhenBackgrounded`].
    Background,
    /// There is no title.
    Exited,
}

/// What somebody asked the shell to do.
///
/// Serialisable, because the window and the guest are different processes (D032). This
/// type is the wire message itself, not a copy in `orbistoun-proto` that could drift.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Request {
    /// Draw the shell over the running title.
    OpenOverlay,
    /// Dismiss the overlay and give the title the controller back.
    CloseOverlay,
    /// Put the title behind the shell.
    ToShell,
    /// Bring the title back to the front.
    Resume,
    /// End the title.
    Quit,
}

/// Why a request was not carried out.
///
/// Named causes, so the front-end can say why a button did nothing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refused {
    /// The request asks for the state it is already in.
    AlreadyThere,
    /// There is no title to do this to.
    NoTitle,
    /// A legal request, but not from here.
    NotFromHere,
}

/// Whether a backgrounded title keeps running.
///
/// Suspend is the default: a title running behind the shell keeps submitting flips and
/// audio into surfaces nobody shows, and the video path does not guarantee those are
/// absorbed rather than acknowledged as presented. [`WhenBackgrounded::KeepRunning`] is
/// what the modelled system does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum WhenBackgrounded {
    /// Stop the guest threads until it is resumed.
    #[default]
    Suspend,
    /// Let it carry on executing behind the shell.
    KeepRunning,
}

/// Who receives controller input.
///
/// Derived from the lifecycle, never stored, so a title cannot act on a press the shell
/// consumed because two copies of the answer drifted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Focus {
    /// The title reads real pad state.
    Title,
    /// The shell reads it, and the title must see a neutral pad - see
    /// [`crate::session::Focus::neutral_for_title`].
    Shell,
}

impl Focus {
    /// Whether the title must be handed a neutral pad rather than the real one.
    ///
    /// Neutral, not disconnected: a title that loses its controller may stop or prompt for
    /// a reconnect, while a connected controller with nothing pressed is ordinary. Reading
    /// the real state instead would repeat whatever was held when the shell took focus.
    #[must_use]
    pub fn neutral_for_title(self) -> bool {
        matches!(self, Self::Shell)
    }
}

/// Who owns the presenting surface.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Video {
    /// The title's frames go to the screen.
    Title,
    /// The title presents and the shell is composited on top of it.
    TitleBehindShell,
    /// The shell presents. Title frames, if any are still being produced, go nowhere.
    Shell,
}

/// Whether the guest's threads should be executing.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Execution {
    /// Threads run.
    Running,
    /// Threads are stopped and can be started again.
    Suspended,
    /// There are no threads.
    Stopped,
}

/// A carried-out request: where it left the session, and what the guest must be told.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Taken {
    /// The new state.
    pub state: Lifecycle,
    /// Events the guest is owed, in the order they happened.
    ///
    /// A `Vec` because a transition raises at most two, at the rate a person presses
    /// buttons; this is not the allocation-free trace path.
    pub raise: Vec<ShellEvent>,
}

impl Lifecycle {
    /// Carries out a request, or says why it did not.
    ///
    /// # Errors
    ///
    /// [`Refused`] when the request does not apply from this state.
    pub fn on(self, request: Request) -> Result<Taken, Refused> {
        use Lifecycle::{Background, Exited, Foreground, Overlaid};
        use Request::{CloseOverlay, OpenOverlay, Quit, Resume, ToShell};

        let taken = |state, raise: &[ShellEvent]| {
            Ok(Taken {
                state,
                raise: raise.to_vec(),
            })
        };

        // Nothing is running, so nothing can be done to it; its own cause, because the
        // front-end acts on it differently from "already there".
        if self == Exited {
            return Err(Refused::NoTitle);
        }

        // Every live state answers Quit the same way, so it is handled once here.
        if request == Quit {
            return taken(Exited, &[ShellEvent::Quitting]);
        }

        match (self, request) {
            (Foreground, OpenOverlay) => taken(Overlaid, &[ShellEvent::FocusLost]),
            (Foreground, ToShell) => taken(
                Background,
                &[ShellEvent::FocusLost, ShellEvent::Backgrounded],
            ),

            (Overlaid, CloseOverlay) => taken(Foreground, &[ShellEvent::FocusGained]),
            // The title has already lost focus, so only the backgrounding is raised.
            (Overlaid, ToShell) => taken(Background, &[ShellEvent::Backgrounded]),

            (Background, Resume) => taken(
                Foreground,
                &[ShellEvent::Foregrounded, ShellEvent::FocusGained],
            ),

            // Asking for the current state; a front-end can ignore these.
            (Foreground, CloseOverlay | Resume)
            | (Overlaid, OpenOverlay)
            | (Background, ToShell) => Err(Refused::AlreadyThere),

            // Legal requests from the wrong state: the caller has lost track of where it
            // is, and treating one as a resume would bring the title forward unasked.
            (Overlaid, Resume) | (Background, OpenOverlay | CloseOverlay) => {
                Err(Refused::NotFromHere)
            }

            // Handled above; spelled out rather than a wildcard so a new variant of either
            // enum is a compile error.
            (Exited, _) | (_, Quit) => unreachable!("returned above"),
        }
    }

    /// Who receives controller input here.
    #[must_use]
    pub fn focus(self) -> Focus {
        match self {
            Self::Foreground => Focus::Title,
            Self::Overlaid | Self::Background | Self::Exited => Focus::Shell,
        }
    }

    /// Who owns the presenting surface here.
    #[must_use]
    pub fn video(self) -> Video {
        match self {
            Self::Foreground => Video::Title,
            Self::Overlaid => Video::TitleBehindShell,
            Self::Background | Self::Exited => Video::Shell,
        }
    }

    /// Whether the guest's threads should be executing here.
    #[must_use]
    pub fn execution(self, policy: WhenBackgrounded) -> Execution {
        match self {
            Self::Foreground | Self::Overlaid => Execution::Running,
            Self::Background => match policy {
                WhenBackgrounded::Suspend => Execution::Suspended,
                WhenBackgrounded::KeepRunning => Execution::Running,
            },
            Self::Exited => Execution::Stopped,
        }
    }

    /// Whether a title exists at all.
    #[must_use]
    pub fn has_title(self) -> bool {
        !matches!(self, Self::Exited)
    }
}

#[cfg(test)]
mod tests {
    use super::{Execution, Focus, Lifecycle, Refused, Request, Video, WhenBackgrounded};
    use crate::event::ShellEvent;

    /// Walks a sequence of requests, asserting each is accepted.
    fn walk(from: Lifecycle, requests: &[Request]) -> Lifecycle {
        requests.iter().fold(from, |state, request| {
            state
                .on(*request)
                .unwrap_or_else(|refused| panic!("{state:?} refused {request:?}: {refused:?}"))
                .state
        })
    }

    /// A title survives going behind the shell and can be brought back.
    #[test]
    fn a_title_survives_being_backgrounded_and_can_be_returned_to() {
        let after = walk(
            Lifecycle::Foreground,
            &[
                Request::ToShell,
                Request::Resume,
                Request::ToShell,
                Request::Resume,
            ],
        );
        assert_eq!(after, Lifecycle::Foreground);
        assert!(after.has_title());
    }

    /// The title keeps executing behind an overlay.
    #[test]
    fn an_overlay_leaves_the_title_running() {
        let overlaid = walk(Lifecycle::Foreground, &[Request::OpenOverlay]);

        assert_eq!(
            overlaid.execution(WhenBackgrounded::Suspend),
            Execution::Running
        );
        assert_eq!(overlaid.video(), Video::TitleBehindShell);
        assert_eq!(
            overlaid.focus(),
            Focus::Shell,
            "the title runs, but the buttons are the shell's"
        );
    }

    /// A backgrounded title is suspended by default.
    #[test]
    fn a_backgrounded_title_is_suspended_unless_policy_says_otherwise() {
        let background = walk(Lifecycle::Foreground, &[Request::ToShell]);

        assert_eq!(
            background.execution(WhenBackgrounded::default()),
            Execution::Suspended
        );
        assert_eq!(
            background.execution(WhenBackgrounded::KeepRunning),
            Execution::Running
        );
    }

    /// Focus is derived from the lifecycle, so title and shell never both hold input.
    #[test]
    fn only_a_foregrounded_title_reads_the_real_pad() {
        assert!(!Lifecycle::Foreground.focus().neutral_for_title());
        for state in [
            Lifecycle::Overlaid,
            Lifecycle::Background,
            Lifecycle::Exited,
        ] {
            assert!(
                state.focus().neutral_for_title(),
                "{state:?} must hand the title a neutral pad"
            );
        }
    }

    /// The guest is told when it loses and regains the machine.
    ///
    /// Asserted on the events rather than the state: a title backgrounded without being
    /// told resumes believing no time passed.
    #[test]
    fn a_title_is_told_when_it_is_backgrounded_and_when_it_comes_back() {
        let sent = Lifecycle::Foreground
            .on(Request::ToShell)
            .expect("the shell is reachable from the foreground");
        assert_eq!(
            sent.raise,
            vec![ShellEvent::FocusLost, ShellEvent::Backgrounded]
        );

        let back = sent
            .state
            .on(Request::Resume)
            .expect("resume is allowed from the background");
        assert_eq!(
            back.raise,
            vec![ShellEvent::Foregrounded, ShellEvent::FocusGained]
        );
    }

    /// Going to the shell from an overlay does not re-announce a focus loss.
    ///
    /// A title counting focus changes would otherwise pause twice.
    #[test]
    fn a_title_that_has_already_lost_focus_is_not_told_twice() {
        let raised = walk(Lifecycle::Foreground, &[Request::OpenOverlay]);
        let to_shell = raised.on(Request::ToShell).expect("the shell is reachable");

        assert_eq!(to_shell.raise, vec![ShellEvent::Backgrounded]);
    }

    /// Every refusal names its own cause, since a front-end acts on each differently.
    #[test]
    fn a_request_that_does_not_apply_is_refused_with_a_reason() {
        assert_eq!(
            Lifecycle::Exited.on(Request::Resume),
            Err(Refused::NoTitle),
            "there is nothing to resume"
        );
        assert_eq!(
            Lifecycle::Foreground.on(Request::Resume),
            Err(Refused::AlreadyThere)
        );
        assert_eq!(
            Lifecycle::Background.on(Request::OpenOverlay),
            Err(Refused::NotFromHere),
            "an overlay is drawn over a title, and the shell already has the screen"
        );
    }

    /// Exited is final: no request leaves it.
    #[test]
    fn nothing_brings_a_quit_title_back() {
        let gone = walk(Lifecycle::Foreground, &[Request::Quit]);
        assert!(!gone.has_title());
        assert_eq!(
            gone.execution(WhenBackgrounded::KeepRunning),
            Execution::Stopped
        );

        for request in [
            Request::OpenOverlay,
            Request::CloseOverlay,
            Request::ToShell,
            Request::Resume,
            Request::Quit,
        ] {
            assert_eq!(
                gone.on(request),
                Err(Refused::NoTitle),
                "{request:?} must not resurrect a title"
            );
        }
    }
}
