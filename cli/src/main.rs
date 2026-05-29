//! Knight problems on a numbered square spiral (CLI front-end over `knights_core`).
//!
//! Subcommands:
//!   trapped    Problem 1 — one knight hops to the lowest unvisited square until
//!              trapped; prints the trap square (text only, no image).
//!   courteous  Problem 2 — place a knight on every square not attacked by an
//!              earlier knight; renders the clusters to PNG.
//!   redblack   Problem 3 — multi-color placement game (+ variants); renders to PNG.
//!   custom     Arbitrary placement game from a TOML config; renders to PNG.
//!
//! All images are indexed-color PNG. (The interactive editor / display lives in the
//! web front-end; this CLI is for headless and large-radius renders.)

use knights_core::config;
use knights_core::courteous::simulate_courteous;
use knights_core::engine::{self, Board};
use knights_core::knight::simulate_trapped_knight;
use knights_core::raster;
use knights_core::redblack::{self, simulate_redblack, Variant};
use std::time::Instant;

const USAGE: &str = "\
Knight problems on a numbered square spiral.

Usage:
  knights [trapped]   [--start <n>]
  knights  courteous  [--radius <r>] [--squaresize <px>] [--out <path>]
  knights  redblack   [--radius <r>] [--variant canonical|rot180|mirror|quad] [--squaresize <px>] [--out <path>]
  knights  custom     --config <path> [--radius <r>] [--squaresize <px>] [--out <path>]

Subcommands:
  trapped     One knight hops to the lowest-numbered unvisited square until trapped
              (default). With --start 1 it traps at square 2084. Prints text only.
  courteous   Place a knight on each square not attacked by an earlier knight,
              scanning the spiral over a square region of the given radius.
  redblack    Multi-color placement game; --variant selects the rule set.
  custom      Arbitrary placement game from a TOML config: any number of pieces,
              each with its own attack offsets, spiral, and color.

Options:
  --start <n>        Number of the center square (default 1; trapped only).
  --radius <r>       Half-width of the region in cells (default 30; 80 for redblack/custom).
  --variant <v>      Red/black only: canonical (default; OEIS A392177/A392178), rot180,
                     mirror, or quad. All but canonical are non-canonical experiments.
  --config <path>    TOML config for the custom subcommand (required there): defines
                     piece types (attack offsets) and pieces (type, color, spiral).
  --squaresize <px>  PNG pixels per square (default 1).
  --out <path>       Output PNG path (defaults per subcommand).
  -h, --help         Show this help.";

enum Problem {
    Trapped,
    Courteous,
    RedBlack,
    Custom,
}

