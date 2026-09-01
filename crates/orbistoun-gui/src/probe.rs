//! Connecting to a probe: ask it things, read what it says.
//!
//! # Why this exists as its own window
//!
//! Everything else in this application looks at files that already exist - a title, a
//! trace, a report from a run that finished. This is the one surface that asks a live
//! question of something running elsewhere and gets an answer back, and the difference
//! matters enough to keep it separate rather than folding it into a detail panel.
//!
//! # What is on the other end
//!
//! **A probe, and nothing more specific than that.** It may be running on the target
//! platform, on a stand-in, or inside another emulator, and this window cannot tell which -
//! a probe cannot certify its own machine. So nothing here says "hardware" or names a
//! device: it says *probe*, which is the one thing that is actually known.
//!
//! What it is running on is the operator's to assert, it belongs to the corpus rather than
//! to a connection, and it is deliberately not asked for here.
//!
//! # What it will not do
//!
//! **Flatter a non-answer.** `died`, `timeout` and `lost` are rendered as themselves and
//! never as a value. The probe dying is the *normal* case here - a well-formed but fatal
//! address is called, not refused - so a window that showed a death as a blank result, or
//! as a zero, would be lying about the single thing it exists to observe.
//!
//! **Decide what an answer means.** Grading belongs to the corpus path, where the operator
//! has said what machine this is. A value here is what came off the wire and is labelled as
//! that.
//!
//! # Threading
//!
//! The connection lives on a worker thread and speaks to the interface through a channel.
//! A socket read blocks for as long as its budget allows - thirty seconds by default - and
//! a frame that waited on one would freeze the whole application on a probe that has gone
//! quiet, which is the exact condition worth watching.

use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};

use orbistoun_probe::client::{Client, ClientError, DEFAULT_PORT};
use orbistoun_probe::{Capability, Confidence, Outcome, Record, Transcript};

/// What the worker sends back to the interface.
enum Event {
    /// Negotiation succeeded: session identifier and what the probe announced.
    Connected {
        session: String,
        capabilities: Vec<Capability>,
    },
    /// The target's account of itself, read from the report stream.
    SelfReport(Vec<(String, Confidence, String)>),
    /// One line for the log.
    Line(String),
    /// A command finished, however it finished.
    Answered {
        outcome: Outcome,
        detail: String,
        records: Vec<Record>,
    },
    /// The connection is gone.
    Closed(String),
}

/// What the interface asks the worker to do.
enum Request {
    /// A verb and its arguments.
    Command(Vec<String>),
    /// Close the session and stop.
    Disconnect,
}

/// One line of the exchange.
///
/// Kept as a kind rather than pre-formatted text so the view can colour a death differently
/// from a result without parsing its own output back.
pub(crate) enum Entry {
    /// Something the operator sent.
    Sent(String),
    /// Something the probe said.
    Received(String),
    /// A command that answered with a value.
    Returned(String),
    /// A command that did not answer. Never rendered as a result.
    NonAnswer(String),
    /// A note from this application rather than from the wire.
    Note(String),
}

/// The probe window's state.
#[derive(Default)]
pub(crate) struct Panel {
    /// Whether the window is showing.
    pub(crate) open: bool,
    /// Address to dial, as the operator typed it.
    pub(crate) address: String,
    /// The session secret, which the probe shows when it starts listening.
    pub(crate) key: String,
    /// What the operator wants to ask next.
    pub(crate) entry: String,
    /// Everything that has happened, oldest first.
    pub(crate) log: Vec<Entry>,
    /// Session identifier, once negotiated.
    session: Option<String>,
    /// What the probe announced it can do.
    capabilities: Vec<Capability>,
    /// The target's self-report, if a run produced one.
    self_report: Vec<(String, Confidence, String)>,
    /// Channel to the worker, while one is running.
    to_worker: Option<Sender<Request>>,
    /// Channel from the worker.
    from_worker: Option<Receiver<Event>>,
    /// Whether a command is outstanding.
    ///
    /// One command in flight at a time is a protocol requirement, not a simplification:
    /// with two outstanding and a probe that has just died, nothing says which one killed
    /// it - and that attribution is the finding.
    busy: bool,
}

impl Panel {
    /// Whether a session is open.
    pub(crate) const fn connected(&self) -> bool {
        self.session.is_some()
    }

