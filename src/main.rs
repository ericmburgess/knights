//! Knight problems on a numbered square spiral.
//!
//! Subcommands:
//!   trapped    Problem 1 — one knight always hopping to the lowest unvisited
//!              square until trapped; renders its path.
//!   courteous  Problem 2 — place a knight on every square not attacked by an
//!              earlier knight; renders the resulting clusters.
//!   redblack   Problem 3 — two teams alternate placing knights; renders the
//!              territories that emerge.
//!   custom     Arbitrary placement game from a TOML config (any pieces, spirals,
//!              and colors) — the general engine the other placement games are presets of.
//!
//! The board problems can render to SVG (default) or, for huge radii, to an
//! indexed PNG (`--format png`).

mod config;
mod courteous;
mod engine;
mod knight;
mod piece;
mod raster;
mod redblack;
mod render;
mod spiral;

use courteous::simulate_courteous;
use engine::Board;
use knight::simulate_trapped_knight;
use redblack::{simulate_redblack, Variant};
use std::time::Instant;

const USAGE: &str = "\
Knight problems on a numbered square spiral.

Usage:
  knights [trapped]   [--start <n>] [--canvas <px>] [--out <path>]
  knights  courteous  [--radius <r>] [--format svg|png] [--canvas <px>] [--squaresize <px>] [--out <path>]
  knights  redblack   [--radius <r>] [--variant canonical|rot180|mirror|quad] [--format svg|png] [--canvas <px>] [--squaresize <px>] [--out <path>]
  knights  custom     --config <path> [--radius <r>] [--format svg|png] [--canvas <px>] [--squaresize <px>] [--out <path>]

Subcommands:
  trapped     One knight hops to the lowest-numbered unvisited square until
              trapped (default). With --start 1 it traps at square 2084.
  courteous   Place a knight on each square not attacked by an earlier knight,
              scanning the spiral over a square region of the given radius.
  redblack    Two teams alternate (Black first), each placing on the lowest
              square not attacked by the other color, over the given radius.
  custom      Run an arbitrary placement game from a TOML config: any number of
              pieces, each with its own attack offsets, spiral, and color.

