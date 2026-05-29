//! The square-spiral coordinate system.
//!
//! Maps between a 0-based spiral *position* and a lattice *coordinate* `(x, y)`
//! with x pointing right and y pointing up. [`SpiralWalker::new`] is the standard
//! counterclockwise Ulam spiral: starting at the center, step right, up, left, down,
//! with arm lengths 1, 1, 2, 2, 3, 3, … The handedness/orientation does not affect the
//! trapped-knight result (knight moves are symmetric under rotation/reflection),
//! only the picture's orientation.
//!
//! [`SpiralWalker::oriented`] generalizes this to the eight square-spiral orientations
//! (any of the four start [`Direction`]s × either [`Handedness`]). Each is a symmetry
//! image of the canonical spiral, so all eight cover the same radius-`r` square — they
//! differ only in traversal order. The placement engine uses this to give each piece
//! its own spiral.
//!
//! The human-facing **square number** is `position + start` (start defaults to 1).
//! Because `start` is a constant offset, comparing square numbers is identical to
//! comparing positions, so the simulation works purely on positions.

use std::collections::HashMap;

/// The direction a spiral's first arm travels (its overall orientation, since the
/// rest of the arms follow from this and the [`Handedness`]). Indices 0..=3 match the
/// `dir` cycle right, up, left, down.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Right,
    Up,
    Left,
    Down,
}

impl Direction {
    /// Position in the right→up→left→down cycle (0..=3).
    fn index(self) -> u32 {
        match self {
            Direction::Right => 0,
            Direction::Up => 1,
            Direction::Left => 2,
            Direction::Down => 3,
        }
    }
}

/// Which way the spiral turns at each arm: counterclockwise (right→up→left→down) or
/// clockwise (right→down→left→up).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Handedness {
    Ccw,
    Cw,
}

/// Walks the square spiral one cell at a time, holding only O(1) state and no
/// history. This is the stepping primitive: [`Spiral`] uses it to fill its
/// lookup tables, and the placement simulations that don't need a stored table
/// drive it directly (so they never allocate per-cell memory).
pub struct SpiralWalker {
    /// position of the cell `step` will return next
    pos: u64,
    /// coordinate of that cell
    x: i32,
    y: i32,
    /// index of the current spiral arm (0 = first arm)
    arm: u32,
    /// steps remaining before the current arm turns
    steps_left: u32,
    /// direction of the current arm: 0=right, 1=up, 2=left, 3=down
    dir: usize,
    /// direction index of the first arm (0..=3)
    start_dir: u32,
    /// per-arm rotation: 1 = counterclockwise, 3 = clockwise (≡ -1 mod 4)
    turn: u32,
}

impl SpiralWalker {
    /// The canonical spiral: first arm right, turning counterclockwise.
    pub fn new() -> Self {
        Self::oriented(Direction::Right, Handedness::Ccw)
    }

    /// A spiral whose first arm goes `direction` and which turns per `handed`. The
    /// arm-length pattern (1, 1, 2, 2, 3, 3, …) is the same for every orientation, so
    /// every choice tiles the plane identically — only the order of visitation differs.
    pub fn oriented(direction: Direction, handed: Handedness) -> Self {
        let start_dir = direction.index();
        SpiralWalker {
            pos: 0,
            x: 0,
            y: 0,
            arm: 0,
            steps_left: 0,
            dir: start_dir as usize,
            start_dir,
            turn: match handed {
                Handedness::Ccw => 1,
                Handedness::Cw => 3,
            },
        }
    }

    /// The position of the cell the next [`step`](Self::step) will return.
    pub fn position(&self) -> u64 {
        self.pos
    }

    /// Return the current cell as `(position, x, y)` and advance to the next.
    pub fn step(&mut self) -> (u64, i32, i32) {
        let here = (self.pos, self.x, self.y);
        if self.steps_left == 0 {
            // Begin a new arm. Arm lengths grow 1,1,2,2,3,3,…; the direction starts at
            // `start_dir` and advances by `turn` each arm (ccw: +1, cw: -1 ≡ +3 mod 4).
            self.steps_left = self.arm / 2 + 1;
            self.dir = ((self.start_dir + self.turn * self.arm) % 4) as usize;
            self.arm += 1;
        }
        let (dx, dy) = match self.dir {
            0 => (1, 0),  // right
            1 => (0, 1),  // up
            2 => (-1, 0), // left
            _ => (0, -1), // down
        };
        self.x += dx;
        self.y += dy;
        self.steps_left -= 1;
        self.pos += 1;
        here
    }
}

