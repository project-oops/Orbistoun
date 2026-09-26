//! Navigation over a row of categories with a column of items under each.
//!
//! A pure type over a shape (how many items each category holds), so the edge cases are
//! tested here rather than in the window, which supplies the shape and draws the result.
//! Nothing wraps: holding a direction comes to rest at the end. The item is clamped to the
//! new column rather than remembered per category, so moving to a shorter category lands
//! on its last item.

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
    /// `shape` is one count per category. An empty shape leaves the highlight where it is,
    /// because there is no category to move to.
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
    /// Called after every move, and by the caller whenever the shape changes: a rescan that
    /// finds fewer titles would otherwise leave the highlight past the end.
    pub fn clamp(&mut self, shape: &[usize]) {
        if shape.is_empty() {
            *self = Self::default();
            return;
        }
        self.category = self.category.min(shape.len() - 1);
        // An empty category is reachable ("no titles here yet"), so the item rests at zero
        // rather than the category being skipped.
        self.item = self.item.min(shape[self.category].saturating_sub(1));
    }
}

#[cfg(test)]
mod tests {
    use super::{Cross, Move};

    /// Three categories holding twelve, three and one.
    const SHAPE: [usize; 3] = [12, 3, 1];

    /// Nothing wraps, at either end of either axis.
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

    /// Moving to a shorter category lands on its last item, not past it.
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

    /// An empty category is reachable, not skipped over.
    #[test]
    fn an_empty_category_can_be_selected_and_rests_at_zero() {
        let shape = [0_usize, 2];
        let mut at = Cross::default();
        at.steer(Move::Down, &shape);

        assert_eq!(at.category, 0);
        assert_eq!(at.item, 0);
    }

    /// A shape that shrinks underneath the highlight brings it back inside.
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
