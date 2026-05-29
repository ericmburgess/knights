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
//! The board itself is the only large structure: a single dense `Vec<u8>` of
//! occupant codes, indexed by coordinate. It serves three roles at once — the
//! occupancy the rule queries, the result the renderer reads, and (in raster
//! layout already) the eventual pixels. There is no list of placements and no
//! stored coordinate table: positions are walked on the fly with a [`SpiralWalker`],
//! and each knight is written straight into its cell. So memory is ~1 byte per
//! square and nothing else scales with the board — billions of cells fit in a few
//! GB of RAM, and the PNG dump needs no reordering.
//!
//! A cell is one byte holding a small *code* (an index into [`palette`]); using a
//! byte rather than packed bits keeps the hot neighbor checks fast and leaves room
//! for more piece types later. The decision for a square is final the instant its
//! cursor reaches it, so simulating from 0 up to any bound reproduces the infinite
//! game exactly below that bound; to render a clean radius-`R` window we simulate a
//! few rings past it (BUFFER) so boundary cells see every opponent that could block.
//!
//! ## Spiral variants
//!
//! Both colors normally scan the *same* spiral, so Black — who moves first — also
//! gets first pick of the lowest squares; that bias is baked into the canonical
//! sequence. The experimental variants give Red its own orientation so the two
//! colors sweep the board apart: [`Variant::Rot180`] rotates Red's spiral 180°
//! (`(x,y) -> (-x,-y)`) and [`Variant::Mirror`] reflects it across the y-axis
//! (`(x,y) -> (-x, y)`). The same O(1) [`SpiralWalker`] drives every variant — only
//! Red's emitted coordinate is transformed (see [`Orient`]) — so the cursor still
//! never rewinds. Neither can undo the very first move (Black still seeds the
//! center), but each makes the board nearly symmetric under "apply Red's transform
//! and swap colors": [`RedBlackResult::rot_swap_symmetry`] /
//! [`mirror_swap_symmetry`](RedBlackResult::mirror_swap_symmetry) measure the residual
//! gap, which is exactly the first-mover asymmetry the spiral can't remove. These are
//! experiments, not the Numberphile problem: only [`Variant::Canonical`] reproduces
//! A392177/A392178.
//!
//! A separate axis is the *number* of teams. [`Variant::Quad`] keeps every spiral in
//! the standard orientation but plays four colors — Black, Red, Green, Yellow — in
//! round-robin, each avoiding squares attacked by *any other* color (same color still
//! cooperates). The engine is written team-generic: any number of cursors take turns,
//! so the two-color and four-color games share one loop. The cursor-never-rewinds
//! argument is unchanged — a square still leaves a team's candidate set only by being
//! occupied or attacked by another color, both permanent.

use crate::knight::KNIGHT_OFFSETS;
use crate::spiral::SpiralWalker;

/// Occupant codes; also the palette indices used when rendering. Extend with more
/// piece types as needed (up to 256 — the cell is a full byte). The two-color game
/// uses only BLACK/RED; [`Variant::Quad`] adds GREEN/YELLOW.
pub const EMPTY: u8 = 0;
pub const BLACK: u8 = 1;
pub const RED: u8 = 2;
pub const GREEN: u8 = 3;
pub const YELLOW: u8 = 4;

/// RGB for every occupant code, indexed by the code itself. A result exposes just
/// the prefix its teams use via [`RedBlackResult::palette`], so two-color renders
/// still index a 3-entry palette (and keep their compact PNG bit depth).
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
    /// Black and Red, both scanning the same spiral. The canonical game (A392177/A392178).
    Canonical,
    /// Black and Red, with Red's spiral rotated 180° (`(x,y) -> (-x,-y)`) to mitigate
    /// the first-mover bias. Non-canonical — no OEIS match.
    Rot180,
    /// Black and Red, with Red's spiral mirrored across the y-axis (`(x,y) -> (-x, y)`),
    /// so its arms run left, up, right, down. Non-canonical — no OEIS match.
    Mirror,
    /// Four teams — Black, Red, Green, Yellow — all on the same spiral, taking turns in
    /// that order. Each avoids squares attacked by *any other* color. Non-canonical.
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

    /// The teams that play, as `(occupant code, spiral orientation)` in turn order.
    /// The single source of truth for who plays and how their spiral is oriented;
    /// Black always leads on the standard spiral.
    fn teams(self) -> Vec<(u8, Orient)> {
        match self {
            Variant::Canonical => vec![(BLACK, Orient::Same), (RED, Orient::Same)],
            Variant::Rot180 => vec![(BLACK, Orient::Same), (RED, Orient::Rot180)],
            Variant::Mirror => vec![(BLACK, Orient::Same), (RED, Orient::MirrorX)],
            Variant::Quad => vec![
                (BLACK, Orient::Same),
                (RED, Orient::Same),
                (GREEN, Orient::Same),
                (YELLOW, Orient::Same),
            ],
        }
    }
}

