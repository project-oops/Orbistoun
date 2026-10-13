//! The system keyboard: text entered a key at a time with a controller (D826).
//!
//! A title that asks for text opens it, and while it is open it has the controller: the title sees
//! a neutral pad. A highlight moves over a grid of keys with the four directions, and a press
//! types the highlighted key. The layout is a plain grid of our own - no artwork, arrangement or
//! proportions of anyone else's are copied - and every key is reachable with the directions and one
//! button. The window draws it from a [`View`]; the rules live here, where they are tested.

use serde::{Deserialize, Serialize};

use crate::cross::Move;

/// A key on the grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Key {
    /// Types its character, upper case while shifted.
    Char(char),
    /// Switches letters between lower and upper case.
    Shift,
    /// Types a space.
    Space,
    /// Removes the last character.
    Delete,
    /// Ends the entry with the text typed.
    Done,
}

/// What a controller press asks of the keyboard.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Press {
    /// Moves the highlight.
    Move(Move),
    /// Presses the highlighted key.
    Select,
    /// Removes the last character, whatever is highlighted.
    Delete,
    /// Types a space, whatever is highlighted.
    Space,
    /// Ends the entry with the text typed, whatever is highlighted.
    Done,
    /// Ends the entry without it.
    Cancel,
}

/// How the entry stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Outcome {
    /// Still being typed.
    Typing,
    /// Ended with the text typed.
    Entered,
    /// Ended without it.
    Cancelled,
}

/// Columns every row spans; a key takes one or more.
pub const COLUMNS: usize = 10;

/// The rows, top to bottom: each key with the columns it spans, adding up to [`COLUMNS`].
const ROWS: [&[(Key, usize)]; 5] = [
    &[
        (Key::Char('1'), 1),
        (Key::Char('2'), 1),
        (Key::Char('3'), 1),
        (Key::Char('4'), 1),
        (Key::Char('5'), 1),
        (Key::Char('6'), 1),
        (Key::Char('7'), 1),
        (Key::Char('8'), 1),
        (Key::Char('9'), 1),
        (Key::Char('0'), 1),
    ],
    &[
        (Key::Char('q'), 1),
        (Key::Char('w'), 1),
        (Key::Char('e'), 1),
        (Key::Char('r'), 1),
        (Key::Char('t'), 1),
        (Key::Char('y'), 1),
        (Key::Char('u'), 1),
        (Key::Char('i'), 1),
        (Key::Char('o'), 1),
        (Key::Char('p'), 1),
    ],
    &[
        (Key::Char('a'), 1),
        (Key::Char('s'), 1),
        (Key::Char('d'), 1),
        (Key::Char('f'), 1),
        (Key::Char('g'), 1),
        (Key::Char('h'), 1),
        (Key::Char('j'), 1),
        (Key::Char('k'), 1),
        (Key::Char('l'), 1),
        (Key::Char('-'), 1),
    ],
    &[
        (Key::Char('z'), 1),
        (Key::Char('x'), 1),
        (Key::Char('c'), 1),
        (Key::Char('v'), 1),
        (Key::Char('b'), 1),
        (Key::Char('n'), 1),
        (Key::Char('m'), 1),
        (Key::Char('.'), 1),
        (Key::Char('@'), 1),
        (Key::Char('_'), 1),
    ],
    &[
        (Key::Shift, 2),
        (Key::Space, 4),
        (Key::Delete, 2),
        (Key::Done, 2),
    ],
];

/// The keyboard as it stands: the text, the highlight and the case.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Keyboard {
    text: Vec<char>,
    most: usize,
    row: usize,
    /// The column the highlight is over; the key there is the one highlighted, so moving up and
    /// down keeps to the same column.
    column: usize,
    shifted: bool,
    outcome: Outcome,
}

/// What the window draws: the rows of keys, which is highlighted, the text and the case.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct View {
    /// Each row's keys with the columns each spans, top to bottom.
    pub rows: Vec<Vec<(Key, usize)>>,
    /// The highlighted key, by row and its index in the row.
    pub highlight: (usize, usize),
    /// The text typed so far.
    pub text: String,
    /// The most characters the title takes.
    pub most: usize,
    /// Whether letters type in upper case.
    pub shifted: bool,
}

