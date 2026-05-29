//! Hand-rolled indexed-color PNG output, std-only, streaming.
//!
//! Cells carry a small *code* that is also a palette index, so the encoder picks
//! the minimal PNG bit depth (1/2/4/8) from the palette size — nothing assumes a
//! particular number of piece types. The encoder is row-streaming: it pulls one
//! scanline at a time from a callback, frames the bytes into DEFLATE *stored*
//! blocks, and batches those into bounded IDAT chunks. So memory is O(row) no
//! matter how large the image, and red/black can stream directly out of its
//! occupancy grid with no intermediate copy.

use crate::courteous::CourteousResult;
use crate::engine::Board;
use crate::redblack::RedBlackResult;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::sync::OnceLock;

/// A small image held in full (one palette index per pixel). Used by courteous,
/// whose cluster coloring isn't a simple grid; red/black streams without this.
pub struct IndexedImage {
    pub width: u32,
    pub height: u32,
    pub palette: Vec<(u8, u8, u8)>,
    pub pixels: Vec<u8>,
}

/// Rasterize Courteous Knights at `scale` pixels per cell, colored by cluster size.
pub fn courteous_image(result: &CourteousResult, scale: u32) -> IndexedImage {
    // 0 = empty, then cluster sizes 1,2,3,4,5,6+ (matches render::cluster_color).
    let palette = vec![
        (255, 255, 255),
        (158, 158, 158),
        (46, 125, 50),
        (239, 108, 0),
        (21, 101, 192),
        (198, 40, 40),
        (106, 27, 154),
    ];
    let r = result.radius;
    let dim = (2 * r + 1) as u32 * scale;
    let mut pixels = vec![0u8; (dim as usize) * (dim as usize)];
    for (i, &(_, x, y)) in result.knights.iter().enumerate() {
        let size = result.cluster_sizes[result.cluster_of[i]];
        let idx = size.min(6) as u8;
        let col = (x + r) as u32 * scale;
        let row = (r - y) as u32 * scale;
        for dy in 0..scale {
            let start = (row + dy) as usize * dim as usize + col as usize;
            for px in &mut pixels[start..start + scale as usize] {
                *px = idx;
            }
        }
    }
    IndexedImage {
        width: dim,
        height: dim,
        palette,
        pixels,
    }
}

/// Write a fully-materialized image (courteous).
pub fn write_indexed(path: &str, img: &IndexedImage) -> io::Result<()> {
    let w_px = img.width as usize;
    let mut w = BufWriter::new(File::create(path)?);
    encode(&mut w, img.width, img.height, &img.palette, |y, out| {
        out.extend_from_slice(&img.pixels[y as usize * w_px..][..w_px]);
    })?;
    w.flush()
}

/// Encode any [`Board`] to PNG bytes at `scale` px/cell. Buffers the whole file in
/// memory — fine for interactive/export sizes; the CLI's huge renders stream instead
/// via [`write_board_png`].
pub fn board_png_bytes(board: &dyn Board, scale: u32) -> Vec<u8> {
    let mut buf = Vec::new();
    encode_board(&mut buf, board, scale).expect("writing to a Vec is infallible");
    buf
}

/// Write any [`Board`] straight from its occupancy grid at `scale` px/cell, streaming
/// one scanline at a time (no intermediate image buffer).
pub fn write_board_png(path: &str, board: &dyn Board, scale: u32) -> io::Result<()> {
    let mut w = BufWriter::new(File::create(path)?);
    encode_board(&mut w, board, scale)?;
    w.flush()
}

/// Shared body: encode `board` as a `scale`-px/cell indexed PNG into `w`.
fn encode_board<W: Write>(w: &mut W, board: &dyn Board, scale: u32) -> io::Result<()> {
    let palette = board.palette();
    let r = board.radius();
    let dim = (2 * r + 1) as u32 * scale;
    encode(w, dim, dim, &palette, |oy, out| {
        let world_y = r - (oy / scale) as i32; // image y points down; flip
        for ox in 0..dim {
            let world_x = (ox / scale) as i32 - r;
            out.push(board.cell(world_x, world_y)); // byte == palette index
        }
    })
}

/// Write Red & Black (or Quad) — a thin alias over [`write_board_png`].
pub fn write_redblack_png(path: &str, result: &RedBlackResult, scale: u32) -> io::Result<()> {
    write_board_png(path, result, scale)
}

const SIGNATURE: [u8; 8] = [137, 80, 78, 71, 13, 10, 26, 10];

