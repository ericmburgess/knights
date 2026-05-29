//! Turn a simulation result into an SVG string and write it to disk.
//!
//! The canvas size (square, in pixels) is a parameter. The margin and all UI
//! elements (stroke widths, marker dots, fonts, legend) scale with it, so a
//! larger `--canvas` yields a higher-detail picture with consistent proportions
//! rather than just enlarging a fixed layout.

use crate::courteous::CourteousResult;
use crate::knight::SimResult;
use crate::redblack::{Color, RedBlackResult};
use std::fs;
use std::io;
use std::path::Path;

/// The default canvas size everything else is proportioned against.
const REFERENCE_CANVAS: f64 = 1600.0;

/// Maps lattice coordinates onto the canvas: square aspect, centered, y flipped
/// (SVG's y axis points down).
struct Viewport {
    canvas: f64,
    /// Pixels per lattice cell.
    scale: f64,
    data_cx: f64,
    data_cy: f64,
}

impl Viewport {
    fn from_bounds(canvas: f64, margin: f64, min_x: i32, max_x: i32, min_y: i32, max_y: i32) -> Self {
        let span = ((max_x - min_x).max(max_y - min_y)).max(1) as f64;
        Viewport {
            canvas,
            scale: (canvas - 2.0 * margin) / span,
            data_cx: (min_x + max_x) as f64 / 2.0,
            data_cy: (min_y + max_y) as f64 / 2.0,
        }
    }

    fn to_px(&self, x: i32, y: i32) -> (f64, f64) {
        (
            self.canvas / 2.0 + (x as f64 - self.data_cx) * self.scale,
            self.canvas / 2.0 - (y as f64 - self.data_cy) * self.scale,
        )
    }
}

/// Margin around the drawing, proportional to the canvas (48px at 1600px).
fn margin_for(canvas: f64) -> f64 {
    canvas * 0.03
}

/// UI scale factor relative to the reference canvas, for fonts/markers/legend.
fn ui_scale(canvas: f64) -> f64 {
    canvas / REFERENCE_CANVAS
}

fn svg_open(capacity: usize, canvas: f64) -> String {
    let mut svg = String::with_capacity(capacity);
    svg.push_str(&format!(
        "<svg xmlns=\"http://www.w3.org/2000/svg\" width=\"{c}\" height=\"{c}\" \
         viewBox=\"0 0 {c} {c}\">\n",
        c = canvas as i64
    ));
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"white\"/>\n");
    svg
}

/// Render a trapped-knight path: short segments whose hue sweeps blue -> red
/// along the order of travel, with green start and red trap markers.
pub fn render_path_svg(result: &SimResult, canvas: f64) -> String {
    let path = &result.path;
    let margin = margin_for(canvas);
    let ui = ui_scale(canvas);

    let (mut min_x, mut max_x) = (i32::MAX, i32::MIN);
    let (mut min_y, mut max_y) = (i32::MAX, i32::MIN);
    for &(_, x, y) in path {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }
    let vp = Viewport::from_bounds(canvas, margin, min_x, max_x, min_y, max_y);

    let mut svg = svg_open(path.len() * 96 + 512, canvas);

    let stroke_w = (vp.scale * 0.18).clamp(1.2 * ui, 4.0 * ui);
    let denom = (path.len().saturating_sub(1)).max(1) as f64;
    svg.push_str(&format!(
        "<g fill=\"none\" stroke-width=\"{:.2}\" stroke-linecap=\"round\">\n",
        stroke_w
    ));
    for i in 1..path.len() {
        let (_, x0, y0) = path[i - 1];
        let (_, x1, y1) = path[i];
        let (px0, py0) = vp.to_px(x0, y0);
        let (px1, py1) = vp.to_px(x1, y1);
        let t = (i - 1) as f64 / denom;
        let (r, g, b) = hsl_to_rgb(240.0 * (1.0 - t), 0.85, 0.5);
        svg.push_str(&format!(
            "<line x1=\"{px0:.1}\" y1=\"{py0:.1}\" x2=\"{px1:.1}\" y2=\"{py1:.1}\" \
             stroke=\"#{r:02x}{g:02x}{b:02x}\"/>\n"
        ));
    }
    svg.push_str("</g>\n");

    let dot = (vp.scale * 0.32).clamp(4.0 * ui, 9.0 * ui);
    let font = (vp.scale * 0.9).clamp(14.0 * ui, 28.0 * ui);
    if let Some(&(_, x, y)) = path.first() {
        let (px, py) = vp.to_px(x, y);
        marker(&mut svg, px, py, dot, font, "#0a8f3c", &result.start_square().to_string());
    }
    if let Some(&(_, x, y)) = path.last() {
        let (px, py) = vp.to_px(x, y);
        marker(&mut svg, px, py, dot, font, "#d62020", &result.trap_square().to_string());
    }

    svg.push_str("</svg>\n");
    svg
}

