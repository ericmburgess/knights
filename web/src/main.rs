//! Minimal web (and native) front-end for the knight placement engine.
//!
//! This first slice is a viewer: pick a radius and a redblack variant, hit **Simulate**,
//! and the resulting board is uploaded as a texture and shown. The visual piece editor
//! and pan/zoom come next. Built with eframe/egui; `trunk serve` compiles it to WASM.

use eframe::egui;
use knights_core::engine::Board;
use knights_core::redblack::{self, simulate_redblack, Variant};

/// Native entry: opens a window.
#[cfg(not(target_arch = "wasm32"))]
fn main() -> eframe::Result {
    let options = eframe::NativeOptions::default();
    eframe::run_native(
        "knights",
        options,
        Box::new(|cc| Ok(Box::new(KnightsApp::new(cc)))),
    )
}

/// Web entry: mounts on the `<canvas id="the_canvas_id">` in index.html.
#[cfg(target_arch = "wasm32")]
fn main() {
    use eframe::wasm_bindgen::JsCast as _;

    eframe::WebLogger::init(log::LevelFilter::Debug).ok();
    let options = eframe::WebOptions::default();
    wasm_bindgen_futures::spawn_local(async {
        let document = web_sys::window()
            .expect("no window")
            .document()
            .expect("no document");
        let canvas = document
            .get_element_by_id("the_canvas_id")
            .expect("missing element id=the_canvas_id")
            .dyn_into::<web_sys::HtmlCanvasElement>()
            .expect("the_canvas_id is not a <canvas>");
        let result = eframe::WebRunner::new()
            .start(
                canvas,
                options,
                Box::new(|cc| Ok(Box::new(KnightsApp::new(cc)))),
            )
            .await;
        if let Err(e) = result {
            log::error!("eframe failed to start: {e:?}");
        }
    });
}

struct KnightsApp {
    radius: i32,
    variant: Variant,
    /// The last simulated board, uploaded as a texture for display.
    board: Option<egui::TextureHandle>,
    status: String,
}

impl KnightsApp {
    fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self {
            radius: 80,
            variant: Variant::Canonical,
            board: None,
            status: "Pick a radius and variant, then Simulate.".to_owned(),
        }
    }

    /// Run the engine and upload the occupancy grid as a nearest-neighbor texture.
    fn simulate(&mut self, ctx: &egui::Context) {
        let result = simulate_redblack(self.radius, self.variant);
        let palette = result.palette();
        let n = (2 * self.radius + 1) as usize;
        let mut pixels = Vec::with_capacity(n * n);
        // egui images are top-left origin (y down); our board's +y is up, so emit rows
        // from the top (y = +radius) downward.
        for y in (-self.radius..=self.radius).rev() {
            for x in -self.radius..=self.radius {
                let (r, g, b) = palette[result.cell(x, y) as usize];
                pixels.push(egui::Color32::from_rgb(r, g, b));
            }
        }
        let image = egui::ColorImage { size: [n, n], pixels };
        self.board = Some(ctx.load_texture("board", image, egui::TextureOptions::NEAREST));

        let breakdown = result
            .teams()
            .iter()
            .map(|&c| format!("{} {}", result.count(c), redblack::color_name(c)))
            .collect::<Vec<_>>()
            .join(", ");
        let empty = result.squares_considered - result.placed();
        self.status = format!(
            "{} variant, radius {}: {} placed ({}), {} empty.",
            self.variant.name(),
            result.radius,
            result.placed(),
            breakdown,
            empty
        );
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
                egui::ComboBox::from_id_salt("variant")
                    .selected_text(self.variant.name())
                    .show_ui(ui, |ui| {
                        for v in [Variant::Canonical, Variant::Rot180, Variant::Mirror, Variant::Quad] {
                            ui.selectable_value(&mut self.variant, v, v.name());
                        }
                    });
                if ui.button("Simulate").clicked() {
                    self.simulate(ctx);
                }
            });
            ui.label(&self.status);
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            match &self.board {
                Some(texture) => {
                    let sized = egui::load::SizedTexture::from_handle(texture);
                    ui.add(egui::Image::new(sized).shrink_to_fit());
                }
                None => {
                    ui.centered_and_justified(|ui| ui.label("No board yet — press Simulate."));
                }
            }
        });
    }
}
