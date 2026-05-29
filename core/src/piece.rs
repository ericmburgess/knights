//! Pieces and the cell encoding the placement engine writes.
//!
//! A *piece type* is just a finite list of relative coordinates it attacks (the
//! generalization of `knight::KNIGHT_OFFSETS`). What actually lands on the board is a
//! *kind*: a distinct `(attack-offset set, color)` combination. Two configured pieces
//! that share both their offsets and their color collapse to one kind — and one cell
//! byte — so a board with hundreds of same-team pieces still uses a single code.
//!
//! Each board cell is one `u8`: `0` is empty, `1..=255` index a [`Kind`]. From the
//! byte the engine recovers the occupant's attack offsets (whose piece attacks where)
//! and its `color_id` (who is friend vs. foe — same color cooperates, even across
//! piece types). The byte is also a palette index for rendering, exactly as the old
//! fixed `EMPTY/BLACK/RED` codes were.

/// An RGB color, matching the `(u8, u8, u8)` palette tuples used by the renderers.
pub type Rgb = (u8, u8, u8);

/// Empty-cell code (palette index 0, white).
pub const EMPTY: u8 = 0;

/// At most 255 distinct kinds can occupy the board — the cell is a `u8` and code 0 is
/// reserved for empty.
pub const MAX_KINDS: usize = 255;

/// One distinct `(offsets, color)` combination that can occupy cells.
struct Kind {
    /// Attack offsets of this kind's piece type (deduped and sorted).
    offsets: Vec<(i32, i32)>,
    /// Render color.
    color: Rgb,
    /// Team identity: kinds sharing a color are friends (interned, so comparison is a
    /// cheap integer compare rather than an RGB compare).
    color_id: u16,
    /// Human-facing label for the legend.
    label: String,
}

/// Maps each cell byte to its [`Kind`]. Index 0 is the empty sentinel (white, no
/// offsets); placed kinds occupy `1..len()`.
pub struct KindTable {
    kinds: Vec<Kind>,
    /// Largest Chebyshev magnitude over every kind's offsets — sets the grid padding
    /// so backward attack reads stay in bounds.
    max_cheby: i32,
}

impl KindTable {
    /// Palette indexed by cell byte (index 0 = empty/white).
    pub fn palette(&self) -> Vec<Rgb> {
        self.kinds.iter().map(|k| k.color).collect()
    }

    /// Team id of a kind (kinds with equal ids are the same color / cooperate).
    pub fn color_id(&self, kind: u8) -> u16 {
        self.kinds[kind as usize].color_id
    }

    /// Legend label of a kind.
    pub fn label(&self, kind: u8) -> &str {
        &self.kinds[kind as usize].label
    }

    /// Largest Chebyshev magnitude over all kinds' offsets (0 if none).
    pub fn max_chebyshev(&self) -> i32 {
        self.max_cheby
    }

    /// Number of palette slots, including the empty sentinel.
    pub fn len(&self) -> usize {
        self.kinds.len()
    }

    /// Placed kinds (skipping EMPTY) as `(cell_byte, color_id, offsets)`, the form the
    /// engine's attack check iterates.
    pub fn placed(&self) -> impl Iterator<Item = (u8, u16, &[(i32, i32)])> {
        self.kinds
            .iter()
            .enumerate()
            .skip(1)
            .map(|(i, k)| (i as u8, k.color_id, k.offsets.as_slice()))
    }
}

/// Interns `(offsets, color)` combinations into a [`KindTable`], assigning cell bytes
/// and deduping repeats. Color ids are interned in parallel so same-color pieces share
/// a team id regardless of piece type.
pub struct KindBuilder {
    kinds: Vec<Kind>,
    colors: Vec<Rgb>,
}

impl KindBuilder {
    /// A builder seeded with the empty sentinel at index 0.
    pub fn new() -> Self {
        KindBuilder {
            kinds: vec![Kind {
                offsets: Vec::new(),
                color: (255, 255, 255),
                color_id: u16::MAX,
                label: String::new(),
            }],
            colors: Vec::new(),
        }
    }

