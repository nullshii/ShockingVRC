pub mod canvas;
pub mod menu;
pub mod node_ui;
pub mod side;

use std::collections::{BTreeSet, HashMap, HashSet, VecDeque};
use std::time::{Duration, Instant};

use shocking_vrc_app::app_state::DeviceSlotInfo;
use shocking_vrc_core::graph::{
    DeviceBinding, Graph, GraphPreset, GraphPresetKind, Node, NodeId, NodeKind, flat_id,
};

use crate::bridge::{Bridge, Command, GraphDocShared, Snapshot, ToastKind};
use crate::theme::Palette;

pub const UNDO_DEPTH: usize = 128;
const SEND_INTERVAL: Duration = Duration::from_millis(33);
const VIEW_SETTLE: Duration = Duration::from_millis(400);
pub const TRACE_LEN: usize = 120;
pub const GRID: f32 = 20.0;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SideTab {
    #[default]
    Inspector,
    Presets,
    Devices,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PinEnd {
    pub node: NodeId,
    pub key: String,
    pub output: bool,
}

#[derive(Debug, Clone, Default)]
pub enum Interaction {
    #[default]
    None,
    MoveNodes {
        moved: bool,
    },
    BoxSelect {
        start: egui::Pos2,
        additive: bool,
    },
    Wire {
        from: PinEnd,
    },
    Cut {
        points: Vec<egui::Pos2>,
    },
    Pan,
}

#[derive(Debug, Clone)]
pub struct AddMenu {
    pub screen: egui::Pos2,
    pub at: egui::Pos2,
    pub filter: String,
    pub from_wire: Option<PinEnd>,
    pub focus: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Typing {
    pub node: NodeId,
    pub key: String,
    pub buf: String,
}

#[derive(Default)]
struct History {
    undo: VecDeque<Graph>,
    redo: Vec<Graph>,
}

impl History {
    fn push(&mut self, g: Graph) {
        self.undo.push_back(g);
        while self.undo.len() > UNDO_DEPTH {
            self.undo.pop_front();
        }
        self.redo.clear();
    }
}

pub struct GraphEditor {
    pub doc: Graph,
    initialized: bool,
    synced_rev: u64,
    synced_replaced: u64,
    pub path: Vec<NodeId>,
    pub selection: BTreeSet<NodeId>,
    history: History,
    gesture_open: bool,
    dirty: bool,
    last_send: Option<Instant>,
    view_changed: Option<Instant>,
    clipboard: Option<Graph>,
    pub interaction: Interaction,
    pub menu: Option<AddMenu>,
    pub side_tab: SideTab,
    pub preset_filter: String,
    pub preset_mine_only: bool,
    pub sel_preset: Option<String>,
    pub typing: Option<Typing>,
    pub snap_grid: bool,
    pub minimap: bool,
    pub traces: HashMap<(u64, usize), VecDeque<f32>>,
    last_trace_tick: u64,
    pub canvas_rect: egui::Rect,
    pub side_rect: egui::Rect,
    pub pointer_canvas: Option<egui::Pos2>,
    pub focus_request: Option<NodeId>,
    pub frame_request: bool,
}

impl Default for GraphEditor {
    fn default() -> Self {
        Self {
            doc: Graph::default(),
            initialized: false,
            synced_rev: 0,
            synced_replaced: 0,
            path: Vec::new(),
            selection: BTreeSet::new(),
            history: History::default(),
            gesture_open: false,
            dirty: false,
            last_send: None,
            view_changed: None,
            clipboard: None,
            interaction: Interaction::None,
            menu: None,
            side_tab: SideTab::Inspector,
            preset_filter: String::new(),
            preset_mine_only: false,
            sel_preset: None,
            typing: None,
            snap_grid: false,
            minimap: true,
            traces: HashMap::new(),
            last_trace_tick: 0,
            canvas_rect: egui::Rect::NOTHING,
            side_rect: egui::Rect::NOTHING,
            pointer_canvas: None,
            focus_request: None,
            frame_request: false,
        }
    }
}

pub fn scope_of<'a>(doc: &'a Graph, path: &[NodeId]) -> &'a Graph {
    doc.scope(path).unwrap_or(doc)
}

pub struct Env<'a> {
    pub pal: &'a Palette,
    pub snap: &'a Snapshot,
    pub bridge: &'a Bridge,
}

