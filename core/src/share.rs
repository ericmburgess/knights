//! Compact, backend-free sharing of a placement config as a base64url string.
//!
//! A [`ShareConfig`] is the portable form of a board setup: the pieces (each a built-in
//! piece *name* or inline attack offsets, plus color and spiral), the radius, and a
//! format `version`. [`encode`] serializes it to JSON and base64url-encodes that;
//! [`decode`] reverses it. The string is meant to live in a URL fragment, so the
//! receiver reconstructs the board entirely client-side — no server involved.
//!
//! **Stability contract.** Built-in pieces are referenced by their stable name (see
//! [`crate::piece::library`]), never by index: the library is append-only and a name's
//! offsets never change, so old codes keep decoding to the same board. The leading
//! `version` lets the format evolve without silently misreading old codes.

use crate::spiral::{Direction, Handedness};
use base64::Engine as _;
use serde::{Deserialize, Serialize};

/// Current encoding version. Bump when the on-the-wire shape changes incompatibly.
pub const VERSION: u8 = 1;

const B64: base64::engine::general_purpose::GeneralPurpose =
    base64::engine::general_purpose::URL_SAFE_NO_PAD;

/// A piece's type within a share code: either a built-in by name, or inline offsets.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PieceRef {
    /// A built-in piece, referenced by its stable [`library`](crate::piece::library) name.
    Builtin(String),
    /// A custom piece, its attack offsets carried inline.
    Inline(Vec<(i32, i32)>),
}

/// One piece in a shared config.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SharePiece {
    pub piece: PieceRef,
    pub color: [u8; 3],
    pub direction: Direction,
    pub orientation: Handedness,
    pub label: String,
}

/// A custom (non-built-in) piece type carried in a share code: a name + its offsets.
/// Lets a shared config bring along the author's custom pieces — including ones not
/// placed on the board — with their names intact.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareType {
    pub name: String,
    pub offsets: Vec<(i32, i32)>,
}

/// The portable form of a board setup.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ShareConfig {
    pub version: u8,
    pub radius: i32,
    /// The author's custom piece types (placed or not). `serde(default)` keeps older
    /// codes that predate this field decodable.
    #[serde(default)]
    pub custom_types: Vec<ShareType>,
    pub pieces: Vec<SharePiece>,
}

/// What can go wrong decoding a share string.
#[derive(Debug)]
pub enum ShareError {
    Base64,
    Json(serde_json::Error),
    /// The code's version isn't one this build understands.
    Version(u8),
    /// A built-in name in the code isn't in the library.
    UnknownPiece(String),
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ShareError::Base64 => write!(f, "not a valid share code (bad base64)"),
            ShareError::Json(e) => write!(f, "corrupt share code: {e}"),
            ShareError::Version(v) => write!(f, "unsupported share-code version {v} (this build reads v{VERSION})"),
            ShareError::UnknownPiece(name) => write!(f, "share code references unknown built-in piece '{name}'"),
        }
    }
}

impl std::error::Error for ShareError {}

/// Encode a config to a base64url string (no padding), suitable for a URL fragment.
pub fn encode(config: &ShareConfig) -> String {
    let json = serde_json::to_vec(config).expect("ShareConfig is always serializable");
    B64.encode(json)
}

/// Decode and validate a base64url share string.
pub fn decode(code: &str) -> Result<ShareConfig, ShareError> {
    let bytes = B64.decode(code.trim()).map_err(|_| ShareError::Base64)?;
    let config: ShareConfig = serde_json::from_slice(&bytes).map_err(ShareError::Json)?;
    if config.version != VERSION {
        return Err(ShareError::Version(config.version));
    }
    // Every referenced built-in must exist (keeps the stability contract honest).
    let library = crate::piece::library();
    for p in &config.pieces {
        if let PieceRef::Builtin(name) = &p.piece {
            if !library.iter().any(|(n, _)| n == name) {
                return Err(ShareError::UnknownPiece(name.clone()));
            }
        }
    }
    Ok(config)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample() -> ShareConfig {
        ShareConfig {
            version: VERSION,
            radius: 200,
            custom_types: vec![ShareType {
                name: "myleaper".into(),
                offsets: vec![(2, 2), (-2, -2)],
            }],
            pieces: vec![
                SharePiece {
                    piece: PieceRef::Builtin("knight".into()),
                    color: [26, 26, 26],
                    direction: Direction::Right,
                    orientation: Handedness::Ccw,
                    label: "Black".into(),
                },
                SharePiece {
                    piece: PieceRef::Inline(vec![(1, 2), (3, 0), (-2, -2)]),
                    color: [209, 31, 31],
                    direction: Direction::Left,
                    orientation: Handedness::Cw,
                    label: "Custom".into(),
                },
            ],
        }
    }

    #[test]
    fn round_trips() {
        let cfg = sample();
        let code = encode(&cfg);
        // base64url: no '+', '/', or '=' so it's URL-fragment safe.
        assert!(!code.contains(['+', '/', '=']));
        assert_eq!(decode(&code).unwrap(), cfg);
    }

    #[test]
    fn rejects_garbage() {
        assert!(matches!(decode("not valid base64!!!"), Err(ShareError::Base64)));
    }

    #[test]
    fn rejects_bad_json() {
        let code = B64.encode(b"{not json");
        assert!(matches!(decode(&code), Err(ShareError::Json(_))));
    }

    #[test]
    fn rejects_wrong_version() {
        let mut cfg = sample();
        cfg.version = 99;
        let code = encode(&cfg);
        assert!(matches!(decode(&code), Err(ShareError::Version(99))));
    }

    #[test]
    fn rejects_unknown_builtin() {
        let mut cfg = sample();
        cfg.pieces[0].piece = PieceRef::Builtin("griffin".into());
        let code = encode(&cfg);
        assert!(matches!(decode(&code), Err(ShareError::UnknownPiece(n)) if n == "griffin"));
    }

    #[test]
    fn decodes_codes_without_custom_types() {
        // A v1 code that predates the `custom_types` field still decodes (serde default).
        let code = B64.encode(br#"{"version":1,"radius":50,"pieces":[]}"#);
        let cfg = decode(&code).unwrap();
        assert!(cfg.custom_types.is_empty());
        assert_eq!(cfg.radius, 50);
    }
}
