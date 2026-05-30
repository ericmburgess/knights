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
use knights_core::share::{self, PieceRef, ShareConfig, SharePiece, ShareType};
use knights_core::spiral::{Direction, Handedness};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use web_time::Instant;

/// What we persist to browser localStorage (via eframe): the user's custom piece types
/// (built-ins are code-defined) and their named saved configs.
#[derive(Default, Serialize, Deserialize)]
struct Persisted {
    #[serde(default)]
    custom_types: Vec<StoredType>,
    #[serde(default)]
    saved: Vec<SavedConfig>,
    /// The exact last working board (pieces + radius + custom types) to reopen into.
    #[serde(default)]
    session: Option<ShareConfig>,
    #[serde(default)]
    selected_type: usize,
}

#[derive(Clone, Serialize, Deserialize)]
struct StoredType {
    name: String,
    offsets: Vec<(i32, i32)>,
}

#[derive(Clone, Serialize, Deserialize)]
struct SavedConfig {
    name: String,
    config: ShareConfig,
}

/// Colors auto-assigned to pieces as they're added, cycled in order.
const AUTO_COLORS: [[u8; 3]; 8] = [
    [26, 26, 26],   // black
    [209, 31, 31],  // red
    [38, 160, 65],  // green
    [242, 201, 33], // yellow
    [40, 103, 222], // blue
    [35, 178, 196], // cyan
    [190, 55, 170], // magenta
    [237, 139, 32], // orange
];

/// Half-width of the offset editor grid (covers leapers reaching up to 4 squares).
const GRID_R: i32 = 5;

/// Native entry: opens a window.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native("knights", options, Box::new(|cc| Ok(Box::new(KnightsApp::new(cc, None)))))
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
        // A "#<code>" fragment means we were opened from a share link — decode it.
        let initial = web_sys::window()
            .and_then(|w| w.location().hash().ok())
            .map(|h| h.trim_start_matches('#').to_string())
            .filter(|s| !s.is_empty())
            .and_then(|code| knights_core::share::decode(&code).ok());
        let result = eframe::WebRunner::new()
            .start(canvas, options, Box::new(move |cc| Ok(Box::new(KnightsApp::new(cc, initial)))))
            .await;
        if let Err(e) = result {
            log::error!("eframe failed to start: {e:?}");
        }
    });
}

/// A piece type being edited: a name and the set of squares it attacks. Built-in
/// (library) types are read-only — duplicate one with "Copy" to customize it.
struct PieceTypeEdit {
    name: String,
    offsets: BTreeSet<(i32, i32)>,
    builtin: bool,
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
    /// Index into `types` whose grid is shown/edited in the Piece types detail pane.
    selected_type: usize,
    pieces: Vec<PieceEdit>,
    radius: i32,
    export_scale: u32,
    board: Option<egui::TextureHandle>,
    status: String,
    // Board view transform.
    zoom: f32,
    pan: egui::Vec2,
    // Persistence + sharing.
    saved: Vec<SavedConfig>,
    save_name: String,
    /// Last generated share link, shown for manual copy as a clipboard fallback.
    share_link: String,
}

impl KnightsApp {
    /// The base state: built-in piece types (read-only) and the canonical red/black setup.
    fn fresh() -> Self {
        let types: Vec<PieceTypeEdit> = library_types()
            .into_iter()
            .map(|(name, offsets)| PieceTypeEdit { name: name.to_owned(), offsets, builtin: true })
            .collect();
        let knight = types.iter().position(|t| t.name == "knight").unwrap_or(0);
        Self {
            selected_type: knight,
            types,
            pieces: vec![
                PieceEdit { type_idx: knight, color: AUTO_COLORS[0], direction: Direction::Right, handed: Handedness::Ccw, label: "Black".to_owned() },
                PieceEdit { type_idx: knight, color: AUTO_COLORS[1], direction: Direction::Right, handed: Handedness::Ccw, label: "Red".to_owned() },
            ],
            radius: 400,
            export_scale: 4,
            board: None,
            status: "Edit pieces, then press Simulate.".to_owned(),
            zoom: 1.0,
            pan: egui::Vec2::ZERO,
            saved: Vec::new(),
            save_name: String::new(),
            share_link: String::new(),
        }
    }

    /// Build the app: start fresh, restore persisted custom types + saved configs, then
    /// (if a share link was opened) apply that config on top.
    fn new(cc: &eframe::CreationContext<'_>, initial: Option<ShareConfig>) -> Self {
        let mut app = Self::fresh();
        if let Some(storage) = cc.storage {
            if let Some(p) = eframe::get_value::<Persisted>(storage, eframe::APP_KEY) {
                for st in p.custom_types {
                    app.types.push(PieceTypeEdit {
                        name: st.name,
                        offsets: st.offsets.into_iter().collect(),
                        builtin: false,
                    });
                }
                app.saved = p.saved;
                // Restore the exact last board (its custom types dedupe against the above).
                if let Some(session) = p.session {
                    app.apply_share(session);
                }
                app.selected_type = p.selected_type.min(app.types.len() - 1);
            }
        }
        // A share link wins over the restored session (you clicked it to view that board).
        if let Some(cfg) = initial {
            app.apply_share(cfg);
            app.status = "Loaded a shared config — press Simulate.".to_owned();
        }
        app
    }

