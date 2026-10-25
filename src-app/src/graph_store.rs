use std::path::{Path, PathBuf};

use shocking_vrc_core::graph::{Graph, parse_graph};

use crate::presets::slugify;

pub const GRAPH_FILE: &str = "graph.json";
pub const GRAPHS_DIR: &str = "graphs";

pub struct LoadedGraph {
    pub graph: Graph,
    pub notes: Vec<String>,
}

pub fn load_graph() -> LoadedGraph {
    load_graph_at(Path::new(GRAPH_FILE))
}

fn load_graph_at(path: &Path) -> LoadedGraph {
    let mut notes = Vec::new();
    let graph = match std::fs::read_to_string(path) {
        Err(_) => {
            log::info!("[graph] No {} yet — starting from an empty graph", path.display());
            Graph::default()
        }
        Ok(text) => match parse_graph(&text) {
            Ok(g) => {
                log::info!("[graph] Loaded {} ({} node(s))", path.display(), g.nodes.len());
                g
            }
            Err(e) => {
                let backup = path.with_extension("json.bak");
                let _ = std::fs::write(&backup, &text);
                log::warn!(
                    "[graph] {} is unreadable ({e}) — kept a copy as {}, starting empty",
                    path.display(),
                    backup.display()
                );
                notes.push(format!("graph.json could not be read: {e}"));
                Graph::default()
            }
        },
    };
    for t in graph.missing_types() {
        notes.push(format!("Unknown node type '{t}' loaded as a placeholder"));
    }
    LoadedGraph { graph, notes }
}

pub fn load_graph_from(path: &Path) -> Result<Graph, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    parse_graph(&text)
}

pub fn save_graph_to(path: &Path, graph: &Graph) -> Result<(), String> {
    if let Some(dir) = path.parent()
        && !dir.as_os_str().is_empty()
    {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let json = serde_json::to_string_pretty(graph).map_err(|e| e.to_string())?;
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, json).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn save_graph(graph: &Graph) -> Result<(), String> {
    save_graph_to(Path::new(GRAPH_FILE), graph)
}

pub fn graphs_dir() -> PathBuf {
    PathBuf::from(GRAPHS_DIR)
}

pub fn export_graph(name: &str, graph: &Graph) -> Result<PathBuf, String> {
    let path = graphs_dir().join(format!("{}.json", slugify(name)));
    save_graph_to(&path, graph)?;
    Ok(path)
}

pub fn list_graph_files() -> Vec<PathBuf> {
    let Ok(read) = std::fs::read_dir(graphs_dir()) else {
        return Vec::new();
    };
    let mut files: Vec<PathBuf> = read
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().and_then(|e| e.to_str()) == Some("json"))
        .collect();
    files.sort();
    files
}

#[cfg(test)]
mod tests {
    use super::*;
    use shocking_vrc_core::graph::{NodeKind, kind::NoParams};

    fn temp(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("shockingvrc-graph-store-{tag}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("temp dir");
        dir
    }

    #[test]
    fn save_and_load_round_trip_with_layout() {
        let dir = temp("round-trip");
        let path = dir.join("graph.json");
        let mut g = Graph::default();
        let id = g.add_kind(NodeKind::Time(NoParams {}), [12.5, -40.0]);
        g.node_mut(id).unwrap().comment = "keep me".into();
        g.view.zoom = 0.75;
        g.view.pan = [100.0, 20.0];
        save_graph_to(&path, &g).expect("save");
        let back = load_graph_at(&path).graph;
        assert_eq!(back, g, "identical layout, values and view");
    }

    #[test]
    fn a_broken_file_is_backed_up_not_lost() {
        let dir = temp("broken");
        let path = dir.join("graph.json");
        std::fs::write(&path, "{ definitely not json").unwrap();
        let loaded = load_graph_at(&path);
        assert!(loaded.graph.nodes.is_empty());
        assert!(!loaded.notes.is_empty());
        assert!(dir.join("graph.json.bak").exists());
    }

    #[test]
    fn a_missing_file_is_an_empty_graph() {
        let dir = temp("missing");
        let loaded = load_graph_at(&dir.join("nope.json"));
        assert!(loaded.graph.nodes.is_empty() && loaded.notes.is_empty());
    }
}
