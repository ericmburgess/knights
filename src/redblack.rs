//! Red & Black Knights (Problem 3).
//!
//! Two players alternate, Black first. On Black's turn he places a knight on the
//! lowest-numbered unoccupied square not attacked by any Red knight; Red does the
//! mirror. Knights are permanent. (OEIS A392177 = Black squares, A392178 = Red.)
//!
//! Naively each turn rescans the spiral from 0, which is O(n^2). But a square only
//! ever leaves a color's candidate set — it gets occupied, or attacked by the
//! opponent, both permanent — so the *lowest* candidate for each color only moves
//! forward. We keep a per-color cursor that never rewinds, making the whole
//! simulation O(squares scanned).
//!
//! The decision for a square is final the instant its color's cursor reaches it,
//! and depends only on knights already placed. So simulating from square 0 up to
//! any bound reproduces the infinite game exactly below that bound. To render a
//! clean radius-`R` window we simulate a few rings past it (BUFFER) so the
//! boundary knights see every opponent that could block them.

use crate::knight::KNIGHT_OFFSETS;
use crate::spiral::Spiral;
use std::collections::HashMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Color {
    Black,
    Red,
}

/// Extra rings simulated beyond the rendered radius so boundary cells are exact.
const BUFFER: i32 = 3;

pub struct RedBlackResult {
    /// Chebyshev radius of the rendered square region.
    pub radius: i32,
    /// Number of spiral squares in the rendered window: `(2*radius + 1)^2`.
    pub squares_considered: u64,
    /// Placed knights within the window as `(position, x, y, color)`, in placement order.
    pub knights: Vec<(u64, i32, i32, Color)>,
    pub black: usize,
    pub red: usize,
}

/// Simulate the game and render a square window of Chebyshev radius `radius`.
/// Placement is determined purely by spiral geometry, so there is no numbering
/// offset to pass — unlike the other two problems, nothing here reports a
/// human-facing square number.
pub fn simulate_redblack(radius: i32) -> RedBlackResult {
    let radius = radius.max(0);
    let window = squares_in_radius(radius);
    let sim_max = squares_in_radius(radius + BUFFER);

    let all = run(sim_max);
    let knights: Vec<_> = all.into_iter().filter(|&(p, ..)| p < window).collect();
    let black = knights.iter().filter(|&&(_, _, _, c)| c == Color::Black).count();
    let red = knights.len() - black;

    RedBlackResult {
        radius,
        squares_considered: window,
        knights,
        black,
        red,
    }
}

/// `(2r + 1)^2`, the number of spiral squares within Chebyshev radius `r`.
fn squares_in_radius(r: i32) -> u64 {
    let d = (2 * r + 1) as u64;
    d * d
}

/// Run the alternating game over squares `[0, max_pos)`, returning every
/// placement in turn order.
fn run(max_pos: u64) -> Vec<(u64, i32, i32, Color)> {
    let mut spiral = Spiral::new();
    ensure_positions(&mut spiral, max_pos);

    let mut occ: HashMap<(i32, i32), Color> = HashMap::new();
    let mut out: Vec<(u64, i32, i32, Color)> = Vec::new();
    let mut cursor_black: u64 = 0;
    let mut cursor_red: u64 = 0;
    let mut black_done = false;
    let mut red_done = false;

    while !(black_done && red_done) {
        // Black moves, then Red — alternating, Black first.
        if !black_done {
            match next_spot(&spiral, &occ, &mut cursor_black, max_pos, Color::Red) {
                Some((p, x, y)) => {
                    occ.insert((x, y), Color::Black);
                    out.push((p, x, y, Color::Black));
                }
                None => black_done = true,
            }
        }
        if !red_done {
            match next_spot(&spiral, &occ, &mut cursor_red, max_pos, Color::Black) {
                Some((p, x, y)) => {
                    occ.insert((x, y), Color::Red);
                    out.push((p, x, y, Color::Red));
                }
                None => red_done = true,
            }
        }
    }
    out
}

/// Advance `cursor` to the lowest square that is unoccupied and not attacked by
/// `opponent`, returning it (and leaving the cursor just past it). Squares it
/// passes are permanently unavailable, so the cursor never needs to rewind.
fn next_spot(
    spiral: &Spiral,
    occ: &HashMap<(i32, i32), Color>,
    cursor: &mut u64,
    max_pos: u64,
    opponent: Color,
) -> Option<(u64, i32, i32)> {
    while *cursor < max_pos {
        let p = *cursor;
        *cursor = p + 1;
        let (x, y) = spiral.index_to_coord(p);
        if occ.contains_key(&(x, y)) {
            continue;
        }
        let attacked = KNIGHT_OFFSETS
            .iter()
            .any(|&(dx, dy)| occ.get(&(x + dx, y + dy)) == Some(&opponent));
        if !attacked {
            return Some((p, x, y));
        }
    }
    None
}

/// Extend the spiral so positions `0..n` are all mapped.
fn ensure_positions(spiral: &mut Spiral, n: u64) {
    let mut r = 0;
    while squares_in_radius(r) < n {
        r += 1;
    }
    spiral.ensure_radius(r);
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Split a run into (Black positions, Red positions) in placement order.
    fn seqs(max_pos: u64) -> (Vec<u64>, Vec<u64>) {
        let mut black = Vec::new();
        let mut red = Vec::new();
        for (p, _, _, c) in run(max_pos) {
            match c {
                Color::Black => black.push(p),
                Color::Red => red.push(p),
            }
        }
        (black, red)
    }

    /// The first 30 Black and Red squares match OEIS A392177 / A392178.
    #[test]
    fn matches_oeis_a392177_a392178() {
        const BLACK: [u64; 30] = [
            0, 2, 5, 9, 11, 15, 20, 21, 30, 31, 36, 40, 42, 47, 48, 50, 56, 61, 65, 67, 69, 70,
            71, 75, 76, 81, 83, 85, 87, 89,
        ];
        const RED: [u64; 30] = [
            1, 3, 4, 6, 10, 12, 24, 25, 34, 35, 37, 41, 44, 49, 55, 57, 58, 63, 64, 66, 68, 72,
            78, 82, 84, 86, 88, 90, 95, 96,
        ];
        let (black, red) = seqs(2000);
        assert_eq!(&black[..BLACK.len()], &BLACK);
        assert_eq!(&red[..RED.len()], &RED);
    }

    /// No Black knight is a knight's move from a Red knight (mutual exclusion),
    /// which must hold across the whole rendered window including its boundary.
    #[test]
    fn opposite_colors_never_attack() {
        let r = simulate_redblack(20);
        let color_at: HashMap<(i32, i32), Color> =
            r.knights.iter().map(|&(_, x, y, c)| ((x, y), c)).collect();
        // A knight-neighbor may exist, but only of the same color — opposite
        // colors are never a knight's move apart. (Same color is allowed.)
        for (&(x, y), &c) in &color_at {
            for (dx, dy) in KNIGHT_OFFSETS {
                if let Some(&other) = color_at.get(&(x + dx, y + dy)) {
                    assert_eq!(c, other, "opposite colors attack at ({x},{y})");
                }
            }
        }
    }

    /// Black starts at the center; Red takes square 1.
    #[test]
    fn black_starts_at_center() {
        let (black, red) = seqs(50);
        assert_eq!(black[0], 0);
        assert_eq!(red[0], 1);
    }
}