    /// Map the current editor state to a portable [`ShareConfig`].
    fn to_share(&self) -> ShareConfig {
        let pieces = self
            .pieces
            .iter()
            .map(|p| {
                let t = &self.types[p.type_idx];
                let piece = if t.builtin {
                    PieceRef::Builtin(t.name.clone())
                } else {
                    PieceRef::Inline(t.offsets.iter().copied().collect())
                };
                SharePiece {
                    piece,
                    color: p.color,
                    direction: p.direction,
                    orientation: p.handed,
                    label: p.label.clone(),
                }
            })
            .collect();
        // Carry every custom type (placed or not) so names and unplaced pieces travel.
        let custom_types = self
            .types
            .iter()
            .filter(|t| !t.builtin)
            .map(|t| ShareType { name: t.name.clone(), offsets: t.offsets.iter().copied().collect() })
            .collect();
        ShareConfig { version: share::VERSION, radius: self.radius, custom_types, pieces }
    }

    /// Rebuild the pieces (and radius) from a [`ShareConfig`], creating or reusing types.
    fn apply_share(&mut self, cfg: ShareConfig) {
        self.radius = cfg.radius;
        // Bring in the author's custom types (including unplaced ones), keeping their
        // names; dedupe against existing types by offset set.
        for st in &cfg.custom_types {
            let set: BTreeSet<(i32, i32)> = st.offsets.iter().copied().collect();
            if !set.is_empty() && !self.types.iter().any(|t| t.offsets == set) {
                self.types.push(PieceTypeEdit { name: st.name.clone(), offsets: set, builtin: false });
            }
        }
        let pieces: Vec<PieceEdit> = cfg
            .pieces
            .into_iter()
            .map(|sp| PieceEdit {
                type_idx: self.ensure_type(&sp.piece),
                color: sp.color,
                direction: sp.direction,
                handed: sp.orientation,
                label: sp.label,
            })
            .collect();
        if !pieces.is_empty() {
            self.pieces = pieces;
        }
        self.selected_type = self.selected_type.min(self.types.len() - 1);
    }

