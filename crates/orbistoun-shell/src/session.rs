//! Where a title stands relative to the shell, and what follows from it.
//!
//! # The decision this file encodes
//!
//! A launcher and a multitasker are different emulators, and the difference is entirely
//! here. A launcher has two states - a title is running or it is not - and "return to the
//! shell" means the process died. A multitasker keeps the title alive while something else
//! has the screen, which means every subsystem now has to answer a question it never had
//! to: *who is this frame for, who is this button for, is this thread supposed to be
//! executing right now.*
//!
//! This crate takes the multitasker. It costs more and it is what the thing being modelled
//! actually does.
//!
//! # Why the transitions are a refusal and not a set of flags
//!
//! The tempting shape is three booleans - overlay open, backgrounded, exited - and it is
//! wrong within a day, because it can represent states that do not exist (exited *and*
//! overlaid) and silently accepts requests that make no sense. Principle 3: an explicit
//! refusal beats a state nobody can reach on purpose.
//!
//! So [`Lifecycle`] is one value, [`Lifecycle::on`] is total, and everything else -
//! [`Focus`], [`Video`], [`Execution`] - is **derived rather than stored**. Two fields that
//! can disagree about whether the title has input focus will eventually disagree.

use serde::{Deserialize, Serialize};

use crate::event::ShellEvent;

/// Where the title stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Lifecycle {
    /// The title owns the screen and the controller. The ordinary case.
    Foreground,
    /// The title is still running and still presenting; the shell is drawn over it.
    ///
    /// **The title keeps executing, and that is the whole point of the state.** A shell
    /// that suspended the guest to draw a strip over it would show a frozen frame behind
    /// itself, which is a different product.
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
/// **Serialisable, because this vocabulary crosses a process boundary.** The window and the
/// guest are different processes (D032), so a shell request is a wire message - and it is
/// this type rather than a copy of it in `orbistoun-proto`, because two enums meaning the
/// same thing drift and the drift shows up as a button that does the wrong thing.
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
/// **Named causes rather than a bare `None`.** A refusal a caller cannot explain is one
/// a caller will paper over with a retry, and the front-end has to be able to say why a
/// button did nothing.
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
/// **Suspend is the default, and it is the conservative answer rather than the faithful
/// one.** A title that keeps executing behind the shell goes on submitting flips and audio
/// into surfaces nobody is showing, and nothing in this emulator yet guarantees those are
/// absorbed harmlessly rather than acknowledged as presented. Telling a title its frame
/// reached a screen when it did not is exactly the plausible-output failure principle 3
/// exists to forbid.
///
/// [`WhenBackgrounded::KeepRunning`] is here because it is what the modelled system does,
/// and it becomes the sensible default once the video path can honestly absorb a frame
/// from a title nobody is watching.
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
/// **Derived from the lifecycle, never stored.** The single most likely bug in a shell is
/// a title acting on a button press the shell already consumed, and it is exactly what a
/// second copy of this answer produces when the two drift.
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
    /// **Neutral, not disconnected.** Reporting no controller is a different lie and a
    /// worse one: a title that loses its pad will often stop, put up a reconnect prompt, or
    /// treat it as the player walking away. A connected pad with nothing pressed is an
    /// ordinary state every title already handles on its quietest frame.
    ///
    /// Nor may the title simply keep reading: whatever was held when the shell took focus
    /// stays held forever, so the last input before opening the shell repeats for as long
    /// as it is open.
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
    /// A `Vec` because a transition raises at most two and this runs at the rate a person
    /// presses buttons. Principle 9's no-allocation rule governs the trace path, which
    /// this is not.
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

        // Nothing is running, so nothing can be done to it. Its own cause rather than a
        // general refusal: "there is no title" and "you are already there" send a
        // front-end in completely different directions.
        if self == Exited {
            return Err(Refused::NoTitle);
        }

        // Quit is the one request every live state answers the same way, so it is lifted
        // out rather than repeated three times. Below this line every arm is a *state*
        // deciding something, which is what makes the rest readable as a table.
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
            // Straight to the shell without passing through the foreground. The title has
            // already lost focus, so only the backgrounding is news.
            (Overlaid, ToShell) => taken(Background, &[ShellEvent::Backgrounded]),

            (Background, Resume) => taken(
                Foreground,
                &[ShellEvent::Foregrounded, ShellEvent::FocusGained],
            ),

            // Asking for the state you are in. A front-end can ignore these.
            (Foreground, CloseOverlay | Resume)
            | (Overlaid, OpenOverlay)
            | (Background, ToShell) => Err(Refused::AlreadyThere),

            // Legal requests from the wrong place, which means the caller has lost track
            // of where it is. An overlay in particular is a thing drawn over a *title*:
            // quietly treating one as a resume would put a title in front of somebody who
            // did not ask for it.
            (Overlaid, Resume) | (Background, OpenOverlay | CloseOverlay) => {
                Err(Refused::NotFromHere)
            }

            // Handled above, and spelled out rather than caught by a wildcard: a `_` here
            // would silently absorb any variant added to either enum later.
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

    /// **The property that makes this a multitasker rather than a launcher.**
    ///
    /// A title survives going behind the shell and can be brought back. If this ever
    /// reduces to "quit and relaunch", the design decision has been lost.
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

    /// **The title keeps executing behind an overlay.**
    ///
    /// An overlay that suspended the guest would composite itself over a frozen frame,
    /// which is a screenshot with a menu on it rather than an overlay.
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

    /// **A backgrounded title stops by default, and the reason is honesty not fidelity.**
    ///
    /// Letting it run means acknowledging flips into a surface nobody presents.
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

    /// **Focus is answered once.** The failure this prevents is a title acting on a press
    /// the shell already consumed, so the two must not be separately settable.
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
    /// Asserted on the *events*, not on the state, because the state change is the easy
    /// half - a title that is backgrounded without being told has been suspended behind
    /// its own back and will resume believing no time passed.
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
    /// A duplicate event is not harmless: a title counting focus changes to decide whether
    /// to auto-pause would pause twice and need two resumes.
    #[test]
    fn a_title_that_has_already_lost_focus_is_not_told_twice() {
        let raised = walk(Lifecycle::Foreground, &[Request::OpenOverlay]);
        let to_shell = raised.on(Request::ToShell).expect("the shell is reachable");

        assert_eq!(to_shell.raise, vec![ShellEvent::Backgrounded]);
    }

    /// **Every refusal names its own cause.**
    ///
    /// The three are acted on differently: "already there" is a no-op a front-end can
    /// ignore, "no title" means the button should not have been offered, and "not from
    /// here" is a bug in the caller.
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

    /// **Exited is final, and nothing walks back out of it.**
    ///
    /// The state that would break every other guarantee if it were reachable in reverse:
    /// a resumed title whose process is gone.
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
