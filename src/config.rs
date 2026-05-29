//! TOML configuration for arbitrary placement games.
//!
//! A config defines named *piece types* (each a finite list of attacked relative
//! coordinates) and a list of *pieces* — each a `(type, color, spiral)` — that take
//! turns in listed order. It deserializes with serde, is validated into an
//! [`EngineConfig`], and runs through the general [`engine`](crate::engine). Example:
//!
//! ```toml
//! [[piece_type]]
//! name = "knight"
//! offsets = [[1,2],[2,1],[2,-1],[1,-2],[-1,-2],[-2,-1],[-2,1],[-1,2]]
//!
//! [[piece]]
//! type = "knight"
//! color = "#1a1a1a"
//! direction = "right"
//! orientation = "ccw"
//!
//! [[piece]]
//! type = "knight"
//! color = "#d11f1f"
//! direction = "right"
//! orientation = "ccw"
//! ```

use crate::engine::{EngineConfig, PieceSpec};
use crate::piece::{KindBuilder, Rgb};
use crate::spiral::{Direction, Handedness};
use serde::Deserialize;
use std::collections::HashMap;

/// Largest attack-offset list a piece type may have (a guard, not a fundamental limit).
const MAX_OFFSETS: usize = 64;
/// Largest number of pieces (the cell byte caps distinct kinds at 255, and kinds ≤ pieces).
const MAX_PIECES: usize = 255;

/// The raw, deserialized config (before validation).
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PlacementConfig {
    #[serde(default, rename = "piece_type")]
    piece_types: Vec<PieceTypeDef>,
    #[serde(default, rename = "piece")]
    pieces: Vec<PieceDef>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PieceTypeDef {
    name: String,
    /// Attack offsets as `[dx, dy]` pairs (arrays, not tuples — sidesteps any toml
    /// tuple-arity quirk; converted to `(i32, i32)` during validation).
    offsets: Vec<[i32; 2]>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PieceDef {
    #[serde(rename = "type")]
    piece_type: String,
    color: String,
    direction: DirectionDef,
    orientation: OrientationDef,
    /// Optional legend label; defaults to the piece type's name.
    #[serde(default)]
    label: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum DirectionDef {
    Right,
    Up,
    Left,
    Down,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "lowercase")]
enum OrientationDef {
    Cw,
    Ccw,
}

impl DirectionDef {
    fn to_spiral(self) -> Direction {
        match self {
            DirectionDef::Right => Direction::Right,
            DirectionDef::Up => Direction::Up,
            DirectionDef::Left => Direction::Left,
            DirectionDef::Down => Direction::Down,
        }
    }
}

impl OrientationDef {
    fn to_spiral(self) -> Handedness {
        match self {
            OrientationDef::Cw => Handedness::Cw,
            OrientationDef::Ccw => Handedness::Ccw,
        }
    }
}

/// Everything that can go wrong loading a config.
#[derive(Debug)]
pub enum ConfigError {
    Read(std::io::Error),
    Parse(toml::de::Error),
    NoPieces,
    TooManyPieces(usize),
    DuplicateTypeName(String),
    EmptyOffsets(String),
    TooManyOffsets { name: String, count: usize },
    ZeroOffset(String),
    UnknownType { index: usize, name: String, known: Vec<String> },
    BadColor { value: String, reason: &'static str },
}

impl std::fmt::Display for ConfigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConfigError::Read(e) => write!(f, "could not read config: {e}"),
            ConfigError::Parse(e) => write!(f, "config parse error: {e}"),
            ConfigError::NoPieces => write!(f, "config must define at least one [[piece]]"),
            ConfigError::TooManyPieces(n) => {
                write!(f, "too many pieces: {n} (max {MAX_PIECES})")
            }
            ConfigError::DuplicateTypeName(name) => {
                write!(f, "duplicate piece_type name '{name}'")
            }
            ConfigError::EmptyOffsets(name) => {
                write!(f, "piece_type '{name}': offsets list is empty")
            }
            ConfigError::TooManyOffsets { name, count } => {
                write!(f, "piece_type '{name}': {count} offsets exceeds max {MAX_OFFSETS}")
            }
            ConfigError::ZeroOffset(name) => {
                write!(f, "piece_type '{name}': offset [0, 0] attacks its own square")
            }
            ConfigError::UnknownType { index, name, known } => write!(
                f,
                "piece #{index}: unknown type '{name}'; known types: {}",
                known.join(", ")
            ),
            ConfigError::BadColor { value, reason } => {
                write!(f, "invalid color '{value}': {reason}")
            }
        }
    }
}

impl std::error::Error for ConfigError {}

/// Parse and validate a config from a TOML string into a runnable [`EngineConfig`].
pub fn from_toml(text: &str) -> Result<EngineConfig, ConfigError> {
    let raw: PlacementConfig = toml::from_str(text).map_err(ConfigError::Parse)?;
    raw.build()
}

/// Read a TOML config file and validate it into a runnable [`EngineConfig`].
pub fn load(path: &str) -> Result<EngineConfig, ConfigError> {
    let text = std::fs::read_to_string(path).map_err(ConfigError::Read)?;
    from_toml(&text)
}