    /// Resolve a shared piece reference to a type index, adding a custom type if needed.
    fn ensure_type(&mut self, piece: &PieceRef) -> usize {
        match piece {
            // decode() guarantees the name exists; fall back to 0 only defensively.
            PieceRef::Builtin(name) => {
                self.types.iter().position(|t| t.builtin && &t.name == name).unwrap_or(0)
            }
            PieceRef::Inline(offsets) => {
                let set: BTreeSet<(i32, i32)> = offsets.iter().copied().collect();
                if let Some(i) = self.types.iter().position(|t| t.offsets == set) {
                    i
                } else {
                    let n = self.types.iter().filter(|t| !t.builtin).count() + 1;
                    self.types.push(PieceTypeEdit { name: format!("shared {n}"), offsets: set, builtin: false });
                    self.types.len() - 1
                }
            }
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
        let t = Instant::now();
        let result = engine::simulate(self.radius, cfg);
        let elapsed = t.elapsed();
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
        self.pan = egui::Vec2::ZERO; // recenter the fresh board (zoom is left as-is)

        let breakdown = result
            .legend()
            .iter()
            .map(|row| format!("{} {}", row.count, row.label))
            .collect::<Vec<_>>()
            .join(", ");
        let placed: u64 = result.legend().iter().map(|row| row.count).sum();
        self.status = format!(
            "radius {}: {} placed ({}), {} empty. Simulated in {:.1?}.",
            result.radius,
            placed,
            breakdown,
            result.squares_considered - placed,
            elapsed
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

    /// A grid centered on the piece (gray center cell), one cell per relative square.
    /// When `editable`, clicking toggles the attacked square under the cursor; otherwise
    /// it's display-only (built-in types) and drawn in a muted tone.
    fn offset_grid(ui: &mut egui::Ui, offsets: &mut BTreeSet<(i32, i32)>, editable: bool) {
        let cells = (2 * GRID_R + 1) as f32;
        let side = cells * 16.0;
        let sense = if editable { egui::Sense::click() } else { egui::Sense::hover() };
        let (rect, response) = ui.allocate_exact_size(egui::vec2(side, side), sense);
        let painter = ui.painter_at(rect);
        let cs = side / cells;
        let on = if editable {
            egui::Color32::from_rgb(70, 130, 220)
        } else {
            egui::Color32::from_rgb(70, 95, 140)
        };
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
                    on
                } else {
                    egui::Color32::from_gray(40)
                };
                painter.rect_filled(cell.shrink(1.0), 2.0, fill);
            }
        }
        if editable && response.clicked() {
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
        ui.horizontal(|ui| {
            if ui.button("New").on_hover_text("Create a blank editable type").clicked() {
                let n = self.types.iter().filter(|t| !t.builtin).count() + 1;
                self.types.push(PieceTypeEdit { name: format!("custom {n}"), offsets: BTreeSet::new(), builtin: false });
                self.selected_type = self.types.len() - 1;
            }
            if ui.button("Copy").on_hover_text("Duplicate the selected type as an editable copy").clicked() {
                let src = &self.types[self.selected_type];
                let name = format!("{} copy", src.name);
                let offsets = src.offsets.clone();
                self.types.push(PieceTypeEdit { name, offsets, builtin: false });
                self.selected_type = self.types.len() - 1;
            }
        });

        let mut remove_type: Option<usize> = None;
        ui.horizontal_top(|ui| {
            // Left: the list of every defined type; select one to edit it on the right.
            ui.vertical(|ui| {
                for (i, t) in self.types.iter().enumerate() {
                    let label = if t.builtin { format!("🔒 {}", t.name) } else { t.name.clone() };
                    ui.selectable_value(&mut self.selected_type, i, label);
                }
            });
            ui.add_space(12.0);
            // Right: detail (name + grid) for the selected type.
            ui.vertical(|ui| {
                let i = self.selected_type.min(self.types.len() - 1);
                self.selected_type = i;
                let t = &mut self.types[i];
                if t.builtin {
                    ui.horizontal(|ui| {
                        ui.strong(&t.name);
                        ui.label(egui::RichText::new("🔒 built-in").weak());
                    });
                    ui.small("Read-only — press Copy to make an editable version.");
                } else {
                    ui.horizontal(|ui| {
                        ui.label("name");
                        ui.text_edit_singleline(&mut t.name);
                    });
                }
                Self::offset_grid(ui, &mut t.offsets, !t.builtin);
                ui.small(format!("{} attacked squares", t.offsets.len()));
                if !t.builtin && ui.button("🗑 delete").clicked() {
                    remove_type = Some(i);
                }
            });
        });
        if let Some(i) = remove_type {
            self.types.remove(i);
            if self.selected_type >= i && self.selected_type > 0 {
                self.selected_type -= 1;
            }
            for p in &mut self.pieces {
                if p.type_idx == i {
                    p.type_idx = 0;
                } else if p.type_idx > i {
                    p.type_idx -= 1;
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
                    type_idx: self.selected_type,
                    color: AUTO_COLORS[self.pieces.len() % AUTO_COLORS.len()],
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

        ui.separator();
        ui.heading("Saved configs");
        ui.horizontal(|ui| {
            ui.text_edit_singleline(&mut self.save_name);
            if ui.button("Save current").clicked() {
                let name = self.save_name.trim().to_string();
                if !name.is_empty() {
                    let config = self.to_share();
                    self.saved.retain(|s| s.name != name); // overwrite same name
                    self.saved.push(SavedConfig { name, config });
                    self.save_name.clear();
                }
            }
        });
        let mut load_cfg: Option<ShareConfig> = None;
        let mut delete: Option<usize> = None;
        for (i, s) in self.saved.iter().enumerate() {
            ui.horizontal(|ui| {
                ui.label(&s.name);
                if ui.button("Load").clicked() {
                    load_cfg = Some(s.config.clone());
                }
                if ui.button("🗑").clicked() {
                    delete = Some(i);
                }
            });
        }
        if let Some(cfg) = load_cfg {
            self.apply_share(cfg);
        }
        if let Some(i) = delete {
            self.saved.remove(i);
        }
        if !self.share_link.is_empty() {
            ui.add_space(4.0);
            ui.label("Last share link (also copied to clipboard):");
            ui.text_edit_singleline(&mut self.share_link);
        }
    }

    fn board_view(&mut self, ui: &mut egui::Ui) {
        // Use the theme's canvas color so the board panel follows dark/light mode.
        let bg = ui.visuals().extreme_bg_color;
        let hint = ui.visuals().weak_text_color();
        let (rect, response) =
            ui.allocate_exact_size(ui.available_size(), egui::Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 0.0, bg);

        let Some(tex) = &self.board else {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "Press Simulate",
                egui::FontId::proportional(18.0),
                hint,
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
    /// Persist custom piece types + named configs to localStorage (eframe autosaves
    /// periodically and on close).
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        let custom_types = self
            .types
            .iter()
            .filter(|t| !t.builtin)
            .map(|t| StoredType { name: t.name.clone(), offsets: t.offsets.iter().copied().collect() })
            .collect();
        let persisted = Persisted {
            custom_types,
            saved: self.saved.clone(),
            session: Some(self.to_share()),
            selected_type: self.selected_type,
        };
        eframe::set_value(storage, eframe::APP_KEY, &persisted);
    }

    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::TopBottomPanel::top("controls").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.heading("knights");
                ui.separator();
                ui.label("radius");
                for r in [100, 200, 400, 800, 1600] {
                    if ui.selectable_label(self.radius == r, r.to_string()).clicked() {
                        self.radius = r;
                    }
                }
                ui.add(
                    egui::DragValue::new(&mut self.radius)
                        .speed(5)
                        .range(10..=2000),
                )
                .on_hover_text("Manual radius (board display is capped here; the CLI renders bigger)");
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
                ui.separator();
                if ui.button("Copy link").on_hover_text("Copy a shareable link to this config").clicked() {
                    let code = share::encode(&self.to_share());
                    self.share_link = share_link(&code);
                    ui.output_mut(|o| o.copied_text = self.share_link.clone());
                    self.status = format!("Share link copied ({} chars).", self.share_link.len());
                }
            });
            ui.label(&self.status);
        });

        egui::SidePanel::left("editor").resizable(true).default_width(430.0).show(ctx, |ui| {
            egui::ScrollArea::vertical().show(ui, |ui| self.editor_panel(ui));
        });

        egui::CentralPanel::default().show(ctx, |ui| self.board_view(ui));
    }
}