impl Env<'_> {
    pub fn toast(&self, kind: ToastKind, text: impl Into<String>) {
        self.bridge.shared.notify(kind, text);
    }
}

impl GraphEditor {
    pub fn initialized(&self) -> bool {
        self.initialized
    }

    pub fn scope(&self) -> &Graph {
        scope_of(&self.doc, &self.path)
    }

    pub fn scope_mut(&mut self) -> &mut Graph {
        if self.doc.scope(&self.path).is_none() {
            self.path.clear();
        }
        let GraphEditor { doc, path, .. } = self;
        if path.is_empty() {
            return doc;
        }
        doc.scope_mut(path).expect("path was validated just above")
    }

    pub fn flat(&self, id: NodeId) -> u64 {
        let mut p = self.path.clone();
        p.push(id);
        flat_id(&p)
    }

    pub fn dismiss(&mut self) -> bool {
        if self.menu.take().is_some() || self.typing.take().is_some() {
            return true;
        }
        if !matches!(self.interaction, Interaction::None) {
            self.interaction = Interaction::None;
            return true;
        }
        false
    }

    pub fn begin_edit(&mut self) {
        if !self.gesture_open {
            self.history.push(self.doc.clone());
            self.gesture_open = true;
        }
    }

    pub fn edit<R>(&mut self, f: impl FnOnce(&mut Graph) -> R) -> R {
        self.begin_edit();
        let r = f(self.scope_mut());
        self.dirty = true;
        r
    }

    pub fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    pub fn view_changed(&mut self) {
        self.view_changed = Some(Instant::now());
    }

    pub fn frame_begin(&mut self, ctx: &egui::Context) {
        let busy = ctx.input(|i| i.pointer.any_down()) || ctx.egui_wants_keyboard_input();
        if !busy {
            self.gesture_open = false;
        }
        if self.doc.scope(&self.path).is_none() {
            self.path.clear();
        }
        let scope_ids: HashSet<NodeId> = self.scope().nodes.iter().map(|n| n.id).collect();
        self.selection.retain(|id| scope_ids.contains(id));
    }

    pub fn can_undo(&self) -> bool {
        !self.history.undo.is_empty()
    }

    pub fn can_redo(&self) -> bool {
        !self.history.redo.is_empty()
    }

    pub fn undo(&mut self) {
        if let Some(prev) = self.history.undo.pop_back() {
            let now = std::mem::replace(&mut self.doc, prev);
            self.history.redo.push(now);
            self.after_restore();
        }
    }

    pub fn redo(&mut self) {
        if let Some(next) = self.history.redo.pop() {
            let now = std::mem::replace(&mut self.doc, next);
            self.history.undo.push_back(now);
            self.after_restore();
        }
    }

    fn after_restore(&mut self) {
        self.gesture_open = false;
        self.dirty = true;
        self.typing = None;
        if self.doc.scope(&self.path).is_none() {
            self.path.clear();
        }
    }

    pub fn sync(&mut self, shared: &GraphDocShared) {
        if shared.rev == 0 {
            return;
        }
        if !self.initialized || shared.replaced != self.synced_replaced {
            if self.initialized {
                self.history.push(self.doc.clone());
            }
            self.doc = (*shared.graph).clone();
            self.path.clear();
            self.selection.clear();
            self.initialized = true;
            self.synced_rev = shared.rev;
            self.synced_replaced = shared.replaced;
            return;
        }
        if shared.rev != self.synced_rev {
            self.synced_rev = shared.rev;
            let mut added = false;
            for n in &shared.graph.nodes {
                if let Some(DeviceBinding::Mac(m)) = n.device_binding()
                    && self.doc.device_node_for(m).is_none()
                    && !self.doc.contains(n.id)
                {
                    self.doc.nodes.push(n.clone());
                    added = true;
                }
            }
            if added {
                self.dirty = true;
            }
        }
    }

    pub fn flush(&mut self, bridge: &Bridge, ctx: &egui::Context) {
        if let Some(t) = self.view_changed
            && t.elapsed() >= VIEW_SETTLE
            && !ctx.input(|i| i.pointer.any_down())
        {
            self.view_changed = None;
            self.dirty = true;
        }
        if !self.dirty {
            if self.view_changed.is_some() {
                ctx.request_repaint_after(VIEW_SETTLE);
            }
            return;
        }
        if self.last_send.is_some_and(|t| t.elapsed() < SEND_INTERVAL) {
            ctx.request_repaint_after(SEND_INTERVAL);
            return;
        }
        bridge.send(Command::SetGraph(Box::new(self.doc.clone())));
        self.dirty = false;
        self.last_send = Some(Instant::now());
    }