impl PlacementConfig {
    fn build(self) -> Result<EngineConfig, ConfigError> {
        // Resolve and validate piece types into name -> offset list.
        let mut types: HashMap<&str, Vec<(i32, i32)>> = HashMap::new();
        let mut known: Vec<String> = Vec::new();
        for t in &self.piece_types {
            if types.contains_key(t.name.as_str()) {
                return Err(ConfigError::DuplicateTypeName(t.name.clone()));
            }
            if t.offsets.is_empty() {
                return Err(ConfigError::EmptyOffsets(t.name.clone()));
            }
            if t.offsets.len() > MAX_OFFSETS {
                return Err(ConfigError::TooManyOffsets {
                    name: t.name.clone(),
                    count: t.offsets.len(),
                });
            }
            let mut offsets = Vec::with_capacity(t.offsets.len());
            for &[dx, dy] in &t.offsets {
                if dx == 0 && dy == 0 {
                    return Err(ConfigError::ZeroOffset(t.name.clone()));
                }
                offsets.push((dx, dy));
            }
            known.push(t.name.clone());
            types.insert(t.name.as_str(), offsets);
        }

        // Validate the piece list and intern kinds in turn order.
        if self.pieces.is_empty() {
            return Err(ConfigError::NoPieces);
        }
        if self.pieces.len() > MAX_PIECES {
            return Err(ConfigError::TooManyPieces(self.pieces.len()));
        }
        let mut kinds = KindBuilder::new();
        let mut pieces = Vec::with_capacity(self.pieces.len());
        for (index, p) in self.pieces.iter().enumerate() {
            let offsets = types.get(p.piece_type.as_str()).ok_or_else(|| ConfigError::UnknownType {
                index,
                name: p.piece_type.clone(),
                known: known.clone(),
            })?;
            let color = parse_hex(&p.color)?;
            let label = p.label.clone().unwrap_or_else(|| p.piece_type.clone());
            // At most MAX_PIECES (<= MAX_KINDS) pieces, so interning never overflows.
            let kind = kinds
                .intern(offsets.clone(), color, &label)
                .expect("piece count is capped at MAX_PIECES, so kinds always fit");
            pieces.push(PieceSpec {
                kind,
                direction: p.direction.to_spiral(),
                handed: p.orientation.to_spiral(),
            });
        }

        Ok(EngineConfig { pieces, kinds: kinds.finish() })
    }
}

