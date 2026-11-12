// SPDX-License-Identifier: MIT
//! Offline resource import. The user supplies the original JAR; nothing is downloaded or run.
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::{self, Cursor, Read},
    path::{Path, PathBuf},
    sync::atomic::{AtomicU64, Ordering},
};
use zip::ZipArchive;

pub const INPUT_DIR: &str = "horizon-mods/create-resources";
pub const CREATE_VERSION: &str = "6.0.10+mc1.21.1 (NeoForge)";
pub const JAR_SHA256: &str = "ef87fe5709f1ba1f5b8bb20a2925b5afb4669e178fd6d8bf10c167759eefe37a";
// Bump whenever the import catalog changes: the cache key pins the JAR, not
// BLOCKS/ITEMS/PARTS, so an older valid cache lacks newly required resources.
const SCHEMA: u32 = 12;
const MAX_JAR: u64 = 128 << 20;
const MAX_FILE: u64 = 16 << 20;
const MAX_TOTAL: usize = 64 << 20;
const MAX_ENTRIES: usize = 65_536;
mod catalog;
pub use catalog::{BLOCKS, EXTRA_BLOCKSTATES, EXTRA_MODELS, ITEMS, PARTS};
pub const WOODS: &[&str] = &[
    "oak", "spruce", "birch", "jungle", "acacia", "dark_oak", "mangrove", "cherry", "bamboo",
    "crimson", "warped", "pale_oak",
];
pub const WHEEL_PARTS: &[&str] = &[
    "water_wheel/wheel",
    "large_water_wheel/block",
    "large_water_wheel/block_extension",
];

#[derive(Debug)]
pub struct Imported {
    pub pack: PathBuf,
    pub files: usize,
    pub cached: bool,
}

/// Materialize this native mod's public content in an owned folder discoverable
/// by Horizon's content loader. Other mod files and enable settings are untouched.
pub fn bundled_content(game_dir: &Path, bytes: &[u8]) -> io::Result<PathBuf> {
    let files: BTreeMap<String, String> = serde_json::from_slice(bytes)?;
    if !files.contains_key("horizon-mod.toml")
        || files.len() > 1024
        || files.iter().any(|(name, value)| {
            !safe_path(name)
                || !(name == "horizon-mod.toml"
                    || name.starts_with("assets/create/")
                    || name.starts_with("data/create/")
                    || matches!(
                        name.as_str(),
                        "data/minecraft/tags/damage_type/is_fire.json"
                            | "data/minecraft/tags/damage_type/bypasses_armor.json"
                            | "data/minecraft/tags/damage_type/bypasses_shield.json"
                            | "data/minecraft/tags/block/mineable/axe.json"
                            | "data/minecraft/tags/block/mineable/pickaxe.json"
                            | "data/minecraft/tags/block/convertable_to_mud.json"
                    )
                    || name.starts_with("data/c/tags/item/"))
                || value.len() > MAX_FILE as usize
        })
    {
        return Err(invalid("Invalid bundled Create content"));
    }
    let cache = game_dir.join(".horizon-cache/create-content");
    fs::create_dir_all(&cache)?;
    let mods = game_dir.join("horizon-mods");
    fs::create_dir_all(&mods)?;
    let target = mods.join(".create-native-content");
    let valid = || {
        files.iter().all(|(name, expected)| {
            let path = target.join(name);
            fs::symlink_metadata(&path).is_ok_and(|m| m.is_file())
                && fs::read(&path).is_ok_and(|actual| actual == expected.as_bytes())
        })
    };
    if valid() {
        return Ok(target);
    }
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cache.join(".content.lock"))?;
    lock.lock()?;
    if !valid() {
        let stage = Stage::new(&mods)?;
        for (name, value) in files {
            let path = stage.0.join(name);
            fs::create_dir_all(path.parent().expect("content parent"))?;
            fs::write(path, value)?;
        }
        if target.exists() {
            fs::remove_dir_all(&target)?;
        }
        fs::rename(&stage.0, &target)?;
    }
    Ok(target)
}

