# knights

[![Deploy](https://github.com/ericmburgess/knights/actions/workflows/deploy.yml/badge.svg)](https://github.com/ericmburgess/knights/actions/workflows/deploy.yml)

Chess-knight problems on a numbered square-spiral grid (from a [Numberphile
video](https://www.youtube.com/watch?v=UiX4CFIiegM)), generalized into a configurable
placement engine — define your own pieces by the squares they attack, give each a
spiral and a color, and watch the territories emerge.

### ▶ [Live demo](https://ericmburgess.github.io/knights/)

An in-browser visual editor (Rust → WebAssembly, no backend): build piece types on a
click-grid, add colored pieces on their own spirals, **Simulate**, pan/zoom, and export
a PNG.

## What's here

- **Three classic problems** (see [`KNIGHTS.md`](KNIGHTS.md) for statements and the
  validated OEIS outcomes): the trapped knight (A316667), courteous knights (A308885),
  and red & black knights (A392177/A392178).
- **A general placement engine** the multi-color games are presets of — arbitrary
  finite-offset pieces, any number of teams, each on one of 8 spiral orientations.
- **Variants** of red & black: `rot180` and `mirror` (reorient Red's spiral) and `quad`
  (four colors).

## Layout

A Cargo workspace:

- `core/` — `knights_core`, the engine library (spiral, pieces, placement engine, TOML
  config, PNG). Pure compute + bytes, so it runs on the CLI and in the browser.
- `cli/` — headless / large-radius PNG renders.
- `web/` — the egui→WASM editor ([`web/README.md`](web/README.md) to run it locally).

## Quick start

```sh
# Reproduce the Numberphile red & black board:
cargo run --release -- redblack --radius 600 --format png

# Run an arbitrary game from a TOML config:
cargo run -- custom --config examples/mixed-pieces.toml

# The interactive editor in a browser:
cd web && trunk serve --open      # needs: rustup target add wasm32-unknown-unknown; cargo install trunk
```

`cargo test` runs the whole workspace, including the OEIS verification oracles.
