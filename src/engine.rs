//! The general placement engine.
//!
//! A game is a list of pieces (in turn order) plus the [`KindTable`] they write into.
//! Each piece scans its own [`SpiralWalker`] and, on its turn, takes the lowest cell
//! that is unoccupied and not attacked by any *opposing-color* piece. Pieces are
//! permanent and never move; play round-robins until no piece can place.
//!
//! **Attack rule (the load-bearing detail).** A piece at `P` attacks square `S` iff
//! `(S − P)` is in that piece's attack-offset list. So a candidate `S` is illegal for
//! color `C` iff some already-placed piece of a different color sits at `S − o` for one
//! of *its* offsets `o`. We therefore look *backward* (`grid[S − o]`) over each placed
//! kind's offsets — not forward from `S`. For offset sets closed under negation (like
//! the knight) backward and forward coincide, so the canonical red/black game is
//! reproduced exactly; for asymmetric pieces only the backward reading is correct.
//!
//! **Cursor never rewinds.** A square leaves a piece's candidate set only by becoming
//! occupied or attacked by an opponent — both permanent — so each walker's position
//! only advances, and the whole simulation is O(squares scanned), exactly as the
//! original two-color engine.

use crate::piece::{KindTable, Rgb, EMPTY};
use crate::spiral::{Direction, Handedness, SpiralWalker};

/// Smallest simulation buffer (extra rings beyond the rendered radius). Kept at 3 so a
/// knight game (Chebyshev reach 2) simulates the identical extent as the original
/// engine; wider pieces raise it to their own reach.
const MIN_BUFFER: i32 = 3;

/// One configured piece: the cell byte it writes plus its spiral assignment.
pub struct PieceSpec {
    pub kind: u8,
    pub direction: Direction,
    pub handed: Handedness,
}

/// A fully-resolved game ready to simulate.
pub struct EngineConfig {
    /// Pieces in turn order (the first listed seeds the center).
    pub pieces: Vec<PieceSpec>,
    /// Cell-byte → kind lookup (offsets, color/team, label, palette).
    pub kinds: KindTable,
}

/// A render-facing view of a finished board: a colored cell grid plus a legend. Both
/// [`PlacementResult`] and `redblack::RedBlackResult` implement it so the SVG/PNG
/// renderers work over either.
pub trait Board {
    fn radius(&self) -> i32;
    fn cell(&self, x: i32, y: i32) -> u8;
    fn palette(&self) -> Vec<Rgb>;
    fn legend(&self) -> Vec<LegendRow>;
}

/// One legend entry: the palette slot (cell byte), its label, and its count.
pub struct LegendRow {
    pub slot: u8,
    pub label: String,
    pub count: u64,
}

/// The occupancy of a simulated window, plus per-kind tallies.
pub struct PlacementResult {
    /// Chebyshev radius of the rendered square region.
    pub radius: i32,
    /// Spiral squares in the rendered window: `(2*radius + 1)^2`.
    pub squares_considered: u64,
    /// Grid half-width: cells cover `[-half, half]` on both axes.
    half: i32,
    /// Occupant cell bytes, row-major over the `(2*half + 1)`-square.
    cells: Vec<u8>,
    /// Cell-byte → kind lookup.
    kinds: KindTable,
    /// Count per cell byte within the window (index 0 = empty squares).
    counts: Vec<u64>,
    /// Distinct kinds that played, in turn order — the legend's row order.
    turn_kinds: Vec<u8>,
}

impl PlacementResult {
    /// Occupant byte at lattice coordinate `(x, y)` (valid for `|x|, |y| <= half`).
    pub fn cell(&self, x: i32, y: i32) -> u8 {
        let w = (2 * self.half + 1) as usize;
        self.cells[(y + self.half) as usize * w + (x + self.half) as usize]
    }

    /// Knights of the given cell byte within the window.
    pub fn count(&self, kind: u8) -> u64 {
        self.counts[kind as usize]
    }

    /// Total pieces placed within the window.
    pub fn placed(&self) -> u64 {
        self.turn_kinds.iter().map(|&k| self.counts[k as usize]).sum()
    }

