//! The card an orbistoun-aot build shows before its title starts: which key presses which pad
//! control on the first port (D724).
//!
//! Information only. It reads the mapping the window plays with, so it cannot disagree with it,
//! and it changes nothing.

use orbistoun_input::{Button, Port, Push};

/// How long the card stays up when nobody presses anything.
pub(crate) const SHOWN_FOR: std::time::Duration = std::time::Duration::from_secs(10);

/// One pad control and the keys that drive it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Row {
    /// The control as the pad marks it.
    pub(crate) control: &'static str,
    /// Its keys, or `unbound`.
    pub(crate) keys: String,
}

/// The controls, grouped as the pad lays them out, with the keys `port` gives them.
pub(crate) fn rows(port: &Port) -> Vec<(&'static str, Vec<Row>)> {
    let key = |button: Button| shown(port.keys.get(&button).map(String::as_str));
    let keys = |buttons: &[Button]| {
        buttons
            .iter()
            .map(|b| key(*b))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let stick = |pushes: [Push; 4]| {
        pushes
            .iter()
            .map(|p| shown(port.axes.get(p).map(String::as_str)))
            .collect::<Vec<_>>()
            .join(" ")
    };
    let row = |control, keys| Row { control, keys };
    vec![
        (
            "Face buttons",
            vec![
                row("\u{2715} cross", key(Button::South)),
                row("\u{25CB} circle", key(Button::East)),
                row("\u{25A1} square", key(Button::West)),
                row("\u{25B3} triangle", key(Button::North)),
            ],
        ),
        (
            "Directional pad",
            vec![row(
                "up, left, down, right",
                keys(&[Button::Up, Button::Left, Button::Down, Button::Right]),
            )],
        ),
        (
            "Shoulders",
            vec![
                row("L1", key(Button::L1)),
                row("R1", key(Button::R1)),
                row("L2", key(Button::L2)),
                row("R2", key(Button::R2)),
            ],
        ),
        (
            "Sticks",
            vec![
                row(
                    "left stick up, left, down, right",
                    stick([
                        Push::LeftUp,
                        Push::LeftLeft,
                        Push::LeftDown,
                        Push::LeftRight,
                    ]),
                ),
                row(
                    "right stick up, left, down, right",
                    stick([
                        Push::RightUp,
                        Push::RightLeft,
                        Push::RightDown,
                        Push::RightRight,
                    ]),
                ),
                row("L3 (left stick press)", key(Button::L3)),
                row("R3 (right stick press)", key(Button::R3)),
            ],
        ),
        (
            "System",
            vec![
                row("options", key(Button::Start)),
                row("create", key(Button::Select)),
                row("home", key(Button::Shell)),
            ],
        ),
    ]
}

/// A key name as a person reads it on a keyboard.
fn shown(name: Option<&str>) -> String {
    match name {
        None => "unbound".to_owned(),
        Some("ArrowUp") => "\u{2191}".to_owned(),
        Some("ArrowDown") => "\u{2193}".to_owned(),
        Some("ArrowLeft") => "\u{2190}".to_owned(),
        Some("ArrowRight") => "\u{2192}".to_owned(),
        Some(other) => other.to_owned(),
    }
}

/// Draws the card for `title`, and answers whether it is done: a key or a click dismisses it, and
/// it goes by itself after [`SHOWN_FOR`].
pub(crate) fn card(
    ctx: &egui::Context,
    title: &str,
    port: &Port,
    shown_for: std::time::Duration,
) -> bool {
    let pressed = ctx.input(|i| {
        i.pointer.any_click()
            || i.events
                .iter()
                .any(|e| matches!(e, egui::Event::Key { pressed: true, .. }))
    });
    if pressed || shown_for >= SHOWN_FOR {
        return true;
    }
    let left = SHOWN_FOR.saturating_sub(shown_for).as_secs() + 1;
    egui::CentralPanel::default()
        .frame(egui::Frame::none().fill(egui::Color32::from_gray(16)))
        .show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(24.0);
                ui.heading(title);
                ui.label("Controls on the keyboard");
                ui.add_space(12.0);
                for (group, rows) in rows(port) {
                    ui.strong(group);
                    egui::Grid::new(group)
                        .num_columns(2)
                        .spacing([32.0, 4.0])
                        .show(ui, |ui| {
                            for row in rows {
                                ui.label(row.control);
                                ui.monospace(row.keys);
                                ui.end_row();
                            }
                        });
                    ui.add_space(8.0);
                }
                ui.add_space(12.0);
                ui.label(format!(
                    "Press any key to start - starting by itself in {left} s"
                ));
            });
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(250));
    false
}

#[cfg(test)]
mod tests {
    use super::rows;
    use orbistoun_input::{Button, Port, Push};

    /// The default port's keys appear against the controls they drive, and a control with no key
    /// says so.
    #[test]
    fn the_card_lists_the_keys_the_port_plays_with() {
        let mut port = Port {
            keys: orbistoun_input::mapping::default_keys(),
            axes: orbistoun_input::mapping::default_axes(),
            ..Port::default()
        };
        let find = |port: &Port, control: &str| {
            rows(port)
                .into_iter()
                .flat_map(|(_, rows)| rows)
                .find(|row| row.control == control)
                .map(|row| row.keys)
        };
        assert_eq!(find(&port, "\u{2715} cross").as_deref(), Some("K"));
        assert_eq!(
            find(&port, "up, left, down, right").as_deref(),
            Some("\u{2191} \u{2190} \u{2193} \u{2192}")
        );
        assert_eq!(
            find(&port, "left stick up, left, down, right").as_deref(),
            Some("W A S D")
        );
        assert_eq!(find(&port, "options").as_deref(), Some("Enter"));

        port.keys.remove(&Button::North);
        port.axes.remove(&Push::RightUp);
        assert_eq!(find(&port, "\u{25B3} triangle").as_deref(), Some("unbound"));
        assert!(
            find(&port, "right stick up, left, down, right")
                .is_some_and(|keys| keys.starts_with("unbound"))
        );
    }
}