impl Keyboard {
    /// A keyboard over `text` already in the title's field, taking at most `most` characters,
    /// highlighting the first key.
    #[must_use]
    pub fn new(text: &str, most: usize) -> Self {
        Self {
            text: text.chars().take(most).collect(),
            most,
            row: 0,
            column: 0,
            shifted: false,
            outcome: Outcome::Typing,
        }
    }

    /// Acts on one press. Once the entry has ended, nothing changes it.
    pub fn press(&mut self, press: Press) {
        if self.outcome != Outcome::Typing {
            return;
        }
        match press {
            Press::Move(direction) => self.steer(direction),
            Press::Select => self.select(),
            Press::Delete => {
                self.text.pop();
            }
            Press::Space => self.type_char(' '),
            Press::Done => self.outcome = Outcome::Entered,
            Press::Cancel => self.outcome = Outcome::Cancelled,
        }
    }

    /// The text typed so far.
    #[must_use]
    pub fn text(&self) -> String {
        self.text.iter().collect()
    }

    /// How the entry stands.
    #[must_use]
    pub const fn outcome(&self) -> Outcome {
        self.outcome
    }

    /// The highlighted key.
    #[must_use]
    pub fn highlighted(&self) -> Key {
        ROWS[self.row][self.index()].0
    }

    /// What the window draws.
    #[must_use]
    pub fn view(&self) -> View {
        View {
            rows: ROWS.iter().map(|row| row.to_vec()).collect(),
            highlight: (self.row, self.index()),
            text: self.text(),
            most: self.most,
            shifted: self.shifted,
        }
    }

    /// The index in its row of the key over the highlight's column.
    fn index(&self) -> usize {
        let mut start = 0;
        for (index, &(_, span)) in ROWS[self.row].iter().enumerate() {
            if self.column < start + span {
                return index;
            }
            start += span;
        }
        ROWS[self.row].len() - 1
    }

    /// The column the key at `index` of the highlight's row starts at.
    fn start_of(&self, index: usize) -> usize {
        ROWS[self.row][..index].iter().map(|&(_, span)| span).sum()
    }

    /// Moves the highlight; nothing wraps, as on the shell's own grid.
    fn steer(&mut self, direction: Move) {
        match direction {
            Move::Up => self.row = self.row.saturating_sub(1),
            Move::Down => self.row = (self.row + 1).min(ROWS.len() - 1),
            Move::Left => {
                let index = self.index();
                if index > 0 {
                    self.column = self.start_of(index - 1);
                }
            }
            Move::Right => {
                let index = self.index();
                if index + 1 < ROWS[self.row].len() {
                    self.column = self.start_of(index + 1);
                }
            }
        }
    }

    fn select(&mut self) {
        match self.highlighted() {
            Key::Char(c) => {
                let c = if self.shifted {
                    c.to_ascii_uppercase()
                } else {
                    c
                };
                self.type_char(c);
            }
            Key::Shift => self.shifted = !self.shifted,
            Key::Space => self.type_char(' '),
            Key::Delete => {
                self.text.pop();
            }
            Key::Done => self.outcome = Outcome::Entered,
        }
    }