    /// Palette indexed by cell byte.
    pub fn palette(&self) -> Vec<Rgb> {
        self.kinds.palette()
    }

    /// Distinct kinds that played, in turn order.
    pub fn turn_kinds(&self) -> &[u8] {
        &self.turn_kinds
    }
}

impl Board for PlacementResult {
    fn radius(&self) -> i32 {
        self.radius
    }
    fn cell(&self, x: i32, y: i32) -> u8 {
        PlacementResult::cell(self, x, y) // inherent method; not the trait method
    }
    fn palette(&self) -> Vec<Rgb> {
        self.kinds.palette()
    }
    fn legend(&self) -> Vec<LegendRow> {
        self.turn_kinds
            .iter()
            .map(|&k| LegendRow {
                slot: k,
                label: self.kinds.label(k).to_string(),
                count: self.counts[k as usize],
            })
            .collect()
    }
}

/// `(2r + 1)^2`, the number of spiral squares within Chebyshev radius `r`.
fn squares_in_radius(r: i32) -> u64 {
    let d = (2 * r + 1) as u64;
    d * d
}

/// Simulate `config` and return the occupancy of a window of Chebyshev radius `radius`.
/// A few rings past the window (`MIN_BUFFER`, or the widest piece's reach) are simulated
/// so boundary cells see every opponent that could block them.
pub fn simulate(radius: i32, config: EngineConfig) -> PlacementResult {
    let radius = radius.max(0);
    let reach = config.kinds.max_chebyshev().max(1);
    let sim_radius = radius + reach.max(MIN_BUFFER);
    let (cells, half) = run_with(sim_radius, &config, |_, _| {});

    // Tally cell bytes within the rendered window (buffer rings excluded).
    let w = (2 * half + 1) as usize;
    let at = |x: i32, y: i32| cells[(y + half) as usize * w + (x + half) as usize];
    let mut counts = vec![0u64; config.kinds.len()];
    for y in -radius..=radius {
        for x in -radius..=radius {
            counts[at(x, y) as usize] += 1;
        }
    }

    // Distinct kinds, in the order their pieces first take a turn.
    let mut turn_kinds: Vec<u8> = Vec::new();
    for p in &config.pieces {
        if !turn_kinds.contains(&p.kind) {
            turn_kinds.push(p.kind);
        }
    }

    PlacementResult {
        radius,
        squares_considered: squares_in_radius(radius),
        half,
        cells,
        counts,
        turn_kinds,
        kinds: config.kinds,
    }
}

/// Run the round-robin game over the square region of radius `sim_radius`, returning
/// the occupancy grid and its half-width. `visit(position, kind)` fires per placement
/// in turn order (a no-op in production; tests use it to recover placement sequences).
/// The grid is padded by the widest piece's reach + 1 so every backward attack read is
/// in bounds (and out-of-region cells read as EMPTY).
pub fn run_with<F: FnMut(u64, u8)>(
    sim_radius: i32,
    config: &EngineConfig,
    mut visit: F,
) -> (Vec<u8>, i32) {
    let max_pos = squares_in_radius(sim_radius);
    let half = sim_radius + config.kinds.max_chebyshev().max(1) + 1;
    let w = (2 * half + 1) as usize;
    let mut grid = vec![EMPTY; w * w];
    let cell = |x: i32, y: i32| -> usize { (y + half) as usize * w + (x + half) as usize };

    struct Cursor {
        kind: u8,
        color_id: u16,
        walker: SpiralWalker,
        done: bool,
    }
    let mut cursors: Vec<Cursor> = config
        .pieces
        .iter()
        .map(|p| Cursor {
            kind: p.kind,
            color_id: config.kinds.color_id(p.kind),
            walker: SpiralWalker::oriented(p.direction, p.handed),
            done: false,
        })
        .collect();

    // Round-robin in turn order until a full round places nothing.
    loop {
        let mut placed_any = false;
        for c in &mut cursors {
            if c.done {
                continue;
            }
            match next_spot(&mut c.walker, &grid, max_pos, half, w, c.color_id, &config.kinds) {
                Some((p, x, y)) => {
                    grid[cell(x, y)] = c.kind;
                    visit(p, c.kind);
                    placed_any = true;
                }
                None => c.done = true,
            }
        }
        if !placed_any {
            break;
        }
    }
    (grid, half)
}

