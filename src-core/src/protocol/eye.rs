
pub const EYE_OPCODE: u8 = 0x50;
pub const EYE_PACKET_LEN: usize = 17;

pub const EYE_COLORS: [(&str, [u8; 3]); 6] = [
    ("Yellow", [255, 210, 0]),
    ("Red", [255, 40, 40]),
    ("Purple", [170, 70, 255]),
    ("Blue", [40, 80, 255]),
    ("Bright blue", [0, 220, 255]),
    ("Green", [40, 220, 90]),
];

pub fn valid_preset(preset: u8) -> bool {
    (1..=EYE_COLORS.len() as u8).contains(&preset)
}

pub fn label(preset: u8) -> Option<&'static str> {
    valid_preset(preset).then(|| EYE_COLORS[preset as usize - 1].0)
}

pub fn packet(preset: u8) -> Option<[u8; EYE_PACKET_LEN]> {
    valid_preset(preset).then(|| {
        let mut b = [0u8; EYE_PACKET_LEN];
        b[0] = EYE_OPCODE;
        b[1] = preset;
        b
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn packet_matches_capture() {
        assert_eq!(packet(3).unwrap(), [0x50, 0x03, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn range_is_one_to_six() {
        assert!(packet(0).is_none() && packet(7).is_none());
        assert_eq!(label(2), Some("Red"));
        assert_eq!(label(6), Some("Green"));
    }
}
