use std::collections::{HashMap, HashSet};

use serde::{Deserialize, Serialize};

use super::device::DeviceBinding;
use super::kind::NodeKind;
use super::model::{Edge, Graph, NodeId, PinRef};

pub const GRAPH_PRESET_VERSION: u32 = 1;

fn default_version() -> u32 {
    GRAPH_PRESET_VERSION
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GraphPresetKind {
    #[default]
    Graph,
    Fragment,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct GraphPreset {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default)]
    pub kind: GraphPresetKind,
    pub name: String,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub author: String,
    #[serde(default)]
    pub graph: Graph,
}

#[derive(Debug, Clone)]
pub struct GraphPresetEntry {
    pub id: String,
    pub preset: GraphPreset,
    pub user: bool,
}

impl GraphPresetEntry {
    pub fn name(&self) -> &str {
        if self.preset.name.is_empty() {
            &self.id
        } else {
            &self.preset.name
        }
    }
}

fn anonymize(graph: &Graph) -> Graph {
    let mut g = graph.clone();
    let mut slots: HashMap<String, u32> = HashMap::new();
    let mut next = 1u32;
    for n in &mut g.nodes {
        if let NodeKind::Device(d) = &mut n.kind {
            let slot = match &d.binding {
                DeviceBinding::Mac(m) => *slots.entry(m.to_uppercase()).or_insert_with(|| {
                    next += 1;
                    next - 1
                }),
                DeviceBinding::Slot(s) => *s,
                DeviceBinding::Unbound => {
                    next += 1;
                    next - 1
                }
            };
            d.binding = DeviceBinding::Slot(slot);
            n.title = format!("Device {slot}");
        }
    }
    g.view = Default::default();
    g
}

impl GraphPreset {
    pub fn from_graph(name: &str, author: &str, graph: &Graph) -> Self {
        Self {
            version: GRAPH_PRESET_VERSION,
            kind: GraphPresetKind::Graph,
            name: name.trim().to_string(),
            description: String::new(),
            author: author.trim().to_string(),
            graph: anonymize(graph),
        }
    }

    pub fn fragment(name: &str, author: &str, selection: &Graph) -> Self {
        let mut p = Self::from_graph(name, author, selection);
        p.kind = GraphPresetKind::Fragment;
        if let Some((min, _)) = p.graph.bounds() {
            for n in &mut p.graph.nodes {
                n.pos = [n.pos[0] - min[0], n.pos[1] - min[1]];
            }
        }
        p
    }

    pub fn node_count(&self) -> usize {
        self.graph.nodes.len()
    }

    pub fn device_slots(&self) -> usize {
        let slots: HashSet<u32> = self
            .graph
            .nodes
            .iter()
            .filter_map(|n| match n.device_binding() {
                Some(DeviceBinding::Slot(s)) => Some(*s),
                _ => None,
            })
            .collect();
        slots.len()
    }

    pub fn instantiate(&self, devices: &[String]) -> Graph {
        let mut g = Graph {
            view: self.graph.view,
            ..Graph::default()
        };
        g.insert(&self.graph, [0.0, 0.0]);
        for n in &mut g.nodes {
            if let NodeKind::Device(d) = &mut n.kind
                && let DeviceBinding::Slot(s) = d.binding
            {
                d.binding = match devices.get((s as usize).saturating_sub(1)) {
                    Some(mac) => DeviceBinding::Mac(mac.to_uppercase()),
                    None => DeviceBinding::Unbound,
                };
            }
        }
        g.sanitize();
        g
    }