    pub fn record_traces(&mut self, snap: &Snapshot) {
        let tick = snap.graph.tick;
        if tick == self.last_trace_tick {
            return;
        }
        self.last_trace_tick = tick;
        let mut live = HashSet::new();
        let scope = scope_of(&self.doc, &self.path);
        let wanted: Vec<(u64, usize)> = scope
            .nodes
            .iter()
            .flat_map(|n| {
                let mut p = self.path.clone();
                p.push(n.id);
                let f = flat_id(&p);
                node_ui::traced_outputs(&n.kind).iter().map(move |o| (f, *o)).collect::<Vec<_>>()
            })
            .collect();
        for key in wanted {
            let Some(v) = snap
                .graph
                .probe
                .nodes
                .get(&key.0)
                .and_then(|p| p.outputs.get(key.1))
                .copied()
            else {
                continue;
            };
            live.insert(key);
            let t = self.traces.entry(key).or_default();
            t.push_back(v);
            while t.len() > TRACE_LEN {
                t.pop_front();
            }
        }
        self.traces.retain(|k, _| live.contains(k));
    }

    pub fn add_node(&mut self, kind: NodeKind, at: egui::Pos2) -> NodeId {
        let pos = self.snapped([at.x, at.y]);
        let id = self.edit(|g| g.add(Node::new(kind, pos)));
        self.selection = [id].into_iter().collect();
        id
    }

    pub fn snapped(&self, p: [f32; 2]) -> [f32; 2] {
        if self.snap_grid {
            [(p[0] / GRID).round() * GRID, (p[1] / GRID).round() * GRID]
        } else {
            p
        }
    }

