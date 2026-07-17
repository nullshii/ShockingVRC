pub mod config;
pub mod engine;

pub use config::{
    AggregationMode, ChannelConfig, CliConfig, ContactMode, MotionNorms, PowerLimits, UkfConfig,
    ZoneEntry, ZoneId, ZONE_ACTIVATION_THRESHOLD, apply_zone_activation_range,
    clamp_zone_activation_threshold,
};
pub use engine::{ChannelStatus, CliEngine, CliStatus, CliStopHandle};