/// Parse a `#rrggbb` hex color.
fn parse_hex(s: &str) -> Result<Rgb, ConfigError> {
    let body = s
        .strip_prefix('#')
        .ok_or(ConfigError::BadColor { value: s.to_string(), reason: "must start with '#'" })?;
    if body.len() != 6 || !body.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(ConfigError::BadColor {
            value: s.to_string(),
            reason: "expected '#rrggbb' (6 hex digits)",
        });
    }
    let n = u32::from_str_radix(body, 16).expect("validated as 6 hex digits");
    Ok(((n >> 16) as u8, (n >> 8) as u8, n as u8))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine;
    use crate::redblack::{self, Variant};

    const CANONICAL: &str = r##"
        [[piece_type]]
        name = "knight"
        offsets = [[1,2],[2,1],[2,-1],[1,-2],[-1,-2],[-2,-1],[-2,1],[-1,2]]

        [[piece]]
        type = "knight"
        color = "#1a1a1a"
        direction = "right"
        orientation = "ccw"

        [[piece]]
        type = "knight"
        color = "#d11f1f"
        direction = "right"
        orientation = "ccw"
    "##;

    #[test]
    fn parses_canonical_toml() {
        let cfg = from_toml(CANONICAL).expect("valid config");
        assert_eq!(cfg.pieces.len(), 2);
        assert_eq!(cfg.kinds.len(), 3, "EMPTY + Black + Red");
        assert_eq!(cfg.kinds.max_chebyshev(), 2, "knight reach");
        // Turn order assigns Black byte 1, Red byte 2.
        assert_eq!(cfg.pieces[0].kind, 1);
        assert_eq!(cfg.pieces[1].kind, 2);
    }

    #[test]
    fn custom_two_color_matches_redblack_canonical() {
        let cfg = from_toml(CANONICAL).expect("valid config");
        let via_config = engine::simulate(20, cfg);
        let via_preset = redblack::simulate_redblack(20, Variant::Canonical);
        for y in -20..=20 {
            for x in -20..=20 {
                assert_eq!(
                    via_config.cell(x, y),
                    via_preset.cell(x, y),
                    "mismatch at ({x},{y})"
                );
            }
        }
    }

    fn err(text: &str) -> ConfigError {
        match from_toml(text) {
            Ok(_) => panic!("expected config to fail"),
            Err(e) => e,
        }
    }

    #[test]
    fn rejects_malformed_toml() {
        assert!(matches!(err("this is not = = toml"), ConfigError::Parse(_)));
    }

    #[test]
    fn rejects_unknown_top_level_key() {
        // deny_unknown_fields catches `pieces` (plural typo) as a parse error.
        let text = "[[pieces]]\ntype = \"knight\"\n";
        assert!(matches!(err(text), ConfigError::Parse(_)));
    }

    #[test]
    fn rejects_no_pieces() {
        let text = "[[piece_type]]\nname = \"k\"\noffsets = [[1,2]]\n";
        assert!(matches!(err(text), ConfigError::NoPieces));
    }

    #[test]
    fn rejects_too_many_pieces() {
        let mut text = String::from("[[piece_type]]\nname=\"k\"\noffsets=[[1,2]]\n");
        for _ in 0..(MAX_PIECES + 1) {
            text.push_str("[[piece]]\ntype=\"k\"\ncolor=\"#010101\"\ndirection=\"right\"\norientation=\"ccw\"\n");
        }
        assert!(matches!(err(&text), ConfigError::TooManyPieces(n) if n == MAX_PIECES + 1));
    }

    #[test]
    fn rejects_duplicate_type_name() {
        let text = "[[piece_type]]\nname=\"k\"\noffsets=[[1,2]]\n[[piece_type]]\nname=\"k\"\noffsets=[[2,1]]\n";
        assert!(matches!(err(text), ConfigError::DuplicateTypeName(n) if n == "k"));
    }

    #[test]
    fn rejects_empty_offsets() {
        let text = "[[piece_type]]\nname=\"k\"\noffsets=[]\n[[piece]]\ntype=\"k\"\ncolor=\"#010101\"\ndirection=\"right\"\norientation=\"ccw\"\n";
        assert!(matches!(err(text), ConfigError::EmptyOffsets(n) if n == "k"));
    }

    #[test]
    fn rejects_too_many_offsets() {
        let mut offs = String::new();
        for i in 0..(MAX_OFFSETS + 1) {
            offs.push_str(&format!("[{},1],", i + 1));
        }
        let text = format!("[[piece_type]]\nname=\"k\"\noffsets=[{offs}]\n");
        assert!(matches!(err(&text), ConfigError::TooManyOffsets { count, .. } if count == MAX_OFFSETS + 1));
    }

    #[test]
    fn rejects_zero_offset() {
        let text = "[[piece_type]]\nname=\"k\"\noffsets=[[0,0]]\n";
        assert!(matches!(err(text), ConfigError::ZeroOffset(n) if n == "k"));
    }

    #[test]
    fn rejects_unknown_type() {
        let text = "[[piece_type]]\nname=\"knight\"\noffsets=[[1,2]]\n[[piece]]\ntype=\"bishop\"\ncolor=\"#010101\"\ndirection=\"right\"\norientation=\"ccw\"\n";
        match err(text) {
            ConfigError::UnknownType { index, name, known } => {
                assert_eq!(index, 0);
                assert_eq!(name, "bishop");
                assert_eq!(known, vec!["knight".to_string()]);
            }
            other => panic!("expected UnknownType, got {other:?}"),
        }
    }

    #[test]
    fn renders_a_custom_config_end_to_end() {
        let toml = r##"
            [[piece_type]]
            name = "knight"
            offsets = [[1,2],[2,1],[2,-1],[1,-2],[-1,-2],[-2,-1],[-2,1],[-1,2]]
            [[piece_type]]
            name = "wazir"
            offsets = [[1,0],[-1,0],[0,1],[0,-1]]
            [[piece]]
            type = "knight"
            color = "#112233"
            direction = "right"
            orientation = "ccw"
            label = "Knights"
            [[piece]]
            type = "wazir"
            color = "#aabbcc"
            direction = "left"
            orientation = "cw"
            label = "Wazirs"
        "##;
        let cfg = from_toml(toml).expect("valid config");
        let result = engine::simulate(8, cfg);

        // SVG carries both colors and both legend labels.
        let svg = crate::render::render_board_svg(&result, "Custom Placement", 800.0);
        for needle in ["#112233", "#aabbcc", "Knights", "Wazirs", "<rect"] {
            assert!(svg.contains(needle), "SVG missing {needle:?}");
        }

        // PNG streams out a structurally valid file (starts with the PNG signature).
        let path = std::env::temp_dir().join("knights_custom_e2e.png");
        let p = path.to_str().unwrap();
        crate::raster::write_board_png(p, &result, 2).expect("write png");
        let bytes = std::fs::read(p).expect("read png");
        assert_eq!(&bytes[..8], &[137, 80, 78, 71, 13, 10, 26, 10], "PNG signature");
        std::fs::remove_file(p).ok();
    }

    #[test]
    fn rejects_bad_color() {
        let base = "[[piece_type]]\nname=\"k\"\noffsets=[[1,2]]\n";
        let piece = |c: &str| {
            format!("{base}[[piece]]\ntype=\"k\"\ncolor=\"{c}\"\ndirection=\"right\"\norientation=\"ccw\"\n")
        };
        assert!(matches!(err(&piece("1a1a1a")), ConfigError::BadColor { .. }), "missing #");
        assert!(matches!(err(&piece("#abc")), ConfigError::BadColor { .. }), "too short");
        assert!(matches!(err(&piece("#gggggg")), ConfigError::BadColor { .. }), "non-hex");
    }
}