    /// Whether the probe announced a capability.
    fn can(&self, capability: &Capability) -> bool {
        self.capabilities.contains(capability)
    }

    /// Starts a worker and negotiates.
    ///
    /// The worker is handed a context so it can ask for a repaint **when it has something
    /// to say**. The first version polled instead - a repaint every two hundred
    /// milliseconds for as long as a session was open - which redraws the window
    /// continuously whether or not anything happened, and shows as a cursor that will not
    /// settle. A timer is the wrong instrument for an event that already knows when it
    /// occurred.
    fn connect(&mut self, ctx: &egui::Context) {
        let address = if self.address.contains(':') {
            self.address.clone()
        } else {
            format!("{}:{DEFAULT_PORT}", self.address.trim())
        };
        let key = (!self.key.trim().is_empty()).then(|| self.key.trim().to_owned());

        let (to_worker, requests) = channel::<Request>();
        let (events, from_worker) = channel::<Event>();
        self.to_worker = Some(to_worker);
        self.from_worker = Some(from_worker);
        self.log
            .push(Entry::Note(format!("connecting to {address}")));

        let ctx = ctx.clone();
        std::thread::spawn(move || {
            worker(&address, key.as_deref(), &requests, &events, &ctx);
        });
    }

    /// Drains whatever the worker has said since the last frame.
    ///
    /// Non-blocking on purpose. A frame that waited on a socket would freeze the window on
    /// a probe that has gone quiet, and a probe going quiet is a thing worth being able to
    /// watch happen.
    pub(crate) fn poll(&mut self) {
        let Some(from_worker) = self.from_worker.as_ref() else {
            return;
        };
        loop {
            match from_worker.try_recv() {
                Ok(Event::Connected {
                    session,
                    capabilities,
                }) => {
                    self.log.push(Entry::Note(format!("session {session}")));
                    self.log.push(Entry::Note(format!(
                        "the probe can: {}",
                        capabilities
                            .iter()
                            .map(|c| format!("{c:?}").to_lowercase())
                            .collect::<Vec<_>>()
                            .join(", ")
                    )));
                    self.session = Some(session);
                    self.capabilities = capabilities;
                    self.busy = false;
                }
                Ok(Event::SelfReport(fields)) => self.self_report = fields,
                Ok(Event::Line(line)) => self.log.push(Entry::Received(line)),
                Ok(Event::Answered {
                    outcome,
                    detail,
                    records,
                }) => {
                    for record in &records {
                        self.log.push(Entry::Received(format!("{record:?}")));
                    }
                    // The distinction the whole window is for. A command that answered gets
                    // its value; one that did not gets said so, and never a value.
                    let text = if detail.is_empty() {
                        outcome.to_string()
                    } else {
                        format!("{outcome} - {detail}")
                    };
                    if outcome.answered() {
                        self.log.push(Entry::Returned(text));
                    } else {
                        self.log.push(Entry::NonAnswer(text));
                    }
                    self.busy = false;
                }
                Ok(Event::Closed(why)) => {
                    self.log.push(Entry::Note(why));
                    self.session = None;
                    self.capabilities.clear();
                    self.to_worker = None;
                    self.from_worker = None;
                    self.busy = false;
                    return;
                }
                Err(TryRecvError::Empty) => return,
                Err(TryRecvError::Disconnected) => {
                    self.from_worker = None;
                    self.to_worker = None;
                    self.session = None;
                    self.busy = false;
                    return;
                }
            }
        }
    }

    /// Sends what the operator typed.
    fn send(&mut self) {
        let text = self.entry.trim().to_owned();
        if text.is_empty() || self.busy {
            return;
        }
        let parts: Vec<String> = text.split_whitespace().map(str::to_owned).collect();
        let Some(verb) = parts.first() else {
            return;
        };

        // Refused here rather than on the wire. A client that sends a verb the probe never
        // announced has already put a command it does not implement in front of a target
        // that faults easily - and being told "no" afterwards does not take it back.
        if let Some(needed) = capability_for(verb) {
            if !self.can(&needed) {
                self.log.push(Entry::Note(format!(
                    "`{verb}` was not announced by this probe, so it was not sent"
                )));
                self.entry.clear();
                return;
            }
        }

        if let Some(to_worker) = self.to_worker.as_ref() {
            if to_worker.send(Request::Command(parts)).is_ok() {
                self.log.push(Entry::Sent(text));
                self.busy = true;
            }
        }
        self.entry.clear();
    }

