use shocking_vrc_core::cli::{
    AggregationMode, ChannelConfig, CliConfig, ContactMode, PowerLimits, ZoneEntry, ZoneId,
};
use shocking_vrc_core::OldZoneType;
use shocking_vrc_core::ZoneEvent;

pub fn demo_config() -> CliConfig {
    CliConfig {
        channel_a: ChannelConfig {
            zones: vec![
                ZoneEntry::new(
                    ZoneId::new(OldZoneType::Orf, "Pussy"),
                    ContactMode::Depth,
                ),
                ZoneEntry::new(
                    ZoneId::new(OldZoneType::DGB, "TouchAreaA"),
                    ContactMode::Depth,
                ),
            ],
            frequency: [30, 200, 30, 200],
            intensity: [100, 100, 100, 100],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
            ..ChannelConfig::default()
        },
        channel_b: ChannelConfig {
            zones: vec![ZoneEntry::new(
                ZoneId::new(OldZoneType::Pen, "Cock"),
                ContactMode::Depth,
            )],
            frequency: [30, 220, 60, 140],
            intensity: [100, 100, 100, 100],
            limits: PowerLimits::new(0, 30),
            aggregation: AggregationMode::Max,
            ..ChannelConfig::default()
        },
        ..CliConfig::default()
    }
}

pub fn demo_avatar_zones() -> Vec<ZoneEvent> {
    vec![
        fake_zone(OldZoneType::Orf, "Pussy", 0.45),
        fake_zone(OldZoneType::Pen, "Cock", 0.72),
        fake_zone(OldZoneType::DGB, "TouchAreaA", 0.30),
        fake_zone(OldZoneType::DGB, "Chest", 0.0),
        fake_zone(OldZoneType::Touch, "Head", 0.15),
    ]
}

fn fake_zone(zone_type: OldZoneType, id: &str, level: f32) -> ZoneEvent {
    ZoneEvent {
        zone_type,
        id: id.to_string(),
        is_tps: false,
        level,
        velocity: 0.0,
        acceleration: 0.0,
        recoil: 0.0,
    }
}
