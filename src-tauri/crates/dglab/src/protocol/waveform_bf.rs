#[derive(Debug, Clone, Copy)]
pub struct WaveformBF {
    pub head: u8,
    pub strength_upper_limit_a: u8,
    pub strength_upper_limit_b: u8,
    pub strength_form_para_a: u8,
    pub strength_form_para_b: u8,
    pub strength_volt_para_a: u8,
    pub strength_volt_para_b: u8,
}

impl WaveformBF {
    pub fn new(
        limit_a: u8,
        limit_b: u8,
        form_a: u8,
        form_b: u8,
        volt_a: u8,
        volt_b: u8,
    ) -> Self {
        Self {
            head: 0xBF,
            strength_upper_limit_a: limit_a,
            strength_upper_limit_b: limit_b,
            strength_form_para_a: form_a,
            strength_form_para_b: form_b,
            strength_volt_para_a: volt_a,
            strength_volt_para_b: volt_b,
        }
    }

    pub fn symmetric(limit: u8) -> Self {
        Self::new(limit, limit, 0, 0, 0, 0)
    }

    pub fn to_bytes(&self) -> [u8; 7] {
        [
            self.head,
            self.strength_upper_limit_a,
            self.strength_upper_limit_b,
            self.strength_form_para_a,
            self.strength_form_para_b,
            self.strength_volt_para_a,
            self.strength_volt_para_b,
        ]
    }
}

impl Default for WaveformBF {
    fn default() -> Self {
        Self::new(200, 0, 0, 0, 0, 0)
    }
}

impl From<WaveformBF> for Vec<u8> {
    fn from(w: WaveformBF) -> Self {
        w.to_bytes().to_vec()
    }
}