    pub fn delete_selection(&mut self, env: &Env) {
        let connected: HashSet<String> = env
            .snap
            .devices
            .iter()
            .filter(|d| d.connected)
            .map(|d| d.mac.to_uppercase())
            .collect();
        let scope = self.scope();
        let mut kept = 0;
        let ids: HashSet<NodeId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| {
                let protected = scope
                    .node(*id)
                    .and_then(|n| n.device_binding())
                    .and_then(|b| b.mac())
                    .is_some_and(|m| connected.contains(&m.to_uppercase()));
                if protected {
                    kept += 1;
                }
                !protected
            })
            .collect();
        if kept > 0 {
            env.toast(
                ToastKind::Info,
                "A connected device's node stays — unplug the device to remove it",
            );
        }
        if ids.is_empty() {
            return;
        }
        self.edit(|g| g.remove_nodes(&ids));
        self.selection.retain(|id| !ids.contains(id));
    }

    fn selection_set(&self) -> HashSet<NodeId> {
        self.selection.iter().copied().collect()
    }

    pub fn copy_selection(&mut self) -> Option<Graph> {
        if self.selection.is_empty() {
            return None;
        }
        let mut frag = self.scope().extract(&self.selection_set());
        frag.nodes.retain(|n| !n.kind.is_device());
        if frag.nodes.is_empty() {
            return None;
        }
        self.clipboard = Some(frag.clone());
        Some(frag)
    }

    pub fn paste(&mut self, frag: Option<Graph>, at: Option<egui::Pos2>) {
        let Some(frag) = frag.or_else(|| self.clipboard.clone()) else {
            return;
        };
        let offset = match (frag.bounds(), at) {
            (Some((min, _)), Some(at)) => [at.x - min[0], at.y - min[1]],
            _ => [30.0, 30.0],
        };
        let ids = self.edit(|g| g.insert(&frag, offset));
        self.selection = ids.into_iter().collect();
    }

    pub fn duplicate(&mut self) {
        if let Some(frag) = self.copy_selection() {
            let at = frag.bounds().map(|(min, _)| egui::pos2(min[0] + 40.0, min[1] + 40.0));
            self.paste(Some(frag), at);
        }
    }

    pub fn group_selection(&mut self, env: &Env) {
        let ids = self.selection_set();
        if ids.is_empty() {
            return;
        }
        match self.edit(|g| g.group(&ids, "Group")) {
            Some(id) => self.selection = [id].into_iter().collect(),
            None => env.toast(ToastKind::Info, "Device nodes stay outside groups"),
        }
    }

    pub fn ungroup_selection(&mut self) {
        let groups: Vec<NodeId> = self
            .selection
            .iter()
            .copied()
            .filter(|id| matches!(self.scope().node(*id).map(|n| &n.kind), Some(NodeKind::Group(_))))
            .collect();
        if groups.is_empty() {
            return;
        }
        let mut new_sel = BTreeSet::new();
        for gid in groups {
            new_sel.extend(self.edit(|g| g.ungroup(gid)));
        }
        self.selection = new_sel;
    }

    pub fn enter_group(&mut self, id: NodeId) {
        if matches!(self.scope().node(id).map(|n| &n.kind), Some(NodeKind::Group(_))) {
            self.path.push(id);
            self.selection.clear();
            self.menu = None;
        }
    }

    pub fn exit_group(&mut self) {
        if let Some(id) = self.path.pop() {
            self.selection = [id].into_iter().collect();
            self.menu = None;
        }
    }

    pub fn apply_preset(&mut self, preset: &GraphPreset, devices: &[DeviceSlotInfo]) {
        let macs = connected_macs(devices);
        let mut g = preset.instantiate(&macs);
        let named: Vec<(String, String)> =
            devices.iter().map(|d| (d.mac.clone(), d.name.clone())).collect();
        g.ensure_device_nodes(&named);
        for n in &mut g.nodes {
            if let Some(DeviceBinding::Mac(m)) = n.device_binding().cloned()
                && let Some(d) = devices.iter().find(|d| d.mac.eq_ignore_ascii_case(&m))
            {
                n.title = shocking_vrc_core::graph::model::device_title(&d.name, &d.mac);
            }
        }
        self.begin_edit();
        self.doc = g;
        self.path.clear();
        self.selection.clear();
        self.dirty = true;
        self.focus_request = None;
        self.frame_request = true;
        self.gesture_open = false;
    }

    pub fn insert_preset(&mut self, preset: &GraphPreset, devices: &[DeviceSlotInfo], at: egui::Pos2) {
        let macs = connected_macs(devices);
        let ids = if self.path.is_empty() {
            self.begin_edit();
            let ids = preset.insert_into(&mut self.doc, [at.x, at.y], &macs);
            self.dirty = true;
            ids
        } else {
            let mut frag = preset.graph.clone();
            frag.nodes.retain(|n| !n.kind.is_device());
            frag.prune_dangling_edges();
            let offset = frag
                .bounds()
                .map(|(min, _)| [at.x - min[0], at.y - min[1]])
                .unwrap_or([at.x, at.y]);
            self.edit(|g| g.insert(&frag, offset))
        };
        self.selection = ids.into_iter().collect();
    }

    pub fn preset_from_selection(&self, name: &str, author: &str) -> Option<GraphPreset> {
        if self.selection.is_empty() {
            return None;
        }
        let frag = self.scope().extract(&self.selection_set());
        let mut p = GraphPreset::fragment(name, author, &frag);
        p.kind = GraphPresetKind::Fragment;
        Some(p)
    }

    pub fn preset_from_graph(&self, name: &str, author: &str) -> GraphPreset {
        GraphPreset::from_graph(name, author, &self.doc)
    }

    pub fn view_centre(&self) -> egui::Pos2 {
        let v = self.scope().view;
        let r = self.canvas_rect;
        if !r.is_finite() || r.width() <= 0.0 {
            return egui::pos2(0.0, 0.0);
        }
        egui::pos2(
            (r.width() * 0.5 - v.pan[0]) / v.zoom.max(0.05),
            (r.height() * 0.5 - v.pan[1]) / v.zoom.max(0.05),
        )
    }
}