/// A coordinate transform applied to a color's spiral walk. Black always uses
/// [`Orient::Same`]; Red follows the variant so the colors sweep the board apart.
#[derive(Clone, Copy)]
enum Orient {
    /// Identity — share the standard spiral.
    Same,
    /// 180° rotation: `(x, y) -> (-x, -y)`.
    Rot180,
    /// Reflection across the y-axis: `(x, y) -> (-x, y)`.
    MirrorX,
}

impl Orient {
    fn apply(self, x: i32, y: i32) -> (i32, i32) {
        match self {
            Orient::Same => (x, y),
            Orient::Rot180 => (-x, -y),
            Orient::MirrorX => (-x, y),
        }
    }
}

/// Extra rings simulated beyond the rendered radius so boundary cells are exact.
const BUFFER: i32 = 3;

pub struct RedBlackResult {
    /// Chebyshev radius of the rendered square region.
    pub radius: i32,
    /// Number of spiral squares in the rendered window: `(2*radius + 1)^2`.
    pub squares_considered: u64,
    /// Team occupant codes that played, in turn order (e.g. `[BLACK, RED]`).
    teams: Vec<u8>,
    /// Knight count per occupant code within the window, indexed by the code
    /// (so `counts[BLACK]`, …); `counts[EMPTY]` holds the empty-square count.
    counts: Vec<u64>,
    /// Grid half-width: cells cover `[-half, half]` on both axes.
    half: i32,
    /// Occupant codes, row-major over the `(2*half + 1)`-square.
    cells: Vec<u8>,
}

impl RedBlackResult {
    /// Occupant code at lattice coordinate `(x, y)`. Valid for `|x|, |y| <= half`,
    /// which covers the rendered window and its neighbors.
    pub fn cell(&self, x: i32, y: i32) -> u8 {
        let w = (2 * self.half + 1) as usize;
        self.cells[(y + self.half) as usize * w + (x + self.half) as usize]
    }

    /// The team codes that played, in turn order.
    pub fn teams(&self) -> &[u8] {
        &self.teams
    }

    /// How many knights of the given occupant code lie within the window.
    pub fn count(&self, code: u8) -> u64 {
        self.counts[code as usize]
    }

    /// Total knights placed within the window (all teams).
    pub fn placed(&self) -> u64 {
        self.teams.iter().map(|&c| self.counts[c as usize]).sum()
    }

    /// Palette covering EMPTY plus exactly the teams that played, indexable by code.
    /// Two-color variants get a 3-entry palette; [`Variant::Quad`] gets all five.
    pub fn palette(&self) -> Vec<(u8, u8, u8)> {
        let max = self.teams.iter().copied().max().unwrap_or(EMPTY) as usize;
        palette()[..=max].to_vec()
    }

    /// Fraction of window cells that equal their 180°-rotated, color-swapped
    /// counterpart: `cell(x, y) == swap(cell(-x, -y))`. 1.0 is perfect symmetry;
    /// [`Variant::Rot180`] maximizes this and the shortfall is the first-mover
    /// asymmetry (Black seeding the center) that no spiral can erase.
    pub fn rot_swap_symmetry(&self) -> f64 {
        self.swap_symmetry(|x, y| (-x, -y))
    }

