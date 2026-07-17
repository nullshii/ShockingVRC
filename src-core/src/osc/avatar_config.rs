use std::fs;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::Deserialize;

use crate::error::{DGLabError, Result};

#[derive(Debug, Deserialize)]
struct AvatarOscConfig {
    #[serde(default)]
    id: String,
    #[serde(default)]
    name: String,
    #[serde(default)]
    parameters: Vec<AvatarParam>,
}

#[derive(Debug, Deserialize)]
struct AvatarParam {
    #[serde(default)]
    name: String,
    input: Option<ParamEndpoint>,
    output: Option<ParamEndpoint>,
}

#[derive(Debug, Deserialize)]
struct ParamEndpoint {
    #[serde(default)]
    address: String,
}

pub fn default_vrchat_osc_root() -> Option<PathBuf> {
    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))?;
    let path = PathBuf::from(home)
        .join("AppData")
        .join("LocalLow")
        .join("VRChat")
        .join("VRChat")
        .join("OSC");
    path.is_dir().then_some(path)
}

pub fn default_vrchat_osc_root_display() -> PathBuf {
    let home = std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .unwrap_or_default();
    PathBuf::from(home)
        .join("AppData")
        .join("LocalLow")
        .join("VRChat")
        .join("VRChat")
        .join("OSC")
}

pub fn resolve_osc_root(override_dir: Option<&Path>) -> Option<PathBuf> {
    if let Some(p) = override_dir {
        let s = p.to_string_lossy();
        let trimmed = s.trim().trim_end_matches(['/', '\\']);
        if !trimmed.is_empty() {
            let path = PathBuf::from(trimmed);
            if path.is_dir() {
                return Some(path);
            }
        }
    }
    default_vrchat_osc_root()
}

pub fn load_avatar_param_paths(
    avatar_id: &str,
    override_dir: Option<&Path>,
) -> Result<(String, Vec<String>)> {
    let root = resolve_osc_root(override_dir).ok_or_else(|| {
        DGLabError::OscError(format!(
            "OSC folder not found — set path in Setup or use {}",
            default_vrchat_osc_root_display().display()
        ))
    })?;

    let path = find_avatar_config(&root, avatar_id).ok_or_else(|| {
        DGLabError::OscError(format!(
            "No OSC config for avatar {avatar_id} under {}",
            root.display()
        ))
    })?;

    load_param_paths_from_file(&path)
}

pub fn load_latest_avatar_param_paths(
    override_dir: Option<&Path>,
) -> Result<(String, Vec<String>)> {
    let root = resolve_osc_root(override_dir).ok_or_else(|| {
        DGLabError::OscError(format!(
            "OSC folder not found — set path in Setup or use {}",
            default_vrchat_osc_root_display().display()
        ))
    })?;

    let mut candidates = list_avatar_configs(&root);
    if candidates.is_empty() {
        return Err(DGLabError::OscError(format!(
            "No avatar OSC configs under {} — enable OSC in VRChat and wear a published avatar",
            root.display()
        )));
    }
    candidates.sort_by_key(|(mtime, _)| std::cmp::Reverse(*mtime));

    let mut last_err = None;
    for (_, path) in candidates {
        match load_param_paths_from_file(&path) {
            Ok(v) => return Ok(v),
            Err(e) => {
                log::debug!("Skipping {}: {e}", path.display());
                last_err = Some(e);
            }
        }
    }
    Err(last_err.unwrap_or_else(|| {
        DGLabError::OscError("No readable avatar OSC configs found".into())
    }))
}

fn find_avatar_config(root: &Path, avatar_id: &str) -> Option<PathBuf> {
    let file_name = format!("{avatar_id}.json");
    let mut matches: Vec<PathBuf> = Vec::new();

    if let Ok(user_dirs) = fs::read_dir(root) {
        for user_dir in user_dirs.flatten() {
            let candidate = user_dir.path().join("Avatars").join(&file_name);
            if candidate.is_file() {
                matches.push(candidate);
            }
        }
    }

    let nested = root.join("Avatars").join(&file_name);
    if nested.is_file() {
        matches.push(nested);
    }

    let flat = root.join(&file_name);
    if flat.is_file() {
        matches.push(flat);
    }

    collect_named_json(root, &file_name, 0, 3, &mut matches);

    matches.into_iter().max_by_key(|p| file_mtime(p))
}

fn collect_named_json(
    dir: &Path,
    file_name: &str,
    depth: u32,
    max_depth: u32,
    out: &mut Vec<PathBuf>,
) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_named_json(&path, file_name, depth + 1, max_depth, out);
        } else if path.file_name().and_then(|n| n.to_str()) == Some(file_name) {
            out.push(path);
        }
    }
}

