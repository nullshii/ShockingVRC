pub mod compile;
pub mod curve;
pub mod device;
pub mod eval;
pub mod kind;
pub mod model;
pub mod preset;
pub mod safety;

pub use compile::{GraphIssue, Severity, flat_id};
pub use curve::{Curve, CurveInterp, CurvePreset};
pub use device::{ChannelFrame, DeviceBinding, DeviceFrame, FreqMode};
pub use eval::{AlarmSample, ExternalSample, GraphEvaluator, GraphProbe, NodeProbe, ScopeTrace};
pub use kind::{Category, NodeKind, PinSpec, PinType};
pub use model::{ConnectError, Edge, Graph, Node, NodeId, NodeTint, PinRef, ViewState, parse_graph};
pub use preset::{GraphPreset, GraphPresetEntry, GraphPresetKind, parse_graph_preset};

#[cfg(test)]
mod tests;