pub fn node_probe(
    probe: &shocking_vrc_core::graph::GraphProbe,
    path: &[NodeId],
    node: &Node,
) -> Option<shocking_vrc_core::graph::NodeProbe> {
    let mut p = path.to_vec();
    p.push(node.id);
    let NodeKind::Group(g) = &node.kind else {
        return probe.nodes.get(&flat_id(&p)).cloned();
    };
    let inner = |want_input: bool| {
        g.graph
            .nodes
            .iter()
            .find(|n| match &n.kind {
                NodeKind::GroupInput(_) => want_input,
                NodeKind::GroupOutput(_) => !want_input,
                _ => false,
            })
            .and_then(|n| {
                let mut q = p.clone();
                q.push(n.id);
                probe.nodes.get(&flat_id(&q))
            })
    };
    let (gi, go) = (inner(true), inner(false));
    if gi.is_none() && go.is_none() {
        return None;
    }
    Some(shocking_vrc_core::graph::NodeProbe {
        inputs: gi.map(|x| x.inputs.clone()).unwrap_or_default(),
        outputs: go.map(|x| x.outputs.clone()).unwrap_or_default(),
    })
}

pub fn connected_macs(devices: &[DeviceSlotInfo]) -> Vec<String> {
    devices.iter().map(|d| d.mac.to_uppercase()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use shocking_vrc_core::graph::kind::{ConstantParams, NoParams};
    use std::sync::Arc;

    fn shared(g: Graph, rev: u64, replaced: u64) -> GraphDocShared {
        GraphDocShared {
            graph: Arc::new(g),
            rev,
            replaced,
        }
    }

    #[test]
    fn undo_redo_walk_the_history() {
        let mut ed = GraphEditor::default();
        ed.sync(&shared(Graph::default(), 1, 0));
        for v in [0.1, 0.2, 0.3] {
            ed.gesture_open = false;
            ed.add_node(
                NodeKind::Constant(ConstantParams { value: v }),
                egui::pos2(0.0, 0.0),
            );
        }
        assert_eq!(ed.doc.nodes.len(), 3);
        ed.undo();
        ed.undo();
        assert_eq!(ed.doc.nodes.len(), 1);
        ed.redo();
        assert_eq!(ed.doc.nodes.len(), 2);
        assert!(ed.can_undo() && ed.can_redo());
    }

    #[test]
    fn history_keeps_at_least_a_hundred_steps() {
        let mut ed = GraphEditor::default();
        ed.sync(&shared(Graph::default(), 1, 0));
        for _ in 0..150 {
            ed.gesture_open = false;
            ed.add_node(NodeKind::Time(NoParams {}), egui::pos2(0.0, 0.0));
        }
        let mut steps = 0;
        while ed.can_undo() {
            ed.undo();
            steps += 1;
        }
        assert!(steps >= 100, "only {steps} undo steps");
    }

    #[test]
    fn runtime_added_device_nodes_are_merged_not_overwritten() {
        let mut ed = GraphEditor::default();
        ed.sync(&shared(Graph::default(), 1, 0));
        ed.add_node(NodeKind::Time(NoParams {}), egui::pos2(0.0, 0.0));
        let mut runtime = Graph::default();
        runtime.ensure_device_nodes(&[("AA:BB".into(), "Coyote".into())]);
        ed.sync(&shared(runtime, 2, 0));
        assert_eq!(ed.doc.nodes.len(), 2, "local edit kept, device node merged");
        assert!(ed.doc.device_node_for("AA:BB").is_some());
    }

    #[test]
    fn a_reload_replaces_the_document_as_an_undo_step() {
        let mut ed = GraphEditor::default();
        ed.sync(&shared(Graph::default(), 1, 0));
        ed.add_node(NodeKind::Time(NoParams {}), egui::pos2(0.0, 0.0));
        ed.sync(&shared(Graph::default(), 2, 1));
        assert!(ed.doc.nodes.is_empty());
        ed.undo();
        assert_eq!(ed.doc.nodes.len(), 1);
    }

    #[test]
    fn copy_paste_keeps_internal_wires() {
        use shocking_vrc_core::graph::PinRef;
        let mut ed = GraphEditor::default();
        ed.sync(&shared(Graph::default(), 1, 0));
        let a = ed.add_node(NodeKind::Time(NoParams {}), egui::pos2(0.0, 0.0));
        let b = ed.add_node(
            NodeKind::Math(Default::default()),
            egui::pos2(100.0, 0.0),
        );
        ed.edit(|g| g.connect(PinRef::new(a, "t"), PinRef::new(b, "a")).unwrap());
        ed.selection = [a, b].into_iter().collect();
        ed.duplicate();
        assert_eq!(ed.doc.nodes.len(), 4);
        assert_eq!(ed.doc.edges.len(), 2);
        assert_eq!(ed.selection.len(), 2);
    }
}
