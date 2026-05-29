//! Core of the knight-spiral visualizations: spiral coordinates, pieces, the general
//! placement engine, TOML config, and PNG rasterization.
//!
//! Everything here is pure computation plus byte output — no terminal or argument
//! handling — so it backs both the CLI and the (WASM) web front-end. See each module's
//! doc-comment for its design; `KNIGHTS.md` has the problem statements and oracles.

pub mod config;
pub mod courteous;
pub mod engine;
pub mod knight;
pub mod piece;
pub mod raster;
pub mod redblack;
pub mod spiral;
