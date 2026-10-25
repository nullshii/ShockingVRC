use std::collections::{BTreeMap, HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

use super::device::DeviceBinding;
use super::kind::{self, NodeKind, PinSpec};

pub const GRAPH_VERSION: u32 = 1;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, Default)]
#[serde(transparent)]
pub struct NodeId(pub u64);

impl NodeId {
    pub fn random() -> Self {
        let v = rand::random::<u64>() & ((1u64 << 53) - 1);
        NodeId(v.max(1))
    }
}

impl fmt::Display for NodeId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "#{:x}", self.0)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PinRef {
    pub node: NodeId,
    pub pin: String,
}

impl PinRef {
    pub fn new(node: NodeId, pin: impl Into<String>) -> Self {
        Self {
            node,
            pin: pin.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Edge {
    pub from: PinRef,
    pub to: PinRef,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NodeTint {
    Accent,
    Success,
    Warning,
    Error,
    Muted,
}

impl NodeTint {
    pub const ALL: [NodeTint; 5] = [
        NodeTint::Accent,
        NodeTint::Success,
        NodeTint::Warning,
        NodeTint::Error,
        NodeTint::Muted,
    ];
}

fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    #[serde(serialize_with = "kind::serialize_kind", deserialize_with = "kind::deserialize_kind")]
    pub kind: NodeKind,
    #[serde(default)]
    pub pos: [f32; 2],
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub title: String,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub comment: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tint: Option<NodeTint>,
    #[serde(default, skip_serializing_if = "is_false")]
    pub collapsed: bool,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub defaults: BTreeMap<String, f32>,
}

impl Node {
    pub fn new(kind: NodeKind, pos: [f32; 2]) -> Self {
        Self {
            id: NodeId::random(),
            kind,
            pos,
            title: String::new(),
            comment: String::new(),
            tint: None,
            collapsed: false,
            defaults: BTreeMap::new(),
        }
    }

    pub fn display_title(&self) -> String {
        if self.title.trim().is_empty() {
            self.kind.label()
        } else {
            self.title.clone()
        }
    }

    pub fn inputs(&self) -> Vec<PinSpec> {
        self.kind.inputs()
    }

    pub fn outputs(&self) -> Vec<PinSpec> {
        self.kind.outputs()
    }

    pub fn input_default(&self, spec: &PinSpec) -> f32 {
        self.defaults
            .get(spec.key.as_ref())
            .copied()
            .filter(|v| v.is_finite())
            .unwrap_or(spec.default)
    }

    pub fn set_input_default(&mut self, key: &str, value: f32) {
        if value.is_finite() {
            self.defaults.insert(key.to_string(), value);
        }
    }

    pub fn device_binding(&self) -> Option<&DeviceBinding> {
        match &self.kind {
            NodeKind::Device(d) => Some(&d.binding),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ViewState {
    pub pan: [f32; 2],
    pub zoom: f32,
}

impl Default for ViewState {
    fn default() -> Self {
        Self {
            pan: [0.0, 0.0],
            zoom: 1.0,
        }
    }
}

impl ViewState {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn default_version() -> u32 {
    GRAPH_VERSION
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Graph {
    #[serde(default = "default_version")]
    pub version: u32,
    #[serde(default, skip_serializing_if = "ViewState::is_default")]
    pub view: ViewState,
    #[serde(default)]
    pub nodes: Vec<Node>,
    #[serde(default)]
    pub edges: Vec<Edge>,
}

impl Default for Graph {
    fn default() -> Self {
        Self {
            version: GRAPH_VERSION,
            view: ViewState::default(),
            nodes: Vec::new(),
            edges: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConnectError {
    UnknownNode,
    UnknownPin(String),
    SelfLoop,
    Cycle,
}

impl fmt::Display for ConnectError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConnectError::UnknownNode => write!(f, "that node no longer exists"),
            ConnectError::UnknownPin(p) => write!(f, "pin '{p}' does not exist"),
            ConnectError::SelfLoop => write!(f, "a node cannot feed itself"),
            ConnectError::Cycle => write!(
                f,
                "that wire would make a loop — feedback needs a Delay node in the path"
            ),
        }
    }
}

impl Graph {
    pub fn node(&self, id: NodeId) -> Option<&Node> {
        self.nodes.iter().find(|n| n.id == id)
    }

    pub fn node_mut(&mut self, id: NodeId) -> Option<&mut Node> {
        self.nodes.iter_mut().find(|n| n.id == id)
    }

    pub fn contains(&self, id: NodeId) -> bool {
        self.nodes.iter().any(|n| n.id == id)
    }

    pub fn add(&mut self, node: Node) -> NodeId {
        let mut node = node;
        while self.contains(node.id) {
            node.id = NodeId::random();
        }
        let id = node.id;
        self.nodes.push(node);
        id
    }

    pub fn add_kind(&mut self, kind: NodeKind, pos: [f32; 2]) -> NodeId {
        self.add(Node::new(kind, pos))
    }

    pub fn remove_nodes(&mut self, ids: &HashSet<NodeId>) {
        self.nodes.retain(|n| !ids.contains(&n.id));
        self.edges
            .retain(|e| !ids.contains(&e.from.node) && !ids.contains(&e.to.node));
    }

    pub fn source_of(&self, to: &PinRef) -> Option<&Edge> {
        self.edges.iter().find(|e| e.to == *to)
    }

    pub fn is_connected_input(&self, to: &PinRef) -> bool {
        self.source_of(to).is_some()
    }

    pub fn disconnect_input(&mut self, to: &PinRef) -> bool {
        let before = self.edges.len();
        self.edges.retain(|e| e.to != *to);
        self.edges.len() != before
    }

    pub fn connect(&mut self, from: PinRef, to: PinRef) -> Result<(), ConnectError> {
        let (Some(src), Some(dst)) = (self.node(from.node), self.node(to.node)) else {
            return Err(ConnectError::UnknownNode);
        };
        if !src.outputs().iter().any(|p| p.key == from.pin) {
            return Err(ConnectError::UnknownPin(from.pin.clone()));
        }
        if !dst.inputs().iter().any(|p| p.key == to.pin) {
            return Err(ConnectError::UnknownPin(to.pin.clone()));
        }
        if from.node == to.node && !dst.kind.is_delay() {
            return Err(ConnectError::SelfLoop);
        }
        if self.would_cycle(from.node, to.node) {
            return Err(ConnectError::Cycle);
        }
        self.edges.retain(|e| e.to != to);
        self.edges.push(Edge { from, to });
        Ok(())
    }

    pub fn would_cycle(&self, from: NodeId, to: NodeId) -> bool {
        if self.node(to).is_some_and(|n| n.kind.is_delay()) {
            return false;
        }
        if from == to {
            return true;
        }
        let delays: HashSet<NodeId> = self
            .nodes
            .iter()
            .filter(|n| n.kind.is_delay())
            .map(|n| n.id)
            .collect();
        let mut stack = vec![to];
        let mut seen = HashSet::new();
        while let Some(n) = stack.pop() {
            if n == from {
                return true;
            }
            if !seen.insert(n) {
                continue;
            }
            for e in &self.edges {
                if e.from.node == n && !delays.contains(&e.to.node) {
                    stack.push(e.to.node);
                }
            }
        }
        false
    }

    fn retarget_device_pins(&mut self) {
        let mut moves: Vec<(NodeId, String, String)> = Vec::new();
        for n in &self.nodes {
            if !n.kind.is_device() {
                continue;
            }
            let keys: HashSet<String> = n.inputs().into_iter().map(|p| p.key.into_owned()).collect();
            let used = self
                .edges
                .iter()
                .filter(|e| e.to.node == n.id)
                .map(|e| e.to.pin.clone())
                .chain(n.defaults.keys().cloned());
            for old in used {
                if keys.contains(&old) {
                    continue;
                }
                let split = format!("{old}1");
                let joined = old.strip_suffix('1').map(str::to_string);
                if keys.contains(&split) {
                    moves.push((n.id, old, split));
                } else if let Some(j) = joined.filter(|j| keys.contains(j)) {
                    moves.push((n.id, old, j));
                }
            }
        }
        for (id, old, new) in moves {
            for e in &mut self.edges {
                if e.to.node == id && e.to.pin == old {
                    e.to.pin = new.clone();
                }
            }
            if let Some(n) = self.node_mut(id)
                && let Some(v) = n.defaults.remove(&old)
            {
                n.defaults.entry(new).or_insert(v);
            }
        }
    }

    pub fn prune_dangling_edges(&mut self) -> usize {
        self.retarget_device_pins();
        let mut inputs: HashMap<NodeId, Option<Vec<String>>> = HashMap::new();
        let mut outputs: HashMap<NodeId, Option<Vec<String>>> = HashMap::new();
        for n in &self.nodes {
            let missing = n.kind.is_missing();
            inputs.insert(
                n.id,
                (!missing).then(|| n.inputs().into_iter().map(|p| p.key.into_owned()).collect()),
            );
            outputs.insert(
                n.id,
                (!missing).then(|| n.outputs().into_iter().map(|p| p.key.into_owned()).collect()),
            );
        }
        let before = self.edges.len();
        self.edges.retain(|e| {
            let src_ok = match outputs.get(&e.from.node) {
                Some(Some(keys)) => keys.contains(&e.from.pin),
                Some(None) => true,
                None => false,
            };
            let dst_ok = match inputs.get(&e.to.node) {
                Some(Some(keys)) => keys.contains(&e.to.pin),
                Some(None) => true,
                None => false,
            };
            src_ok && dst_ok
        });
        before - self.edges.len()
    }

    pub fn bounds(&self) -> Option<([f32; 2], [f32; 2])> {
        let mut it = self.nodes.iter();
        let first = it.next()?;
        let mut min = first.pos;
        let mut max = first.pos;
        for n in it {
            min[0] = min[0].min(n.pos[0]);
            min[1] = min[1].min(n.pos[1]);
            max[0] = max[0].max(n.pos[0]);
            max[1] = max[1].max(n.pos[1]);
        }
        Some((min, max))
    }

    pub fn device_node_for(&self, mac: &str) -> Option<&Node> {
        self.nodes.iter().find(|n| {
            matches!(n.device_binding(), Some(DeviceBinding::Mac(m)) if m.eq_ignore_ascii_case(mac))
        })
    }

    pub fn ensure_device_nodes(&mut self, devices: &[(String, String)]) -> Vec<NodeId> {
        let mut added = Vec::new();
        for (mac, name) in devices {
            if self.device_node_for(mac).is_some() {
                continue;
            }
            let (x, y) = match self.bounds() {
                Some((_, max)) => (max[0] + 280.0, self.free_device_row()),
                None => (420.0, 40.0),
            };
            let mut node = Node::new(
                NodeKind::Device(super::kind::DeviceParams::bound_to(mac)),
                [x.max(420.0), y],
            );
            node.title = device_title(name, mac);
            added.push(self.add(node));
        }
        added
    }

    fn free_device_row(&self) -> f32 {
        let devices = self
            .nodes
            .iter()
            .filter(|n| matches!(n.kind, NodeKind::Device(_)))
            .count();
        40.0 + devices as f32 * 420.0
    }

    pub fn extract(&self, ids: &HashSet<NodeId>) -> Graph {
        Graph {
            version: GRAPH_VERSION,
            view: ViewState::default(),
            nodes: self
                .nodes
                .iter()
                .filter(|n| ids.contains(&n.id))
                .cloned()
                .collect(),
            edges: self
                .edges
                .iter()
                .filter(|e| ids.contains(&e.from.node) && ids.contains(&e.to.node))
                .cloned()
                .collect(),
        }
    }

    pub fn insert(&mut self, other: &Graph, offset: [f32; 2]) -> Vec<NodeId> {
        let mut remap = HashMap::new();
        let mut ids = Vec::with_capacity(other.nodes.len());
        for n in &other.nodes {
            let mut copy = n.clone();
            copy.id = NodeId::random();
            copy.pos = [n.pos[0] + offset[0], n.pos[1] + offset[1]];
            let id = self.add(copy);
            remap.insert(n.id, id);
            ids.push(id);
        }
        for e in &other.edges {
            if let (Some(&from), Some(&to)) = (remap.get(&e.from.node), remap.get(&e.to.node)) {
                self.edges.retain(|x| !(x.to.node == to && x.to.pin == e.to.pin));
                self.edges.push(Edge {
                    from: PinRef::new(from, e.from.pin.clone()),
                    to: PinRef::new(to, e.to.pin.clone()),
                });
            }
        }
        ids
    }

    pub fn scope(&self, path: &[NodeId]) -> Option<&Graph> {
        let mut g = self;
        for id in path {
            match &g.node(*id)?.kind {
                NodeKind::Group(p) => g = &p.graph,
                _ => return None,
            }
        }
        Some(g)
    }

    pub fn scope_mut(&mut self, path: &[NodeId]) -> Option<&mut Graph> {
        let mut g = self;
        for id in path {
            match &mut g.node_mut(*id)?.kind {
                NodeKind::Group(p) => g = &mut p.graph,
                _ => return None,
            }
        }
        Some(g)
    }

    pub fn group(&mut self, ids: &HashSet<NodeId>, name: &str) -> Option<NodeId> {
        let ids: HashSet<NodeId> = ids
            .iter()
            .copied()
            .filter(|id| {
                self.node(*id)
                    .is_some_and(|n| !matches!(n.kind, NodeKind::Device(_)))
            })
            .collect();
        if ids.is_empty() {
            return None;
        }
        let mut inner = self.extract(&ids);
        let (min, max) = inner.bounds()?;
        let centre = [(min[0] + max[0]) * 0.5, (min[1] + max[1]) * 0.5];

        let incoming: Vec<Edge> = self
            .edges
            .iter()
            .filter(|e| !ids.contains(&e.from.node) && ids.contains(&e.to.node))
            .cloned()
            .collect();
        let outgoing: Vec<Edge> = self
            .edges
            .iter()
            .filter(|e| ids.contains(&e.from.node) && !ids.contains(&e.to.node))
            .cloned()
            .collect();

        let mut in_ports = Vec::new();
        let mut in_map: Vec<(PinRef, String)> = Vec::new();
        for e in &incoming {
            if in_map.iter().any(|(src, _)| *src == e.from) {
                continue;
            }
            let k = format!("in{}", in_ports.len() + 1);
            let label = self
                .node(e.to.node)
                .and_then(|n| n.inputs().into_iter().find(|p| p.key == e.to.pin))
                .map(|p| p.label.into_owned())
                .unwrap_or_else(|| k.clone());
            in_ports.push(super::kind::GroupPort { key: k.clone(), label });
            in_map.push((e.from.clone(), k));
        }
        let mut out_ports = Vec::new();
        let mut out_map: Vec<(PinRef, String)> = Vec::new();
        for e in &outgoing {
            if out_map.iter().any(|(src, _)| *src == e.from) {
                continue;
            }
            let k = format!("out{}", out_ports.len() + 1);
            let label = self
                .node(e.from.node)
                .and_then(|n| n.outputs().into_iter().find(|p| p.key == e.from.pin))
                .map(|p| p.label.into_owned())
                .unwrap_or_else(|| k.clone());
            out_ports.push(super::kind::GroupPort { key: k.clone(), label });
            out_map.push((e.from.clone(), k));
        }

        let gin = inner.add_kind(
            NodeKind::GroupInput(super::kind::PortsParams { ports: in_ports }),
            [min[0] - 240.0, centre[1]],
        );
        let gout = inner.add_kind(
            NodeKind::GroupOutput(super::kind::PortsParams { ports: out_ports }),
            [max[0] + 240.0, centre[1]],
        );
        for e in &incoming {
            let key = in_map.iter().find(|(s, _)| *s == e.from).map(|(_, k)| k.clone());
            if let Some(k) = key {
                inner.edges.push(Edge {
                    from: PinRef::new(gin, k),
                    to: e.to.clone(),
                });
            }
        }
        for (src, k) in &out_map {
            inner.edges.push(Edge {
                from: src.clone(),
                to: PinRef::new(gout, k.clone()),
            });
        }

        self.remove_nodes(&ids);
        let group = self.add_kind(
            NodeKind::Group(super::kind::GroupParams {
                name: name.to_string(),
                graph: Box::new(inner),
            }),
            centre,
        );
        for e in incoming {
            let key = in_map.iter().find(|(s, _)| *s == e.from).map(|(_, k)| k.clone());
            if let Some(k) = key
                && !self.edges.iter().any(|x| x.to.node == group && x.to.pin == k)
            {
                self.edges.push(Edge {
                    from: e.from,
                    to: PinRef::new(group, k),
                });
            }
        }
        for e in outgoing {
            if let Some((_, k)) = out_map.iter().find(|(s, _)| *s == e.from) {
                self.edges.push(Edge {
                    from: PinRef::new(group, k.clone()),
                    to: e.to,
                });
            }
        }
        Some(group)
    }

    pub fn ungroup(&mut self, id: NodeId) -> Vec<NodeId> {
        let Some(node) = self.node(id).cloned() else {
            return Vec::new();
        };
        let NodeKind::Group(params) = node.kind else {
            return Vec::new();
        };
        let inner = *params.graph;
        let gin = inner
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::GroupInput(_)))
            .map(|n| n.id);
        let gout = inner
            .nodes
            .iter()
            .find(|n| matches!(n.kind, NodeKind::GroupOutput(_)))
            .map(|n| n.id);

        let body_ids: HashSet<NodeId> = inner
            .nodes
            .iter()
            .filter(|n| Some(n.id) != gin && Some(n.id) != gout)
            .map(|n| n.id)
            .collect();
        let body = inner.extract(&body_ids);
        let centre = node.pos;
        let offset = match body.bounds() {
            Some((min, max)) => [
                centre[0] - (min[0] + max[0]) * 0.5,
                centre[1] - (min[1] + max[1]) * 0.5,
            ],
            None => [0.0, 0.0],
        };
        let new_ids = self.insert(&body, offset);
        let remap: HashMap<NodeId, NodeId> = body
            .nodes
            .iter()
            .map(|n| n.id)
            .zip(new_ids.iter().copied())
            .collect();

        let outer_in: Vec<Edge> = self.edges.iter().filter(|e| e.to.node == id).cloned().collect();
        let outer_out: Vec<Edge> = self.edges.iter().filter(|e| e.from.node == id).cloned().collect();

        for e in &inner.edges {
            if Some(e.from.node) == gin
                && let Some(&to) = remap.get(&e.to.node)
                && let Some(src) = outer_in.iter().find(|o| o.to.pin == e.from.pin)
            {
                self.edges.push(Edge {
                    from: src.from.clone(),
                    to: PinRef::new(to, e.to.pin.clone()),
                });
            }
            if Some(e.to.node) == gout
                && let Some(&from) = remap.get(&e.from.node)
            {
                for o in outer_out.iter().filter(|o| o.from.pin == e.to.pin) {
                    self.edges.push(Edge {
                        from: PinRef::new(from, e.from.pin.clone()),
                        to: o.to.clone(),
                    });
                }
            }
        }
        let mut gone = HashSet::new();
        gone.insert(id);
        self.remove_nodes(&gone);
        new_ids
    }

    pub fn sanitize(&mut self) {
        let mut seen = HashSet::new();
        for n in &mut self.nodes {
            if !seen.insert(n.id) {
                n.id = NodeId::random();
                seen.insert(n.id);
            }
            if !n.pos[0].is_finite() || !n.pos[1].is_finite() {
                n.pos = [0.0, 0.0];
            }
            n.defaults.retain(|_, v| v.is_finite());
            n.kind.sanitize();
        }
        if !self.view.zoom.is_finite() || self.view.zoom <= 0.0 {
            self.view.zoom = 1.0;
        }
        if !self.view.pan[0].is_finite() || !self.view.pan[1].is_finite() {
            self.view.pan = [0.0, 0.0];
        }
    }

    pub fn missing_types(&self) -> Vec<String> {
        let mut out = Vec::new();
        for n in &self.nodes {
            match &n.kind {
                NodeKind::Missing(m) => out.push(m.type_name.clone()),
                NodeKind::Group(g) => out.extend(g.graph.missing_types()),
                _ => {}
            }
        }
        out
    }
}

pub fn device_title(name: &str, mac: &str) -> String {
    let clean: String = mac.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let tail = &clean[clean.len().saturating_sub(4)..];
    if name.trim().is_empty() {
        format!("Coyote {}", tail.to_uppercase())
    } else {
        format!("{} {}", name.trim(), tail.to_uppercase())
    }
}

pub fn parse_graph(json: &str) -> Result<Graph, String> {
    let mut g: Graph = serde_json::from_str(json).map_err(|e| format!("invalid graph: {e}"))?;
    g.sanitize();
    Ok(g)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::graph::kind::{MathOp, MathParams, NoParams};

    fn add(op: MathOp) -> NodeKind {
        NodeKind::Math(MathParams { op })
    }

    #[test]
    fn connect_replaces_an_existing_input_wire() {
        let mut g = Graph::default();
        let a = g.add_kind(NodeKind::Time(NoParams {}), [0.0, 0.0]);
        let b = g.add_kind(NodeKind::Time(NoParams {}), [0.0, 0.0]);
        let m = g.add_kind(add(MathOp::Add), [0.0, 0.0]);
        g.connect(PinRef::new(a, "t"), PinRef::new(m, "a")).unwrap();
        g.connect(PinRef::new(b, "t"), PinRef::new(m, "a")).unwrap();
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].from.node, b);
    }

    #[test]
    fn cycles_are_rejected_at_connect_time() {
        let mut g = Graph::default();
        let x = g.add_kind(add(MathOp::Add), [0.0, 0.0]);
        let y = g.add_kind(add(MathOp::Add), [0.0, 0.0]);
        g.connect(PinRef::new(x, "out"), PinRef::new(y, "a")).unwrap();
        assert_eq!(
            g.connect(PinRef::new(y, "out"), PinRef::new(x, "a")),
            Err(ConnectError::Cycle)
        );
        assert_eq!(
            g.connect(PinRef::new(x, "out"), PinRef::new(x, "b")),
            Err(ConnectError::SelfLoop)
        );
    }

    #[test]
    fn feedback_through_a_delay_is_allowed() {
        use crate::graph::kind::DelayParams;
        let mut g = Graph::default();
        let x = g.add_kind(add(MathOp::Add), [0.0, 0.0]);
        let d = g.add_kind(NodeKind::Delay(DelayParams::default()), [0.0, 0.0]);
        g.connect(PinRef::new(x, "out"), PinRef::new(d, "in")).unwrap();
        g.connect(PinRef::new(d, "out"), PinRef::new(x, "a")).unwrap();
        assert_eq!(g.edges.len(), 2);
    }

    #[test]
    fn unknown_pins_are_rejected() {
        let mut g = Graph::default();
        let x = g.add_kind(add(MathOp::Add), [0.0, 0.0]);
        let y = g.add_kind(add(MathOp::Add), [0.0, 0.0]);
        assert!(matches!(
            g.connect(PinRef::new(x, "nope"), PinRef::new(y, "a")),
            Err(ConnectError::UnknownPin(_))
        ));
    }

    #[test]
    fn group_and_ungroup_keep_the_wiring() {
        let mut g = Graph::default();
        let t = g.add_kind(NodeKind::Time(NoParams {}), [0.0, 0.0]);
        let m1 = g.add_kind(add(MathOp::Add), [200.0, 0.0]);
        let m2 = g.add_kind(add(MathOp::Multiply), [400.0, 0.0]);
        let sink = g.add_kind(add(MathOp::Add), [600.0, 0.0]);
        g.connect(PinRef::new(t, "t"), PinRef::new(m1, "a")).unwrap();
        g.connect(PinRef::new(m1, "out"), PinRef::new(m2, "a")).unwrap();
        g.connect(PinRef::new(m2, "out"), PinRef::new(sink, "a")).unwrap();

        let sel: HashSet<NodeId> = [m1, m2].into_iter().collect();
        let group = g.group(&sel, "pair").expect("group");
        assert_eq!(g.nodes.len(), 3);
        assert!(g.edges.iter().any(|e| e.to.node == group && e.from.node == t));
        assert!(g.edges.iter().any(|e| e.from.node == group && e.to.node == sink));

        let ids = g.ungroup(group);
        assert_eq!(ids.len(), 2);
        assert_eq!(g.nodes.len(), 4);
        assert_eq!(g.edges.len(), 3, "both crossing wires and the inner wire survive");
    }

    #[test]
    fn expanding_a_device_pin_keeps_its_wire_on_segment_one() {
        let mut g = Graph::default();
        let t = g.add_kind(NodeKind::Time(NoParams {}), [0.0, 0.0]);
        let d = g.add_kind(
            NodeKind::Device(crate::graph::kind::DeviceParams::bound_to("AA")),
            [0.0, 0.0],
        );
        g.connect(PinRef::new(t, "t"), PinRef::new(d, "a.freq")).unwrap();
        let set = |g: &mut Graph, on: bool| {
            if let NodeKind::Device(p) = &mut g.node_mut(d).unwrap().kind {
                p.channels[0].set_expanded("freq", on);
            }
            g.prune_dangling_edges();
        };
        set(&mut g, true);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].to.pin, "a.freq1");
        set(&mut g, false);
        assert_eq!(g.edges.len(), 1);
        assert_eq!(g.edges[0].to.pin, "a.freq");
    }

    #[test]
    fn device_nodes_are_added_once_per_mac() {
        let mut g = Graph::default();
        let devs = vec![("AA:BB".to_string(), "Coyote".to_string())];
        assert_eq!(g.ensure_device_nodes(&devs).len(), 1);
        assert!(g.ensure_device_nodes(&devs).is_empty());
        assert!(g.device_node_for("aa:bb").is_some());
    }

    #[test]
    fn malformed_json_is_an_error_not_a_panic() {
        assert!(parse_graph("{ nope").is_err());
        assert!(parse_graph(r#"{"nodes": 5}"#).is_err());
        let g = parse_graph("{}").expect("empty object is an empty graph");
        assert!(g.nodes.is_empty());
    }

    #[test]
    fn unknown_node_types_load_as_placeholders_and_round_trip() {
        let json = r#"{"version":1,"nodes":[
            {"id":7,"pos":[1,2],"kind":{"type":"FutureThing","knob":3}},
            {"id":8,"kind":{"type":"Time"}}
        ],"edges":[{"from":{"node":7,"pin":"x"},"to":{"node":8,"pin":"y"}}]}"#;
        let g = parse_graph(json).expect("parse");
        assert_eq!(g.missing_types(), vec!["FutureThing".to_string()]);
        let out = serde_json::to_string(&g).unwrap();
        assert!(out.contains("FutureThing") && out.contains("\"knob\":3"));
        assert_eq!(g.edges.len(), 1, "wires to a placeholder are preserved");
    }
}
