//! Red & Black Knights (Problem 3) and its variants, as presets over the general
//! [`engine`](crate::engine).
//!
//! The Numberphile game: two teams alternate (Black first); on its turn a team places
//! a knight on the lowest unoccupied square not attacked by the *other* color. Knights
//! are permanent. (OEIS A392177 = Black squares, A392178 = Red.) This is just one
//! configuration of the general placement engine — one knight piece type, two colors,
//! both on the standard spiral — so this module only builds [`EngineConfig`]s and
//! forwards to [`engine::simulate`]; the placement loop, the never-rewinding cursors,
//! and the occupancy grid all live in [`engine`](crate::engine).
//!
//! ## Spiral variants
//!
//! Both colors normally scan the *same* spiral, so Black — who moves first — also gets
//! first pick of the lowest squares; that bias is baked into the canonical sequence.
//! The experimental variants give Red its own spiral orientation so the two colors
//! sweep the board apart: [`Variant::Rot180`] rotates Red's spiral 180° (start left,
//! still counterclockwise) and [`Variant::Mirror`] reflects it across the y-axis (start
//! left, clockwise — arms run left, up, right, down). Neither can undo the very first
//! move (Black still seeds the center), but each makes the board nearly symmetric under
//! "apply Red's transform and swap colors": [`RedBlackResult::rot_swap_symmetry`] /
//! [`mirror_swap_symmetry`](RedBlackResult::mirror_swap_symmetry) measure the residual
//! gap. These are experiments, not the Numberphile problem: only [`Variant::Canonical`]
//! reproduces A392177/A392178.
//!
//! [`Variant::Quad`] is a separate axis — four colors (Black, Red, Green, Yellow), all
//! on the standard spiral, taking turns in that order; each avoids squares attacked by
//! *any other* color. All four games flow through the one engine.

use crate::engine::{self, Board, EngineConfig, LegendRow, PieceSpec};
use crate::knight::KNIGHT_OFFSETS;
use crate::piece::KindBuilder;
use crate::spiral::{Direction, Handedness};

/// Occupant codes, which double as palette indices and (because teams are interned in
/// this order) the engine's cell bytes for these presets. The two-color game uses only
/// BLACK/RED; [`Variant::Quad`] adds GREEN/YELLOW. (Empty cells are [`piece::EMPTY`].)
pub const BLACK: u8 = 1;
pub const RED: u8 = 2;
pub const GREEN: u8 = 3;
pub const YELLOW: u8 = 4;

/// RGB for every occupant code, indexed by the code itself.
pub fn palette() -> Vec<(u8, u8, u8)> {
    vec![
        (255, 255, 255), // EMPTY  -> white
        (26, 26, 26),    // BLACK  -> near-black
        (209, 31, 31),   // RED
        (38, 160, 65),   // GREEN
        (242, 201, 33),  // YELLOW
    ]
}

/// Human-facing name of a team color, for legends and the CLI summary.
pub fn color_name(code: u8) -> &'static str {
    match code {
        BLACK => "Black",
        RED => "Red",
        GREEN => "Green",
        YELLOW => "Yellow",
        _ => "Empty",
    }
}

/// Which teams play and how each one's spiral is oriented. See the module doc.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Variant {
    /// Black and Red, both on the standard spiral. The canonical game (A392177/A392178).
    Canonical,
    /// Black and Red, with Red's spiral rotated 180° (start left, ccw). Non-canonical.
    Rot180,
    /// Black and Red, with Red's spiral mirrored across the y-axis (start left, cw).
    /// Non-canonical.
    Mirror,
    /// Four teams — Black, Red, Green, Yellow — all on the standard spiral, in turn
    /// order. Each avoids squares attacked by *any other* color. Non-canonical.
    Quad,
}

impl Variant {
    /// Short flag-facing name (`"canonical"` / `"rot180"` / `"mirror"` / `"quad"`).
    pub fn name(self) -> &'static str {
        match self {
            Variant::Canonical => "canonical",
            Variant::Rot180 => "rot180",
            Variant::Mirror => "mirror",
            Variant::Quad => "quad",
        }
    }

    /// Build the engine configuration for this preset: one knight piece type, the two
    /// or four colors, each with its spiral assignment, in turn order (Black leads).
    /// rot180 = Red on (left, ccw); mirror = Red on (left, cw) — the orientations that
    /// reproduce the old coordinate transforms `(-x,-y)` and `(-x,y)`.
    pub fn engine_config(self) -> EngineConfig {
        use Direction::{Left, Right};
        use Handedness::{Ccw, Cw};
        let specs: &[(u8, Direction, Handedness)] = match self {
            Variant::Canonical => &[(BLACK, Right, Ccw), (RED, Right, Ccw)],
            Variant::Rot180 => &[(BLACK, Right, Ccw), (RED, Left, Ccw)],
            Variant::Mirror => &[(BLACK, Right, Ccw), (RED, Left, Cw)],
            Variant::Quad => &[
                (BLACK, Right, Ccw),
                (RED, Right, Ccw),
                (GREEN, Right, Ccw),
                (YELLOW, Right, Ccw),
            ],
        };
        build_knight_config(specs)
    }
}

