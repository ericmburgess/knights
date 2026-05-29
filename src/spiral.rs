//! The square-spiral coordinate system.
//!
//! Maps between a 0-based spiral *position* and a lattice *coordinate* `(x, y)`
//! with x pointing right and y pointing up. We use the standard counterclockwise
//! Ulam spiral: starting at the center, step right, up, left, down, with arm
//! lengths 1, 1, 2, 2, 3, 3, … The handedness/orientation does not affect the
//! trapped-knight result (knight moves are symmetric under rotation/reflection),
//! only the picture's orientation.
//!
//! The human-facing **square number** is `position + start` (start defaults to 1).
//! Because `start` is a constant offset, comparing square numbers is identical to
//! comparing positions, so the simulation works purely on positions.

use std::collections::HashMap;

pub struct Spiral {
    /// position (0-based) -> coordinate
    coords: Vec<(i32, i32)>,
    /// coordinate -> position (0-based)
    index: HashMap<(i32, i32), u64>,
    // --- incremental walker state ---
    /// coordinate of the last cell placed
    cx: i32,
    cy: i32,
    /// index of the current spiral arm (0 = first arm going right)
    arm: u32,
    /// steps remaining before the current arm turns
    steps_left_in_arm: u32,
    /// direction of the current arm: 0=right, 1=up, 2=left, 3=down
    dir: usize,
}

impl Spiral {
    pub fn new() -> Self {
        let mut s = Spiral {
            coords: Vec::new(),
            index: HashMap::new(),
            cx: 0,
            cy: 0,
            arm: 0,
            steps_left_in_arm: 0,
            dir: 0,
        };
        // Position 0 is the center.
        s.coords.push((0, 0));
        s.index.insert((0, 0), 0);
        s
    }

    /// Append the next cell to the spiral.
    fn extend_by_one(&mut self) {
        if self.steps_left_in_arm == 0 {
            // Begin a new arm. Arm lengths grow 1,1,2,2,3,3,… and the
            // direction cycles right, up, left, down.
            self.steps_left_in_arm = self.arm / 2 + 1;
            self.dir = (self.arm % 4) as usize;
            self.arm += 1;
        }
        let (dx, dy) = match self.dir {
            0 => (1, 0),  // right
            1 => (0, 1),  // up
            2 => (-1, 0), // left
            _ => (0, -1), // down
        };
        self.cx += dx;
        self.cy += dy;
        self.steps_left_in_arm -= 1;

        let pos = self.coords.len() as u64;
        self.coords.push((self.cx, self.cy));
        self.index.insert((self.cx, self.cy), pos);
    }

    /// Walk the spiral until every cell within Chebyshev radius `r` of the
    /// center exists. A completed ring of radius `r` contains `(2r+1)^2` cells.
    pub fn ensure_radius(&mut self, r: i32) {
        let r = r.max(0) as u64;
        let d = 2 * r + 1;
        let target = d * d;
        while (self.coords.len() as u64) < target {
            self.extend_by_one();
        }
    }

    /// The spiral position at coordinate `(x, y)`, if it has been mapped.
    pub fn coord_to_index(&self, x: i32, y: i32) -> Option<u64> {
        self.index.get(&(x, y)).copied()
    }

    /// The coordinate at spiral position `n`. The caller must have extended the
    /// spiral far enough (e.g. via [`ensure_radius`]) to cover `n`.
    pub fn index_to_coord(&self, n: u64) -> (i32, i32) {
        self.coords[n as usize]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn first_ring_matches_known_layout() {
        let mut s = Spiral::new();
        s.ensure_radius(1);
        // Positions 0..=8 of the counterclockwise spiral.
        let expected = [
            (0, 0),   // 0 center
            (1, 0),   // 1 right
            (1, 1),   // 2 up
            (0, 1),   // 3 left
            (-1, 1),  // 4 left
            (-1, 0),  // 5 down
            (-1, -1), // 6 down
            (0, -1),  // 7 right
            (1, -1),  // 8 right (completes ring 1)
        ];
        for (n, &coord) in expected.iter().enumerate() {
            assert_eq!(s.index_to_coord(n as u64), coord, "position {n}");
            assert_eq!(s.coord_to_index(coord.0, coord.1), Some(n as u64));
        }
    }

    #[test]
    fn ensure_radius_fills_full_square() {
        let mut s = Spiral::new();
        s.ensure_radius(3);
        // Every cell within Chebyshev radius 3 must be mapped.
        for y in -3..=3 {
            for x in -3..=3 {
                assert!(s.coord_to_index(x, y).is_some(), "({x},{y}) missing");
            }
        }
    }
}
