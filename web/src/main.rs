//! Web (and native) front-end for the knight placement engine: a visual config editor
//! plus a pan/zoom board view and PNG export.
//!
//! The left panel edits *piece types* (each a set of attacked offsets, toggled on a
//! click-grid) and *pieces* (a type + color + spiral + label). **Simulate** builds an
//! `EngineConfig` from that state and runs `knights_core`; the board is shown as a
//! texture you can drag (pan) and scroll (zoom). **Export PNG** re-renders at a chosen
//! scale — a file natively, a download in the browser. Built with eframe/egui;
//! `trunk serve` compiles it to WASM (see web/README.md).

use eframe::egui;
use knights_core::engine::{self, Board, EngineConfig, PieceSpec};
use knights_core::piece::KindBuilder;
use knights_core::raster;
use knights_core::spiral::{Direction, Handedness};
use std::collections::BTreeSet;

/// Half-width of the offset editor grid (covers leapers reaching up to 4 squares).
const GRID_R: i32 = 4;

/// Native entry: opens a window.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native("knights", options, Box::new(|cc| Ok(Box::new(KnightsApp::new(cc)))))
}

/// Web entry: mounts on the `<canvas id="the_canvas_id">` in index.html.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Debug).ok();
    let options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window().expect("no window").document().expect("no document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("missing element id=the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id is not a <canvas>");
        let result = eframe::WebRunner::new()
            .start(canvas, options, Box::new(|cc| Ok(Box::new(KnightsApp::new(cc)))))
            .await;
        if let Err(e) = result {
            log::error!("eframe failed to start: {e:?}");
        }
    });
}

/// A piece type being edited: a name and the set of squares it attacks.
struct PieceTypeEdit {
    name: String,
    offsets: BTreeSet<(i32, i32)>,
}

/// A piece being edited: which type, its color, spiral, and legend label.
struct PieceEdit {
    type_idx: usize,
    color: [u8; 3],
    direction: Direction,
    handed: Handedness,
    label: String,
}

struct KnightsApp {
    types: Vec<PieceTypeEdit>,
    pieces: Vec<PieceEdit>,
    radius: i32,
    export_scale: u32,
    board: Option<egui::TextureHandle>,
    status: String,
    // Board view transform.
    zoom: f32,
    pan: egui::Vec2,
}