    pub fn insert_into(&self, target: &mut Graph, at: [f32; 2], devices: &[String]) -> Vec<NodeId> {
        let mut redirect: HashMap<NodeId, NodeId> = HashMap::new();
        for n in &self.graph.nodes {
            if let Some(DeviceBinding::Slot(s)) = n.device_binding()
                && let Some(mac) = devices.get((*s as usize).saturating_sub(1))
                && let Some(existing) = target.device_node_for(mac)
            {
                redirect.insert(n.id, existing.id);
            }
        }
        let keep: HashSet<NodeId> = self
            .graph
            .nodes
            .iter()
            .map(|n| n.id)
            .filter(|id| !redirect.contains_key(id))
            .collect();
        let body = self.graph.extract(&keep);
        let offset = match body.bounds() {
            Some((min, _)) => [at[0] - min[0], at[1] - min[1]],
            None => at,
        };
        let new_ids = target.insert(&body, offset);
        let remap: HashMap<NodeId, NodeId> = body
            .nodes
            .iter()
            .map(|n| n.id)
            .zip(new_ids.iter().copied())
            .collect();

        for e in &self.graph.edges {
            let end = |id: NodeId| remap.get(&id).or_else(|| redirect.get(&id)).copied();
            let touches_redirect = redirect.contains_key(&e.from.node) || redirect.contains_key(&e.to.node);
            if !touches_redirect {
                continue;
            }
            if let (Some(from), Some(to)) = (end(e.from.node), end(e.to.node)) {
                let to = PinRef::new(to, e.to.pin.clone());
                target.edges.retain(|x| x.to != to);
                target.edges.push(Edge {
                    from: PinRef::new(from, e.from.pin.clone()),
                    to,
                });
            }
        }

        for id in &new_ids {
            let slot = match target.node(*id).and_then(|n| n.device_binding()) {
                Some(DeviceBinding::Slot(s)) => *s,
                _ => continue,
            };
            let free = devices
                .get((slot as usize).saturating_sub(1))
                .filter(|m| target.device_node_for(m).is_none())
                .cloned();
            if let Some(n) = target.node_mut(*id)
                && let NodeKind::Device(d) = &mut n.kind
            {
                d.binding = match free {
                    Some(mac) => DeviceBinding::Mac(mac.to_uppercase()),
                    None => DeviceBinding::Unbound,
                };
            }
        }
        target.prune_dangling_edges();
        new_ids
    }
}

pub fn parse_graph_preset(json: &str) -> Result<GraphPreset, String> {
    let mut p: GraphPreset =
        serde_json::from_str(json).map_err(|e| format!("invalid graph preset: {e}"))?;
    p.graph.sanitize();
    Ok(p)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::kind::{ContactParams, DeviceParams};
    use crate::graph::model::PinRef;

    fn two_device_graph() -> Graph {
        let mut g = Graph::default();
        let c = g.add_kind(NodeKind::Contact(ContactParams::default()), [0.0, 0.0]);
        let d1 = g.add_kind(NodeKind::Device(DeviceParams::bound_to("AA:AA")), [300.0, 0.0]);
        let d2 = g.add_kind(NodeKind::Device(DeviceParams::bound_to("BB:BB")), [300.0, 300.0]);
        g.node_mut(d1).unwrap().title = "Coyote AAAA".into();
        g.connect(PinRef::new(c, "level"), PinRef::new(d1, "a.power")).unwrap();
        g.connect(PinRef::new(c, "level"), PinRef::new(d2, "b.power")).unwrap();
        g
    }

    #[test]
    fn presets_never_store_a_mac() {
        let p = GraphPreset::from_graph("x", "me", &two_device_graph());
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("AA:AA") && !json.contains("BB:BB") && !json.contains("AAAA"));
        assert_eq!(p.device_slots(), 2);
    }

    #[test]
    fn apply_binds_slots_in_order_and_leaves_placeholders() {
        let p = GraphPreset::from_graph("x", "me", &two_device_graph());
        let g = p.instantiate(&["CC:CC".to_string()]);
        let bindings: Vec<_> = g.nodes.iter().filter_map(|n| n.device_binding().cloned()).collect();
        assert!(bindings.contains(&DeviceBinding::Mac("CC:CC".into())));
        assert!(bindings.contains(&DeviceBinding::Unbound));
        assert_eq!(g.edges.len(), 2, "wiring survives");
        let original: HashSet<NodeId> = two_device_graph().nodes.iter().map(|n| n.id).collect();
        assert!(g.nodes.iter().all(|n| !original.contains(&n.id)), "fresh ids");
    }

    #[test]
    fn a_fragment_reuses_the_existing_device_node() {
        let frag = GraphPreset::fragment("f", "", &two_device_graph());
        let mut target = Graph::default();
        target.ensure_device_nodes(&[("CC:CC".into(), "Coyote".into())]);
        let before = target.nodes.len();
        let ids = frag.insert_into(&mut target, [50.0, 50.0], &["CC:CC".into()]);
        assert_eq!(ids.len(), 2, "contact + the second device's placeholder");
        assert_eq!(target.nodes.len(), before + 2);
        let dev = target.device_node_for("CC:CC").unwrap().id;
        assert!(target.edges.iter().any(|e| e.to.node == dev && e.to.pin == "a.power"));
    }

    #[test]
    fn round_trip_and_bad_input() {
        let p = GraphPreset::from_graph("Name", "Author", &two_device_graph());
        let json = serde_json::to_string_pretty(&p).unwrap();
        let back = parse_graph_preset(&json).unwrap();
        assert_eq!(back.name, "Name");
        assert_eq!(back.node_count(), 3);
        assert!(parse_graph_preset("[]").is_err());
        assert_eq!(parse_graph_preset(r#"{"name":"bare"}"#).unwrap().version, GRAPH_PRESET_VERSION);
    }
}
