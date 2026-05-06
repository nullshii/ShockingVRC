use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};

use crate::zone_type::ZoneType;

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InputZone {
    pub zone_type: ZoneType,
    pub name: String,
}

impl Display for InputZone {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result<(), std::fmt::Error> {
        writeln!(f, "{}({})", self.zone_type, self.name)
    }
}

impl FromStr for InputZone {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((z, n)) = s.split_once(char::is_whitespace) {
            Ok(Self {
                zone_type: ZoneType::from_str(z)?,
                name: n.to_string(),
            })
        } else {
            Err("".to_string())
        }
    }
}