/// Advance `walker` to the lowest square that is unoccupied and not attacked by any
/// piece of a color other than `own_color`. "Piece at `P` attacks `S`" means
/// `(S − P) ∈ offsets(P)`, so we read backward: for each placed kind and each of its
/// offsets `o`, a piece sitting at `S − o` attacks `S`.
fn next_spot(
    walker: &mut SpiralWalker,
    grid: &[u8],
    max_pos: u64,
    half: i32,
    w: usize,
    own_color: u16,
    kinds: &KindTable,
) -> Option<(u64, i32, i32)> {
    let cell = |x: i32, y: i32| -> usize { (y + half) as usize * w + (x + half) as usize };
    while walker.position() < max_pos {
        let (p, x, y) = walker.step();
        if grid[cell(x, y)] != EMPTY {
            continue;
        }
        let attacked = kinds.placed().any(|(k, color_id, offsets)| {
            color_id != own_color && offsets.iter().any(|&(ox, oy)| grid[cell(x - ox, y - oy)] == k)
        });
        if !attacked {
            return Some((p, x, y));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::KindBuilder;

    const KNIGHT: [(i32, i32); 8] = [
        (1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2),
    ];

    fn two_color() -> EngineConfig {
        let mut b = KindBuilder::new();
        let black = b.intern(KNIGHT.to_vec(), (26, 26, 26), "Black").unwrap();
        let red = b.intern(KNIGHT.to_vec(), (209, 31, 31), "Red").unwrap();
        EngineConfig {
            pieces: vec![
                PieceSpec { kind: black, direction: Direction::Right, handed: Handedness::Ccw },
                PieceSpec { kind: red, direction: Direction::Right, handed: Handedness::Ccw },
            ],
            kinds: b.finish(),
        }
    }

    #[test]
    fn black_seeds_center_red_takes_one() {
        let cfg = two_color();
        let (black, red) = (cfg.pieces[0].kind, cfg.pieces[1].kind);
        let r = simulate(5, cfg);
        assert_eq!(r.cell(0, 0), black, "Black seeds the center");
        assert_eq!(r.cell(1, 0), red, "Red takes square 1");
        assert!(r.count(black) > 0 && r.count(red) > 0);
    }

    /// Pins the attack rule: a piece at `P` attacks `P + offset`, and the check reads
    /// *backward* over the attacker's own offsets. With an asymmetric single-offset
    /// piece `[(1,2)]`, by the round in which color Z scans (2,1) color A already sits
    /// at (1,-1), and (1,-1) + (1,2) = (2,1) — so A attacks (2,1): Z must skip it and A
    /// then takes it. The negated offset (-1,-2) is *not* in A's list, so a forward or
    /// symmetric reading (which would let Z keep (2,1)) is wrong. This trace is exact
    /// because no earlier candidate is ever attacked.
    #[test]
    fn asymmetric_attack_uses_attacker_offsets() {
        let mut b = KindBuilder::new();
        let a = b.intern(vec![(1, 2)], (0, 0, 0), "A").unwrap(); // kind 1
        let z = b.intern(vec![(1, 2)], (255, 0, 0), "Z").unwrap(); // kind 2
        let cfg = EngineConfig {
            pieces: vec![
                PieceSpec { kind: a, direction: Direction::Right, handed: Handedness::Ccw },
                PieceSpec { kind: z, direction: Direction::Right, handed: Handedness::Ccw },
            ],
            kinds: b.finish(),
        };
        let r = simulate(4, cfg);
        assert_eq!(r.cell(2, 1), a, "A attacks (2,1) from (1,-1); Z skips, A takes it");
        assert_eq!(r.cell(2, 2), z, "Z advanced past the attacked square to (2,2)");
    }
}