/// Smallest PNG bit depth (1/2/4/8) that can index `palette_len` colors.
fn depth_for(palette_len: usize) -> u8 {
    match palette_len {
        0..=2 => 1,
        3..=4 => 2,
        5..=16 => 4,
        _ => 8,
    }
}

/// Encode an indexed PNG, pulling each row from `fill_row` (which appends exactly
/// `width` palette indices). O(row) memory regardless of image size.
fn encode<W: Write>(
    w: &mut W,
    width: u32,
    height: u32,
    palette: &[(u8, u8, u8)],
    mut fill_row: impl FnMut(u32, &mut Vec<u8>),
) -> io::Result<()> {
    let depth = depth_for(palette.len());

    w.write_all(&SIGNATURE)?;

    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[depth, 3, 0, 0, 0]); // bit depth, color type 3 (indexed), rest default
    write_chunk(w, b"IHDR", &ihdr)?;

    let mut plte = Vec::with_capacity(palette.len() * 3);
    for &(r, g, b) in palette {
        plte.extend_from_slice(&[r, g, b]);
    }
    write_chunk(w, b"PLTE", &plte)?;

    let row_bytes = (width as usize * depth as usize + 7) / 8;
    let mut indices: Vec<u8> = Vec::with_capacity(width as usize);
    let mut scan: Vec<u8> = Vec::with_capacity(1 + row_bytes);
    let mut idat = IdatStream::new(w);
    idat.emit(&[0x78, 0x01])?; // zlib header (not part of the raw data / Adler)
    for y in 0..height {
        indices.clear();
        fill_row(y, &mut indices);
        scan.clear();
        scan.push(0); // filter type 0 (none)
        pack_row(&indices, depth, &mut scan);
        idat.feed(&scan)?;
    }
    idat.finish()?;

    write_chunk(w, b"IEND", &[])
}

/// Pack palette indices into a scanline (MSB-first within each byte), appending
/// to `out`. Depth 8 is a straight copy.
fn pack_row(indices: &[u8], depth: u8, out: &mut Vec<u8>) {
    if depth == 8 {
        out.extend_from_slice(indices);
        return;
    }
    let mask = (1u8 << depth) - 1;
    let per_byte = 8 / depth as usize;
    let mut byte = 0u8;
    let mut count = 0usize;
    for &idx in indices {
        byte |= (idx & mask) << (8 - depth as usize * (count + 1));
        count += 1;
        if count == per_byte {
            out.push(byte);
            byte = 0;
            count = 0;
        }
    }
    if count > 0 {
        out.push(byte);
    }
}

/// Streams a zlib/DEFLATE-stored data stream into one or more IDAT chunks.
/// Holds at most one stored block plus one IDAT chunk worth of bytes.
struct IdatStream<'a, W: Write> {
    w: &'a mut W,
    chunk: Vec<u8>, // buffered IDAT payload, flushed at CHUNK_CAP
    block: Vec<u8>, // raw bytes for the current stored block (<= BLOCK_CAP)
    adler_a: u32,
    adler_b: u32,
}

const CHUNK_CAP: usize = 1 << 20; // 1 MiB IDAT chunks
const BLOCK_CAP: usize = 65535; // max DEFLATE stored block
const ADLER_MOD: u32 = 65521;

impl<'a, W: Write> IdatStream<'a, W> {
    fn new(w: &'a mut W) -> Self {
        IdatStream {
            w,
            chunk: Vec::with_capacity(CHUNK_CAP + BLOCK_CAP),
            block: Vec::with_capacity(BLOCK_CAP),
            adler_a: 1,
            adler_b: 0,
        }
    }

    /// Append zlib-stream bytes, flushing whole IDAT chunks as they fill.
    fn emit(&mut self, bytes: &[u8]) -> io::Result<()> {
        self.chunk.extend_from_slice(bytes);
        while self.chunk.len() >= CHUNK_CAP {
            let rest = self.chunk.split_off(CHUNK_CAP);
            write_chunk(self.w, b"IDAT", &self.chunk)?;
            self.chunk = rest;
        }
        Ok(())
    }

    /// Feed raw (filtered) image bytes: update Adler-32 and frame into stored blocks.
    fn feed(&mut self, data: &[u8]) -> io::Result<()> {
        for window in data.chunks(5552) {
            for &b in window {
                self.adler_a += b as u32;
                self.adler_b += self.adler_a;
            }
            self.adler_a %= ADLER_MOD;
            self.adler_b %= ADLER_MOD;
        }
        let mut i = 0;
        while i < data.len() {
            let take = (BLOCK_CAP - self.block.len()).min(data.len() - i);
            self.block.extend_from_slice(&data[i..i + take]);
            i += take;
            if self.block.len() == BLOCK_CAP {
                self.flush_block(false)?;
            }
        }
        Ok(())
    }

