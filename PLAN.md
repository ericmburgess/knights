# Milestone 1 — Recreate the "Trapped Knight"

## Context
We're starting a brand-new Rust project (empty folder, only `KNIGHTS.md`) to build a tool
that visualizes chess-knight problems on a number-spiral grid, inspired by a Numberphile
video. The user is new to Rust and hands-off from the code.

`KNIGHTS.md` describes three problems. This milestone implements **Problem 1: the Trapped
Knight Sequence** and renders it the way the video does. A knight starts on the center of
an infinitely large board whose cells are numbered along a square spiral. Each turn it hops
to the **lowest-numbered unvisited cell** a knight's move away. It eventually has no
unvisited cell within reach and is "trapped."

Decisions from the user:
- **Output: a static SVG** of the knight's path (vector, no extra dependencies, easy to restyle).
- **Numbering starts at 1** at the center, so the knight gets trapped at square **2084** —
  matching the video and OEIS A316667. This gives us a known answer to verify against.

The goal of this milestone is a correct, verifiable base we can build the user's own ideas on.

## Approach

A single Cargo **binary crate** named `knights`, **edition 2021**, with **zero external
dependencies** (std only). Hand-rolled SVG keeps the first compile fast and sidesteps any
linker/C-toolchain surprises from third-party crates. CLI args parsed by hand from
`std::env::args` (no `clap` yet).

### Files
- `Cargo.toml` — crate metadata, no dependencies.
- `src/main.rs` — entry point. Minimal flag parsing: `--start <n>` (default `1`),
  `--out <path>` (default `out/trapped_knight.svg`). Runs the simulation, prints stats,
  writes the SVG.
- `src/spiral.rs` — the square-spiral coordinate system.
- `src/knight.rs` — knight moves + the trapped-knight simulation.
- `src/render.rs` — turn a path into an SVG string + write it to disk.

### `spiral.rs` — number-spiral grid
Maps between a 0-based spiral position and a lattice coordinate `(x, y)` (x right, y up).
Convention: counterclockwise Ulam spiral — step right, then up, then left, then down, with
arm lengths 1,1,2,2,3,3,… (the standard square spiral; its handedness doesn't affect the
result, only the picture's orientation).

- A growable table: `Vec<(i32,i32)>` (position → coord) and `HashMap<(i32,i32), u64>`
  (coord → position), plus a small saved walker state (pos, direction, steps left in the
  current arm) so it can be **extended incrementally**.
- `ensure_radius(r)` — walk the spiral until every cell within Chebyshev radius `r` of the
  center exists (i.e. until `len() >= (2r+1)^2`).
- `coord_to_index(x, y) -> Option<u64>` and `index_to_coord(n) -> (i32, i32)`.
- The human-facing **square number** = position + `start` (1 by default). Because `start`
  is a constant offset, "lowest-numbered" comparisons are identical to comparing positions,
  so the simulation works purely on positions and only adds `start` when reporting.

### `knight.rs` — the simulation
- The 8 knight offsets: `(±1,±2)`, `(±2,±1)`.
- Start at position 0 (center). Track `visited: HashSet<u64>` and `path: Vec<(u64, i32, i32)>`.
- Each step: `ensure_radius(max(|x|,|y|) + 2)` so all 8 neighbors are mapped, look up each
  neighbor's position, keep the unvisited ones, move to the **minimum** position. If none are
  unvisited, stop — the knight is trapped.
- Returns the path plus stats: squares visited, number of moves, and the final (trap) square
  number.

### `render.rs` — SVG
- Compute the bounding box of the path; map cell centers onto a fixed canvas (~1600×1600)
  with a margin, square aspect, **y flipped** (SVG y points down).
- White background; draw the path as many short `<line>` segments, each stroked with a color
  interpolated along the path (hue sweep, e.g. blue→red via a simple HSL→RGB helper) so you
  can read the order of travel — the iconic look from the video.
- Mark the **start** square (green dot) and the **trap** square (red dot), each labeled with
  its number.
- Write the string to the `--out` path, creating the `out/` directory if needed.

## Verification
1. `cargo run` (debug) — first real build; confirms the toolchain links cleanly (also
   resolves the earlier `cc` question). A trivial run early catches any environment issue.
2. The program prints, e.g., `Visited 2017 squares in 2016 moves; trapped at square 2084.`
   The **trap square must be 2084** with `--start 1`. This is the correctness check against
   OEIS A316667 / the video.
3. A `#[test]` (`cargo test`) asserting the trap square equals `2084` for `start = 1`, as a
   permanent regression guard.
4. Open `out/trapped_knight.svg` in a browser and eyeball it: a dense scribble that works
   outward and then gets stuck — visually matching the known image. (I'll report the result;
   the user can open the file too.)
5. Sanity-check `--start 0` reports a trap at `2083` (same shape, labels shifted by one).

## Out of scope (later milestones)
Problems 2 (Courteous Knights) and 3 (Red & Black Knights); animation, PNG, interactivity;
performance work for millions of cells (the closed-form spiral / no-HashMap path). The code
is structured (`spiral` / `knight` / `render` split) so these slot in without a rewrite, and
so the user's own ideas are easy to graft on.