/// The built-in fairy-piece library, as editable piece types.
fn library_types() -> Vec<(&'static str, BTreeSet<(i32, i32)>)> {
    knights_core::piece::library()
        .into_iter()
        .map(|(name, offsets)| (name, offsets.into_iter().collect()))
        .collect()
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

/// Build a shareable link from a base64url config code: a `…/#<code>` URL on the web
/// (opening it reloads the board), or just `#<code>` natively where there's no URL.
#[cfg(target_arch = "wasm32")]
fn share_link(code: &str) -> String {
    let loc = web_sys::window().expect("window").location();
    let origin = loc.origin().unwrap_or_default();
    let path = loc.pathname().unwrap_or_default();
    format!("{origin}{path}#{code}")
}

#[cfg(not(target_arch = "wasm32"))]
fn share_link(code: &str) -> String {
    format!("#{code}")
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A board with a built-in piece, a placed custom piece, and an *unplaced* custom
    /// type survives encode → decode → apply: built-ins resolve by name, placed custom
    /// pieces keep their type name, and the unplaced custom type comes along too.
    #[test]
    fn share_round_trip_preserves_board_and_custom_types() {
        let placed_offsets: BTreeSet<(i32, i32)> = [(2, 2), (-2, -2)].into_iter().collect();
        let unplaced_offsets: BTreeSet<(i32, i32)> = [(3, 0), (-3, 0)].into_iter().collect();

        let mut a = KnightsApp::fresh();
        a.types.push(PieceTypeEdit { name: "placed".to_owned(), offsets: placed_offsets.clone(), builtin: false });
        let placed = a.types.len() - 1;
        a.types.push(PieceTypeEdit { name: "unplaced".to_owned(), offsets: unplaced_offsets.clone(), builtin: false });
        a.radius = 123;
        a.pieces = vec![
            PieceEdit { type_idx: 0, color: [1, 2, 3], direction: Direction::Up, handed: Handedness::Cw, label: "W".to_owned() },
            PieceEdit { type_idx: placed, color: [9, 8, 7], direction: Direction::Left, handed: Handedness::Ccw, label: "C".to_owned() },
        ];

        let code = share::encode(&a.to_share());
        let cfg = share::decode(&code).expect("valid code");

        let mut b = KnightsApp::fresh();
        b.apply_share(cfg);
        assert_eq!(b.radius, 123);
        assert_eq!(b.pieces.len(), 2);

        // Built-in piece resolves by name (type 0 = wazir).
        let t0 = &b.types[b.pieces[0].type_idx];
        assert!(t0.builtin);
        assert_eq!(t0.name, a.types[0].name);
        assert_eq!(b.pieces[0].direction, Direction::Up);
        assert_eq!(b.pieces[0].handed, Handedness::Cw);

        // Placed custom piece keeps its type name and offsets.
        let t1 = &b.types[b.pieces[1].type_idx];
        assert!(!t1.builtin);
        assert_eq!(t1.name, "placed");
        assert_eq!(t1.offsets, placed_offsets);
        assert_eq!(b.pieces[1].label, "C");

        // The unplaced custom type travels along with its name.
        assert!(b
            .types
            .iter()
            .any(|t| !t.builtin && t.name == "unplaced" && t.offsets == unplaced_offsets));
    }
}