    fn flush_block(&mut self, final_block: bool) -> io::Result<()> {
        let len = self.block.len() as u16;
        self.emit(&[final_block as u8])?; // BFINAL bit, BTYPE = 00 (stored)
        self.emit(&len.to_le_bytes())?;
        self.emit(&(!len).to_le_bytes())?;
        let data = std::mem::take(&mut self.block);
        self.emit(&data)?;
        self.block = data;
        self.block.clear();
        Ok(())
    }

    /// Emit the final (possibly empty) block, the Adler-32 trailer, and flush.
    fn finish(mut self) -> io::Result<()> {
        self.flush_block(true)?;
        let adler = (self.adler_b << 16) | self.adler_a;
        self.emit(&adler.to_be_bytes())?;
        if !self.chunk.is_empty() {
            let chunk = std::mem::take(&mut self.chunk);
            write_chunk(self.w, b"IDAT", &chunk)?;
        }
        Ok(())
    }
}

fn write_chunk<W: Write>(w: &mut W, kind: &[u8; 4], data: &[u8]) -> io::Result<()> {
    w.write_all(&(data.len() as u32).to_be_bytes())?;
    w.write_all(kind)?;
    w.write_all(data)?;
    w.write_all(&crc32(kind, data).to_be_bytes())
}

fn crc_table() -> &'static [u32; 256] {
    static TABLE: OnceLock<[u32; 256]> = OnceLock::new();
    TABLE.get_or_init(|| {
        let mut t = [0u32; 256];
        for (n, slot) in t.iter_mut().enumerate() {
            let mut c = n as u32;
            for _ in 0..8 {
                c = if c & 1 != 0 { 0xEDB8_8320 ^ (c >> 1) } else { c >> 1 };
            }
            *slot = c;
        }
        t
    })
}

fn crc32(kind: &[u8], data: &[u8]) -> u32 {
    let table = crc_table();
    let mut crc = 0xFFFF_FFFFu32;
    for &b in kind.iter().chain(data) {
        crc = table[((crc ^ b as u32) & 0xFF) as usize] ^ (crc >> 8);
    }
    crc ^ 0xFFFF_FFFF
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packs_indices_msb_first() {
        // depth 2, indices [1, 2] -> 0b01_10_00_00 = 0x60.
        let mut out = Vec::new();
        pack_row(&[1, 2], 2, &mut out);
        assert_eq!(out, vec![0b0110_0000]);
    }

    #[test]
    fn crc32_of_iend_is_known() {
        assert_eq!(crc32(b"IEND", &[]), 0xAE42_6082);
    }

    /// Encode a multi-stored-block image and verify the byte structure: signature,
    /// every chunk's CRC, IHDR dimensions, and an IEND terminator.
    #[test]
    fn produces_structurally_valid_png() {
        // 600x600, depth 2 -> ~90 KB of raw data, spanning two 65535-byte blocks.
        let (w, h) = (600u32, 600u32);
        let palette = vec![(0, 0, 0), (255, 0, 0), (0, 0, 255)];
        let mut buf = Vec::new();
        encode(&mut buf, w, h, &palette, |y, out| {
            for x in 0..w {
                out.push(((x + y) % 3) as u8);
            }
        })
        .unwrap();

        assert_eq!(&buf[..8], &SIGNATURE);
        let mut i = 8;
        let mut saw_ihdr = false;
        let mut saw_iend = false;
        while i < buf.len() {
            let len = u32::from_be_bytes(buf[i..i + 4].try_into().unwrap()) as usize;
            let kind: [u8; 4] = buf[i + 4..i + 8].try_into().unwrap();
            let data = &buf[i + 8..i + 8 + len];
            let crc = u32::from_be_bytes(buf[i + 8 + len..i + 12 + len].try_into().unwrap());
            assert_eq!(crc, crc32(&kind, data), "bad CRC for {:?}", kind);
            if &kind == b"IHDR" {
                assert_eq!(u32::from_be_bytes(data[0..4].try_into().unwrap()), w);
                assert_eq!(u32::from_be_bytes(data[4..8].try_into().unwrap()), h);
                saw_ihdr = true;
            }
            if &kind == b"IEND" {
                saw_iend = true;
            }
            i += 12 + len;
        }
        assert!(saw_ihdr && saw_iend);
        assert_eq!(i, buf.len(), "trailing bytes after IEND");
    }
}
