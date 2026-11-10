//! Copy native mods into a staging workspace, rewriting Cargo dependencies to point at the
//! staged copies. The staged workspace is compiled instead of the player's originals.
use super::{api, dependencies, Bundle, Profile, Target, MANIFEST};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Stages a `mods` folder into `target/<profile>/mods/` with rewritten Cargo.toml files.
/// Returns the staged workspace root and the list of installed mod ids in dispatch order.
pub(super) fn stage(
    mods: &[Bundle],
    target: Target,
    profile: Profile,
    project_root: &Path,
) -> Result<(PathBuf, Vec<String>), String> {
    let mods_incoming = project_root.join("mods.incoming");
    if !mods_incoming.exists() {
        return Err("mods.incoming not found; run prepare first".into());
    }
    let staged = project_root
        .join("target")
        .join(profile.as_str())
        .join("mods");
    if staged.exists() {
        fs::remove_dir_all(&staged).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&staged).map_err(|e| e.to_string())?;

    // Resolve dependencies first to get dispatch order.
    let resolved = dependencies::resolve(&dependencies::Mod::from_bundles(mods))?;
    let ordered = resolved
        .order
        .iter()
        .map(|&i| &mods[i])
        .collect::<Vec<_>>();

    let mut installed_ids = Vec::new();
    for bundle in &ordered {
        let dest = staged.join(&bundle.manifest.id);
        copy_bundle(bundle, &mods_incoming, &dest)?;
        rewrite_cargo(&dest, bundle, &ordered)?;
        installed_ids.push(bundle.manifest.id.clone());
    }

    // Write a helper manifest listing all installed mods for inject_cache::key.
    let manifest = staged.join("MODS.toml");
    let mut table = toml::Table::new();
    table.insert(
        "mods".into(),
        toml::Value::Array(
            installed_ids
                .iter()
                .map(|id| toml::Value::String(id.clone()))
                .collect(),
        ),
    );
    fs::write(&manifest, toml::to_string(&table).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    Ok((staged, installed_ids))
}

/// Copies a bundle from `mods.incoming/<id>/` to the staged workspace.
fn copy_bundle(bundle: &Bundle, incoming: &Path, dest: &Path) -> Result<(), String> {
    let source = incoming.join(&bundle.manifest.id);
    if !source.exists() {
        return Err(format!("mod {} not found in mods.incoming", bundle.manifest.id));
    }
    fs::create_dir_all(dest).map_err(|e| e.to_string())?;
    copy_dir_all(&source, dest)?;
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

/// Rewrites the staged bundle's Cargo.toml to point at other staged mods.
fn rewrite_cargo(
    dest: &Path,
    bundle: &Bundle,
    ordered: &[&Bundle],
) -> Result<(), String> {
    let cargo_toml = dest.join("Cargo.toml");
    if !cargo_toml.exists() {
        // No Cargo.toml = content-only mod, nothing to rewrite.
        return Ok(());
    }
    let mut manifest: toml::Table = fs::read_to_string(&cargo_toml)
        .map_err(|e| format!("{}: {e}", cargo_toml.display()))?
        .parse()
        .map_err(|e| format!("{}: {e}", cargo_toml.display()))?;

    // Build the list of dependency packages for dependencies::rewrite.
    let deps = dependencies::parse(&bundle.manifest.id, manifest.get("dependencies"))?;
    let packages: Vec<_> = deps
        .iter()
        .map(|dep| {
            let target = ordered.iter().find(|b| b.manifest.id == dep.id).expect("resolved");
            let api = api::read(&target.dir).ok().flatten();
            dependencies::Package {
                dependency: dep,
                package: api.as_ref().map(|a| a.package.as_str()),
            }
        })
        .collect();

    let enabled = dependencies::rewrite(&bundle.manifest.id, &mut manifest, &packages)?;
    fs::write(&cargo_toml, toml::to_string(&manifest).map_err(|e| e.to_string())?)
        .map_err(|e| e.to_string())?;

    // Add features for optional dependencies to the workspace's Cargo.toml.
    if !enabled.is_empty() {
        let workspace = dest.parent().unwrap().join("Cargo.toml");
        if workspace.exists() {
            let mut ws: toml::Table = fs::read_to_string(&workspace)
                .map_err(|e| format!("{}: {e}", workspace.display()))?
                .parse()
                .map_err(|e| format!("{}: {e}", workspace.display()))?;
            let features = ws
                .entry("features")
                .or_insert_with(|| toml::Value::Table(toml::Table::new()))
                .as_table_mut()
                .ok_or("workspace features not a table")?;
            for id in enabled {
                if !features.contains_key(&id) {
                    features.insert(id, toml::Value::Array(vec![]));
                }
            }
            fs::write(&workspace, toml::to_string(&ws).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

/// The isolated cargo workspace root (target/<profile>/mods) with its own Cargo.lock.
/// This workspace is compiled independently, then its artifacts are merged into the game.
pub(super) fn workspace_root(
    project_root: &Path,
    profile: Profile,
) -> PathBuf {
    project_root
        .join("target")
        .join(profile.as_str())
        .join("mods")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn bundle(root: &Path, id: &str, api_path: Option<&str>, deps: &str) -> Bundle {
        let dir = root.join(id);
        fs::create_dir_all(&dir).unwrap();
        let mut text = format!("[mod]\nid = \"{id}\"\nversion = \"1.0.0\"\n[compatibility]\nhorizon_revision = \"{}\"\n", "a".repeat(40));
        if let Some(path) = api_path {
            text += &format!("[api]\npath = \"{path}\"\npackage = \"{id}-api\"\n");
        }
        if !deps.is_empty() {
            text += &format!("[dependencies]\n{deps}\n");
        }
        fs::write(dir.join(MANIFEST), text).unwrap();
        let cargo_dir = dir.join(if api_path.is_some() { "engine" } else { "" });
        if api_path.is_some() {
            fs::create_dir_all(&cargo_dir).unwrap();
            fs::write(cargo_dir.join("Cargo.toml"), "[package]\nname = \"dummy\"\nversion = \"1.0.0\"\n").unwrap();
            fs::write(cargo_dir.join("src/lib.rs"), "").unwrap();
        }
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
    fn stages_mods_in_dispatch_order() {
        let root = std::env::temp_dir().join(format!("isolate-order-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let incoming = root.join("mods.incoming");
        fs::create_dir_all(&incoming).unwrap();
        let a = bundle(&incoming, "a", Some("engine"), "base = \"^1\"");
        let b = bundle(&incoming, "base", Some("engine"), "");
        let c = bundle(&incoming, "c", None, "");
        let mods = [a, b, c];
        let (staged, order) = stage(&mods, Target::Client, Profile::Performance, &root).unwrap();
        assert_eq!(order, vec!["base", "a", "c"]);
        assert!(staged.join("base").exists());
        assert!(staged.join("a").exists());
        assert!(staged.join("c").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rewrites_dependencies_to_staged_paths() {
        let root = std::env::temp_dir().join(format!("isolate-rewrite-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let incoming = root.join("mods.incoming");
        fs::create_dir_all(&incoming).unwrap();
        let a = bundle(&incoming, "a", Some("engine"), "base = \"^1\"");
        let base = bundle(&incoming, "base", Some("engine"), "");
        let mods = [a, base];
        let (staged, _) = stage(&mods, Target::Client, Profile::Performance, &root).unwrap();
        let cargo = fs::read_to_string(staged.join("a/Cargo.toml")).unwrap();
        assert!(cargo.contains("path = \"../base\""));
        assert!(cargo.contains("package = \"base-api\""));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn content_only_mods_are_copied_without_cargo_rewrite() {
        let root = std::env::temp_dir().join(format!("isolate-content-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let incoming = root.join("mods.incoming");
        fs::create_dir_all(&incoming).unwrap();
        let a = bundle(&incoming, "a", None, "");
        let mods = [a];
        let (staged, _) = stage(&mods, Target::Client, Profile::Performance, &root).unwrap();
        assert!(staged.join("a").exists());
        assert!(!staged.join("a/Cargo.toml").exists());
        fs::remove_dir_all(root).unwrap();
    }
}