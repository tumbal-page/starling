//! Reusable, immutable mixin output cache keyed by Horizon revision + mixins revision + profile
//! + installed mod list + their API targets.
use super::{api, Bundle, Profile, Target};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, fs, path::{Path, PathBuf}};

const CACHE: &str = "native-mods";
const MANIFEST: &str = "horizon-mixin.toml";

/// The cache key covers everything that affects the generated output:
/// - Horizon's git revision and the mixins tool's revision.
/// - The compilation profile (fast/release).
/// - The exact installed mod set and order (dependencies before dependants).
/// - Each mod's API package folder content (its `target/` under `target/<profile>`).
/// The result is a content-addressed cache directory name.
pub(super) fn key(
    horizon: &str,
    mixins: &str,
    profile: Profile,
    mods: &[&Bundle],
) -> Result<String, String> {
    let mut hash = Sha256::new();
    hash.update(horizon.as_bytes());
    hash.update(b"|");
    hash.update(mixins.as_bytes());
    hash.update(b"|");
    hash.update(profile.as_str().as_bytes());
    hash.update(b"|");
    for bundle in mods {
        hash.update(bundle.manifest.id.as_bytes());
        hash.update(b"|");
        hash.update(bundle.manifest.version.as_bytes());
        hash.update(b"|");
        // Each mod's API crate target folder is part of the cache key.
        if let Some(api) = api::read(&bundle.dir)? {
            let target = api::api_target(&bundle.dir, Some(&api.path))?;
            if let Some(target) = target {
                hash.update(target.display().to_string().as_bytes());
                hash.update(b"|");
            }
        }
        // The mixin sources themselves (all files under mixins/<id>/).
        let prefix = format!("mixins/{}/", bundle.manifest.id);
        for (name, bytes) in &bundle.files {
            if name == MANIFEST {
                continue;
            }
            if let Some(rest) = name.strip_prefix(&prefix) {
                if rest.is_empty() {
                    continue;
                }
                hash.update(rest.as_bytes());
                hash.update(b"|");
                hash.update(&(bytes.len() as u64).to_le_bytes());
                hash.update(bytes);
            }
        }
    }
    let digest = hash.finalize();
    let mut key = String::with_capacity(1 + digest.len() * 2);
    key.push(match profile {
        Profile::Performance => 'p',
        Profile::Development => 'd',
    });
    for b in digest {
        key.push_str(&format!("{b:02x}"));
    }
    Ok(key)
}

/// The on-disk cache folder for a key, under `target/<profile>/native-mods/<key>`.
pub(super) fn dir(target: Target, profile: Profile, key: &str) -> Result<PathBuf, String> {
    let root = match target {
        Target::Client => PathBuf::from("crates/horizon-client"),
        Target::Server => PathBuf::from("crates/horizon-server"),
    };
    let profile = match profile {
        Profile::Performance => "release",
        Profile::Development => "debug",
    };
    Ok(root.join("target").join(profile).join(CACHE).join(key))
}

/// Copies the cached mixin crate into the staged workspace, if the cache entry exists.
/// The cache contains a complete, self-contained Cargo package (its own `Cargo.toml` and `src/`).
pub(super) fn restore(
    cache: &Path,
    staged: &Path,
    target: Target,
) -> Result<bool, String> {
    let crate_name = match target {
        Target::Client => "horizon-client",
        Target::Server => "horizon-server",
    };
    let source = cache.join(crate_name);
    if !source.exists() {
        return Ok(false);
    }
    let dest = staged.join(crate_name);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    copy_dir_all(&source, &dest).map_err(|e| e.to_string())?;
    Ok(true)
}

