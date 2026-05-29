//! Knight moves and the trapped-knight simulation (Problem 1).

use crate::spiral::Spiral;
use std::collections::HashSet;

/// The 8 knight moves: (±1, ±2) and (±2, ±1).
pub const KNIGHT_OFFSETS: [(i32, i32); 8] = [
    (1, 2),
    (2, 1),
    (2, -1),
    (1, -2),
    (-1, -2),
    (-2, -1),
    (-2, 1),
    (-1, 2),
];

/// The result of one trapped-knight run.
pub struct SimResult {
    /// The knight's path: `(position, x, y)` for each square it occupied, in order.
    pub path: Vec<(u64, i32, i32)>,
    /// The offset added to a 0-based position to get its human-facing square number.
    pub start: u64,
}

impl SimResult {
    /// Number of squares the knight occupied (including the start).
    pub fn squares_visited(&self) -> usize {
        self.path.len()
    }

    /// Number of hops the knight made.
    pub fn moves(&self) -> usize {
        self.path.len().saturating_sub(1)
    }

    /// Human-facing square number of the starting square.
    pub fn start_square(&self) -> u64 {
        self.path.first().map_or(self.start, |&(p, _, _)| p + self.start)
    }

    /// Human-facing square number of the final (trap) square.
    pub fn trap_square(&self) -> u64 {
        self.path.last().map_or(self.start, |&(p, _, _)| p + self.start)
    }
}

/// Run the trapped-knight simulation.
///
/// The knight starts at the center (position 0) and on each turn hops to the
/// lowest-numbered unvisited square a knight's move away. It stops when all 8
/// knight moves land on already-visited squares — the knight is trapped.
///
/// `start` is the number assigned to the center square; it only shifts the
/// reported square numbers and does not change which squares are visited.
pub fn simulate_trapped_knight(start: u64) -> SimResult {
    let mut spiral = Spiral::new();
    let mut visited: HashSet<u64> = HashSet::new();
    let mut path: Vec<(u64, i32, i32)> = Vec::new();

    // Begin at the center (position 0).
    let (mut x, mut y) = spiral.index_to_coord(0);
    visited.insert(0);
    path.push((0, x, y));

    loop {
        // Make sure all 8 knight neighbors (Chebyshev distance 2) are mapped.
        spiral.ensure_radius(x.abs().max(y.abs()) + 2);

        // Find the lowest-numbered unvisited neighbor.
        let mut best: Option<(u64, i32, i32)> = None;
        for (dx, dy) in KNIGHT_OFFSETS {
            let (nx, ny) = (x + dx, y + dy);
            if let Some(np) = spiral.coord_to_index(nx, ny) {
                if !visited.contains(&np) && best.map_or(true, |(b, _, _)| np < b) {
                    best = Some((np, nx, ny));
                }
            }
        }

        match best {
            Some((np, nx, ny)) => {
                x = nx;
                y = ny;
                visited.insert(np);
                path.push((np, nx, ny));
            }
            None => break, // trapped
        }
    }

    SimResult { path, start }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Regression guard against OEIS A316667 / the Numberphile video: with the
    /// center numbered 1, the knight is trapped at square 2084.
    #[test]
    fn trapped_at_2084_with_start_1() {
        let r = simulate_trapped_knight(1);
        assert_eq!(r.trap_square(), 2084);
    }

    /// With the center numbered 0, the same shape shifts every label down by one.
    #[test]
    fn trapped_at_2083_with_start_0() {
        let r = simulate_trapped_knight(0);
        assert_eq!(r.trap_square(), 2083);
    }

    /// The starting square reports as `start` regardless of offset.
    #[test]
    fn start_square_is_offset() {
        assert_eq!(simulate_trapped_knight(1).start_square(), 1);
        assert_eq!(simulate_trapped_knight(0).start_square(), 0);
    }
}