    /// Types `c` unless the field is full.
    fn type_char(&mut self, c: char) {
        if self.text.len() < self.most {
            self.text.push(c);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{COLUMNS, Key, Keyboard, Outcome, Press, ROWS};
    use crate::cross::Move;

    /// Every row spans the same columns, so up and down keep to a column.
    #[test]
    fn every_row_spans_the_grid() {
        for row in ROWS {
            assert_eq!(row.iter().map(|&(_, span)| span).sum::<usize>(), COLUMNS);
        }
    }

    /// A name typed as a controller types it: across to a letter, shift for a capital, a press for
    /// each, a slip deleted, then Done.
    #[test]
    fn a_name_is_typed_with_the_directions_and_one_button() {
        let mut keyboard = Keyboard::new("", 16);
        let press = |keyboard: &mut Keyboard, presses: &[Press]| {
            for &press in presses {
                keyboard.press(press);
            }
        };
        // Down to the bottom row, onto Shift, then back up to the first letter row: "T".
        press(
            &mut keyboard,
            &[
                Press::Move(Move::Down),
                Press::Move(Move::Down),
                Press::Move(Move::Down),
                Press::Move(Move::Down),
                Press::Select,
                Press::Move(Move::Up),
                Press::Move(Move::Up),
                Press::Move(Move::Up),
            ],
        );
        for _ in 0..4 {
            keyboard.press(Press::Move(Move::Right));
        }
        assert_eq!(keyboard.highlighted(), Key::Char('t'));
        keyboard.press(Press::Select);
        // Unshift on the bottom row, then "u" and a slip "i", deleted, then "x".
        press(
            &mut keyboard,
            &[
                Press::Move(Move::Down),
                Press::Move(Move::Down),
                Press::Move(Move::Down),
            ],
        );
        assert_eq!(
            keyboard.highlighted(),
            Key::Space,
            "column 4 lies in Space's span"
        );
        keyboard.press(Press::Move(Move::Left));
        assert_eq!(keyboard.highlighted(), Key::Shift);
        keyboard.press(Press::Select);
        press(
            &mut keyboard,
            &[
                Press::Move(Move::Up),
                Press::Move(Move::Up),
                Press::Move(Move::Up),
            ],
        );
        for _ in 0..6 {
            keyboard.press(Press::Move(Move::Right));
        }
        assert_eq!(keyboard.highlighted(), Key::Char('u'));
        keyboard.press(Press::Select);
        keyboard.press(Press::Move(Move::Right));
        keyboard.press(Press::Select);
        keyboard.press(Press::Delete);
        press(
            &mut keyboard,
            &[
                Press::Move(Move::Down),
                Press::Move(Move::Down),
                Press::Move(Move::Left),
                Press::Move(Move::Left),
                Press::Move(Move::Left),
                Press::Move(Move::Left),
                Press::Move(Move::Left),
                Press::Move(Move::Left),
                Press::Select,
            ],
        );
        assert_eq!(keyboard.text(), "Tux");
        assert_eq!(keyboard.outcome(), Outcome::Typing);
        keyboard.press(Press::Done);
        assert_eq!(keyboard.outcome(), Outcome::Entered);
        keyboard.press(Press::Select);
        assert_eq!(
            keyboard.text(),
            "Tux",
            "nothing changes it once it has ended"
        );
    }

    /// Moving up and down keeps to the column: from Space on the bottom row back up lands over
    /// the column it came from, not the start of the row.
    #[test]
    fn a_wide_key_keeps_the_column_it_was_reached_from() {
        let mut keyboard = Keyboard::new("", 4);
        for _ in 0..5 {
            keyboard.press(Press::Move(Move::Right));
        }
        for _ in 0..4 {
            keyboard.press(Press::Move(Move::Down));
        }
        assert_eq!(keyboard.highlighted(), Key::Space);
        for _ in 0..4 {
            keyboard.press(Press::Move(Move::Up));
        }
        assert_eq!(keyboard.highlighted(), Key::Char('6'));
    }

    /// The field takes no more than the title allows, and the text it already held is kept;
    /// a cancel ends without it.
    #[test]
    fn the_field_holds_what_the_title_allows() {
        let mut keyboard = Keyboard::new("abcdef", 4);
        assert_eq!(keyboard.text(), "abcd", "cut to the most it takes");
        keyboard.press(Press::Space);
        assert_eq!(keyboard.text(), "abcd", "full");
        keyboard.press(Press::Delete);
        keyboard.press(Press::Space);
        assert_eq!(keyboard.text(), "abc ");
        keyboard.press(Press::Cancel);
        assert_eq!(keyboard.outcome(), Outcome::Cancelled);
    }
}