    /// Draws the window.
    pub(crate) fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        self.poll();

        let mut open = self.open;
        egui::Window::new("Probe")
            .open(&mut open)
            .default_width(620.0)
            .default_height(460.0)
            .show(ctx, |ui| self.body(ui));
        self.open = open;
    }

    fn body(&mut self, ui: &mut egui::Ui) {
        self.connection_row(ui);
        ui.separator();
        if !self.self_report.is_empty() {
            self.self_report_row(ui);
            ui.separator();
        }
        self.log_view(ui);
        ui.separator();
        self.entry_row(ui);
    }

    /// Address and key, which are the only two things needed to connect.
    fn connection_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Address");
            ui.add(
                egui::TextEdit::singleline(&mut self.address)
                    .desired_width(180.0)
                    .hint_text("192.168.1.50"),
            );
            ui.label("Key");
            ui.add(
                egui::TextEdit::singleline(&mut self.key)
                    .desired_width(240.0)
                    .password(true)
                    .hint_text("shown by the probe on startup"),
            );

            if self.connected() {
                if ui.button("Disconnect").clicked() {
                    if let Some(to_worker) = self.to_worker.as_ref() {
                        let _ = to_worker.send(Request::Disconnect);
                    }
                }
            } else if ui
                .add_enabled(
                    !self.address.trim().is_empty(),
                    egui::Button::new("Connect"),
                )
                .clicked()
            {
                self.connect(ui.ctx());
            }
        });

        ui.horizontal(|ui| {
            if let Some(session) = &self.session {
                ui.label(format!("session {session}"));
                if self.busy {
                    ui.spinner();
                    ui.label("waiting");
                }
            } else {
                ui.weak("not connected");
            }
        });
    }

    /// What the probe reports about where it is running - its own account, and only that.
    fn self_report_row(&mut self, ui: &mut egui::Ui) {
        ui.label("Reported by the probe about itself (its own account, not evidence)");
        egui::Grid::new("probe-self-report")
            .num_columns(2)
            .show(ui, |ui| {
                for (field, confidence, value) in &self.self_report {
                    ui.label(field);
                    // The three ways of not knowing stay three things. All of them can read
                    // `unknown`, and a display that collapsed them would show one blank
                    // where there are three different findings.
                    match confidence {
                        Confidence::Known => {
                            ui.label(value);
                        }
                        Confidence::Unconfirmed => {
                            ui.weak(format!("{value} - the probe cannot read this yet"));
                        }
                        Confidence::Absent => {
                            ui.weak(format!("{value} - no such query where this is running"));
                        }
                        Confidence::Unrecognised(state) => {
                            ui.weak(format!("{value} - state {state:?}, unrecognised"));
                        }
                    }
                    ui.end_row();
                }
            });
    }

    fn log_view(&mut self, ui: &mut egui::Ui) {
        egui::ScrollArea::vertical()
            .stick_to_bottom(true)
            .max_height(240.0)
            .show(ui, |ui| {
                for entry in &self.log {
                    match entry {
                        Entry::Sent(text) => {
                            ui.monospace(format!("> {text}"));
                        }
                        Entry::Received(text) => {
                            ui.weak(egui::RichText::new(text).monospace());
                        }
                        Entry::Returned(text) => {
                            ui.monospace(egui::RichText::new(text).strong());
                        }
                        // Deliberately marked. A death is the normal case and it is not a
                        // result; the one thing this window must never do is let it read
                        // like one.
                        Entry::NonAnswer(text) => {
                            ui.monospace(
                                egui::RichText::new(format!("{text}  (no result)"))
                                    .color(egui::Color32::from_rgb(220, 120, 90)),
                            );
                        }
                        Entry::Note(text) => {
                            ui.weak(text);
                        }
                    }
                }
            });
    }

    fn entry_row(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let enabled = self.connected() && !self.busy;
            let response = ui.add_enabled(
                enabled,
                egui::TextEdit::singleline(&mut self.entry)
                    .desired_width(380.0)
                    .hint_text("call 0x80019c40 0x0"),
            );
            if response.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)) {
                self.send();
                response.request_focus();
            }
            if ui.add_enabled(enabled, egui::Button::new("Send")).clicked() {
                self.send();
            }
        });

        // The verbs worth one click, and only the ones this probe announced. A button for
        // something reserved would be a button that exists to be refused.
        ui.horizontal(|ui| {
            ui.weak("quick:");
            for (label, text, capability) in [
                ("report", "report", Some(Capability::Report)),
                ("read", "read 0x8003f510 0x20", Some(Capability::Read)),
                ("call", "call 0x0", Some(Capability::Call)),
            ] {
                let available = self.connected()
                    && !self.busy
                    && capability.as_ref().is_none_or(|c| self.can(c));
                if ui
                    .add_enabled(available, egui::Button::new(label).small())
                    .clicked()
                {
                    text.clone_into(&mut self.entry);
                }
            }
        });
    }
}

