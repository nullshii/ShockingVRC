use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, Copy)]
pub enum ZoneType {
    Pen,
    Orf,
    Tps,
    Dgb,
}

impl Display for ZoneType {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        match self {
            ZoneType::Pen => write!(f, "Pen"),
            ZoneType::Orf => write!(f, "Orf"),
            ZoneType::Tps => write!(f, "Tps"),
            ZoneType::Dgb => write!(f, "DGB"),
        }
    }
}

impl FromStr for ZoneType {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "pen" => Ok(ZoneType::Pen),
            "orf" => Ok(ZoneType::Orf),
            "tps" => Ok(ZoneType::Tps),
            "dgb" => Ok(ZoneType::Dgb),
            _ => Err(format!("'{}' is not a valid ZoneType", s)),
        }
    }
}
