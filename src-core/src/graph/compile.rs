use std::collections::{BTreeMap, HashMap, HashSet, VecDeque};
use std::hash::{DefaultHasher, Hash, Hasher};

use crate::modulation::ModulationConfig;

use super::device::DeviceBinding;
use super::kind::{ContactParams, NodeKind, OscFloatParams, PinSpec, PortsParams};
use super::model::{Graph, NodeId};

const MAX_GROUP_DEPTH: usize = 8;

pub fn flat_id(path: &[NodeId]) -> u64 {
    match path {
        [] => 0,
        [one] => one.0,
        _ => {
            let mut h: u64 = 0xcbf2_9ce4_8422_2325;
            for id in path {
                h ^= id.0;
                h = h.wrapping_mul(0x0000_0100_0000_01b3);
                h ^= h >> 29;
            }
            h | (1 << 63)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Severity {
    Error,
    Warning,
    Info,
}

#[derive(Debug, Clone, PartialEq)]
pub struct GraphIssue {
    pub severity: Severity,
    pub node: Option<NodeId>,
    pub message: String,
}

impl GraphIssue {
    fn new(severity: Severity, node: Option<NodeId>, message: impl Into<String>) -> Self {
        Self {
            severity,
            node,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Src {
    Slot(u32),
    Const(f32),
}

#[derive(Debug, Clone)]
pub enum Extra {
    None,
    External(usize),
    Modulation(Box<ModulationConfig>),
    Device(usize),
}

#[derive(Debug, Clone)]
pub struct CNode {
    pub flat: u64,
    pub root: NodeId,
    pub kind: NodeKind,
    pub extra: Extra,
    pub in_start: u32,
    pub in_len: u32,
    pub out_start: u32,
    pub out_len: u32,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ExternalRequest {
    Contact(ContactParams),
    Osc(OscFloatParams),
}

#[derive(Debug, Clone, Default)]
pub struct Plan {
    pub nodes: Vec<CNode>,
    pub inputs: Vec<Src>,
    pub value_count: usize,
    pub delays: Vec<usize>,
    pub external: Vec<ExternalRequest>,
    pub devices: Vec<(NodeId, DeviceBinding)>,
    pub issues: Vec<GraphIssue>,
    pub valid: bool,
    pub signature: u64,
}

impl Plan {
    pub fn has_errors(&self) -> bool {
        self.issues.iter().any(|i| i.severity == Severity::Error)
    }
}

struct Flat {
    flat: u64,
    root: NodeId,
    kind: NodeKind,
    defaults: BTreeMap<String, f32>,
    ins: Vec<PinSpec>,
    outs: Vec<PinSpec>,
}

struct FlatEdge {
    from: (u64, String),
    to: (u64, String),
}

fn passthrough_pins(p: &PortsParams) -> Vec<PinSpec> {
    p.ports
        .iter()
        .map(|port| PinSpec {
            key: port.key.clone().into(),
            label: port.label.clone().into(),
            ty: super::kind::PinType::Float,
            default: 0.0,
            range: (0.0, 1.0),
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
fn flatten(
    g: &Graph,
    path: &mut Vec<NodeId>,
    root: Option<NodeId>,
    group_defaults: Option<&BTreeMap<String, f32>>,
    out: &mut Vec<Flat>,
    edges: &mut Vec<FlatEdge>,
    issues: &mut Vec<GraphIssue>,
) {
    let mut alias: HashMap<NodeId, (Option<u64>, Option<u64>)> = HashMap::new();

    for node in &g.nodes {
        path.push(node.id);
        let flat = flat_id(path);
        let owner = root.unwrap_or(node.id);
        match &node.kind {
            NodeKind::Group(p) => {
                if path.len() > MAX_GROUP_DEPTH {
                    issues.push(GraphIssue::new(
                        Severity::Error,
                        Some(owner),
                        "Groups are nested too deeply",
                    ));
                } else {
                    flatten(&p.graph, path, Some(owner), Some(&node.defaults), out, edges, issues);
                    let mut find = |input: bool| {
                        p.graph
                            .nodes
                            .iter()
                            .find(|n| match &n.kind {
                                NodeKind::GroupInput(_) => input,
                                NodeKind::GroupOutput(_) => !input,
                                _ => false,
                            })
                            .map(|n| {
                                path.push(n.id);
                                let f = flat_id(path);
                                path.pop();
                                f
                            })
                    };
                    alias.insert(node.id, (find(true), find(false)));
                }
            }
            NodeKind::Frame(_) => {}
            NodeKind::Device(_) if root.is_some() => issues.push(GraphIssue::new(
                Severity::Warning,
                Some(owner),
                "A Device node inside a group is ignored — keep devices at the top level",
            )),
            NodeKind::GroupInput(p) => {
                let pins = passthrough_pins(p);
                out.push(Flat {
                    flat,
                    root: owner,
                    kind: node.kind.clone(),
                    defaults: group_defaults.cloned().unwrap_or_default(),
                    ins: pins.clone(),
                    outs: pins,
                });
            }
            NodeKind::GroupOutput(p) => {
                let pins = passthrough_pins(p);
                out.push(Flat {
                    flat,
                    root: owner,
                    kind: node.kind.clone(),
                    defaults: BTreeMap::new(),
                    ins: pins.clone(),
                    outs: pins,
                });
            }
            kind => {
                if let NodeKind::Missing(m) = kind {
                    issues.push(GraphIssue::new(
                        Severity::Warning,
                        Some(owner),
                        format!(
                            "Unknown node type '{}' — kept as a placeholder, it outputs nothing",
                            m.type_name
                        ),
                    ));
                }
                out.push(Flat {
                    flat,
                    root: owner,
                    kind: kind.clone(),
                    defaults: node.defaults.clone(),
                    ins: kind.inputs(),
                    outs: kind.outputs(),
                });
            }
        }
        path.pop();
    }

    for e in &g.edges {
        let from = match alias.get(&e.from.node) {
            Some((_, Some(out_id))) => Some(*out_id),
            Some((_, None)) => None,
            None => {
                path.push(e.from.node);
                let f = flat_id(path);
                path.pop();
                Some(f)
            }
        };
        let to = match alias.get(&e.to.node) {
            Some((Some(in_id), _)) => Some(*in_id),
            Some((None, _)) => None,
            None => {
                path.push(e.to.node);
                let f = flat_id(path);
                path.pop();
                Some(f)
            }
        };
        if let (Some(from), Some(to)) = (from, to) {
            edges.push(FlatEdge {
                from: (from, e.from.pin.clone()),
                to: (to, e.to.pin.clone()),
            });
        }
    }
}

pub fn compile(graph: &Graph) -> Plan {
    let mut issues = Vec::new();
    let mut flats = Vec::new();
    let mut fedges = Vec::new();
    flatten(graph, &mut Vec::new(), None, None, &mut flats, &mut fedges, &mut issues);

    let index: HashMap<u64, usize> = flats.iter().enumerate().map(|(i, f)| (f.flat, i)).collect();

    let mut out_start = Vec::with_capacity(flats.len());
    let mut slots = 0u32;
    for f in &flats {
        out_start.push(slots);
        slots += f.outs.len() as u32;
    }

    let mut feeds: HashMap<(usize, String), u32> = HashMap::new();
    let mut deps: Vec<Vec<usize>> = vec![Vec::new(); flats.len()];
    let mut indeg = vec![0usize; flats.len()];
    let mut dangling = 0usize;
    for e in &fedges {
        let (Some(&fi), Some(&ti)) = (index.get(&e.from.0), index.get(&e.to.0)) else {
            dangling += 1;
            continue;
        };
        if flats[fi].kind.is_missing() || flats[ti].kind.is_missing() {
            continue;
        }
        let Some(oi) = flats[fi].outs.iter().position(|p| p.key == e.from.1) else {
            dangling += 1;
            continue;
        };
        if !flats[ti].ins.iter().any(|p| p.key == e.to.1) {
            dangling += 1;
            continue;
        }
        let key = (ti, e.to.1.clone());
        if feeds.contains_key(&key) {
            issues.push(GraphIssue::new(
                Severity::Warning,
                Some(flats[ti].root),
                format!("Input '{}' has more than one wire — the first one is used", e.to.1),
            ));
            continue;
        }
        feeds.insert(key, out_start[fi] + oi as u32);
        if !flats[ti].kind.is_delay() && fi != ti {
            deps[fi].push(ti);
            indeg[ti] += 1;
        } else if fi == ti && !flats[ti].kind.is_delay() {
            indeg[ti] += 1;
        }
    }
    if dangling > 0 {
        issues.push(GraphIssue::new(
            Severity::Warning,
            None,
            format!("{dangling} wire(s) point at pins that no longer exist and were ignored"),
        ));
    }

    let mut order = Vec::with_capacity(flats.len());
    let mut indeg_left = indeg.clone();
    let mut queue: VecDeque<usize> = (0..flats.len()).filter(|&i| indeg_left[i] == 0).collect();
    while let Some(i) = queue.pop_front() {
        order.push(i);
        for &t in &deps[i] {
            indeg_left[t] -= 1;
            if indeg_left[t] == 0 {
                queue.push_back(t);
            }
        }
    }
    let valid = order.len() == flats.len();
    if !valid {
        let stuck: HashSet<NodeId> = (0..flats.len())
            .filter(|&i| indeg_left[i] > 0)
            .map(|i| flats[i].root)
            .collect();
        for id in stuck {
            issues.push(GraphIssue::new(
                Severity::Error,
                Some(id),
                "Part of a loop — add a Delay node to the feedback path. Output is 0.",
            ));
        }
        for i in 0..flats.len() {
            if !order.contains(&i) {
                order.push(i);
            }
        }
    }

    let mut plan = Plan {
        value_count: slots as usize,
        valid,
        ..Plan::default()
    };
    let mut seen_macs: HashSet<String> = HashSet::new();
    let mut wired_devices = 0usize;
    let top_level: HashSet<NodeId> = graph.nodes.iter().map(|n| n.id).collect();

    for &i in &order {
        let f = &flats[i];
        let in_start = plan.inputs.len() as u32;
        let mut any_wired = false;
        for spec in &f.ins {
            match feeds.get(&(i, spec.key.to_string())) {
                Some(&slot) => {
                    any_wired = true;
                    plan.inputs.push(Src::Slot(slot));
                }
                None => {
                    let v = f
                        .defaults
                        .get(spec.key.as_ref())
                        .copied()
                        .filter(|v| v.is_finite())
                        .unwrap_or(spec.default);
                    plan.inputs.push(Src::Const(v));
                }
            }
        }
        let extra = match &f.kind {
            NodeKind::Contact(p) => {
                plan.external.push(ExternalRequest::Contact(p.clone()));
                Extra::External(plan.external.len() - 1)
            }
            NodeKind::OscFloat(p) => {
                plan.external.push(ExternalRequest::Osc(p.clone()));
                Extra::External(plan.external.len() - 1)
            }
            NodeKind::Function(p) => Extra::Modulation(Box::new(p.to_modulation())),
            NodeKind::Device(p) if top_level.contains(&f.root) => {
                if any_wired {
                    wired_devices += 1;
                }
                let duplicate = match &p.binding {
                    DeviceBinding::Mac(m) => !seen_macs.insert(m.to_uppercase()),
                    _ => false,
                };
                if duplicate {
                    issues.push(GraphIssue::new(
                        Severity::Warning,
                        Some(f.root),
                        "Another Device node already drives this device — this one is ignored",
                    ));
                    Extra::None
                } else {
                    if p.binding == DeviceBinding::Unbound {
                        issues.push(GraphIssue::new(
                            Severity::Info,
                            Some(f.root),
                            "Device node is not bound — pick a device on the node",
                        ));
                    }
                    plan.devices.push((f.root, p.binding.clone()));
                    Extra::Device(plan.devices.len() - 1)
                }
            }
            _ => Extra::None,
        };
        if f.kind.is_delay() {
            plan.delays.push(plan.nodes.len());
        }
        plan.nodes.push(CNode {
            flat: f.flat,
            root: f.root,
            kind: f.kind.clone(),
            extra,
            in_start,
            in_len: f.ins.len() as u32,
            out_start: out_start[i],
            out_len: f.outs.len() as u32,
        });
    }

    if plan.devices.is_empty() {
        issues.push(GraphIssue::new(
            Severity::Info,
            None,
            "No Device node yet — connect a Coyote or add one from the Output menu",
        ));
    } else if wired_devices == 0 {
        issues.push(GraphIssue::new(
            Severity::Warning,
            None,
            "Nothing is wired into a Device node — every output stays at 0",
        ));
    }

    let mut h = DefaultHasher::new();
    let mut ids: Vec<(u64, &str)> = flats.iter().map(|f| (f.flat, f.kind.type_name())).collect();
    ids.sort_unstable();
    ids.hash(&mut h);
    let mut wires: Vec<(u64, &str, u64, &str)> = fedges
        .iter()
        .map(|e| (e.from.0, e.from.1.as_str(), e.to.0, e.to.1.as_str()))
        .collect();
    wires.sort_unstable();
    wires.hash(&mut h);
    plan.signature = h.finish();

    issues.sort_by_key(|i| i.severity);
    plan.issues = issues;
    plan
}
