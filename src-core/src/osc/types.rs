use std::{fmt, str::FromStr};

use serde::{Deserialize, Serialize};

/// A value received from an OSC message argument.
#[derive(Debug, Clone)]
pub enum OscValue {
    Bool(bool),
    Float(f32),
    Int(i32),
}

/// OscValue methods
impl OscValue {
    pub fn as_float(&self) -> f32 {
        match self {
            OscValue::Float(f) => *f,
            OscValue::Int(i) => *i as f32,
            OscValue::Bool(b) => {
                if *b {
                    1.0
                } else {
                    0.0
                }
            }
        }
    }

    pub fn as_bool(&self) -> bool {
        match self {
            OscValue::Bool(b) => *b,
            OscValue::Float(f) => *f > 0.5,
            OscValue::Int(i) => *i != 0,
        }
    }
}

/// SPS zone type corresponding to VRChat avatar contact zones.

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Copy)]
pub enum OldZoneType {
    /// All of them
    Any,
    /// Plug (penetrating) zone — maps to `vrchat.sps.plug`
    Pen,
    /// Socket (receiving) zone — maps to `vrchat.sps.socket`
    Orf,
    /// Touch-only zone — maps to `vrchat.sps.touch`
    Touch,
    /// DGB zone — flat `DGB/<name>` parameter, value is the level directly.
    DGB,
}

impl fmt::Display for OldZoneType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            OldZoneType::Any => write!(f, "Any"),
            OldZoneType::Pen => write!(f, "Pen"),
            OldZoneType::Orf => write!(f, "Orf"),
            OldZoneType::Touch => write!(f, "Touch"),
            OldZoneType::DGB => write!(f, "DGB"),
        }
    }
}

impl FromStr for OldZoneType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "any" => Ok(OldZoneType::Any),
            "pen" => Ok(OldZoneType::Pen),
            "orf" => Ok(OldZoneType::Orf),
            "touch" => Ok(OldZoneType::Touch),
            "dgb" => Ok(OldZoneType::DGB),
            _ => Err(format!("'{}' is not a valid ZoneType", s)),
        }
    }
}

/// Event emitted whenever a zone's computed stimulation level changes.

#[derive(Debug, Clone)]
pub struct ZoneEvent {
    /// Zone type (Any / Pen / Orf / Touch)
    pub zone_type: OldZoneType,
    /// Zone identifier extracted from the parameter path
    pub id: String,
    /// `true` when the zone comes from the `TPS_Internal` prefix
    pub is_tps: bool,
    /// Normalised stimulation level in [0.0, 1.0]
    pub level: f32,
    pub velocity: f32,
    pub acceleration: f32,
    pub recoil: f32,
}