/// Render Courteous Knights: each placed knight as a rounded cell, colored by
/// the size of its (king-adjacent) cluster, plus a legend.
pub fn render_courteous_svg(result: &CourteousResult, canvas: f64) -> String {
    let r = result.radius;
    let margin = margin_for(canvas);
    let ui = ui_scale(canvas);
    let vp = Viewport::from_bounds(canvas, margin, -r, r, -r, r);
    let cell = vp.scale;
    let size_px = (cell * 0.84).max(2.0);
    let rx = size_px * 0.2;

    let mut svg = svg_open(result.placed() * 110 + 1024, canvas);

    for (i, &(_, x, y)) in result.knights.iter().enumerate() {
        let (px, py) = vp.to_px(x, y);
        let color = cluster_color(result.cluster_sizes[result.cluster_of[i]]);
        cell_rect(&mut svg, px, py, size_px, rx, color);
    }

    let rows: Vec<(&str, String)> = result
        .size_histogram()
        .into_iter()
        .map(|(size, clusters)| (cluster_color(size), format!("clusters of {size}: {clusters}")))
        .collect();
    legend(&mut svg, margin, ui, "Courteous Knights", &rows);

    svg.push_str("</svg>\n");
    svg
}

/// Render Red & Black Knights: each placed knight as a near-solid cell in its
/// team color, so territory reads as solid blocks.
pub fn render_redblack_svg(result: &RedBlackResult, canvas: f64) -> String {
    let r = result.radius;
    let margin = margin_for(canvas);
    let ui = ui_scale(canvas);
    let vp = Viewport::from_bounds(canvas, margin, -r, r, -r, r);
    let cell = vp.scale;
    let size_px = (cell * 0.92).max(1.0);
    let rx = size_px * 0.15;

    let mut svg = svg_open(result.knights.len() * 96 + 1024, canvas);
    for &(_, x, y, color) in &result.knights {
        let (px, py) = vp.to_px(x, y);
        cell_rect(&mut svg, px, py, size_px, rx, redblack_color(color));
    }

    let rows = [
        (redblack_color(Color::Black), format!("Black: {}", result.black)),
        (redblack_color(Color::Red), format!("Red: {}", result.red)),
    ];
    legend(&mut svg, margin, ui, "Red & Black Knights", &rows);

    svg.push_str("</svg>\n");
    svg
}

/// A filled, slightly-rounded cell centered on `(px, py)`.
fn cell_rect(svg: &mut String, px: f64, py: f64, size: f64, rx: f64, color: &str) {
    svg.push_str(&format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{size:.1}\" height=\"{size:.1}\" rx=\"{rx:.1}\" \
         fill=\"{color}\"/>\n",
        px - size / 2.0,
        py - size / 2.0,
    ));
}