/// Write the output, exiting with an error message on failure.
fn write_or_fail(result: std::io::Result<()>, out: &str) {
    if let Err(e) = result {
        fail(&format!("failed to write {out}: {e}"));
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();

    // Optional leading subcommand; otherwise default to `trapped`.
    let (problem, mut i) = match args.get(1).map(String::as_str) {
        Some("trapped") => (Problem::Trapped, 2),
        Some("courteous") => (Problem::Courteous, 2),
        Some("redblack") => (Problem::RedBlack, 2),
        Some("custom") => (Problem::Custom, 2),
        Some("-h") | Some("--help") => {
            println!("{USAGE}");
            return;
        }
        Some(s) if s.starts_with('-') => (Problem::Trapped, 1), // flags only
        None => (Problem::Trapped, 1),
        Some(other) => fail(&format!("unknown subcommand: {other}\n\n{USAGE}")),
    };

    let mut start: u64 = 1;
    let mut radius: i32 = match &problem {
        Problem::RedBlack | Problem::Custom => 80,
        _ => 30,
    };
    let mut squaresize: u32 = 1;
    let mut out: Option<String> = None;
    let mut variant = Variant::Canonical;
    let mut config_path: Option<String> = None;
    while i < args.len() {
        match args[i].as_str() {
            "--start" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--start needs a value"));
                start = v.parse().unwrap_or_else(|_| fail(&format!("invalid --start: {v}")));
            }
            "--radius" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--radius needs a value"));
                radius = v.parse().unwrap_or_else(|_| fail(&format!("invalid --radius: {v}")));
            }
            "--squaresize" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--squaresize needs a value"));
                squaresize = v.parse().unwrap_or_else(|_| fail(&format!("invalid --squaresize: {v}")));
                if squaresize == 0 {
                    fail("--squaresize must be at least 1 pixel");
                }
            }
            "--variant" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--variant needs a value"));
                variant = match v.as_str() {
                    "canonical" => Variant::Canonical,
                    "rot180" => Variant::Rot180,
                    "mirror" => Variant::Mirror,
                    "quad" => Variant::Quad,
                    _ => fail(&format!(
                        "invalid --variant: {v} (expected canonical, rot180, mirror, or quad)"
                    )),
                };
            }
            "--config" => {
                i += 1;
                config_path = Some(args.get(i).unwrap_or_else(|| fail("--config needs a value")).clone());
            }
            "--out" => {
                i += 1;
                out = Some(args.get(i).unwrap_or_else(|| fail("--out needs a value")).clone());
            }
            "-h" | "--help" => {
                println!("{USAGE}");
                return;
            }
            other => fail(&format!("unknown argument: {other}\n\n{USAGE}")),
        }
        i += 1;
    }

    // --variant only affects redblack; --config is required by, and only valid for, custom.
    if !matches!(problem, Problem::RedBlack) && !matches!(variant, Variant::Canonical) {
        fail("--variant is only valid for the redblack subcommand");
    }
    if matches!(problem, Problem::Custom) && config_path.is_none() {
        fail("the custom subcommand requires --config <path>");
    }
    if !matches!(problem, Problem::Custom) && config_path.is_some() {
        fail("--config is only valid for the custom subcommand");
    }

    let base = match &problem {
        Problem::Trapped => "trapped_knight",
        Problem::Courteous => "courteous_knights",
        Problem::RedBlack => match variant {
            Variant::Canonical => "red_black_knights",
            Variant::Rot180 => "red_black_knights_rot180",
            Variant::Mirror => "red_black_knights_mirror",
            Variant::Quad => "red_black_knights_quad",
        },
        Problem::Custom => "custom",
    };
    let out = out.unwrap_or_else(|| format!("out/{base}.png"));

    match problem {
        Problem::Trapped => {
            // Text only — the path visualization was retired with SVG.
            let t = Instant::now();
            let result = simulate_trapped_knight(start);
            println!(
                "Visited {} squares in {} moves; trapped at square {} (computed in {:.2?}).",
                result.squares_visited(),
                result.moves(),
                result.trap_square(),
                t.elapsed()
            );
        }
        Problem::Courteous => {
            let t = Instant::now();
            let result = simulate_courteous(radius, start);
            let sim = t.elapsed();
            let hist: Vec<String> = result
                .size_histogram()
                .iter()
                .map(|(size, clusters)| format!("{size}×{clusters}"))
                .collect();
            println!(
                "Placed {} knights among the first {} squares (radius {}).",
                result.placed(),
                result.squares_considered,
                result.radius
            );
            println!("Cluster sizes (size×count): {}", hist.join(", "));
            let t = Instant::now();
            let img = raster::courteous_image(&result, squaresize);
            write_or_fail(raster::write_indexed(&out, &img), &out);
            report(sim, t.elapsed(), &out);
        }
        Problem::RedBlack => {
            let t = Instant::now();
            let result = simulate_redblack(radius, variant);
            let sim = t.elapsed();
            let placed = result.placed();
            let empty = result.squares_considered - placed;
            let breakdown = result
                .teams()
                .iter()
                .map(|&c| format!("{} {}", result.count(c), redblack::color_name(c)))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "Placed {} knights ({}) among the first {} squares (radius {}); {} left empty.",
                placed, breakdown, result.squares_considered, result.radius, empty
            );
            if result.teams().len() == 2 {
                println!(
                    "Variant: {} — symmetry: rotate-180+swap {:.1}%, mirror-Y+swap {:.1}%.",
                    variant.name(),
                    result.rot_swap_symmetry() * 100.0,
                    result.mirror_swap_symmetry() * 100.0
                );
            } else {
                println!("Variant: {}.", variant.name());
            }
            let t = Instant::now();
            write_or_fail(raster::write_board_png(&out, &result, squaresize), &out);
            report(sim, t.elapsed(), &out);
        }
        Problem::Custom => {
            let path = config_path.as_deref().expect("validated present above");
            let cfg = config::load(path).unwrap_or_else(|e| fail(&format!("config error: {e}")));
            let t = Instant::now();
            let result = engine::simulate(radius, cfg);
            let sim = t.elapsed();
            let legend = result.legend();
            let placed: u64 = legend.iter().map(|row| row.count).sum();
            let total = result.squares_considered;
            let breakdown = legend
                .iter()
                .map(|row| format!("{} {}", row.count, row.label))
                .collect::<Vec<_>>()
                .join(", ");
            println!(
                "Placed {} pieces ({}) among the first {} squares (radius {}); {} left empty.",
                placed, breakdown, total, result.radius, total - placed
            );
            let t = Instant::now();
            write_or_fail(raster::write_board_png(&out, &result, squaresize), &out);
            report(sim, t.elapsed(), &out);
        }
    }
}

/// Print the simulate/render timings and output path for the rendering subcommands.
fn report(sim: std::time::Duration, render: std::time::Duration, out: &str) {
    println!("Simulated in {sim:.2?}; rendered in {render:.2?}.");
    println!("Wrote PNG to {out}");
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
