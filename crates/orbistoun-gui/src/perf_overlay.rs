//! The performance overlay: where a running title's time goes, drawn over its picture.
//!
//! The worker streams a [`PerfReport`] about once a second. This draws the frame rate, the
//! submissions and draws behind it, and each part of presenting a frame as its share of the
//! second, including the unmeasured remainder. The biggest share is highlighted.

use orbistoun_proto::PerfReport;

/// The key that shows and hides the overlay.
pub(crate) const TOGGLE: egui::Key = egui::Key::F3;

/// The lines the overlay shows, and which of them is the biggest share. Pure, so what it says is
/// testable without a window.
pub(crate) fn lines(report: &PerfReport) -> (Vec<String>, Option<usize>) {
    let seconds = (report.window_ms / 1000.0).max(f64::EPSILON);
    let per_second = |n: u64| f64::from(u32::try_from(n).unwrap_or(u32::MAX)) / seconds;
    let mut lines = vec![
        format!("{:.1} fps", per_second(report.flips)),
        format!(
            "{:.1} submissions/s  {:.0} draws/s",
            per_second(report.submissions),
            per_second(report.draws)
        ),
    ];
    let share = |ms: f64| 100.0 * ms / report.window_ms.max(f64::EPSILON);
    // The device's busy time runs alongside the host's, so it is its own line and is kept
    // out of the host's shares below.
    if let Some((_, gpu)) = report
        .phases
        .iter()
        .find(|(name, _)| name == orbistoun_proto::GPU_BUSY_PHASE)
    {
        lines.push(format!("gpu busy {:.0}%", share(*gpu)));
    }
    let is_total = |name: &str| {
        name == orbistoun_proto::SUBMIT_TOTAL_PHASE || name == orbistoun_proto::GPU_BUSY_PHASE
    };
    let submit_total: Option<f64> = report
        .phases
        .iter()
        .find(|(name, _)| is_total(name))
        .map(|(_, ms)| *ms);
    let measured: f64 = report
        .phases
        .iter()
        .filter(|(name, _)| !is_total(name))
        .map(|(_, ms)| ms)
        .sum();
    let present: f64 = report
        .phases
        .iter()
        .filter(|(name, _)| name == "present")
        .map(|(_, ms)| ms)
        .sum();
    let mut shares: Vec<(String, f64)> = report
        .phases
        .iter()
        .filter(|(name, ms)| *ms > 0.0 && !is_total(name))
        .map(|(name, ms)| (name.clone(), share(*ms)))
        .collect();
    // A whole submit contains the phases before "present", so its excess over them is the
    // submit path's unmeasured work; the time beyond the submit and "present" is the guest's.
    match submit_total {
        Some(total) => {
            let inside = measured - present;
            shares.push(("submit, other".to_owned(), share((total - inside).max(0.0))));
            shares.push((
                "guest".to_owned(),
                share((report.window_ms - total - present).max(0.0)),
            ));
        }
        None => shares.push((
            "guest + unmeasured".to_owned(),
            share((report.window_ms - measured).max(0.0)),
        )),
    }
    let biggest = shares
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.1.total_cmp(&b.1.1))
        .map(|(index, _)| index + lines.len());
    lines.extend(
        shares
            .into_iter()
            .map(|(name, percent)| format!("{percent:>5.1}%  {name}")),
    );
    (lines, biggest)
}

/// Draws the overlay in the top-left corner of `over`.
pub(crate) fn draw(ui: &egui::Ui, over: egui::Rect, report: &PerfReport) {
    let (lines, biggest) = lines(report);
    let font = egui::FontId::monospace(13.0);
    let line_height = 16.0;
    let width = 260.0;
    let row = |index: usize| f32::from(u16::try_from(index).unwrap_or(u16::MAX));
    let height = line_height * row(lines.len()) + 12.0;
    let panel =
        egui::Rect::from_min_size(over.min + egui::vec2(8.0, 8.0), egui::vec2(width, height));
    let painter = ui.painter_at(over);
    painter.rect_filled(panel, 4.0, egui::Color32::from_black_alpha(170));
    for (index, line) in lines.iter().enumerate() {
        let colour = if Some(index) == biggest {
            egui::Color32::from_rgb(255, 200, 80)
        } else {
            egui::Color32::from_gray(230)
        };
        painter.text(
            panel.min + egui::vec2(8.0, 6.0 + line_height * row(index)),
            egui::Align2::LEFT_TOP,
            line,
            font.clone(),
            colour,
        );
    }
}

#[cfg(test)]
mod tests {
    use orbistoun_proto::PerfReport;

    /// Rates are per second and the unmeasured remainder is shown and can be the highlight.
    #[test]
    fn the_overlay_says_the_rate_and_where_the_rest_went() {
        let report = PerfReport {
            window_ms: 500.0,
            flips: 10,
            submissions: 10,
            draws: 4000,
            phases: vec![
                ("read target".to_owned(), 100.0),
                ("execute".to_owned(), 100.0),
                ("write target".to_owned(), 100.0),
                ("present".to_owned(), 0.0),
            ],
        };
        let (lines, biggest) = super::lines(&report);
        assert_eq!(lines[0], "20.0 fps");
        assert!(lines[1].contains("8000 draws/s"));
        assert!(
            !lines.iter().any(|l| l.contains("present")),
            "an idle phase is left out"
        );
        let guest = lines.iter().position(|l| l.contains("guest")).unwrap();
        assert!(lines[guest].contains("40.0%"));
        assert_eq!(biggest, Some(guest));
    }
}
