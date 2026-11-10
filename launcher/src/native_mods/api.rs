//! API crate helpers: the manifest section that names the mod's Cargo package, and the trait
//! injection trait it generates.
use super::*;
use sha2::{Digest, Sha256};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

/// The `[api]` section of a bundle's `horizon-mixin.toml`, when it has one.
#[derive(Clone, Debug, Default)]
pub struct Api {
    /// The path to the crate's folder, relative to the bundle's `horizon-mixin.toml`.
    pub path: String,
    /// The package name in the crate's `Cargo.toml`.
    pub package: String,
}

/// Parses `[api]` from `bundle.dir/horizon-mixin.toml`.
pub fn read(dir: &Path) -> Result<Option<Api>, String> {
    let text = fs::read_to_string(dir.join(MANIFEST)).map_err(|e| format!("{}: {e}", dir.display()))?;
    let value: toml::Value = text.parse().map_err(|e| format!("{}: {e}", dir.display()))?;
    let api = value.get("api").and_then(|a| a.as_table());
    let Some(api) = api else {
        return Ok(None);
    };
    let path = api
        .get("path")
        .and_then(|p| p.as_str())
        .ok_or_else(|| format!("{}: [api] path missing", dir.display()))?
        .to_owned();
    let package = api
        .get("package")
        .and_then(|p| p.as_str())
        .ok_or_else(|| format!("{}: [api] package missing", dir.display()))?
        .to_owned();
    Ok(Some(Api { path, package }))
}

/// The hash of the API package's `Cargo.toml` and sources, for the build's `native-mods.json`.
pub fn api_hash(bundle: &Bundle) -> Result<Option<String>, String> {
    let api = read(&bundle.dir)?;
    let Some(api) = api else {
        return Ok(None);
    };
    let mut hash = Sha256::new();
    let api_dir = bundle.dir.join(&api.path);
    for (name, path) in files(&api_dir)? {
        let bytes = fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        hash.update(name.as_bytes());
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(&bytes);
    }
    Ok(Some(
        hash.finalize()[..8]
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
    ))
}

/// All bundles that declare an API package, in the order they appear in `mods`.
pub fn with_api(mods: &[Bundle]) -> Vec<&Bundle> {
    mods.iter()
        .filter(|b| read(&b.dir).ok().flatten().is_some())
        .collect()
}

/// Strips `[api]` from a bundle's `horizon-mixin.toml` (the build's copy, not the player's).
pub fn strip(bundle: &Bundle) -> Result<(), String> {
    let path = bundle.dir.join(MANIFEST);
    let text = fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut value: toml::Table = text.parse().map_err(|e| format!("{}: {e}", path.display()))?;
    value.remove("api");
    fs::write(&path, toml::to_string(&value).map_err(|e| e.to_string())?)
        .map_err(|e| format!("{}: {e}", path.display()))
}

/// Validates a mod id: lowercase alphanumeric, `-` or `_`, starts with a letter, max 64.
pub fn valid_id(id: &str) -> bool {
    let mut chars = id.chars();
    let Some(first) = chars.next() else {
        return false;
    };
    if !first.is_ascii_lowercase() {
        return false;
    }
    id.len() <= 64
        && chars
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

/// Finds the mod id that owns `file` (a path under `mixins/<id>/...`), among `mods`.
pub fn mod_id_from_file(mods: &[Bundle], file: &str) -> Option<String> {
    for bundle in mods {
        let prefix = format!("mixins/{}/", bundle.manifest.id);
        if let Some(rest) = file.strip_prefix(&prefix) {
            if !rest.is_empty() {
                return Some(bundle.manifest.id.clone());
            }
        }
    }
    None
}

/// Reads the `[api]` of each bundle and returns its Cargo package name and the path to its target
/// folder (for `inject_cache::key`).
pub fn api_target(root: &Path, api: Option<&str>) -> Result<Option<PathBuf>, String> {
    api.map(|path| root.join(path).canonicalize().map(|p| p.join("target")))
        .transpose()
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn bundle(root: &Path, id: &str, api: Option<(&str, &str)>) -> Bundle {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut text = format!("[mod]\nid = \"{id}\"\nversion = \"1.0.0\"\n");
        if let Some((path, package)) = api {
            text += &format!("[api]\npath = \"{path}\"\npackage = \"{package}\"\n");
        }
        fs::write(dir.join(super::MANIFEST), text).unwrap();
        Bundle {
            manifest: horizon_mods::index::native::Manifest {
                schema: 1,
                id: id.into(),
                version: "1.0.0".into(),
                game: "26.3".into(),
                horizon: "".into(),
                mixins: "".into(),
                hooks: None,
            },
            dir,
            horizon: None,
            mixins: None,
            api: None,
            injections: Vec::new(),
            mixins_content: true,
            files: std::collections::BTreeMap::new(),
            content: std::collections::BTreeMap::new(),
        }
    }

    #[test]
    fn reads_api_from_manifest() {
        let root = std::env::temp_dir().join(format!("api-read-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = bundle(&root, "demo", Some(("engine", "demo-api")));
        let api = read(&b.dir).unwrap().unwrap();
        assert_eq!((api.path, api.package), ("engine".to_string(), "demo-api".to_string()));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn returns_none_without_api() {
        let root = std::env::temp_dir().join(format!("api-none-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = bundle(&root, "demo", None);
        assert!(read(&b.dir).unwrap().is_none());
        fs::remove_dir_all(root).unwrap()
    }

    #[test]
    fn with_api_filters_bundles() {
        let root = std::env::temp_dir().join(format!("api-filter-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let no = bundle(&root, "no", None);
        let yes = bundle(&root, "yes", Some(("engine", "yes-api")));
        let mods = [no, yes];
        let with = with_api(&mods);
        assert_eq!(with.len(), 1);
        assert_eq!(with[0].id, "yes");
        fs::remove_dir_all(root).unwrap()
    }
}