fn invalid(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// Only a single user-supplied JAR is accepted. Removing it disables the imported pack.
pub fn prepare(game_dir: &Path, format: (i64, i64)) -> io::Result<Option<Imported>> {
    let input = game_dir.join(INPUT_DIR);
    fs::create_dir_all(&input)?;
    let readme = input.join("README.txt");
    if !readme.exists() {
        fs::write(
            &readme,
            format!(
                "Place one original Create {CREATE_VERSION} JAR in this folder, then restart Horizon.\n\
             Get it yourself from https://modrinth.com/mod/create/version/UjX6dr61\n\
             SHA-256: {JAR_SHA256}\n\
             The mod imports only resources, never Java code. No download is performed.\n\
             Imports are cached in .horizon-cache/create-assets in this game instance.\n\
             Removing the JAR restores the provisional resources on the next start/reload.\n\
             Original resources retain their authors' All Rights Reserved terms.\n\
             Do not publish the JAR or generated cache as part of this port.\n"
            ),
        )?;
    }
    let mut jars = Vec::new();
    for entry in fs::read_dir(&input)? {
        let entry = entry?;
        if entry
            .path()
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("jar"))
        {
            if !entry.file_type()?.is_file() {
                return Err(invalid("The Create resource JAR must be a regular file"));
            }
            jars.push(entry.path());
        }
    }
    if jars.is_empty() {
        return Ok(None);
    }
    if jars.len() != 1 {
        return Err(invalid(format!(
            "Put exactly one Create JAR in {}",
            input.display()
        )));
    }
    import(
        &jars[0],
        &game_dir.join(".horizon-cache/create-assets/imports"),
        format,
        JAR_SHA256,
    )
    .map(Some)
}

