//! Native mod source handling: extracting .hnmod/.hmod files into mods.incoming/ for building.
use super::{api, Bundle, MANIFEST, Profile, Target};
use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

/// Extracts native mod bundles from installed mods into `target/<profile>/mods.incoming/`.
/// Returns the list of extracted bundles ready for staging.
pub(super) fn extract(
    installed: &[PathBuf],
    target: Target,
    profile: Profile,
    project_root: &Path,
) -> Result<Vec<Bundle>, String> {
    let incoming = project_root
        .join("target")
        .join(profile.as_str())
        .join("mods.incoming");
    if incoming.exists() {
        fs::remove_dir_all(&incoming).map_err(|e| e.to_string())?;
    }
    fs::create_dir_all(&incoming).map_err(|e| e.to_string())?;

    let mut bundles = Vec::new();
    for path in installed {
        if super::is_file(path) && super::holds(path) {
            let bundle = Bundle::read(path)?;
            let dest = incoming.join(&bundle.manifest.id);
            fs::create_dir_all(&dest).map_err(|e| e.to_string())?;
            extract_bundle(&bundle, &dest)?;
            bundles.push(bundle);
        }
    }
    Ok(bundles)
}

/// Extracts a single bundle's native files to the incoming folder.
fn extract_bundle(bundle: &Bundle, dest: &Path) -> Result<(), String> {
    // Write the horizon-native.json manifest
    let manifest_json = serde_json::to_vec(&bundle.manifest).map_err(|e| e.to_string())?;
    fs::write(dest.join(MANIFEST), manifest_json).map_err(|e| e.to_string())?;

    // Write all native source files
    for (name, bytes) in &bundle.files {
        let path = dest.join(name);
        fs::create_dir_all(path.parent().unwrap()).map_err(|e| e.to_string())?;
        fs::write(&path, bytes).map_err(|e| e.to_string())?;
    }

    // For .hmod files with content, also extract content for validation
    if !bundle.content.is_empty() {
        // Content files are already validated during Bundle::read
        // but we don't write them to the incoming folder - they're handled separately
    }

    Ok(())
}

/// Validates that a mod file is a supported native mod format.
pub(super) fn is_file(path: &Path) -> bool {
    path.extension()
        .is_some_and(|e| e == "hnmod" || e == "hmod")
}

/// Checks whether a mod file carries native code (has horizon-native.json).
pub(super) fn holds(path: &Path) -> bool {
    std::fs::File::open(path)
        .ok()
        .and_then(|f| zip::ZipArchive::new(f).ok())
        .is_some_and(|mut zip| zip.by_name(MANIFEST).is_ok())
}

/// Prepares the native mod build environment: extracts mods, resolves dependencies,
/// stages the workspace, and returns the staged path with installed mod list.
pub(super) fn prepare(
    installed: &[PathBuf],
    target: Target,
    profile: Profile,
    project_root: &Path,
) -> Result<(PathBuf, Vec<String>), String> {
    let bundles = extract(installed, target, profile, project_root)?;
    let (staged, order) = super::isolate::stage(&bundles, target, profile, project_root)?;
    Ok((staged, order))
}

/// Reads the API target directory for a bundle (used by inject_cache).
pub(super) fn api_target(root: &Path, api_path: Option<&str>) -> Result<Option<PathBuf>, String> {
    api::api_target(root, api_path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;

    fn hnmod(root: &Path, id: &str, api_path: Option<&str>, mixin: &str) -> PathBuf {
        let path = root.join(format!("{}.hnmod", id));
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        let manifest = super::super::index::native::Manifest {
            schema: 1,
            id: id.into(),
            version: "1.0.0".into(),
            game: "26.3".into(),
            horizon: "a".repeat(40),
            mixins: "b".repeat(40),
            hooks: None,
        };
        zip.start_file(MANIFEST, options).unwrap();
        zip.write_all(&serde_json::to_vec(&manifest).unwrap()).unwrap();
        zip.start_file(format!("mixins/{}/horizon-mixin.toml", id), options)
            .unwrap();
        zip.write_all(mixin.as_bytes()).unwrap();
        if let Some(api) = api_path {
            fs::create_dir_all(root.join(id).join(api)).unwrap();
            fs::write(root.join(id).join(api).join("Cargo.toml"), "[package]\nname = \"api\"\nversion = \"1\"\n").unwrap();
            fs::write(root.join(id).join(api).join("src/lib.rs"), "").unwrap();
            for entry in fs::read_dir(root.join(id).join(api)).unwrap() {
                let entry = entry.unwrap();
                let name = entry.file_name().to_string_lossy().into_owned();
                let path = entry.path();
                zip.start_file(format!("mixins/{}/{}/{}", id, api, name), options).unwrap();
                let bytes = fs::read(path).unwrap();
                zip.write_all(&bytes).unwrap();
            }
        }
        zip.finish().unwrap();
        path
    }

    fn mixin(id: &str) -> String {
        format!(
            "[mod]\nid = \"{id}\"\nversion = \"1.0.0\"\n[compatibility]\nhorizon_revision = \"{}\"\n",
            "a".repeat(40)
        )
    }

    #[test]
    fn extracts_hnmod_to_incoming() {
        let root = std::env::temp_dir().join(format!("source-extract-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let path = hnmod(&root, "testmod", Some("engine"), &mixin("testmod"));
        let bundles = extract(&[path], Target::Client, Profile::Performance, &root).unwrap();
        assert_eq!(bundles.len(), 1);
        assert_eq!(bundles[0].manifest.id, "testmod");
        assert!(root.join("target/performance/mods.incoming/testmod/horizon-native.json").exists());
        assert!(root.join("target/performance/mods.incoming/testmod/mixins/testmod/horizon-mixin.toml").exists());
        assert!(root.join("target/performance/mods.incoming/testmod/mixins/testmod/engine/Cargo.toml").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rejects_non_native_mod_files() {
        let root = std::env::temp_dir().join(format!("source-reject-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        // Create a regular .hmod without native code
        let path = root.join("content.hmod");
        let mut zip = zip::ZipWriter::new(fs::File::create(&path).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("horizon-mod.toml", options).unwrap();
        zip.write_all(b"id = \"content\"\nversion = \"1.0.0\"\n").unwrap();
        zip.finish().unwrap();
        let bundles = extract(&[path], Target::Client, Profile::Performance, &root).unwrap();
        assert_eq!(bundles.len(), 0);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn is_file_and_holds_helpers() {
        let root = std::env::temp_dir().join(format!("source-helpers-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let native = hnmod(&root, "native", None, &mixin("native"));
        let content = root.join("content.hmod");
        let mut zip = zip::ZipWriter::new(fs::File::create(&content).unwrap());
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("horizon-mod.toml", options).unwrap();
        zip.write_all(b"id = \"content\"\nversion = \"1.0.0\"\n").unwrap();
        zip.finish().unwrap();
        assert!(is_file(&native));
        assert!(holds(&native));
        assert!(is_file(&content));
        assert!(!holds(&content));
        fs::remove_dir_all(root).unwrap();
    }
}