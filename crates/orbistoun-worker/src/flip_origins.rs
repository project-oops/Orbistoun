//! Where each presented frame came from: whether the buffer a flip shows is the one orbistoun drew
//! the frame into, another, or one no draw wrote since the last flip. A title whose flips show a
//! buffer its draws never reach presents whatever else wrote that memory, and its frames say
//! nothing of its draws; the run's summary says which it was.

use std::fmt::Write as _;
use std::sync::Mutex;

/// How a run's flips stood against the frames drawn before them.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FlipOrigins {
    /// Flips that showed the buffer the frame before them was drawn into.
    pub drawn: u64,
    /// Flips that showed another buffer than the one the frame before them was drawn into.
    pub elsewhere: u64,
    /// Flips no drawn frame preceded.
    pub undrawn: u64,
    /// The last flip of the second kind: the buffer shown, and the one drawn into.
    pub last_elsewhere: Option<(u64, u64)>,
    /// The buffer the last flip showed.
    pub last: Option<u64>,
}

impl FlipOrigins {
    /// Counts a flip of `shown`, the frame before it drawn into `drawn_into` where one was.
    pub const fn note(&mut self, shown: u64, drawn_into: Option<u64>) {
        self.last = Some(shown);
        match drawn_into {
            Some(drawn) if drawn == shown => self.drawn += 1,
            Some(drawn) => {
                self.elsewhere += 1;
                self.last_elsewhere = Some((shown, drawn));
            }
            None => self.undrawn += 1,
        }
    }

    /// The summary line, where any flip was counted.
    #[must_use]
    pub fn describe(&self) -> Option<String> {
        if self.drawn + self.elsewhere + self.undrawn == 0 {
            return None;
        }
        let mut line = format!(
            "flips: {} showed the buffer their frame was drawn into, {} another buffer, {} followed no drawn frame",
            self.drawn, self.elsewhere, self.undrawn
        );
        if let Some((shown, drawn)) = self.last_elsewhere {
            let _ = write!(
                line,
                " - the last such flip showed {shown:#x} where the frame was drawn into {drawn:#x}"
            );
        }
        if self.drawn == 0 {
            line.push_str(
                "; no flip showed a frame orbistoun drew, so the frames presented are whatever else wrote the flipped buffers",
            );
        }
        Some(line)
    }
}

/// The run's [`FlipOrigins`].
fn origins() -> &'static Mutex<FlipOrigins> {
    static ORIGINS: Mutex<FlipOrigins> = Mutex::new(FlipOrigins {
        drawn: 0,
        elsewhere: 0,
        undrawn: 0,
        last_elsewhere: None,
        last: None,
    });
    &ORIGINS
}

/// Counts a flip of `shown`, the frame before it drawn into `drawn_into` where one was.
pub fn note_flip(shown: u64, drawn_into: Option<u64>) {
    origins()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .note(shown, drawn_into);
}

/// The run's flips so far.
#[must_use]
pub fn flip_origins() -> FlipOrigins {
    *origins()
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

#[cfg(test)]
mod tests {
    use super::FlipOrigins;

    /// Each flip is counted by where its frame went, and a run whose flips never show a drawn
    /// frame says its frames are not orbistoun's.
    #[test]
    fn flips_are_counted_by_where_their_frame_went() {
        let mut origins = FlipOrigins::default();
        assert_eq!(origins.describe(), None);
        origins.note(0x1000, Some(0x1000));
        origins.note(0x2000, Some(0x9000));
        origins.note(0x2000, None);
        let line = origins.describe().expect("counted");
        assert!(
            line.starts_with(
                "flips: 1 showed the buffer their frame was drawn into, 1 another buffer, 1 followed no drawn frame"
            ),
            "{line}"
        );
        assert!(
            line.contains("showed 0x2000 where the frame was drawn into 0x9000"),
            "{line}"
        );
        assert!(!line.contains("no flip showed"), "{line}");
        assert_eq!(
            origins.last,
            Some(0x2000),
            "the buffer the last flip showed"
        );
        let mut never = FlipOrigins::default();
        never.note(0x2000, None);
        assert!(
            never
                .describe()
                .is_some_and(|line| line.contains("no flip showed a frame orbistoun drew")),
        );
    }
}