    /// Fraction of window cells that equal their y-axis-mirrored, color-swapped
    /// counterpart: `cell(x, y) == swap(cell(-x, y))`. The companion metric to
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

/// Swap the two team colors, leaving EMPTY untouched.
fn swap_color(code: u8) -> u8 {
    match code {
        BLACK => RED,
        RED => BLACK,
        other => other,
    }
}

/// Simulate the game and produce a square window of Chebyshev radius `radius`.
/// Placement is determined purely by spiral geometry, so there is no numbering
/// offset — nothing here reports a human-facing square number. `variant` selects
/// the canonical game, a rotated/mirrored-Red experiment, or the four-color game.
pub fn simulate_redblack(radius: i32, variant: Variant) -> RedBlackResult {
    let radius = radius.max(0);
    let (cells, half) = run_with(radius + BUFFER, variant, |_, _| {});

    // Tally knights per occupant code within the rendered window (buffer rings excluded).
    let w = (2 * half + 1) as usize;
    let at = |x: i32, y: i32| cells[(y + half) as usize * w + (x + half) as usize];
    let mut counts = vec![0u64; palette().len()];
    for y in -radius..=radius {
        for x in -radius..=radius {
            counts[at(x, y) as usize] += 1;
        }
    }

    RedBlackResult {
        radius,
        squares_considered: squares_in_radius(radius),
        teams: variant.teams().into_iter().map(|(code, _)| code).collect(),
        counts,
        half,
        cells,
    }
}

/// `(2r + 1)^2`, the number of spiral squares within Chebyshev radius `r`.
fn squares_in_radius(r: i32) -> u64 {
    let d = (2 * r + 1) as u64;
    d * d
}

/// Run the placement game over the full square region of radius `sim_radius`,
/// returning the occupancy grid and its half-width. `variant` selects the teams and
/// each team's spiral orientation; they take turns in order (Black first), each
/// placing on its lowest legal square. `visit(position, code)` is called for each
/// placement in turn order (a no-op in production; tests use it to recover the
/// placement sequences). The grid is padded by 2 so the knight neighbors of any
/// boundary cell are always in bounds (and read as EMPTY).
fn run_with<F: FnMut(u64, u8)>(sim_radius: i32, variant: Variant, mut visit: F) -> (Vec<u8>, i32) {
    let max_pos = squares_in_radius(sim_radius);
    let half = sim_radius + 2;
    let w = (2 * half + 1) as usize;
    let mut grid = vec![EMPTY; w * w];
    let cell = |x: i32, y: i32| -> usize { (y + half) as usize * w + (x + half) as usize };

    // One independent, never-rewinding cursor per team.
    struct Team {
        code: u8,
        orient: Orient,
        walker: SpiralWalker,
        done: bool,
    }
    let mut teams: Vec<Team> = variant
        .teams()
        .into_iter()
        .map(|(code, orient)| Team { code, orient, walker: SpiralWalker::new(), done: false })
        .collect();

    // Round-robin until no team can place. A square seen by a team's cursor is gone
    // for good (occupied, or attacked by another color — both permanent), so cursors
    // never rewind and the whole game is O(squares scanned).
    loop {
        let mut placed_any = false;
        for t in &mut teams {
            if t.done {
                continue;
            }
            match next_spot(&mut t.walker, &grid, max_pos, half, w, t.code, t.orient) {
                Some((p, x, y)) => {
                    grid[cell(x, y)] = t.code;
                    visit(p, t.code);
                    placed_any = true;
                }
                None => t.done = true,
            }
        }
        if !placed_any {
            break;
        }
    }
    (grid, half)
}

/// Advance `walker` to the lowest square that is unoccupied and not attacked by any
/// color other than `own`, returning `(position, x, y)`. Squares it passes are
/// permanently unavailable, so the walker never needs to rewind. `orient` transforms
/// the walker's coordinates (e.g. rotated or mirrored), so the same standard stepper
/// drives every team's scan (`position` stays the walker's own).
fn next_spot(
    walker: &mut SpiralWalker,
    grid: &[u8],
    max_pos: u64,
    half: i32,
    w: usize,
    own: u8,
    orient: Orient,
) -> Option<(u64, i32, i32)> {
    let cell = |x: i32, y: i32| -> usize { (y + half) as usize * w + (x + half) as usize };
    while walker.position() < max_pos {
        let (p, sx, sy) = walker.step();
        let (x, y) = orient.apply(sx, sy);
        if grid[cell(x, y)] != EMPTY {
            continue;
        }
        let attacked = KNIGHT_OFFSETS.iter().any(|&(dx, dy)| {
            let neighbor = grid[cell(x + dx, y + dy)];
            neighbor != EMPTY && neighbor != own
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

    /// Split a run over radius `sim_radius` into (Black positions, Red positions),
    /// in placement order.
    fn seqs(sim_radius: i32, variant: Variant) -> (Vec<u64>, Vec<u64>) {
        let mut black = Vec::new();
        let mut red = Vec::new();
        run_with(sim_radius, variant, |p, code| match code {
            BLACK => black.push(p),
            RED => red.push(p),
            _ => {}
        });
        (black, red)
    }

    /// The first 30 Black and Red squares match OEIS A392177 / A392178.
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
    /// same-color knights a knight's move apart are allowed. The placement rule
    /// forbids exactly this, so it must hold for *every* variant — including the
    /// four-color game — across the whole window, boundary included.
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

    /// Both non-canonical variants still seed Black at the center while Red's first
    /// pick becomes the mirrored center (-1,0) — for rot180 because (1,0) negates to
    /// (-1,0), for mirror because (1,0) reflects to (-1,0). Each changes the board
    /// and lifts the symmetry under *its own* transform above canonical's.
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

        // The two transforms produce genuinely different boards.
        let differ = (-40..=40).any(|y| (-40..=40).any(|x| rot.cell(x, y) != mir.cell(x, y)));
        assert!(differ, "rot180 and mirror should not coincide");
    }

    /// Quad seats four teams in turn order on the standard spiral. The first round
    /// fills the innermost cells: Black at the center, then Red, Green, Yellow take
    /// the next three squares of the spiral (1,0), (1,1), (0,1). All four end up on
    /// the board.
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
        // Every team's tally plus the empties accounts for the whole window.
        let empty = r.squares_considered - r.placed();
        assert_eq!(r.count(EMPTY), empty);
    }
}
