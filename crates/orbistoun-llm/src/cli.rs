//! A model reached by running a command, rather than by an HTTP call or a local weight file.
//!
//! An installed coding assistant is already authenticated, so shelling out to it needs no API
//! key, download or accelerator. The binary-discovery order is carried from another project;
//! see [ACKNOWLEDGEMENTS.md](../../../ACKNOWLEDGEMENTS.md). The command takes no seed and no
//! temperature, so this engine ignores both fields of a [`Request`] and
//! [`CliEngine::describe`] says so; the rotating example window still varies the prompt per
//! round. The command's own instructions cannot be replaced, so system text is prepended to
//! the prompt.

use std::path::PathBuf;
// `Path` is referenced only by the Windows-only launcher discovery and `newest_versioned`.
#[cfg(target_os = "windows")]
use std::path::Path;
use std::process::{Command, Stdio};

use crate::Error;
use crate::engine::{Engine, Request};

/// The only command this knows how to find, and what to call it.
pub const CLAUDE_CODE: &str = "claude-code";

/// Windows: do not flash a console window for a subprocess nobody is watching.
#[cfg(target_os = "windows")]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// A model reached by running a command.
#[derive(Debug)]
pub struct CliEngine {
    /// The executable to run.
    program: PathBuf,
    /// Which command this is, for reporting.
    source: String,
}

impl CliEngine {
    /// Finds the command an entry names.
    ///
    /// # Errors
    ///
    /// If the command is not one this knows, or is not installed.
    pub fn new(source: &str) -> Result<Self, Error> {
        if source != CLAUDE_CODE {
            return Err(Error::Config(format!(
                "no command-line model called {source:?} - this build knows {CLAUDE_CODE}"
            )));
        }
        let program = find_claude().ok_or_else(|| {
            Error::Config(
                concat!(
                    "the Claude Code command was not found. Install it, or pick another ",
                    "entry - nothing here needs it"
                )
                .to_owned(),
            )
        })?;
        Ok(Self {
            program,
            source: source.to_owned(),
        })
    }

    /// Whether the command an entry names is installed.
    #[must_use]
    pub fn available(source: &str) -> bool {
        source == CLAUDE_CODE && find_claude().is_some()
    }

    /// The single string the command is given: system text first, then the prompt, since the
    /// command's own instructions cannot be replaced.
    fn text(request: &Request) -> String {
        match &request.system {
            Some(system) if !system.trim().is_empty() => {
                format!("{}\n\n{}", system.trim(), request.prompt)
            }
            _ => request.prompt.clone(),
        }
    }
}

impl Engine for CliEngine {
    fn describe(&self) -> String {
        format!(
            concat!(
                "{} command line - no key and no download, and **no seed or temperature**: ",
                "both are ignored, so successive requests are not varied by this engine"
            ),
            self.source
        )
    }

    fn model(&self) -> String {
        format!(
            "{} (whatever the command is configured to use)",
            self.source
        )
    }

    fn complete(&self, request: &Request) -> Result<String, Error> {
        let mut command = Command::new(&self.program);
        // The prompt goes on standard input: it carries examples and a vocabulary sample, and a Windows
        // command line stops at about thirty-two thousand characters. The command writes the reply to
        // stdout and its diagnostics to stderr.
        command
            .arg("--print")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        #[cfg(target_os = "windows")]
        {
            use std::os::windows::process::CommandExt as _;
            command.creation_flags(CREATE_NO_WINDOW);
        }

        let text = Self::text(request);
        let mut child = command
            .spawn()
            .map_err(|e| Error::Transport(format!("running {}: {e}", self.program.display())))?;
        {
            use std::io::Write as _;
            let mut stdin = child
                .stdin
                .take()
                .ok_or_else(|| Error::Transport("the command refused a prompt".to_owned()))?;
            stdin
                .write_all(text.as_bytes())
                .map_err(|e| Error::Transport(format!("writing the prompt: {e}")))?;
        }
        let output = child.wait_with_output().map_err(|e| {
            Error::Transport(format!("waiting for {}: {e}", self.program.display()))
        })?;

        let out = String::from_utf8_lossy(&output.stdout);
        let err = String::from_utf8_lossy(&output.stderr);
        read(&out, &err, output.status.success())
    }
}

/// Reads what the command produced, or says why it is not an answer.
///
/// Separate from running it, so every failure branch is testable without the command installed.
fn read(stdout: &str, stderr: &str, success: bool) -> Result<String, Error> {
    // Checked on both streams: the command writes this one to stdout and exits non-zero, so
    // stderr alone would report the exit code instead of the cause.
    if is_unauthenticated(stdout) || is_unauthenticated(stderr) {
        return Err(Error::Model(
            concat!(
                "the Claude Code command is not signed in. Run `claude` once and sign in, ",
                "then try again - this is deliberately not done for you, because a tool that ",
                "takes over the terminal to open a browser is not one that can be left running"
            )
            .to_owned(),
        ));
    }
    if stdout.trim().is_empty() && stderr.trim().is_empty() {
        return Err(Error::Model(
            concat!(
                "the command produced nothing at all on either stream, which usually means a ",
                "desktop application answered instead of the command line"
            )
            .to_owned(),
        ));
    }
    if !success {
        return Err(Error::Model(format!(
            "the command failed: {}",
            first_line(stderr).unwrap_or("no message")
        )));
    }
    // The command chooses its own model, so a reasoning block is possible here too.
    Ok(crate::engine::without_reasoning(stdout).to_owned())
}

/// Whether a stream says the command is not signed in.
fn is_unauthenticated(text: &str) -> bool {
    const SAID: [&str; 3] = ["Not logged in", "Please run /login", "not authenticated"];
    SAID.iter().any(|said| text.contains(said))
}