fn list_avatar_configs(root: &Path) -> Vec<(SystemTime, PathBuf)> {
    let mut out = Vec::new();
    let mut seen = std::collections::HashSet::new();

    let mut push = |path: PathBuf| {
        if path.extension().and_then(|e| e.to_str()) == Some("json") && seen.insert(path.clone()) {
            out.push((file_mtime(&path), path));
        }
    };

    if let Ok(user_dirs) = fs::read_dir(root) {
        for user_dir in user_dirs.flatten() {
            let avatars = user_dir.path().join("Avatars");
            if let Ok(entries) = fs::read_dir(&avatars) {
                for entry in entries.flatten() {
                    push(entry.path());
                }
            }
        }
    }

    if let Ok(entries) = fs::read_dir(root.join("Avatars")) {
        for entry in entries.flatten() {
            push(entry.path());
        }
    }

    if let Ok(entries) = fs::read_dir(root) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                push(path);
            }
        }
    }

    collect_avatars_json(root, 0, 3, &mut |p| push(p));

    out
}

fn collect_avatars_json(dir: &Path, depth: u32, max_depth: u32, push: &mut dyn FnMut(PathBuf)) {
    if depth > max_depth {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        if path.file_name().and_then(|n| n.to_str()) == Some("Avatars") {
            if let Ok(files) = fs::read_dir(&path) {
                for f in files.flatten() {
                    push(f.path());
                }
            }
        } else {
            collect_avatars_json(&path, depth + 1, max_depth, push);
        }
    }
}

fn file_mtime(path: &Path) -> SystemTime {
    fs::metadata(path)
        .and_then(|m| m.modified())
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

fn load_param_paths_from_file(path: &Path) -> Result<(String, Vec<String>)> {
    let bytes = fs::read(path).map_err(|e| {
        DGLabError::OscError(format!("Failed to read {}: {e}", path.display()))
    })?;
    if bytes.is_empty() {
        return Err(DGLabError::OscError(format!(
            "Empty OSC config {}",
            path.display()
        )));
    }

    let text = decode_osc_config_bytes(&bytes)
        .map_err(|e| DGLabError::OscError(format!("{}: {e}", path.display())))?;

    let cfg: AvatarOscConfig = serde_json::from_str(&text).map_err(|e| {
        DGLabError::OscError(format!("Invalid OSC config {}: {e}", path.display()))
    })?;

    let mut paths = Vec::new();
    for param in &cfg.parameters {
        for endpoint in [&param.output, &param.input].into_iter().flatten() {
            if let Some(rel) = strip_avatar_param(&endpoint.address) {
                paths.push(rel.to_string());
            }
        }
        if param.input.is_none() && param.output.is_none() && !param.name.is_empty() {
            if looks_like_contact_param(&param.name) {
                paths.push(param.name.clone());
            }
        }
    }

    paths.sort();
    paths.dedup();

    let label = if !cfg.name.is_empty() {
        cfg.name
    } else if !cfg.id.is_empty() {
        cfg.id.clone()
    } else {
        path.file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("unknown")
            .to_string()
    };

    log::info!(
        "Loaded {} OSC param paths from {} ({label})",
        paths.len(),
        path.display()
    );
    Ok((label, paths))
}

fn decode_osc_config_bytes(bytes: &[u8]) -> std::result::Result<String, String> {
    if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        return String::from_utf8(bytes[3..].to_vec()).map_err(|e| e.to_string());
    }
    if bytes.starts_with(&[0xFF, 0xFE]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&u16s).map_err(|_| "invalid UTF-16 LE".into());
    }
    if bytes.starts_with(&[0xFE, 0xFF]) {
        let u16s: Vec<u16> = bytes[2..]
            .chunks_exact(2)
            .map(|c| u16::from_be_bytes([c[0], c[1]]))
            .collect();
        return String::from_utf16(&u16s).map_err(|_| "invalid UTF-16 BE".into());
    }
    String::from_utf8(bytes.to_vec()).map_err(|e| e.to_string())
}

fn strip_avatar_param(address: &str) -> Option<&str> {
    address
        .strip_prefix("/avatar/parameters/")
        .filter(|p| !p.is_empty())
}

fn looks_like_contact_param(name: &str) -> bool {
    name.starts_with("OGB/")
        || name.starts_with("TPS_Internal/")
        || name.starts_with("VFH/")
        || name.starts_with("DGB/")
}
