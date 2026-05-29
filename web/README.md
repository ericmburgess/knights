# knights-web

An egui front-end for the knight placement engine, compiled to WASM and served in the
browser. (Also runs as a native window via `cargo run -p knights-web`.)

This first slice is a **viewer**: choose a radius and a redblack variant, press
**Simulate**, and the board renders. The visual piece editor, pan/zoom, and PNG export
come next.

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
