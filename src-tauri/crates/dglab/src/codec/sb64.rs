use base64::{Engine, engine::general_purpose::STANDARD};
use flate2::{Compression, read::GzDecoder, write::GzEncoder};
use serde::{Serialize, de::DeserializeOwned};
use std::io::{Read, Write};

use crate::error::{DGLabError, Result};

pub fn encode<T: Serialize>(value: &T) -> Result<String> {
    let json = serde_json::to_string(value)?;

    let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(json.as_bytes())?;
    let compressed = encoder.finish()?;

    Ok(STANDARD.encode(compressed))
}

pub fn decode<T: DeserializeOwned>(base64_str: &str) -> Result<T> {
    let compressed = STANDARD
        .decode(base64_str)
        .map_err(|e| DGLabError::Serialization(e.to_string()))?;

    let mut decoder = GzDecoder::new(&compressed[..]);
    let mut json = String::new();
    decoder.read_to_string(&mut json)?;

    Ok(serde_json::from_str(&json)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::protocol::waveform::WaveformV3;

    #[test]
    fn roundtrip() {
        let wave = WaveformV3::new(50, 30, [100, 110, 120, 130], [10, 20, 30, 40], [0; 4], [0; 4]);
        let encoded = encode(&wave).unwrap();
        let decoded: WaveformV3 = decode(&encoded).unwrap();
        assert_eq!(wave.strength_a, decoded.strength_a);
        assert_eq!(wave.strength_b, decoded.strength_b);
        assert_eq!(wave.frequency_a, decoded.frequency_a);
        assert_eq!(wave.intensity_a, decoded.intensity_a);
    }
}