Options:
  --start <n>        Number of the center square (default 1).
  --config <path>    TOML config for the custom subcommand (required there). Defines
                     piece types (attack offsets) and pieces (type, color, spiral).
  --variant <v>      Red/black only: canonical (default; reproduces OEIS
                     A392177/A392178), rot180 (Red's spiral rotated 180°), mirror
                     (Red's spiral reflected across the y-axis: left, up, right,
                     down), or quad (four colors — Black, Red, Green, Yellow — all
                     on the same spiral). All but canonical are non-canonical.
  --radius <r>       Half-width of the region in cells (default 30; 80 for redblack).
  --format <f>       Output format: svg or png (default svg, or inferred from --out).
                     PNG is indexed-color, for the board problems, and scales to
                     huge radii where SVG would be impractical.
  --canvas <px>      SVG canvas size in pixels (default 1600). SVG only.
  --squaresize <px>  PNG pixels per square (default 1). PNG only.
  --out <path>       Where to write the output (defaults per subcommand and format).
  -h, --help         Show this help.";

enum Problem {
    Trapped,
    Courteous,
    RedBlack,
    Custom,
}

enum Format {
    Svg,
    Png,
}

impl Format {
    fn ext(&self) -> &'static str {
        match self {
            Format::Svg => "svg",
            Format::Png => "png",
        }
    }
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
    let mut canvas: f64 = 1600.0;
    let mut squaresize: u32 = 1;
    let mut out: Option<String> = None;
    let mut format: Option<Format> = None;
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
            "--canvas" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--canvas needs a value"));
                canvas = v.parse().unwrap_or_else(|_| fail(&format!("invalid --canvas: {v}")));
                if !(canvas > 0.0) {
                    fail("--canvas must be a positive number of pixels");
                }
            }
            "--squaresize" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--squaresize needs a value"));
                squaresize = v.parse().unwrap_or_else(|_| fail(&format!("invalid --squaresize: {v}")));
                if squaresize == 0 {
                    fail("--squaresize must be at least 1 pixel");
                }
            }
            "--format" => {
                i += 1;
                let v = args.get(i).unwrap_or_else(|| fail("--format needs a value"));
                format = Some(match v.as_str() {
                    "svg" => Format::Svg,
                    "png" => Format::Png,
                    _ => fail(&format!("invalid --format: {v} (expected svg or png)")),
                });
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

    // Resolve format (explicit --format, else inferred from --out, else SVG) and
    // the output path (default name per subcommand and format).
    let format = format.unwrap_or_else(|| match out.as_deref() {
        Some(p) if p.to_ascii_lowercase().ends_with(".png") => Format::Png,
        _ => Format::Svg,
    });
    // --variant only affects the two-color game; reject it elsewhere rather than
    // silently ignoring it.
    if !matches!(problem, Problem::RedBlack) && !matches!(variant, Variant::Canonical) {
        fail("--variant is only valid for the redblack subcommand");
    }
    // --config is required by, and only valid for, the custom subcommand.
    if matches!(problem, Problem::Custom) && config_path.is_none() {
        fail("the custom subcommand requires --config <path>");
    }
    if !matches!(problem, Problem::Custom) && config_path.is_some() {
        fail("--config is only valid for the custom subcommand");
    }
    let base = match &problem {
        Problem::Trapped => "trapped_knight",
        Problem::Courteous => "courteous_knights",
        // Distinct default name per variant so the experiments never clobber the
        // canonical render (or each other).
        Problem::RedBlack => match variant {
            Variant::Canonical => "red_black_knights",
            Variant::Rot180 => "red_black_knights_rot180",
            Variant::Mirror => "red_black_knights_mirror",
            Variant::Quad => "red_black_knights_quad",
        },
        Problem::Custom => "custom",
    };
    let out = out.unwrap_or_else(|| format!("out/{base}.{}", format.ext()));

    if matches!(format, Format::Png) && matches!(problem, Problem::Trapped) {
        fail("PNG output is only for the board problems (courteous, redblack); \
              the trapped-knight path renders to SVG.");
    }

    // Each arm simulates (timed as `sim`), then renders and writes the output
    // (timed together as `render` — for streamed PNG the two are fused).
    let (kind, sim, render) = match problem {
        Problem::Trapped => {
            let t = Instant::now();
            let result = simulate_trapped_knight(start);
            let sim = t.elapsed();
            println!(
                "Visited {} squares in {} moves; trapped at square {}.",
                result.squares_visited(),
                result.moves(),
                result.trap_square()
            );
            let t = Instant::now();
            let svg = render::render_path_svg(&result, canvas);
            write_or_fail(render::write_svg(&svg, &out), &out);
            ("SVG", sim, t.elapsed())
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
            println!("First squares with knights: {:?}", result.first_squares(16));
            let t = Instant::now();
            let kind = match format {
                Format::Svg => {
                    let svg = render::render_courteous_svg(&result, canvas);
                    write_or_fail(render::write_svg(&svg, &out), &out);
                    "SVG"
                }
                Format::Png => {
                    let img = raster::courteous_image(&result, squaresize);
                    write_or_fail(raster::write_indexed(&out, &img), &out);
                    "PNG"
                }
            };
            (kind, sim, t.elapsed())
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
                "Placed {} knights ({}) among the first {} squares (radius {}); \
                 {} left empty.",
                placed, breakdown, result.squares_considered, result.radius, empty
            );
            // Rotate/mirror symmetry is only meaningful for the two-color variants.
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
            let kind = match format {
                Format::Svg => {
                    let svg = render::render_redblack_svg(&result, canvas);
                    write_or_fail(render::write_svg(&svg, &out), &out);
                    "SVG"
                }
                Format::Png => {
                    write_or_fail(raster::write_redblack_png(&out, &result, squaresize), &out);
                    "PNG"
                }
            };
            (kind, sim, t.elapsed())
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
            let kind = match format {
                Format::Svg => {
                    let svg = render::render_board_svg(&result, "Custom Placement", canvas);
                    write_or_fail(render::write_svg(&svg, &out), &out);
                    "SVG"
                }
                Format::Png => {
                    write_or_fail(raster::write_board_png(&out, &result, squaresize), &out);
                    "PNG"
                }
            };
            (kind, sim, t.elapsed())
        }
    };

    println!("Simulated in {sim:.2?}; rendered in {render:.2?}.");
    println!("Wrote {kind} to {out}");
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
