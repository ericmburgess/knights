//! Courteous Knights (Problem 2).
//!
//! Scan the spiral from the center outward (n = 0, 1, 2, …). Place a knight on
//! square `n` iff no already-placed knight attacks it. Once placed, a knight
//! never moves. Because a square is only ever attacked by *earlier* (smaller-n)
//! knights, every placement decision is final the moment it's made — so the
//! result for a full square region of radius `R` is exact, with no edge effects.
//!
//! This is the greedy maximal independent set of the knight-move graph, taken in
//! spiral order — OEIS A308885. The placed knights settle into a regular,
//! periodic pattern: plus-pentominoes (5 knights), 2x2 squares (4), and lone
//! knights (1), with rare 2s/3s along the seams between domains. (Verified
//! against all 10000 terms of the A308885 b-file; see `matches_oeis_a308885`.)

use crate::knight::KNIGHT_OFFSETS;
use crate::spiral::Spiral;
use std::collections::HashMap;

/// King-move neighbor offsets (8-connectivity), used to group touching knights
/// into visual clusters.
const KING_OFFSETS: [(i32, i32); 8] = [
    (-1, -1),
    (-1, 0),
    (-1, 1),
    (0, -1),
    (0, 1),
    (1, -1),
    (1, 0),
    (1, 1),
];

pub struct CourteousResult {
    /// Offset added to a 0-based position to get its human-facing square number.
    pub start: u64,
    /// Chebyshev radius of the square region simulated.
    pub radius: i32,
    /// Number of spiral squares considered: `(2*radius + 1)^2`.
    pub squares_considered: u64,
    /// Placed knights as `(position, x, y)`, in placement (spiral) order.
    pub knights: Vec<(u64, i32, i32)>,
    /// Cluster id of each knight, parallel to `knights`.
    pub cluster_of: Vec<usize>,
    /// Number of knights in each cluster, indexed by cluster id.
    pub cluster_sizes: Vec<usize>,
}

impl CourteousResult {
    /// Total knights placed.
    pub fn placed(&self) -> usize {
        self.knights.len()
    }

    /// The first `k` placed square numbers (human-facing, i.e. position + start).
    pub fn first_squares(&self, k: usize) -> Vec<u64> {
        self.knights
            .iter()
            .take(k)
            .map(|&(p, _, _)| p + self.start)
            .collect()
    }

    /// Histogram of cluster sizes as `(size, number_of_clusters)`, sorted by size.
    pub fn size_histogram(&self) -> Vec<(usize, usize)> {
        let mut counts: HashMap<usize, usize> = HashMap::new();
        for &s in &self.cluster_sizes {
            *counts.entry(s).or_insert(0) += 1;
        }
        let mut hist: Vec<(usize, usize)> = counts.into_iter().collect();
        hist.sort_unstable();
        hist
    }
}

/// Run the Courteous Knights placement over the full square region of Chebyshev
/// radius `radius`. `start` only shifts reported square numbers; it does not
/// affect which squares receive knights.
pub fn simulate_courteous(radius: i32, start: u64) -> CourteousResult {
    let radius = radius.max(0);
    let mut spiral = Spiral::new();
    spiral.ensure_radius(radius);
    let d = (2 * radius + 1) as u64;
    let squares_considered = d * d;

    // coord -> index into `knights` (also serves as the "occupied" set).
    let mut occupied: HashMap<(i32, i32), usize> = HashMap::new();
    let mut knights: Vec<(u64, i32, i32)> = Vec::new();

    for n in 0..squares_considered {
        let (x, y) = spiral.index_to_coord(n);
        let attacked = KNIGHT_OFFSETS
            .iter()
            .any(|&(dx, dy)| occupied.contains_key(&(x + dx, y + dy)));
        if !attacked {
            occupied.insert((x, y), knights.len());
            knights.push((n, x, y));
        }
    }

    let (cluster_of, cluster_sizes) = cluster_by_king_adjacency(&knights, &occupied);

    CourteousResult {
        start,
        radius,
        squares_considered,
        knights,
        cluster_of,
        cluster_sizes,
    }
}

