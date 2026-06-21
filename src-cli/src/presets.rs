use std::path::{Path, PathBuf};
use std::time::Duration;

use shocking_vrc_core::cli::ChannelConfig;
use shocking_vrc_core::presets::{
    entry_from_file, entry_from_ref, parse_manifest, parse_preset, ChannelPreset, PresetEntry,
    PresetManifest,
};

const GITHUB_RAW_BASE: &str =
    "https://raw.githubusercontent.com/nullshii/ShockingVRC/tui/presets";
const USER_PRESETS_DIR: &str = "presets/user";

pub struct CatalogLoadResult {
    pub entries: Vec<PresetEntry>,
    pub source: String,
}

pub async fn load_catalog() -> Result<CatalogLoadResult, String> {
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(15))
        .user_agent("ShockingVRC-CLI")
        .build()
        .map_err(|e| e.to_string())?;

    let mut entries = Vec::new();
    let mut sources = Vec::new();

    if let Ok(official) = load_from_github(&client).await {
        if !official.is_empty() {
            entries.extend(official);
            sources.push("GitHub");
        }
    } else if let Ok(official) = load_official_local().await {
        if !official.is_empty() {
            entries.extend(official);
            sources.push("local");
        }
    }

    if let Ok(user) = load_user_presets().await {
        if !user.is_empty() {
            entries.extend(user);
            sources.push("mine");
        }
    }

    if entries.is_empty() {
        return Err(
            "No presets found — check network or add .json files to ./presets/".into(),
        );
    }

    sort_preset_entries(&mut entries);
    let source = sources.join(" + ");
    Ok(CatalogLoadResult { entries, source })
}

pub fn sort_preset_entries(entries: &mut [PresetEntry]) {
    entries.sort_by(|a, b| {
        b.user
            .cmp(&a.user)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
}

pub fn save_user_preset(
    ch: &ChannelConfig,
    name: &str,
    author: &str,
) -> Result<PresetEntry, String> {
    let name = name.trim();
    if name.is_empty() {
        return Err("Preset name cannot be empty".into());
    }

    let dir = user_preset_dir();
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;

    let id = slugify(name);
    let filename = format!("{id}.json");
    let preset = ChannelPreset::from_channel(name, author, ch);
    let json = serde_json::to_string_pretty(&preset).map_err(|e| e.to_string())?;
    std::fs::write(dir.join(&filename), &json)
        .map_err(|e| format!("{}: {e}", dir.join(&filename).display()))?;

    Ok(entry_from_file(id, preset, true))
}

pub fn delete_user_preset(id: &str) -> Result<(), String> {
    let dir = user_preset_dir();
    let file_path = dir.join(format!("{id}.json"));
    if file_path.exists() {
        std::fs::remove_file(&file_path)
            .map_err(|e| format!("{}: {e}", file_path.display()))?;
    } else {
        return Err(format!("preset file '{}' not found", file_path.display()));
    }
    Ok(())
}

pub fn user_preset_dir() -> PathBuf {
    PathBuf::from(USER_PRESETS_DIR)
}

pub fn random_preset_name(channel_label: &str) -> String {
    use rand::RngExt;
    let n: u32 = rand::rng().random_range(10_000..99_999);
    format!(
        "preset-{}-{n}",
        channel_label.to_ascii_lowercase()
    )
}

fn slugify(name: &str) -> String {
    let mut out = String::new();
    let mut prev_dash = true;
    for c in name.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_dash = false;
        } else if !prev_dash {
            out.push('-');
            prev_dash = true;
        }
    }
    let out = out.trim_matches('-').to_string();
    if out.is_empty() {
        "preset".into()
    } else {
        out
    }
}

fn local_official_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    dirs.push(PathBuf::from("presets"));
    if let Ok(cwd) = std::env::current_dir() {
        dirs.push(cwd.join("presets"));
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(parent) = exe.parent() {
            dirs.push(parent.join("presets"));
        }
    }
    dirs
}

async fn load_official_local() -> Result<Vec<PresetEntry>, String> {
    for dir in local_official_dirs() {
        if !dir.exists() {
            continue;
        }
        let entries = scan_json_dir(&dir, false)?;
        if !entries.is_empty() {
            return Ok(entries);
        }
    }
    Err("no local official presets".into())
}

async fn load_user_presets() -> Result<Vec<PresetEntry>, String> {
    let dir = user_preset_dir();
    if !dir.exists() {
        return Ok(Vec::new());
    }
    scan_json_dir(&dir, true)
}

async fn load_from_github(client: &reqwest::Client) -> Result<Vec<PresetEntry>, String> {
    let manifest_url = format!("{GITHUB_RAW_BASE}/manifest.json");
    let manifest_text = client
        .get(&manifest_url)
        .send()
        .await
        .map_err(|e| e.to_string())?
        .error_for_status()
        .map_err(|e| e.to_string())?
        .text()
        .await
        .map_err(|e| e.to_string())?;
    let manifest: PresetManifest = parse_manifest(&manifest_text)?;
    let mut entries = Vec::with_capacity(manifest.presets.len());
    for pref in &manifest.presets {
        let url = format!("{GITHUB_RAW_BASE}/{}", pref.file);
        let preset_text = client
            .get(&url)
            .send()
            .await
            .map_err(|e| e.to_string())?
            .error_for_status()
            .map_err(|e| e.to_string())?
            .text()
            .await
            .map_err(|e| e.to_string())?;
        let preset = parse_preset(&preset_text)?;
        entries.push(entry_from_ref(pref, preset, false));
    }
    Ok(entries)
}

fn scan_json_dir(dir: &Path, user: bool) -> Result<Vec<PresetEntry>, String> {
    let read = std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let mut entries = Vec::new();
    for item in read {
        let Ok(item) = item else { continue };
        let path = item.path();
        if path.is_dir() {
            continue;
        }
        if path.extension().and_then(|e| e.to_str()) != Some("json") {
            continue;
        }
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("")
            .to_string();
        if stem == "manifest" {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Ok(preset) = parse_preset(&text) else {
            continue;
        };
        entries.push(entry_from_file(stem, preset, user));
    }
    entries.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    Ok(entries)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_names() {
        assert_eq!(slugify("My Cool Preset"), "my-cool-preset");
        assert_eq!(slugify("  !!!  "), "preset");
    }
}
