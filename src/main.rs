//! Knight problems on a numbered square spiral.
//!
//! Subcommands:
//!   trapped    Problem 1 — one knight always hopping to the lowest unvisited
//!              square until trapped; renders its path.
//!   courteous  Problem 2 — place a knight on every square not attacked by an
//!              earlier knight; renders the resulting clusters.
//!   redblack   Problem 3 — two teams alternate placing knights; renders the
//!              territories that emerge.
//!
//! The board problems can render to SVG (default) or, for huge radii, to an
//! indexed PNG (`--format png`).

mod courteous;
mod knight;
mod raster;
mod redblack;
mod render;
mod spiral;

use courteous::simulate_courteous;
use knight::simulate_trapped_knight;
use redblack::simulate_redblack;
use std::time::Instant;

const USAGE: &str = "\
Knight problems on a numbered square spiral.

Usage:
  knights [trapped]   [--start <n>] [--canvas <px>] [--out <path>]
  knights  courteous  [--radius <r>] [--format svg|png] [--canvas <px>] [--squaresize <px>] [--out <path>]
  knights  redblack   [--radius <r>] [--format svg|png] [--canvas <px>] [--squaresize <px>] [--out <path>]

Subcommands:
  trapped     One knight hops to the lowest-numbered unvisited square until
              trapped (default). With --start 1 it traps at square 2084.
  courteous   Place a knight on each square not attacked by an earlier knight,
              scanning the spiral over a square region of the given radius.
  redblack    Two teams alternate (Black first), each placing on the lowest
              square not attacked by the other color, over the given radius.

Options:
  --start <n>        Number of the center square (default 1).
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
        Problem::RedBlack => 80,
        _ => 30,
    };
    let mut canvas: f64 = 1600.0;
    let mut squaresize: u32 = 1;
    let mut out: Option<String> = None;
    let mut format: Option<Format> = None;
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
    let base = match &problem {
        Problem::Trapped => "trapped_knight",
        Problem::Courteous => "courteous_knights",
        Problem::RedBlack => "red_black_knights",
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
            let result = simulate_redblack(radius);
            let sim = t.elapsed();
            let placed = result.black + result.red;
            let empty = result.squares_considered as usize - placed;
            println!(
                "Placed {} knights ({} Black, {} Red) among the first {} squares (radius {}); \
                 {} left empty.",
                placed, result.black, result.red, result.squares_considered, result.radius, empty
            );
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
    };

    println!("Simulated in {sim:.2?}; rendered in {render:.2?}.");
    println!("Wrote {kind} to {out}");
}

fn fail(msg: &str) -> ! {
    eprintln!("{msg}");
    std::process::exit(1);
}
