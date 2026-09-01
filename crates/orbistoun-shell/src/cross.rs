//! Moving around a row of categories with a column under each.
//!
//! # Why the navigation is here and not in the window
//!
//! It is four rules and every one of them has an edge: what happens at the ends, what
//! happens to the highlighted item when the column beside it is shorter, and what happens
//! when a category is empty. Written inline in a draw function those are discovered by
//! somebody holding a direction until something looks wrong.
//!
//! So it is a pure type over a *shape* - how many items each category holds - and the edges
//! below are assertions (principle 8). The window supplies the shape and draws the result.
//!
//! # The rules, and why these
//!
//! **Nothing wraps.** Holding a direction should come to rest against the end rather than
//! cycling past it: a person navigating by feel counts presses, and a list that wraps turns
//! one press too many into a journey back around.
//!
//! **The item is clamped, not remembered per category.** Moving from a category of twelve
//! titles to one of three settings lands on the last setting rather than nothing. Keeping a
//! separate position per category was the alternative and it is worse in the common case:
//! coming back to a long list and finding the highlight where you left it sounds right, and
//! in practice it means the highlight is somewhere off screen that you did not choose.

/// Which way somebody pushed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Move {
    /// Previous category.
    Left,
    /// Next category.
    Right,
    /// Previous item in this category.
    Up,
    /// Next item in this category.
    Down,
}

/// Where the highlight is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Cross {
    /// Which category, along the row.
    pub category: usize,
    /// Which item, down the column.
    pub item: usize,
}

impl Cross {
    /// Moves the highlight, given how many items each category holds.
    ///
    /// `shape` is one count per category; an empty shape leaves everything where it is,
    /// because there is nowhere to go and pretending otherwise would put the highlight on a
    /// category that does not exist.
    pub fn steer(&mut self, direction: Move, shape: &[usize]) {
        if shape.is_empty() {
            return;
        }
        let last = shape.len() - 1;
        match direction {
            Move::Left => self.category = self.category.saturating_sub(1),
            Move::Right => self.category = (self.category + 1).min(last),
            Move::Up => self.item = self.item.saturating_sub(1),
            Move::Down => self.item += 1,
        }
        self.clamp(shape);
    }

    /// Brings the highlight back inside the shape.
    ///
    /// Called after every move, and **also worth calling when the shape changes underneath**:
    /// a library rescan that finds fewer titles than last time would otherwise leave the
    /// highlight past the end, which draws as nothing selected.
    pub fn clamp(&mut self, shape: &[usize]) {
        if shape.is_empty() {
            *self = Self::default();
            return;
        }
        self.category = self.category.min(shape.len() - 1);
        // An empty category is a real thing to be looking at - "no titles here yet" is a
        // screen somebody has to be able to reach - so the item rests at zero rather than
        // the category being skipped over.
        self.item = self.item.min(shape[self.category].saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::{Cross, Move};

    /// Three categories holding twelve, three and one.
    const SHAPE: [usize; 3] = [12, 3, 1];

    /// **Nothing wraps, at either end of either axis.**
    #[test]
    fn the_highlight_comes_to_rest_against_the_ends() {
        let mut at = Cross::default();
        for _ in 0..5 {
            at.steer(Move::Left, &SHAPE);
        }
        assert_eq!(at, Cross::default(), "the left end holds");

        for _ in 0..10 {
            at.steer(Move::Right, &SHAPE);
        }
        assert_eq!(at.category, 2, "and so does the right");

        for _ in 0..10 {
            at.steer(Move::Down, &SHAPE);
        }
        assert_eq!(at.item, 0, "the last category holds one item");
    }

    /// **Moving to a shorter category lands on its last item, not past it.**
    ///
    /// The bug this prevents draws as nothing being selected, which reads as the window
    /// having stopped responding.
    #[test]
    fn the_item_is_clamped_when_the_column_beside_it_is_shorter() {
        let mut at = Cross::default();
        for _ in 0..9 {
            at.steer(Move::Down, &SHAPE);
        }
        assert_eq!(at.item, 9);

        at.steer(Move::Right, &SHAPE);
        assert_eq!(at.category, 1);
        assert_eq!(at.item, 2, "the second category holds three");
    }

    /// A category with nothing in it is somewhere you can be.
    ///
    /// "No titles here yet" is a screen somebody has to be able to reach, so an empty
    /// category is not skipped over.
    #[test]
    fn an_empty_category_can_be_selected_and_rests_at_zero() {
        let shape = [0_usize, 2];
        let mut at = Cross::default();
        at.steer(Move::Down, &shape);

        assert_eq!(at.category, 0);
        assert_eq!(at.item, 0);
    }

    /// **A shape that shrinks underneath the highlight is brought back in.**
    ///
    /// A rescan finding fewer titles than last time is ordinary, and a highlight left past
    /// the end draws as no selection at all.
    #[test]
    fn clamping_recovers_from_the_shape_changing() {
        let mut at = Cross {
            category: 2,
            item: 9,
        };
        at.clamp(&[4, 4]);

        assert_eq!(at.category, 1);
        assert_eq!(at.item, 3);
    }

    /// With nowhere to go, nothing moves and nothing panics.
    #[test]
    fn an_empty_shape_leaves_the_highlight_alone() {
        let mut at = Cross::default();
        at.steer(Move::Right, &[]);
        assert_eq!(at, Cross::default());
    }
}