/// The first non-empty line, for an error message that fits on one.
fn first_line(text: &str) -> Option<&str> {
    text.lines().map(str::trim).find(|line| !line.is_empty())
}

/// Where the Claude Code command lives, newest first.
///
/// On Windows the launcher under `LOCALAPPDATA` routes to a running desktop application, whose
/// reply the calling process never sees, so the versioned command under `APPDATA` is preferred
/// and the launcher is a last resort. Elsewhere the command on the path is the ordinary install.
fn find_claude() -> Option<PathBuf> {
    #[cfg(not(target_os = "windows"))]
    {
        if let Ok(status) = Command::new("claude")
            .arg("--version")
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
        {
            if status.success() {
                return Some(PathBuf::from("claude"));
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Some(appdata) = std::env::var_os("APPDATA") {
            let base = Path::new(&appdata).join("Claude").join("claude-code");
            if let Some(newest) = newest_versioned(&base) {
                return Some(newest);
            }
        }
        if let Some(local) = std::env::var_os("LOCALAPPDATA") {
            let launcher = Path::new(&local).join("AnthropicClaude").join("claude.exe");
            if launcher.exists() {
                return Some(launcher);
            }
        }
    }

    None
}

/// The command inside the highest-numbered versioned directory under `base`.
///
/// Sorted as text descending, which is not version ordering (`2.1.9` sorts above `2.1.10`);
/// the listing has no two-digit patch components, so no version parser is used. Windows-only,
/// like its one caller.
#[cfg(target_os = "windows")]
fn newest_versioned(base: &Path) -> Option<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(base)
        .ok()?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.is_dir())
        .map(|dir| dir.join("claude.exe"))
        .filter(|path| path.exists())
        .collect();
    found.sort_by(|a, b| b.cmp(a));
    found.into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::{CLAUDE_CODE, CliEngine, is_unauthenticated, read};
    use crate::engine::Request;

    /// A reply is what the command printed, trimmed, and nothing else.
    #[test]
    fn a_successful_run_is_its_trimmed_output() {
        let out = read("  [\"One\", \"Two\"]\n", "", true).expect("a reply");
        assert_eq!(out, "[\"One\", \"Two\"]");
    }

    /// Not signed in is detected on stdout as well as stderr.
    #[test]
    fn not_signed_in_is_read_from_either_stream() {
        for (out, err) in [("Not logged in", ""), ("", "Please run /login")] {
            let error = read(out, err, false).expect_err("should refuse");
            assert!(
                error.to_string().contains("not signed in"),
                "{error} does not name the cause"
            );
        }
    }

    /// A signed-out command is reported, never signed in for the caller: this runs unattended and
    /// must not seize the terminal to open a browser.
    #[test]
    fn a_signed_out_command_is_reported_rather_than_signed_in() {
        let error = read("Not logged in", "", false).expect_err("should refuse");
        let said = error.to_string();
        assert!(said.contains("Run `claude` once"), "{said}");
        assert!(
            !said.contains("opening"),
            "the message suggests something is happening on the caller's behalf: {said}"
        );
    }

    /// Each phrase the command uses for "not signed in" is recognised, so a change to one fails
    /// here.
    #[test]
    fn every_phrase_for_signed_out_is_recognised() {
        for said in ["Not logged in", "Please run /login", "not authenticated"] {
            assert!(is_unauthenticated(said), "{said:?} was not recognised");
            assert!(
                is_unauthenticated(&format!("noise before {said} and after")),
                "{said:?} was not recognised inside other output"
            );
        }
        assert!(!is_unauthenticated("everything is fine"));
    }

    /// Silence on both streams is its own failure, and names the likely cause.
    #[test]
    fn producing_nothing_at_all_is_distinguished_from_failing() {
        let error = read("", "", true).expect_err("should refuse");
        assert!(error.to_string().contains("nothing at all"), "{error}");
    }

    /// A non-zero exit reports the command's own first line, not the number.
    #[test]
    fn a_failed_run_reports_what_the_command_said() {
        let error = read("", "\n  model overloaded\nmore detail\n", false).expect_err("refuses");
        assert!(error.to_string().contains("model overloaded"), "{error}");
    }

    /// The system text is prepended, because there is nowhere else to put it.
    #[test]
    fn system_text_is_prepended_to_the_prompt() {
        let request = Request::new("name some things").with_system("you answer in JSON");
        let text = CliEngine::text(&request);
        assert!(text.starts_with("you answer in JSON"), "{text}");
        assert!(text.ends_with("name some things"), "{text}");
    }

    /// A request with no system text is passed through unchanged.
    #[test]
    fn a_request_without_system_text_is_sent_as_written() {
        assert_eq!(
            CliEngine::text(&Request::new("just this")),
            "just this",
            "something was added to a bare prompt"
        );
    }

    /// The engine declares that it ignores seed and temperature.
    #[test]
    fn it_declares_the_two_fields_it_cannot_honour() {
        let engine = CliEngine {
            program: std::path::PathBuf::from("claude"),
            source: CLAUDE_CODE.to_owned(),
        };
        let said = crate::engine::Engine::describe(&engine);
        assert!(said.contains("seed"), "{said}");
        assert!(said.contains("temperature"), "{said}");
    }

    /// An unknown command is refused by name rather than probed for.
    #[test]
    fn an_unknown_command_is_refused() {
        let error = CliEngine::new("some-other-tool").expect_err("should refuse");
        assert!(error.to_string().contains("some-other-tool"), "{error}");
        assert!(!CliEngine::available("some-other-tool"));
    }
}
