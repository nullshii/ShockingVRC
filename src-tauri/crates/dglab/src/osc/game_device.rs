use std::collections::HashMap;

use super::types::{OscValue, ZoneEvent, ZoneType};

/// Tracks all contact parameter values for one SPS zone and derives a single
/// output level.
pub struct GameDevice {
    pub zone_type: ZoneType,
    pub id: String,
    pub is_tps: bool,
    values: HashMap<String, OscValue>,
    /// Measured span of the *other's* penetrator (Tip − Root), saved while Tip < 1.
    pen_others_len: f32,
    /// Measured span of *self's* penetrator (Tip − Root), saved while Tip < 1.
    pen_self_len: f32,
}

impl GameDevice {
    pub fn new(zone_type: ZoneType, id: String, is_tps: bool) -> Self {
        Self {
            zone_type,
            id,
            is_tps,
            values: HashMap::new(),
            pen_others_len: 0.0,
            pen_self_len: 0.0,
        }
    }

    /// Store a contact parameter value and refresh the saved penetrator length
    /// if a Root/Tip probe was updated.
    pub fn set_value(&mut self, contact: &str, value: OscValue) {
        self.values.insert(contact.to_string(), value);
        self.refresh_pen_len(contact);
    }

    // Penetrator length tracking

    /// While `Tip < 1` the penetrator is partially inserted and we can measure
    /// its span: `length = Tip − Root`.  We save this value so that once `Tip`
    /// reaches 1 (fully through) we still know how long the object is.
    fn refresh_pen_len(&mut self, updated: &str) {
        match updated {
            "PenOthersNewTip" | "PenOthersNewRoot" => {
                if let (Some(tip), Some(root)) = (
                    self.values.get("PenOthersNewTip").map(|v| v.as_float()),
                    self.values.get("PenOthersNewRoot").map(|v| v.as_float()),
                ) {
                    if tip < 1.0 {
                        // clamp to a tiny positive value to avoid divide-by-zero later
                        self.pen_others_len = (tip - root).max(0.001);
                    }
                }
            }
            "PenSelfNewTip" | "PenSelfNewRoot" => {
                if let (Some(tip), Some(root)) = (
                    self.values.get("PenSelfNewTip").map(|v| v.as_float()),
                    self.values.get("PenSelfNewRoot").map(|v| v.as_float()),
                ) {
                    if tip < 1.0 {
                        self.pen_self_len = (tip - root).max(0.001);
                    }
                }
            }
            _ => {}
        }
    }

    // New-model pen level (Root/Tip probes)

    /// Compute penetration level [0, 1] from Root/Tip probes.
    /// Algorithm:
    /// 1. **Length** = `Tip − Root`, saved while `Tip < 1`.
    /// 2. **Active zone** starts when `Root > 1 − length` (the moment the base
    ///    of the penetrator enters the socket).
    /// 3. **Level** rises linearly from 0 (Root = 1 − length) to 1 (Root = 1).
    ///
    /// Returns `None` if the probes have never been received or the length
    /// is unknown, so the caller can fall back to the legacy scalar.
    fn new_pen_level(&self, is_self: bool) -> Option<f32> {
        let (root_key, tip_key, saved_len) = if is_self {
            ("PenSelfNewRoot", "PenSelfNewTip", self.pen_self_len)
        } else {
            ("PenOthersNewRoot", "PenOthersNewTip", self.pen_others_len)
        };

        // Root probe must be present for this model to apply
        let root_val = self.values.get(root_key)?.as_float();

        // Effective length: use the last saved value (measured while tip < 1),
        // or compute from the current tip if it hasn't exited yet.
        let length = if saved_len > 0.0 {
            saved_len
        } else {
            let tip_val = self.values.get(tip_key)?.as_float();
            if tip_val < 1.0 {
                (tip_val - root_val).max(0.001)
            } else {
                // Tip is already at 1 but we never saw it below 1 — length unknown
                return None;
            }
        };

        // threshold = the Root value at which the object starts entering
        let threshold = (1.0 - length).max(0.0);

        if root_val <= threshold {
            Some(0.0)
        } else {
            Some(((root_val - threshold) / length).clamp(0.0, 1.0))
        }
    }

    // Internal helpers
    fn get_float(&self, key: &str) -> f32 {
        self.values.get(key).map(|v| v.as_float()).unwrap_or(0.0)
    }

    fn get_bool_opt(&self, key: &str) -> Option<bool> {
        self.values.get(key).map(|v| v.as_bool())
    }

    fn get_bool(&self, key: &str) -> bool {
        self.get_bool_opt(key).unwrap_or(false)
    }

    // Level computation per zone type
    /// Compute stimulation level in [0.0, 1.0] from all active contacts.
    pub fn compute_level(&self) -> f32 {
        match self.zone_type {
            ZoneType::Any => todo!(),
            ZoneType::Pen => self.compute_pen_level(),
            ZoneType::Orf => self.compute_orf_level(),
            ZoneType::Touch => self.compute_touch_level(),
            // DGB: the OSC value IS the level — no contact hierarchy
            ZoneType::DGB => self.get_float("Value").clamp(0.0, 1.0),
        }
    }

    fn compute_pen_level(&self) -> f32 {
        let touch_self = if self.get_bool("TouchSelfClose") {
            self.get_float("TouchSelf")
        } else {
            0.0
        };
        let touch_others = if self.get_bool("TouchOthersClose") {
            self.get_float("TouchOthers")
        } else {
            0.0
        };

        // New Root/Tip model preferred; fall back to legacy scalar
        let pen_self = self.new_pen_level(true).unwrap_or_else(|| self.get_float("PenSelf"));
        let pen_others = self.new_pen_level(false).unwrap_or_else(|| self.get_float("PenOthers"));

        let frot_others = if self.get_bool("FrotOthersClose") {
            self.get_float("FrotOthers")
        } else {
            0.0
        };

        [touch_self, touch_others, pen_self, pen_others, frot_others]
            .into_iter()
            .fold(0.0f32, f32::max)
    }

    fn compute_orf_level(&self) -> f32 {
        let touch_self = if self.get_bool("TouchSelfClose") {
            self.get_float("TouchSelf")
        } else {
            0.0
        };
        let touch_others = if self.get_bool("TouchOthersClose") {
            self.get_float("TouchOthers")
        } else {
            0.0
        };

        let pen_self = self.new_pen_level(true).unwrap_or_else(|| self.get_float("PenSelf"));

        // PenOthers for Orf: new model takes priority, then legacy scalar
        // (gated by PenOthersClose if present; if absent → legacy always on)
        let pen_others = self.new_pen_level(false).unwrap_or_else(|| {
            let close = self.get_bool_opt("PenOthersClose");
            if close.unwrap_or(true) {
                self.get_float("PenOthers")
            } else {
                0.0
            }
        });

        let frot_others = if self.get_bool("FrotOthersClose") {
            self.get_float("FrotOthers")
        } else {
            0.0
        };

        let depth_in = self.get_float("Depth_In");

        [touch_self, touch_others, pen_self, pen_others, frot_others, depth_in]
            .into_iter()
            .fold(0.0f32, f32::max)
    }

    fn compute_touch_level(&self) -> f32 {
        self.get_float("Self").max(self.get_float("Others"))
    }

    // Public helpers

    /// Snapshot of the current state as a [`ZoneEvent`].
    pub fn to_event(&self) -> ZoneEvent {
        ZoneEvent {
            zone_type: self.zone_type.clone(),
            id: self.id.clone(),
            is_tps: self.is_tps,
            level: self.compute_level(),
        }
    }
}
