//! What the window opens into, decided from the command line and the stored setting.
//!
//! A pure function over the arguments, so its outcomes and refusals are testable without a
//! window. An argument always beats the stored setting, so a launcher entry or script gets
//! the view it names.

use serde::{Deserialize, Serialize};

/// Which view the window opens into.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum View {
    /// The library list, per-title inspection and run diagnostics.
    ///
    /// The default, because it is the view the emulator is developed through.
    #[default]
    List,
    /// The shell, as the system software presents itself.
    Shell,
}

impl View {
    /// The name used on the command line and in the settings file.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Shell => "shell",
        }
    }
}

/// What the window was asked to do.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Start {
    /// Open in a view and wait.
    In(View),
    /// Open and launch this title straight away.
    ///
    /// Carries the view to fall back to when the title cannot be started.
    Title {
        /// The title as it was named on the command line.
        name: String,
        /// Where to land if it cannot be started.
        fallback: View,
    },
}

/// Why a command line could not be acted on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refusal {
    /// Two views were asked for at once.
    TwoViews,
    /// `--title` was given nothing to launch.
    TitleWithoutName,
    /// A view and a title were asked for together.
    ///
    /// Refused rather than resolved, so no flag silently does something other than it says.
    ViewAndTitle,
}

impl Refusal {
    /// What to print, including what to do instead.
    #[must_use]
    pub fn say(&self) -> String {
        match self {
            Self::TwoViews => {
                "--shell and --list ask for different views; give one of them".to_owned()
            }
            Self::TitleWithoutName => "--title needs the name of a title to launch".to_owned(),
            Self::ViewAndTitle => concat!(
                "--title already says what to open; drop --shell or --list, or set the ",
                "view it returns to in the settings file"
            )
            .to_owned(),
        }
    }
}

/// The flag asking for the shell.
pub const SHELL_FLAG: &str = "--shell";

/// The flag asking for the list.
pub const LIST_FLAG: &str = "--list";

/// The flag naming a title to launch.
pub const TITLE_FLAG: &str = "--title";

/// Reads a command line, falling back to the stored default.
///
/// Unrecognised arguments are ignored: the window is re-executed with a worker flag (D033),
/// and the frameworks underneath it take arguments of their own.
///
/// # Errors
///
/// [`Refusal`] when the arguments contradict each other.
pub fn read<I: IntoIterator<Item = String>>(args: I, default: View) -> Result<Start, Refusal> {
    let mut asked: Option<View> = None;
    let mut title: Option<String> = None;
    let mut arguments = args.into_iter();

    while let Some(argument) = arguments.next() {
        match argument.as_str() {
            SHELL_FLAG | LIST_FLAG => {
                let wanted = if argument == SHELL_FLAG {
                    View::Shell
                } else {
                    View::List
                };
                // Repeating the same flag is not a contradiction.
                if asked.is_some_and(|already| already != wanted) {
                    return Err(Refusal::TwoViews);
                }
                asked = Some(wanted);
            }
            TITLE_FLAG => {
                let named = arguments.next().ok_or(Refusal::TitleWithoutName)?;
                // A flag where a name should be is a missing name, not a title called
                // `--shell`.
                if named.starts_with("--") {
                    return Err(Refusal::TitleWithoutName);
                }
                title = Some(named);
            }
            _ => {}
        }
    }

    match (asked, title) {
        (Some(_), Some(_)) => Err(Refusal::ViewAndTitle),
        (_, Some(name)) => Ok(Start::Title {
            name,
            fallback: default,
        }),
        (Some(view), None) => Ok(Start::In(view)),
        (None, None) => Ok(Start::In(default)),
    }
}

#[cfg(test)]
mod tests {
    use super::{Refusal, Start, View, read};

    fn args(given: &[&str]) -> Vec<String> {
        given.iter().map(|a| (*a).to_owned()).collect()
    }

    /// With no arguments, the stored setting decides.
    #[test]
    fn an_empty_command_line_uses_the_stored_default() {
        assert_eq!(read(args(&[]), View::List), Ok(Start::In(View::List)));
        assert_eq!(read(args(&[]), View::Shell), Ok(Start::In(View::Shell)));
    }

    /// An argument beats the setting, in both directions.
    #[test]
    fn an_argument_overrides_the_stored_default() {
        assert_eq!(
            read(args(&["--shell"]), View::List),
            Ok(Start::In(View::Shell))
        );
        assert_eq!(
            read(args(&["--list"]), View::Shell),
            Ok(Start::In(View::List))
        );
    }

    /// A title carries the view it should return to.
    #[test]
    fn a_title_is_launched_and_remembers_where_to_land() {
        assert_eq!(
            read(args(&["--title", "PPSA02664"]), View::Shell),
            Ok(Start::Title {
                name: "PPSA02664".to_owned(),
                fallback: View::Shell,
            })
        );
    }

    /// Contradictory arguments are refused, not resolved.
    #[test]
    fn asking_for_two_views_at_once_is_refused() {
        assert_eq!(
            read(args(&["--shell", "--list"]), View::List),
            Err(Refusal::TwoViews)
        );
        assert_eq!(
            read(args(&["--list", "--shell"]), View::List),
            Err(Refusal::TwoViews)
        );
    }

    /// A repeated flag is not a contradiction.
    #[test]
    fn repeating_one_flag_is_not_a_contradiction() {
        assert_eq!(
            read(args(&["--shell", "--shell"]), View::List),
            Ok(Start::In(View::Shell))
        );
    }

    /// A view and a title together is ambiguous, so it is refused.
    #[test]
    fn a_view_and_a_title_together_is_refused() {
        assert_eq!(
            read(args(&["--shell", "--title", "X"]), View::List),
            Err(Refusal::ViewAndTitle)
        );
    }

    /// A flag where a title name should be is a missing name.
    #[test]
    fn a_title_flag_with_nothing_after_it_is_refused() {
        assert_eq!(
            read(args(&["--title"]), View::List),
            Err(Refusal::TitleWithoutName)
        );
        assert_eq!(
            read(args(&["--title", "--shell"]), View::List),
            Err(Refusal::TitleWithoutName)
        );
    }

    /// Arguments belonging to the worker or the frameworks are left alone.
    #[test]
    fn arguments_this_does_not_own_are_ignored() {
        assert_eq!(
            read(
                args(&["--worker", "--some-framework-flag", "--shell"]),
                View::List
            ),
            Ok(Start::In(View::Shell))
        );
    }

    /// Every refusal says what to do instead.
    #[test]
    fn a_refusal_explains_itself() {
        for refusal in [
            Refusal::TwoViews,
            Refusal::TitleWithoutName,
            Refusal::ViewAndTitle,
        ] {
            let said = refusal.say();
            assert!(said.len() > 20, "{refusal:?} said only {said:?}");
        }
    }
}