    /// Intern a piece. `offsets` is canonicalized (sorted + deduped); an identical
    /// `(offsets, color)` returns the existing cell byte. Returns `None` only if a new
    /// kind would exceed [`MAX_KINDS`].
    pub fn intern(&mut self, mut offsets: Vec<(i32, i32)>, color: Rgb, label: &str) -> Option<u8> {
        offsets.sort_unstable();
        offsets.dedup();
        if let Some(byte) = self
            .kinds
            .iter()
            .position(|k| k.color == color && k.offsets == offsets)
            .filter(|&i| i != 0)
        {
            return Some(byte as u8);
        }
        // Placed kinds occupy bytes 1..=MAX_KINDS, so len() may grow to MAX_KINDS + 1.
        if self.kinds.len() > MAX_KINDS {
            return None;
        }
        let color_id = self.color_id(color);
        let byte = self.kinds.len() as u8;
        self.kinds.push(Kind { offsets, color, color_id, label: label.to_string() });
        Some(byte)
    }

    fn color_id(&mut self, color: Rgb) -> u16 {
        match self.colors.iter().position(|&c| c == color) {
            Some(i) => i as u16,
            None => {
                self.colors.push(color);
                (self.colors.len() - 1) as u16
            }
        }
    }

    /// Finish, computing the max Chebyshev reach across all placed kinds' offsets.
    pub fn finish(self) -> KindTable {
        let max_cheby = self
            .kinds
            .iter()
            .skip(1)
            .flat_map(|k| k.offsets.iter())
            .map(|&(dx, dy)| dx.abs().max(dy.abs()))
            .max()
            .unwrap_or(0);
        KindTable { kinds: self.kinds, max_cheby }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KNIGHT: [(i32, i32); 8] = [
        (1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2),
    ];
    const WAZIR: [(i32, i32); 4] = [(1, 0), (-1, 0), (0, 1), (0, -1)];
    const BLACK: Rgb = (26, 26, 26);
    const RED: Rgb = (209, 31, 31);

    #[test]
    fn kind_dedup_and_color_teams() {
        let mut b = KindBuilder::new();
        let k_black = b.intern(KNIGHT.to_vec(), BLACK, "knight").unwrap();
        // Same offsets+color (even reordered) reuses the same byte.
        let mut shuffled = KNIGHT.to_vec();
        shuffled.reverse();
        assert_eq!(b.intern(shuffled, BLACK, "knight"), Some(k_black));
        // Same offsets, different color -> distinct kind, distinct team.
        let k_red = b.intern(KNIGHT.to_vec(), RED, "knight").unwrap();
        assert_ne!(k_black, k_red);
        // Different offsets, same color -> distinct kind, SAME team (cooperates).
        let k_wazir_black = b.intern(WAZIR.to_vec(), BLACK, "wazir").unwrap();
        assert_ne!(k_wazir_black, k_black);

        let table = b.finish();
        assert_eq!(table.color_id(k_black), table.color_id(k_wazir_black), "same color = same team");
        assert_ne!(table.color_id(k_black), table.color_id(k_red), "different color = different team");
        assert_eq!(table.len(), 4, "EMPTY + 3 distinct kinds");
        assert_eq!(table.palette()[0], (255, 255, 255), "slot 0 is empty/white");
        assert_eq!(table.max_chebyshev(), 2, "knight reach dominates the wazir");
    }

    #[test]
    fn too_many_kinds_is_rejected() {
        let mut b = KindBuilder::new();
        // 255 distinct colors all fit (bytes 1..=255); the 256th kind does not.
        for i in 0..MAX_KINDS as u16 {
            let color = ((i >> 8) as u8, i as u8, 7);
            assert!(b.intern(vec![(1, 2)], color, "p").is_some(), "kind {i} should fit");
        }
        assert_eq!(b.intern(vec![(1, 2)], (1, 1, 1), "overflow"), None);
    }
}