/// Which capability a verb needs, mirroring the client's own rule.
fn capability_for(verb: &str) -> Option<Capability> {
    match verb {
        "call" => Some(Capability::Call),
        "resolve" => Some(Capability::Resolve),
        "read" => Some(Capability::Read),
        "write" => Some(Capability::Write),
        "blob" | "run" => Some(Capability::Blob),
        "reset" => Some(Capability::Reset),
        "report" => Some(Capability::Report),
        _ => None,
    }
}

/// The connection, on its own thread.
fn worker(
    address: &str,
    key: Option<&str>,
    requests: &Receiver<Request>,
    events: &Sender<Event>,
    ctx: &egui::Context,
) {
    /// Sends an event and asks for the frame that will show it.
    ///
    /// Both together, always: an event delivered without a repaint sits unseen until the
    /// pointer happens to move, and a repaint without an event is the flicker.
    macro_rules! announce {
        ($event:expr) => {{
            let _ = events.send($event);
            ctx.request_repaint();
        }};
    }

    let budget = std::time::Duration::from_secs(30);
    let mut client = match orbistoun_probe::client::connect(address, budget) {
        Ok(client) => client,
        Err(e) => {
            announce!(Event::Closed(format!("could not connect: {e}")));
            return;
        }
    };

    match client.hello(orbistoun_probe::VERSION, key) {
        Ok(session) => {
            let capabilities = announced(&client);
            announce!(Event::Connected {
                session,
                capabilities,
            });
        }
        Err(e) => {
            // A wrong or stale key lands here as a clean refusal rather than a broken wire,
            // which is worth saying plainly because the secret is replaced by a restart.
            announce!(Event::Closed(match e {
                ClientError::Refused(orbistoun_probe::Refusal::Unauthorised) => {
                    "refused: the key was wrong or stale - a restart replaces it".to_owned()
                }
                other => format!("could not negotiate: {other}"),
            }));
            return;
        }
    }

    while let Ok(request) = requests.recv() {
        match request {
            Request::Disconnect => break,
            Request::Command(parts) => {
                let (verb, arguments) = parts.split_first().expect("never empty");
                let borrowed: Vec<&str> = arguments.iter().map(String::as_str).collect();
                match client.command(verb, &borrowed) {
                    Ok(answer) => {
                        report_self(&client, events);
                        announce!(Event::Answered {
                            outcome: answer.outcome,
                            detail: answer.detail,
                            records: answer.records,
                        });
                    }
                    Err(e) => {
                        let _ = events.send(Event::Line(format!("refused: {e}")));
                        announce!(Event::Answered {
                            outcome: Outcome::Unrecognised("refused".to_owned()),
                            detail: e.to_string(),
                            records: Vec::new(),
                        });
                    }
                }
            }
        }
    }
    let _ = client.bye();
    announce!(Event::Closed("session closed".to_owned()));
}

/// What the probe announced, read back off its own transcript.
fn announced<S: std::io::Read + std::io::Write>(client: &Client<S>) -> Vec<Capability> {
    Transcript::read(&client.transcript().join("\n"))
        .ok()
        .and_then(|transcript| {
            transcript
                .sessions
                .first()
                .map(|session| session.capabilities.clone())
        })
        .unwrap_or_default()
}

/// Sends the target's self-report on, if the stream carried one.
fn report_self<S: std::io::Read + std::io::Write>(client: &Client<S>, events: &Sender<Event>) {
    let Ok(transcript) = Transcript::read(&client.transcript().join("\n")) else {
        return;
    };
    let fields: Vec<(String, Confidence, String)> = transcript
        .self_report()
        .into_iter()
        .map(|field| (field.field, field.confidence, field.value))
        .collect();
    if !fields.is_empty() {
        let _ = events.send(Event::SelfReport(fields));
    }
}