/// Build an [`EngineConfig`] of knight pieces from `(color code, direction, handedness)`
/// specs. Interning in this order makes each kind byte equal its color code, so the
/// engine's cells, palette, and placement sequence match the legacy two/four-color
/// engine exactly.
fn build_knight_config(specs: &[(u8, Direction, Handedness)]) -> EngineConfig {
    let pal = palette();
    let mut kinds = KindBuilder::new();
    let pieces = specs
        .iter()
        .map(|&(code, direction, handed)| {
            let kind = kinds
                .intern(KNIGHT_OFFSETS.to_vec(), pal[code as usize], color_name(code))
                .expect("redblack presets have at most four kinds");
            debug_assert_eq!(kind, code, "kind byte must equal the legacy color code");
            PieceSpec { kind, direction, handed }
        })
        .collect();
    EngineConfig { pieces, kinds: kinds.finish() }
}

/// A finished two/four-color board. Thin wrapper over the engine's result that keeps
/// the historical color-code-flavored API plus the symmetry metrics.
pub struct RedBlackResult {
    /// Chebyshev radius of the rendered square region.
    pub radius: i32,
    /// Number of spiral squares in the rendered window: `(2*radius + 1)^2`.
    pub squares_considered: u64,
    inner: engine::PlacementResult,
}

impl RedBlackResult {
    /// Occupant code at lattice coordinate `(x, y)`.
    pub fn cell(&self, x: i32, y: i32) -> u8 {
        self.inner.cell(x, y)
    }

    /// The team codes that played, in turn order (e.g. `[BLACK, RED]`).
    pub fn teams(&self) -> &[u8] {
        self.inner.turn_kinds()
    }

    /// How many knights of the given color code lie within the window.
    pub fn count(&self, code: u8) -> u64 {
        self.inner.count(code)
    }

    /// Total knights placed within the window (all teams).
    pub fn placed(&self) -> u64 {
        self.inner.placed()
    }

    /// Fraction of window cells that equal their 180°-rotated, color-swapped
    /// counterpart: `cell(x, y) == swap(cell(-x, -y))`. 1.0 is perfect symmetry;
    /// [`Variant::Rot180`] maximizes this and the shortfall is the first-mover
    /// asymmetry (Black seeding the center) that no spiral can erase.
    pub fn rot_swap_symmetry(&self) -> f64 {
        self.swap_symmetry(|x, y| (-x, -y))
    }

    /// Fraction of window cells that equal their y-axis-mirrored, color-swapped
    /// counterpart: `cell(x, y) == swap(cell(-x, y))`. Companion to
    /// [`rot_swap_symmetry`](Self::rot_swap_symmetry), maximized by [`Variant::Mirror`].
    pub fn mirror_swap_symmetry(&self) -> f64 {
        self.swap_symmetry(|x, y| (-x, y))
    }

    /// Fraction of window cells equal to the color-swap of their image under `map`.
    fn swap_symmetry(&self, map: impl Fn(i32, i32) -> (i32, i32)) -> f64 {
        let mut matching = 0u64;
        for y in -self.radius..=self.radius {
            for x in -self.radius..=self.radius {
                let (mx, my) = map(x, y);
                if self.cell(x, y) == swap_color(self.cell(mx, my)) {
                    matching += 1;
                }
            }
        }
        matching as f64 / self.squares_considered as f64
    }
}

impl Board for RedBlackResult {
    fn radius(&self) -> i32 {
        self.radius
    }
    fn cell(&self, x: i32, y: i32) -> u8 {
        self.inner.cell(x, y)
    }
    fn palette(&self) -> Vec<(u8, u8, u8)> {
        self.inner.palette()
    }
    fn legend(&self) -> Vec<LegendRow> {
        self.inner.legend()
    }
}

/// Swap the two team colors, leaving everything else untouched. (Used by the two-color
/// symmetry metrics, where only BLACK/RED occur.)
fn swap_color(code: u8) -> u8 {
    match code {
        BLACK => RED,
        RED => BLACK,
        other => other,
    }
}