pub struct Spiral {
    /// position (0-based) -> coordinate
    coords: Vec<(i32, i32)>,
    /// coordinate -> position (0-based); empty unless `track_index` is set
    index: HashMap<(i32, i32), u64>,
    /// whether to maintain the reverse `index` map (needed for `coord_to_index`)
    track_index: bool,
    /// stepper used to extend the tables
    walker: SpiralWalker,
}

impl Spiral {
    /// A spiral that maps both directions, so [`coord_to_index`](Self::coord_to_index)
    /// works. Used by the trapped knight, which looks up neighbor positions.
    pub fn new() -> Self {
        Self::build(true)
    }

    /// A spiral that only maps position -> coordinate, with no reverse `HashMap`.
    /// The placement problems never call [`coord_to_index`](Self::coord_to_index),
    /// so this saves a large amount of memory at big radii (millions of cells).
    pub fn positional() -> Self {
        Self::build(false)
    }

    fn build(track_index: bool) -> Self {
        let mut s = Spiral {
            coords: Vec::new(),
            index: HashMap::new(),
            track_index,
            walker: SpiralWalker::new(),
        };
        s.ensure_radius(0); // materialize the center (position 0 = (0, 0))
        s
    }

    /// Walk the spiral until every cell within Chebyshev radius `r` of the
    /// center exists. A completed ring of radius `r` contains `(2r+1)^2` cells.
    pub fn ensure_radius(&mut self, r: i32) {
        let r = r.max(0) as u64;
        let d = 2 * r + 1;
        let target = d * d;
        while (self.coords.len() as u64) < target {
            let (pos, x, y) = self.walker.step();
            self.coords.push((x, y));
            if self.track_index {
                self.index.insert((x, y), pos);
            }
        }
    }

    /// The spiral position at coordinate `(x, y)`, if it has been mapped.
    /// Always returns `None` on a [`positional`](Self::positional) spiral.
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
    fn walker_matches_spiral_layout() {
        let expected = [
            (0, 0),
            (1, 0),
            (1, 1),
            (0, 1),
            (-1, 1),
            (-1, 0),
            (-1, -1),
            (0, -1),
            (1, -1),
        ];
        let mut w = SpiralWalker::new();
        for (n, &coord) in expected.iter().enumerate() {
            assert_eq!(w.position(), n as u64);
            assert_eq!(w.step(), (n as u64, coord.0, coord.1));
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

    /// Walk a full radius-`r` square's worth of steps and return the coordinates in
    /// visitation order.
    fn walk(direction: Direction, handed: Handedness, r: i32) -> Vec<(i32, i32)> {
        let n = ((2 * r + 1) * (2 * r + 1)) as u64;
        let mut w = SpiralWalker::oriented(direction, handed);
        (0..n).map(|_| { let (_, x, y) = w.step(); (x, y) }).collect()
    }

    #[test]
    fn oriented_default_matches_canonical() {
        // new() must be byte-identical to oriented(Right, Ccw).
        assert_eq!(walk(Direction::Right, Handedness::Ccw, 6), {
            let mut w = SpiralWalker::new();
            (0..169u64).map(|_| { let (_, x, y) = w.step(); (x, y) }).collect::<Vec<_>>()
        });
    }

    #[test]
    fn oriented_eight_orientations_each_tile_the_square() {
        use std::collections::HashSet;
        let r = 7;
        let full: HashSet<(i32, i32)> =
            (-r..=r).flat_map(|y| (-r..=r).map(move |x| (x, y))).collect();
        for dir in [Direction::Right, Direction::Up, Direction::Left, Direction::Down] {
            for handed in [Handedness::Ccw, Handedness::Cw] {
                let coords = walk(dir, handed, r);
                let set: HashSet<(i32, i32)> = coords.iter().copied().collect();
                assert_eq!(coords.len(), set.len(), "{dir:?}/{handed:?}: visited a cell twice");
                assert_eq!(set, full, "{dir:?}/{handed:?}: does not tile the radius-{r} square");
            }
        }
    }

    #[test]
    fn oriented_reproduces_redblack_variant_transforms() {
        // The two-color variants are coordinate transforms of the canonical walk:
        //   rot180 = (-x, -y) = Left/Ccw ;  mirror = (-x, y) = Left/Cw.
        let r = 6;
        let canon = walk(Direction::Right, Handedness::Ccw, r);
        let rot180: Vec<_> = canon.iter().map(|&(x, y)| (-x, -y)).collect();
        let mirror: Vec<_> = canon.iter().map(|&(x, y)| (-x, y)).collect();
        assert_eq!(walk(Direction::Left, Handedness::Ccw, r), rot180, "rot180 = Left/Ccw");
        assert_eq!(walk(Direction::Left, Handedness::Cw, r), mirror, "mirror = Left/Cw");
    }
}