impl KnightsApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        // Open on the canonical red/black setup so there's something to Simulate.
        let knight: BTreeSet<(i32, i32)> =
            [(1, 2), (2, 1), (2, -1), (1, -2), (-1, -2), (-2, -1), (-2, 1), (-1, 2)]
                .into_iter()
                .collect();
        Self {
            types: vec![PieceTypeEdit { name: "knight".to_owned(), offsets: knight }],
            pieces: vec![
                PieceEdit { type_idx: 0, color: [26, 26, 26], direction: Direction::Right, handed: Handedness::Ccw, label: "Black".to_owned() },
                PieceEdit { type_idx: 0, color: [209, 31, 31], direction: Direction::Right, handed: Handedness::Ccw, label: "Red".to_owned() },
            ],
            radius: 80,
            export_scale: 4,
            board: None,
            status: "Edit pieces, then press Simulate.".to_owned(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
        }
    }

    /// Build an `EngineConfig` from the editor state, or a message describing what's wrong.
    fn build_config(&self) -> Result<EngineConfig, String> {
        if self.pieces.is_empty() {
            return Err("add at least one piece".to_owned());
        }
        let mut kinds = KindBuilder::new();
        let mut specs = Vec::with_capacity(self.pieces.len());
        for p in &self.pieces {
            let t = self.types.get(p.type_idx).ok_or("a piece references a missing type")?;
            if t.offsets.is_empty() {
                return Err(format!("piece type '{}' has no attack squares", t.name));
            }
            let offsets: Vec<(i32, i32)> = t.offsets.iter().copied().collect();
            let label = if p.label.is_empty() { t.name.clone() } else { p.label.clone() };
            let color = (p.color[0], p.color[1], p.color[2]);
            let kind = kinds
                .intern(offsets, color, &label)
                .ok_or("too many distinct (type, color) combinations (max 255)")?;
            specs.push(PieceSpec { kind, direction: p.direction, handed: p.handed });
        }
        Ok(EngineConfig { pieces: specs, kinds: kinds.finish() })
    }

    /// Run the engine and upload the board as a texture; update the status line.
    fn simulate(&mut self, ctx: &egui::Context) {
        let cfg = match self.build_config() {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("⚠ {e}");
                return;
            }
        };
        let result = engine::simulate(self.radius, cfg);
        let palette = result.palette();
        let n = (2 * self.radius + 1) as usize;
        let mut pixels = Vec::with_capacity(n * n);
        for y in (-self.radius..=self.radius).rev() {
            for x in -self.radius..=self.radius {
                let (r, g, b) = palette[result.cell(x, y) as usize];
                pixels.push(egui::Color32::from_rgb(r, g, b));
            }
        }
        let image = egui::ColorImage { size: [n, n], pixels };
        self.board = Some(ctx.load_texture("board", image, egui::TextureOptions::NEAREST));

        let breakdown = result
            .legend()
            .iter()
            .map(|row| format!("{} {}", row.count, row.label))
            .collect::<Vec<_>>()
            .join(", ");
        let placed: u64 = result.legend().iter().map(|row| row.count).sum();
        self.status = format!(
            "radius {}: {} placed ({}), {} empty.",
            result.radius,
            placed,
            breakdown,
            result.squares_considered - placed
        );
    }

    /// Re-render at the export scale and hand the PNG off (file / download).
    fn export(&mut self) {
        let cfg = match self.build_config() {
            Ok(c) => c,
            Err(e) => {
                self.status = format!("⚠ {e}");
                return;
            }
        };
        let result = engine::simulate(self.radius, cfg);
        let bytes = raster::board_png_bytes(&result, self.export_scale);
        self.deliver_png(&bytes);
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn deliver_png(&mut self, bytes: &[u8]) {
        let path = "knights_export.png";
        self.status = match std::fs::write(path, bytes) {
            Ok(()) => format!("Wrote {path} ({} bytes).", bytes.len()),
            Err(e) => format!("⚠ failed to write {path}: {e}"),
        };
    }

    #[cfg(target_arch = "wasm32")]
    fn deliver_png(&mut self, bytes: &[u8]) {
        self.status = match download_bytes(bytes, "knights.png") {
            Ok(()) => "Downloaded knights.png.".to_owned(),
            Err(_) => "⚠ download failed".to_owned(),
        };
    }

    /// A grid centered on the piece (gray center cell); click to toggle attacked squares.
    fn offset_grid(ui: &mut egui::Ui, offsets: &mut BTreeSet<(i32, i32)>) {
        let cells = (2 * GRID_R + 1) as f32;
        let side = 170.0;
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), egui::Sense::click());
        let painter = ui.painter_at(rect);
        let cs = side / cells;
        for gy in -GRID_R..=GRID_R {
            for gx in -GRID_R..=GRID_R {
                let col = (gx + GRID_R) as f32;
                let row = (GRID_R - gy) as f32; // +y is up
                let cell = egui::Rect::from_min_size(
                    rect.min + egui::vec2(col * cs, row * cs),
                    egui::vec2(cs, cs),
                );
                let fill = if gx == 0 && gy == 0 {
                    egui::Color32::from_gray(110)
                } else if offsets.contains(&(gx, gy)) {
                    egui::Color32::from_rgb(70, 130, 220)
                } else {
                    egui::Color32::from_gray(40)
                };
                painter.rect_filled(cell.shrink(1.0), 2.0, fill);
            }
        }
        if response.clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let local = pos - rect.min;
                let gx = (local.x / cs).floor() as i32 - GRID_R;
                let gy = GRID_R - (local.y / cs).floor() as i32;
                if (gx, gy) != (0, 0) && gx.abs() <= GRID_R && gy.abs() <= GRID_R {
                    if !offsets.remove(&(gx, gy)) {
                        offsets.insert((gx, gy));
                    }
                }
            }
        }
    }

    fn editor_panel(&mut self, ui: &mut egui::Ui) {
        ui.heading("Piece types");
        let mut remove_type: Option<usize> = None;
        let type_count = self.types.len();
        for (i, t) in self.types.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.label("name");
                    ui.text_edit_singleline(&mut t.name);
                    if type_count > 1 && ui.button("🗑").clicked() {
                        remove_type = Some(i);
                    }
                });
                Self::offset_grid(ui, &mut t.offsets);
                ui.small(format!("{} attacked squares", t.offsets.len()));
            });
        }
        if ui.button("+ piece type").clicked() {
            let name = format!("type{}", self.types.len() + 1);
            self.types.push(PieceTypeEdit { name, offsets: BTreeSet::new() });
        }
        if let Some(i) = remove_type {
            if self.types.len() > 1 {
                self.types.remove(i);
                for p in &mut self.pieces {
                    if p.type_idx == i {
                        p.type_idx = 0;
                    } else if p.type_idx > i {
                        p.type_idx -= 1;
                    }
                }
            }
        }

        ui.separator();
        ui.horizontal(|ui| {
            ui.heading("Pieces (turn order)");
            ui.label(egui::RichText::new("(i)").color(egui::Color32::from_rgb(90, 140, 220)))
                .on_hover_text(
                    "Each piece scans its own square spiral.\n\
                     - direction: the first arm's heading (right/up/left/down).\n\
                     - orientation: turn sense as it grows. ccw cycles right, up, left, down; \
                     cw cycles right, down, left, up.\n\
                     Pieces play in list order; the first seeds the center.",
                );
        });
        let type_names: Vec<String> = self.types.iter().map(|t| t.name.clone()).collect();
        let mut remove_piece: Option<usize> = None;
        let mut move_up: Option<usize> = None;
        let count = self.pieces.len();
        for (i, p) in self.pieces.iter_mut().enumerate() {
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.color_edit_button_srgb(&mut p.color);
                    ui.text_edit_singleline(&mut p.label);
                    if i > 0
                        && ui.button("⬆").on_hover_text("Move up (earlier in turn order)").clicked()
                    {
                        move_up = Some(i);
                    }
                    if count > 1 && ui.button("🗑").clicked() {
                        remove_piece = Some(i);
                    }
                });
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt(("type", i))
                        .selected_text(type_names.get(p.type_idx).map(String::as_str).unwrap_or("?"))
                        .show_ui(ui, |ui| {
                            for (ti, name) in type_names.iter().enumerate() {
                                ui.selectable_value(&mut p.type_idx, ti, name);
                            }
                        });
                    egui::ComboBox::from_id_salt(("dir", i))
                        .selected_text(dir_name(p.direction))
                        .show_ui(ui, |ui| {
                            for d in [Direction::Right, Direction::Up, Direction::Left, Direction::Down] {
                                ui.selectable_value(&mut p.direction, d, dir_name(d));
                            }
                        });
                    egui::ComboBox::from_id_salt(("hand", i))
                        .selected_text(hand_name(p.handed))
                        .show_ui(ui, |ui| {
                            for h in [Handedness::Ccw, Handedness::Cw] {
                                ui.selectable_value(&mut p.handed, h, hand_name(h));
                            }
                        });
                });
            });
        }
        ui.horizontal(|ui| {
            if ui.button("+ piece").clicked() {
                self.pieces.push(PieceEdit {
                    type_idx: 0,
                    color: [38, 160, 65],
                    direction: Direction::Right,
                    handed: Handedness::Ccw,
                    label: String::new(),
                });
            }
        });
        if let Some(i) = move_up {
            self.pieces.swap(i - 1, i);
        }
        if let Some(i) = remove_piece {
            if self.pieces.len() > 1 {
                self.pieces.remove(i);
            }
        }
    }

    fn board_view(&mut self, ui: &mut egui::Ui) {
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, egui::Color32::from_gray(247));

        let Some(tex) = &self.board else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Press Simulate",
                egui::FontId::proportional(18.0),
                egui::Color32::GRAY,
            );
            return;
        };

        if response.dragged() {
            self.pan += response.drag_delta();
        }
        let scroll = ui.input(|i| i.raw_scroll_delta.y);
        if scroll != 0.0 && response.hovered() {
            let factor = (scroll * 0.0015).exp();
            if let Some(p) = response.hover_pos() {
                // Keep the point under the cursor fixed while zooming.
                let center = rect.center() + self.pan;
                self.pan -= (p - center) * (factor - 1.0);
            }
            self.zoom = (self.zoom * factor).clamp(0.05, 60.0);
        }

        let size = tex.size_vec2();
        let fit = (rect.width() / size.x).min(rect.height() / size.y);
        let draw = size * fit * self.zoom;
        let img_rect = egui::Rect::from_center_size(rect.center() + self.pan, draw);
        let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
        painter.image(tex.id(), img_rect, uv, egui::Color32::WHITE);
    }
}