/// Writes the staged crate to the cache, if it doesn't already exist.
pub(super) fn store(
    cache: &Path,
    staged: &Path,
    target: Target,
) -> Result<(), String> {
    let crate_name = match target {
        Target::Client => "horizon-client",
        Target::Server => "horizon-server",
    };
    let source = staged.join(crate_name);
    if !source.exists() {
        return Err(format!("staged crate {crate_name} does not exist"));
    }
    let dest = cache.join(crate_name);
    if dest.exists() {
        fs::remove_dir_all(&dest).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
    copy_dir_all(&source, &dest).map_err(|e| e.to_string())?;
    Ok(())
}

fn copy_dir_all(src: &Path, dst: &Path) -> Result<(), String> {
    for entry in fs::read_dir(src).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        let name = entry.file_name();
        let dest = dst.join(&name);
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            copy_dir_all(&path, &dest)?;
        } else {
            fs::copy(&path, &dest).map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// Prunes the cache directory to keep at most `limit` entries, removing the oldest first
/// (by modification time of the cache key folder).
pub(super) fn prune(target: Target, profile: Profile, limit: usize) -> Result<(), String> {
    let cache = dir(target, profile, "")?;
    let cache = cache.parent().ok_or("invalid cache path")?;
    if !cache.exists() {
        return Ok(());
    }
    let mut entries: Vec<_> = fs::read_dir(cache)
        .map_err(|e| e.to_string())?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_dir()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| {
        e.metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::SystemTime::UNIX_EPOCH)
    });
    while entries.len() > limit {
        let oldest = entries.remove(0);
        fs::remove_dir_all(oldest.path()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn bundle(root: &Path, id: &str, api_path: Option<&str>) -> Bundle {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut text = format!("[mod]\nid = \"{id}\"\nversion = \"1.0.0\"\n[compatibility]\nhorizon_revision = \"{}\"\n", "a".repeat(40));
        if let Some(path) = api_path {
            text += &format!("[api]\npath = \"{path}\"\npackage = \"{id}-api\"\n");
        }
        fs::write(dir.join(MANIFEST), text).unwrap();
        Bundle {
            manifest: NativeManifest {
                schema: 1,
                id: id.into(),
                version: "1.0.0".into(),
                game: "26.3".into(),
                horizon: Some("a".repeat(40)),
                mixins: Some("b".repeat(40)),
                hooks: None,
            },
            dir,
            horizon: None,
            mixins: None,
            api: None,
            injections: Vec::new(),
            mixins_content: true,
            files: BTreeMap::new(),
            content: BTreeMap::new(),
        }
    }

    #[test]
    fn key_changes_with_mod_order() {
        let root = std::env::temp_dir().join(format!("cache-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let first = bundle(&root, "first", Some("engine"));
        let second = bundle(&root, "second", Some("engine"));
        let a = key("rev1", "rev2", Profile::Performance, &[&first, &second]).unwrap();
        let b = key("rev1", "rev2", Profile::Performance, &[&second, &first]).unwrap();
        assert_ne!(a, b);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn key_changes_with_api_content() {
        let root = std::env::temp_dir().join(format!("cache-api-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = bundle(&root, "mod", Some("engine"));
        fs::create_dir_all(b.dir.join("engine/src")).unwrap();
        fs::write(b.dir.join("engine/src/lib.rs"), "fn v1() {}").unwrap();
        let key1 = key("rev1", "rev2", Profile::Performance, &[&b]).unwrap();
        fs::write(b.dir.join("engine/src/lib.rs"), "fn v2() {}").unwrap();
        let key2 = key("rev1", "rev2", Profile::Performance, &[&b]).unwrap();
        assert_ne!(key1, key2);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn key_changes_with_profile() {
        let root = std::env::temp_dir().join(format!("cache-profile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let b = bundle(&root, "mod", None);
        let a = key("rev1", "rev2", Profile::Performance, &[&b]).unwrap();
        let b = key("rev1", "rev2", Profile::Development, &[&b]).unwrap();
        assert_ne!(a, b);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn key_includes_mixin_sources() {
        let root = std::env::temp_dir().join(format!("cache-mixin-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let mut b = bundle(&root, "mod", None);
        b.files.insert(
            "mixins/mod/hook.rs".into(),
            b"fn hook() {}",
        );
        let key1 = key("rev1", "rev2", Profile::Performance, &[&b]).unwrap();
        b.files.insert(
            "mixins/mod/hook.rs".into(),
            b"fn hook() { println!(); }",
        );
        let key2 = key("rev1", "rev2", Profile::Performance, &[&b]).unwrap();
        assert_ne!(key1, key2);
        fs::remove_dir_all(root).unwrap();
    }
}