/// Simulate the game and produce a square window of Chebyshev radius `radius`.
pub fn simulate_redblack(radius: i32, variant: Variant) -> RedBlackResult {
    let inner = engine::simulate(radius, variant.engine_config());
    RedBlackResult {
        radius: inner.radius,
        squares_considered: inner.squares_considered,
        inner,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::piece::EMPTY;

    /// Split a run over radius `sim_radius` into (Black positions, Red positions), in
    /// placement order, by driving the general engine with the variant's preset.
    fn seqs(sim_radius: i32, variant: Variant) -> (Vec<u64>, Vec<u64>) {
        let mut black = Vec::new();
        let mut red = Vec::new();
        engine::run_with(sim_radius, &variant.engine_config(), |p, kind| match kind {
            BLACK => black.push(p),
            RED => red.push(p),
            _ => {}
        });
        (black, red)
    }

    /// The first 30 Black and Red squares match OEIS A392177 / A392178 — now routed
    /// through the general engine via the canonical preset.
    #[test]
    fn matches_oeis_a392177_a392178() {
        const A_BLACK: [u64; 30] = [
            0, 2, 5, 9, 11, 15, 20, 21, 30, 31, 36, 40, 42, 47, 48, 50, 56, 61, 65, 67, 69, 70,
            71, 75, 76, 81, 83, 85, 87, 89,
        ];
        const A_RED: [u64; 30] = [
            1, 3, 4, 6, 10, 12, 24, 25, 34, 35, 37, 41, 44, 49, 55, 57, 58, 63, 64, 66, 68, 72,
            78, 82, 84, 86, 88, 90, 95, 96,
        ];
        let (black, red) = seqs(12, Variant::Canonical);
        assert_eq!(&black[..A_BLACK.len()], &A_BLACK);
        assert_eq!(&red[..A_RED.len()], &A_RED);
    }

    /// No knight is a knight's move from a *different* color (mutual exclusion);
    /// same-color knights a knight's move apart are allowed. Must hold for every
    /// variant — including the four-color game — across the whole window.
    #[test]
    fn different_colors_never_attack() {
        for variant in [Variant::Canonical, Variant::Rot180, Variant::Mirror, Variant::Quad] {
            let r = simulate_redblack(20, variant);
            for y in -r.radius..=r.radius {
                for x in -r.radius..=r.radius {
                    let c = r.cell(x, y);
                    if c == EMPTY {
                        continue;
                    }
                    for (dx, dy) in KNIGHT_OFFSETS {
                        let other = r.cell(x + dx, y + dy);
                        if other != EMPTY {
                            assert_eq!(c, other, "{variant:?}: different colors attack at ({x},{y})");
                        }
                    }
                }
            }
        }
    }

    /// Black starts at the center; Red takes square 1.
    #[test]
    fn black_starts_at_center() {
        let (black, red) = seqs(5, Variant::Canonical);
        assert_eq!(black[0], 0);
        assert_eq!(red[0], 1);
    }

    /// Both non-canonical variants still seed Black at the center while Red's first pick
    /// becomes the mirrored center (-1,0); each changes the board and lifts the symmetry
    /// under *its own* transform above canonical's.
    #[test]
    fn variants_mirror_red_seed_and_lift_symmetry() {
        let canon = simulate_redblack(40, Variant::Canonical);

        let rot = simulate_redblack(40, Variant::Rot180);
        assert_eq!(rot.cell(0, 0), BLACK, "rot180: Black still seeds the center");
        assert_eq!(rot.cell(-1, 0), RED, "rot180: Red's first pick is the mirrored center");
        assert!(
            rot.rot_swap_symmetry() > canon.rot_swap_symmetry(),
            "rot180 ({:.3}) should beat canonical ({:.3}) on rotate-and-swap symmetry",
            rot.rot_swap_symmetry(),
            canon.rot_swap_symmetry(),
        );

        let mir = simulate_redblack(40, Variant::Mirror);
        assert_eq!(mir.cell(0, 0), BLACK, "mirror: Black still seeds the center");
        assert_eq!(mir.cell(-1, 0), RED, "mirror: Red's first pick is the mirrored center");
        assert!(
            mir.mirror_swap_symmetry() > canon.mirror_swap_symmetry(),
            "mirror ({:.3}) should beat canonical ({:.3}) on mirror-and-swap symmetry",
            mir.mirror_swap_symmetry(),
            canon.mirror_swap_symmetry(),
        );

        let differ = (-40..=40).any(|y| (-40..=40).any(|x| rot.cell(x, y) != mir.cell(x, y)));
        assert!(differ, "rot180 and mirror should not coincide");
    }

    /// Quad seats four teams in turn order on the standard spiral. The first round fills
    /// the innermost cells: Black at the center, then Red, Green, Yellow take the next
    /// three squares of the spiral (1,0), (1,1), (0,1). All four end up on the board.
    #[test]
    fn quad_seats_four_colors() {
        let r = simulate_redblack(30, Variant::Quad);

        assert_eq!(r.cell(0, 0), BLACK);
        assert_eq!(r.cell(1, 0), RED);
        assert_eq!(r.cell(1, 1), GREEN);
        assert_eq!(r.cell(0, 1), YELLOW);

        assert_eq!(r.teams(), [BLACK, RED, GREEN, YELLOW]);
        for code in [BLACK, RED, GREEN, YELLOW] {
            assert!(r.count(code) > 0, "{} placed nothing", color_name(code));
        }
        let empty = r.squares_considered - r.placed();
        assert_eq!(r.count(EMPTY), empty);
    }
}