fn import(jar: &Path, cache: &Path, format: (i64, i64), expected: &str) -> io::Result<Imported> {
    let file = fs::File::open(jar)?;
    if file.metadata()?.len() > MAX_JAR {
        return Err(invalid("Create resource JAR exceeds 128 MiB"));
    }
    let mut bytes = Vec::new();
    file.take(MAX_JAR + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_JAR {
        return Err(invalid("Create resource JAR exceeds 128 MiB"));
    }
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if hash != expected {
        return Err(invalid(format!(
            "Unsupported Create resource JAR: expected {CREATE_VERSION}, SHA-256 {expected}; got {hash}"
        )));
    }
    let key = format!("v{SCHEMA}-{hash}-{}-{}", format.0, format.1);
    let target = cache.join(key);
    if let Some(files) = valid_cache(&target, &hash, format) {
        return Ok(Imported {
            pack: target.join("pack"),
            files,
            cached: true,
        });
    }
    // Validate and collect everything before publishing any resources.
    let files = collect(bytes)?;
    let total_files = files.len();
    fs::create_dir_all(cache)?;
    let lock = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .open(cache.join(".import.lock"))?;
    lock.lock()?;
    if let Some(files) = valid_cache(&target, &hash, format) {
        return Ok(Imported {
            pack: target.join("pack"),
            files,
            cached: true,
        });
    }
    let stage = Stage::new(cache)?;
    let pack = stage.0.join("pack");
    for (name, bytes) in &files {
        let path = pack.join(name);
        fs::create_dir_all(path.parent().expect("resource parent"))?;
        fs::write(path, bytes)?;
    }
    let metadata = serde_json::to_vec_pretty(&json!({
        "pack": {
            "description": "Create resources imported from your local JAR (All Rights Reserved)",
            "min_format": [format.0, format.1], "max_format": [format.0, format.1]
        }
    }))?;
    fs::write(pack.join("pack.mcmeta"), &metadata)?;
    let hashes: BTreeMap<_, _> = files
        .iter()
        .map(|(name, bytes)| (name, format!("{:x}", Sha256::digest(bytes))))
        .collect();
    fs::write(
        stage.0.join("import.json"),
        serde_json::to_vec_pretty(&json!({
            "schema": SCHEMA, "jar_sha256": hash, "create_version": CREATE_VERSION,
            "resource_format": [format.0, format.1], "files": hashes,
        "metadata_sha256": format!("{:x}", Sha256::digest(&metadata)),
            "notice": "Locally imported files. No redistribution permission is granted by this tool."
        }))?,
    )?;
    // A damaged cache is disposable; the input JAR and installed mod are never modified.
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    match fs::rename(&stage.0, &target) {
        Ok(()) => {}
        Err(_) if valid_cache(&target, &hash, format).is_some() => {}
        Err(error) => return Err(error),
    }
    Ok(Imported {
        pack: target.join("pack"),
        files: total_files,
        cached: false,
    })
}

fn valid_cache(target: &Path, hash: &str, format: (i64, i64)) -> Option<usize> {
    let receipt: Value =
        serde_json::from_slice(&fs::read(target.join("import.json")).ok()?).ok()?;
    if receipt["schema"] != SCHEMA
        || receipt["jar_sha256"] != hash
        || receipt["resource_format"] != json!([format.0, format.1])
    {
        return None;
    }
    let metadata = fs::read(target.join("pack/pack.mcmeta")).ok()?;
    if receipt["metadata_sha256"].as_str()? != format!("{:x}", Sha256::digest(metadata)) {
        return None;
    }
    let files = receipt["files"].as_object()?;
    if files.is_empty() || files.len() > MAX_ENTRIES {
        return None;
    }
    for (name, expected) in files {
        if !safe_path(name) {
            return None;
        }
        let path = target.join("pack").join(name);
        let metadata = fs::symlink_metadata(&path).ok()?;
        if !metadata.is_file() || metadata.len() > MAX_FILE {
            return None;
        }
        let actual = format!("{:x}", Sha256::digest(fs::read(path).ok()?));
        if expected.as_str()? != actual {
            return None;
        }
    }
    Some(files.len())
}

struct Stage(PathBuf);
impl Stage {
    fn new(cache: &Path) -> io::Result<Self> {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        loop {
            let path = cache.join(format!(
                ".import-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
            ));
            match fs::create_dir(&path) {
                Ok(()) => return Ok(Self(path)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e),
            }
        }
    }
}
impl Drop for Stage {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn safe_path(path: &str) -> bool {
    !path.is_empty()
        && !path.contains(['\\', ':', '\0'])
        && path
            .split('/')
            .all(|part| !part.is_empty() && part != "." && part != "..")
}

fn resource_path(id: &str, folder: &str, extension: &str) -> io::Result<Option<String>> {
    let (namespace, path) = id.split_once(':').unwrap_or(("minecraft", id));
    if !safe_path(path)
        || !path
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b"_./-".contains(&b))
    {
        return Err(invalid(format!("Invalid resource identifier: {id}")));
    }
    match namespace {
        "minecraft" => Ok(None),
        "create" => Ok(Some(format!("assets/create/{folder}/{path}.{extension}"))),
        _ => Err(invalid(format!("Unsupported resource namespace: {id}"))),
    }
}

struct Reader {
    zip: ZipArchive<Cursor<Vec<u8>>>,
    files: BTreeMap<String, Vec<u8>>,
    visiting: BTreeSet<String>,
    total: usize,
}
impl Reader {
    fn read(&mut self, path: &str) -> io::Result<Vec<u8>> {
        let mut file = self
            .zip
            .by_name(path)
            .map_err(|e| invalid(format!("Required resource {path}: {e}")))?;
        if file.is_dir()
            || file.unix_mode().is_some_and(|m| m & 0o170000 == 0o120000)
            || file.size() > MAX_FILE
        {
            return Err(invalid(format!("Invalid or oversized resource: {path}")));
        }
        let mut bytes = Vec::new();
        file.by_ref().take(MAX_FILE + 1).read_to_end(&mut bytes)?;
        self.total += bytes.len();
        if bytes.len() as u64 > MAX_FILE || self.total > MAX_TOTAL {
            return Err(invalid("Create resource import exceeds its size limits"));
        }
        Ok(bytes)
    }

    fn json(&mut self, path: &str) -> io::Result<Value> {
        let bytes = self.read(path)?;
        let value: Value =
            serde_json::from_slice(&bytes).map_err(|e| invalid(format!("{path}: {e}")))?;
        if !value.is_object() {
            return Err(invalid(format!("Expected a JSON object: {path}")));
        }
        self.files.insert(path.into(), bytes);
        Ok(value)
    }

    fn texture(&mut self, id: &str) -> io::Result<()> {
        let Some(path) = resource_path(id, "textures", "png")? else {
            return Ok(());
        };
        if self.files.contains_key(&path) {
            return Ok(());
        }
        let bytes = self.read(&path)?;
        if !bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
            return Err(invalid(format!("Not a PNG: {path}")));
        }
        self.files.insert(path.clone(), bytes);
        let metadata = format!("{path}.mcmeta");
        if self.zip.index_for_name(&metadata).is_some() {
            self.json(&metadata)?;
        }
        Ok(())
    }

    fn model(&mut self, id: &str) -> io::Result<()> {
        let Some(path) = resource_path(id, "models", "json")? else {
            return Ok(());
        };
        if self.visiting.contains(&path) {
            return Err(invalid(format!("Model parent cycle: {path}")));
        }
        if self.files.contains_key(&path) {
            return Ok(());
        }
        self.visiting.insert(path.clone());
        let model = self.json(&path)?;
        if let Some(loader) = model.get("loader") {
            if loader.as_str() != Some("neoforge:obj") {
                return Err(invalid(format!(
                    "Custom Java model loaders are unsupported: {path}"
                )));
            }
            // Horizon natively decodes NeoForge OBJ models. Preserve source
            // geometry and material libraries; never execute a Java loader.
            let object = model
                .get("model")
                .and_then(Value::as_str)
                .ok_or_else(|| invalid("OBJ model has no asset location"))?;
            let relative = object
                .strip_prefix("create:")
                .filter(|p| p.starts_with("models/") && p.ends_with(".obj") && safe_path(p))
                .ok_or_else(|| invalid("Unsupported OBJ asset path"))?;
            let name = format!("assets/create/{relative}");
            let bytes = self.read(&name)?;
            let text = std::str::from_utf8(&bytes).map_err(|_| invalid("OBJ must be UTF-8"))?;
            let libraries: Vec<String> = text
                .lines()
                .filter_map(|line| {
                    line.trim()
                        .strip_prefix("mtllib ")
                        .map(str::trim)
                        .map(str::to_owned)
                })
                .collect();
            self.files.insert(name.clone(), bytes);
            for library in libraries {
                if !safe_path(&library) || library.contains('/') || !library.ends_with(".mtl") {
                    return Err(invalid("Unsafe OBJ material library"));
                }
                let material = format!("{}/{}", name.rsplit_once('/').unwrap().0, library);
                let bytes = self.read(&material)?;
                let text = std::str::from_utf8(&bytes).map_err(|_| invalid("MTL must be UTF-8"))?;
                let textures: Vec<String> = text
                    .lines()
                    .filter_map(|line| line.trim().strip_prefix("map_Kd ").map(str::trim))
                    .filter(|id| !id.starts_with('#'))
                    .map(str::to_owned)
                    .collect();
                for texture in textures {
                    self.texture(&texture)?;
                }
                self.files.insert(material, bytes);
            }
        }
        if let Some(parent) = model.get("parent").and_then(Value::as_str) {
            self.model(parent)?;
        }
        if let Some(textures) = model.get("textures").and_then(Value::as_object) {
            for texture in textures
                .values()
                .filter_map(Value::as_str)
                .filter(|s| !s.starts_with('#'))
            {
                self.texture(texture)?;
            }
        }
        if let Some(overrides) = model.get("overrides").and_then(Value::as_array) {
            for item in overrides {
                if let Some(model) = item.get("model").and_then(Value::as_str) {
                    self.model(model)?;
                }
            }
        }
        self.visiting.remove(&path);
        Ok(())
    }

    fn blockstate_models(&mut self, value: &Value) -> io::Result<()> {
        match value {
            Value::Object(map) => {
                if let Some(model) = map.get("model").and_then(Value::as_str) {
                    self.model(model)?;
                }
                for child in map.values() {
                    self.blockstate_models(child)?;
                }
            }
            Value::Array(array) => {
                for child in array {
                    self.blockstate_models(child)?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

fn collect(bytes: Vec<u8>) -> io::Result<BTreeMap<String, Vec<u8>>> {
    let zip = ZipArchive::new(Cursor::new(bytes)).map_err(|e| invalid(e.to_string()))?;
    if zip.len() > MAX_ENTRIES {
        return Err(invalid("Too many entries in Create JAR"));
    }
    let mut names = BTreeSet::new();
    for name in zip.file_names() {
        if !safe_path(name.trim_end_matches('/')) || !names.insert(name.to_owned()) {
            return Err(invalid(format!("Unsafe or duplicate JAR entry: {name}")));
        }
    }
    let mut reader = Reader {
        zip,
        files: BTreeMap::new(),
        visiting: BTreeSet::new(),
        total: 0,
    };
    for block in BLOCKS.iter().chain(EXTRA_BLOCKSTATES) {
        let state = reader.json(&format!("assets/create/blockstates/{block}.json"))?;
        reader.blockstate_models(&state)?;
    }
    for item in BLOCKS.iter().chain(ITEMS) {
        reader.model(&format!("create:item/{item}"))?;
    }
    for part in PARTS {
        reader.model(&format!("create:block/{part}"))?;
    }
    for model in EXTRA_MODELS {
        reader.model(&format!("create:{model}"))?;
    }
    // Generated local texture variants inherit the original OBJ geometry. They
    // remain in the player's private cache, under the original asset terms.
    for part in WHEEL_PARTS {
        for wood in WOODS {
            let log = match *wood {
                "crimson" | "warped" => format!("{wood}_stem"),
                "bamboo" => "bamboo_block".into(),
                _ => format!("{wood}_log"),
            };
            let value = json!({"parent":format!("create:block/{part}"),"textures":{"log":format!("minecraft:block/{log}"),"log_top":format!("minecraft:block/{log}_top"),"planks":format!("minecraft:block/{wood}_planks")}});
            reader.files.insert(
                format!("assets/create/models/block/{part}_material_{wood}.json"),
                serde_json::to_vec(&value)?,
            );
        }
    }
    // Keep every upstream locale byte-for-byte. Translations remain local
    // assets under the same terms as the rest of the player-supplied JAR.
    let locales: Vec<String> = reader
        .zip
        .file_names()
        .filter(|name| name.starts_with("assets/create/lang/") && name.ends_with(".json"))
        .map(|name| name.to_owned())
        .collect();
    for locale in locales {
        reader.json(&locale)?;
    }
    reader.json("assets/create/sounds.json")?;
    reader.texture("create:gui/value_settings")?;
    reader.texture("create:gui/icons")?;
    reader.texture("create:block/belt_scroll")?;
    reader.texture("create:block/belt_diagonal_scroll")?;
    reader.texture("create:block/blaze_burner_flame_scroll")?;
    reader.texture("create:block/blaze_burner_flame_superheated_scroll")?;
    for particle in ["rotation_indicator", "air_flow"] {
        let definition = reader.json(&format!("assets/create/particles/{particle}.json"))?;
        let textures = definition["textures"]
            .as_array()
            .ok_or_else(|| invalid("Particle sprite set has no textures"))?;
        if textures.is_empty() || textures.len() > 64 {
            return Err(invalid("Invalid particle sprite count"));
        }
        for texture in textures {
            let texture = texture
                .as_str()
                .ok_or_else(|| invalid("Invalid particle sprite ID"))?;
            let (namespace, path) = texture.split_once(':').unwrap_or(("minecraft", texture));
            reader.texture(&format!("{namespace}:particle/{path}"))?;
        }
    }
    let sounds: Vec<String> = reader
        .zip
        .file_names()
        .filter(|name| name.starts_with("assets/create/sounds/") && name.ends_with(".ogg"))
        .map(|name| name.to_owned())
        .collect();
    for sound in sounds {
        let bytes = reader.read(&sound)?;
        if !bytes.starts_with(b"OggS") {
            return Err(invalid(format!("Not Ogg audio: {sound}")));
        }
        reader.files.insert(sound, bytes);
    }
    let license = reader.read("LICENSE.md_create-1.21.1")?;
    reader.files.insert("CREATE-LICENSE.md".into(), license);
    Ok(reader.files)
}

#[cfg(test)]
mod tests;