impl eframe::App for KnightsApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("knights");
                ui.separator();
                ui.label("radius");
                ui.add(egui::Slider::new(&mut self.radius, 10..=400));
                if ui.button("Simulate").clicked() {
                    self.simulate(ctx);
                }
                ui.separator();
                ui.label("export ×");
                ui.add(egui::DragValue::new(&mut self.export_scale).range(1..=20));
                if ui.button("Export PNG").clicked() {
                    self.export();
                }
                if ui.button("Reset view").clicked() {
                    self.zoom = 1.0;
                    self.pan = egui::Vec2::ZERO;
                }
            });
            ui.label(&self.status);
        });

        egui::SidePanel::left("editor").resizable(true).default_width(290.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.editor_panel(ui));
        });

        egui::CentralPanel::default().show(ctx, |ui| self.board_view(ui));
    }
}

fn dir_name(d: Direction) -> &'static str {
    match d {
        Direction::Right => "right",
        Direction::Up => "up",
        Direction::Left => "left",
        Direction::Down => "down",
    }
}

fn hand_name(h: Handedness) -> &'static str {
    match h {
        Handedness::Ccw => "ccw",
        Handedness::Cw => "cw",
    }
}

/// Trigger a browser download of `bytes` as `filename`.
#[cfg(target_arch = "wasm32")]
fn download_bytes(bytes: &[u8], filename: &str) -> Result<(), eframe::wasm_bindgen::JsValue> {
    use eframe::wasm_bindgen::{JsCast as _, JsValue};

    let array = js_sys::Uint8Array::new_with_length(bytes.len() as u32);
    array.copy_from(bytes);
    let parts = js_sys::Array::of1(&array);
    let blob = web_sys::Blob::new_with_u8_array_sequence(&parts)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob)?;
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or_else(|| JsValue::from_str("no document"))?;
    let anchor = document
        .create_element("a")?
        .dyn_into::<web_sys::HtmlAnchorElement>()
        .map_err(|_| JsValue::from_str("not an anchor"))?;
    anchor.set_href(&url);
    anchor.set_download(filename);
    anchor.click();
    web_sys::Url::revoke_object_url(&url)?;
    Ok(())
}