/// A title and color key, on a translucent backing, in the top-left corner.
fn legend(svg: &mut String, margin: f64, ui: f64, title: &str, rows: &[(&str, String)]) {
    let x = margin;
    let top = margin;
    let row_h = 30.0 * ui;
    let title_font = 24.0 * ui;
    let row_font = 20.0 * ui;
    let swatch = 22.0 * ui;
    let box_w = 360.0 * ui;
    let box_h = 44.0 * ui + rows.len() as f64 * row_h + 14.0 * ui;
    svg.push_str(&format!(
        "<rect x=\"{:.1}\" y=\"{:.1}\" width=\"{box_w:.1}\" height=\"{box_h:.1}\" rx=\"{:.1}\" \
         fill=\"white\" fill-opacity=\"0.82\" stroke=\"#ddd\"/>\n",
        x - 14.0 * ui,
        top - 18.0 * ui,
        12.0 * ui,
    ));
    svg.push_str(&format!(
        "<text x=\"{x:.1}\" y=\"{:.1}\" font-family=\"sans-serif\" font-size=\"{title_font:.0}\" \
         font-weight=\"bold\" fill=\"#222\">{}</text>\n",
        top + 8.0 * ui,
        xml_escape(title),
    ));
    let mut ly = top + 8.0 * ui + 34.0 * ui;
    for (color, label) in rows {
        svg.push_str(&format!(
            "<rect x=\"{x:.1}\" y=\"{:.1}\" width=\"{swatch:.1}\" height=\"{swatch:.1}\" \
             rx=\"{:.1}\" fill=\"{color}\"/>\n",
            ly - 17.0 * ui,
            5.0 * ui,
        ));
        svg.push_str(&format!(
            "<text x=\"{:.1}\" y=\"{ly:.1}\" font-family=\"sans-serif\" font-size=\"{row_font:.0}\" \
             fill=\"#333\">{}</text>\n",
            x + swatch + 10.0 * ui,
            xml_escape(label),
        ));
        ly += row_h;
    }
}

/// Escape text for inclusion in SVG/XML content.
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
}

/// Team colors for Red & Black Knights.
fn redblack_color(c: Color) -> &'static str {
    match c {
        Color::Black => "#1a1a1a",
        Color::Red => "#d11f1f",
    }
}

/// Color a knight by the size of the cluster it belongs to.
fn cluster_color(size: usize) -> &'static str {
    match size {
        1 => "#9e9e9e", // lone knight
        2 => "#2e7d32", // green
        3 => "#ef6c00", // orange
        4 => "#1565c0", // blue
        5 => "#c62828", // red
        _ => "#6a1b9a", // purple (6+)
    }
}

/// Draw a filled dot plus a number label sitting just above-right of it.
fn marker(svg: &mut String, px: f64, py: f64, dot: f64, font: f64, color: &str, label: &str) {
    svg.push_str(&format!(
        "<circle cx=\"{px:.1}\" cy=\"{py:.1}\" r=\"{dot:.1}\" fill=\"{color}\" \
         stroke=\"white\" stroke-width=\"1.5\"/>\n"
    ));
    svg.push_str(&format!(
        "<text x=\"{lx:.1}\" y=\"{ly:.1}\" font-family=\"sans-serif\" font-size=\"{font:.0}\" \
         font-weight=\"bold\" fill=\"{color}\" stroke=\"white\" stroke-width=\"0.6\" \
         paint-order=\"stroke\">{label}</text>\n",
        lx = px + dot + 3.0,
        ly = py - dot,
    ));
}

/// Convert an HSL color (h in [0,360), s,l in [0,1]) to 8-bit RGB.
fn hsl_to_rgb(h: f64, s: f64, l: f64) -> (u8, u8, u8) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - ((hp % 2.0) - 1.0).abs());
    let (r1, g1, b1) = match hp as i32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    let to_u8 = |v: f64| ((v + m) * 255.0).round().clamp(0.0, 255.0) as u8;
    (to_u8(r1), to_u8(g1), to_u8(b1))
}

/// Write an already-rendered SVG string to `out_path`, creating parent dirs.
pub fn write_svg(svg: &str, out_path: &str) -> io::Result<()> {
    if let Some(parent) = Path::new(out_path).parent() {
        if !parent.as_os_str().is_empty() {
            fs::create_dir_all(parent)?;
        }
    }
    fs::write(out_path, svg)
}