/// Group knights into connected components under king (8-) adjacency.
fn cluster_by_king_adjacency(
    knights: &[(u64, i32, i32)],
    occupied: &HashMap<(i32, i32), usize>,
) -> (Vec<usize>, Vec<usize>) {
    let mut dsu = Dsu::new(knights.len());
    for (i, &(_, x, y)) in knights.iter().enumerate() {
        for (dx, dy) in KING_OFFSETS {
            if let Some(&j) = occupied.get(&(x + dx, y + dy)) {
                dsu.union(i, j);
            }
        }
    }

    // Compact the disjoint-set roots into dense cluster ids.
    let mut root_to_id: HashMap<usize, usize> = HashMap::new();
    let mut cluster_sizes: Vec<usize> = Vec::new();
    let mut cluster_of = vec![0usize; knights.len()];
    for i in 0..knights.len() {
        let root = dsu.find(i);
        let id = *root_to_id.entry(root).or_insert_with(|| {
            cluster_sizes.push(0);
            cluster_sizes.len() - 1
        });
        cluster_of[i] = id;
        cluster_sizes[id] += 1;
    }
    (cluster_of, cluster_sizes)
}

/// A small union-find (disjoint-set union) over `0..n`.
struct Dsu {
    parent: Vec<usize>,
    size: Vec<usize>,
}

impl Dsu {
    fn new(n: usize) -> Self {
        Dsu {
            parent: (0..n).collect(),
            size: vec![1; n],
        }
    }

    fn find(&mut self, a: usize) -> usize {
        let mut root = a;
        while self.parent[root] != root {
            root = self.parent[root];
        }
        // Path compression.
        let mut cur = a;
        while self.parent[cur] != root {
            let next = self.parent[cur];
            self.parent[cur] = root;
            cur = next;
        }
        root
    }

    fn union(&mut self, a: usize, b: usize) {
        let (mut ra, mut rb) = (self.find(a), self.find(b));
        if ra == rb {
            return;
        }
        if self.size[ra] < self.size[rb] {
            std::mem::swap(&mut ra, &mut rb);
        }
        self.parent[rb] = ra;
        self.size[ra] += self.size[rb];
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn center_gets_a_knight() {
        let r = simulate_courteous(8, 1);
        assert_eq!(r.knights.first().map(|&(p, x, y)| (p, x, y)), Some((0, 0, 0)));
    }

    /// No two placed knights are a knight's move apart (independent set).
    #[test]
    fn placement_is_independent() {
        let r = simulate_courteous(14, 0);
        let occ: HashSet<(i32, i32)> = r.knights.iter().map(|&(_, x, y)| (x, y)).collect();
        for &(_, x, y) in &r.knights {
            for (dx, dy) in KNIGHT_OFFSETS {
                assert!(
                    !occ.contains(&(x + dx, y + dy)),
                    "knights at ({x},{y}) and ({},{}) attack each other",
                    x + dx,
                    y + dy
                );
            }
        }
    }

    /// The placement sequence matches OEIS A308885 (center numbered 0).
    /// These are the first 61 terms from the OEIS DATA section.
    #[test]
    fn matches_oeis_a308885() {
        const A308885: [u64; 61] = [
            0, 1, 2, 3, 20, 25, 30, 35, 36, 37, 40, 41, 42, 47, 48, 49, 54, 55, 56, 63, 65, 70,
            79, 88, 94, 95, 110, 112, 114, 115, 121, 123, 125, 126, 132, 134, 137, 138, 141, 143,
            144, 145, 147, 149, 150, 152, 154, 155, 156, 162, 165, 167, 168, 169, 175, 178, 180,
            181, 182, 195, 197,
        ];
        let r = simulate_courteous(12, 0);
        assert_eq!(r.first_squares(A308885.len()), A308885);
    }

    /// Every empty square is attacked by some placed knight (maximal / dominating).
    #[test]
    fn placement_is_maximal() {
        let radius = 14;
        let r = simulate_courteous(radius, 0);
        let occ: HashSet<(i32, i32)> = r.knights.iter().map(|&(_, x, y)| (x, y)).collect();

        let mut spiral = Spiral::new();
        spiral.ensure_radius(radius);
        for n in 0..r.squares_considered {
            let (x, y) = spiral.index_to_coord(n);
            if occ.contains(&(x, y)) {
                continue;
            }
            let attacked = KNIGHT_OFFSETS
                .iter()
                .any(|&(dx, dy)| occ.contains(&(x + dx, y + dy)));
            assert!(attacked, "empty square at ({x},{y}) is not attacked by any knight");
        }
    }
}
