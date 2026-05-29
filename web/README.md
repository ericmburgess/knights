# knights-web

An egui front-end for the knight placement engine, compiled to WASM and served in the
browser. (Also runs as a native window via `cargo run -p knights-web`.)

A visual editor for arbitrary placement games:

- **Piece types** — each a name and a click-grid where you toggle the squares it
  attacks (center = the piece).
- **Pieces** (turn order) — each picks a type, a color, a spiral (direction +
  ccw/cw), and a label; add / remove / reorder them.
- **Simulate** builds an `EngineConfig` from the editor and runs `knights_core`.
- The board view supports **drag to pan** and **scroll to zoom** (Reset view recenters).
- **Export PNG** re-renders at the chosen scale — a browser download on the web, a
  `knights_export.png` file natively.

It opens on the canonical red/black setup so there's something to Simulate immediately.

## Run it in the browser

One-time setup:

```sh
rustup target add wasm32-unknown-unknown
cargo install trunk
```

Then from this directory:

```sh
cd web
trunk serve --open
```

`trunk` compiles `knights-web` to WASM, wires it to `index.html` (which mounts the app
on `<canvas id="the_canvas_id">`), and live-reloads on changes. For a static build to
`web/dist/`, use `trunk build --release`.

## Run it natively (no browser)

```sh
cargo run -p knights-web
```

The native build is handy for quick iteration; the WASM build is the real target.
