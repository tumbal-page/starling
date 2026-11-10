//! Attribution of compiler errors to native mods, so a failing build can name the mod it blames.
use super::{Bundle, MANIFEST};
use std::{collections::HashMap, fs, path::Path};

/// A compiler error attributed to a mod, with the file and message.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(super) struct Owner {
    pub mod_id: String,
    pub file: String,
    pub message: String,
}

/// Reads the staged workspace's `Cargo.lock` to map crate names to the mod that provides them.
pub(super) fn crate_owners(stage: &Path) -> Result<HashMap<String, String>, String> {
    let lock = fs::read_to_string(stage.join("Cargo.lock")).map_err(|e| format!("{}: {e}", stage.join("Cargo.lock").display()))?;
    let lock: toml::Value = lock.parse().map_err(|e| e.to_string())?;
    let mut owners = HashMap::new();
    for package in lock.get("package").and_then(|p| p.as_array()).into_iter().flatten() {
        let name = package.get("name").and_then(|n| n.as_str());
        let source = package.get("source").and_then(|s| s.as_str());
        if let (Some(name), Some(source)) = (name, source) {
            if source.starts_with("registry+") {
                continue;
            }
            // A path dependency from the staged mods folder.
            if let Some(mod_id) = source.strip_prefix("path+") {
                // The mod id is the folder name under native/mods.incoming/
                if let Some(mod_id) = mod_id.strip_prefix("../mods.incoming/") {
                    owners.insert(name.to_owned(), mod_id.to_owned());
                }
            }
        }
    }
    Ok(owners)
}

/// Reads `horizon-mixin.toml` files in `mods` to map their injected files to the mod id.
pub(super) fn injection_owners(mods: &[Bundle]) -> HashMap<String, String> {
    let mut owners = HashMap::new();
    for bundle in mods {
        let path = bundle.dir.join(MANIFEST);
        let Ok(text) = fs::read_to_string(&path) else {
            continue;
        };
        let Ok(value): Result<toml::Value, _> = text.parse() else {
            continue;
        };
        if let Some(injections) = value.get("injection").and_then(|i| i.as_array()) {
            for injection in injections {
                if let Some(file) = injection.get("file").and_then(|f| f.as_str()) {
                    owners.insert(file.to_owned(), bundle.manifest.id.clone());
                }
                if let Some(target) = injection.get("target").and_then(|t| t.as_str()) {
                    owners.insert(target.to_owned(), bundle.manifest.id.clone());
                }
            }
        }
    }
    owners
}

/// Attributes `error` (a compiler error message) to a mod, using `crate_owners` and
/// `injection_owners`. Returns the mod id if found.
pub(super) fn attribute(
    error: &str,
    crate_owners: &HashMap<String, String>,
    injection_owners: &HashMap<String, String>,
) -> Option<String> {
    // Look for crate names in the error (e.g., `error[E0432]: unresolved import `mod_foo::bar``)
    for (crate_name, mod_id) in crate_owners {
        if error.contains(crate_name) {
            return Some(mod_id.clone());
        }
    }
    // Look for injected file paths
    for (file, mod_id) in injection_owners {
        if error.contains(file) {
            return Some(mod_id.clone());
        }
    }
    None
}

/// All mod ids that the compiler's errors were attributed to.
pub(super) fn all_owners(
    errors: &[String],
    crate_owners: &HashMap<String, String>,
    injection_owners: &HashMap<String, String>,
) -> Vec<String> {
    let mut owners = Vec::new();
    for error in errors {
        if let Some(mod_id) = attribute(error, crate_owners, injection_owners) {
            if !owners.contains(&mod_id) {
                owners.push(mod_id);
            }
        }
    }
    owners
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn attributes_by_crate_name() {
        let mut crate_owners = HashMap::new();
        crate_owners.insert("mod_foo".to_owned(), "foo".to_owned());
        let injection_owners = HashMap::new();
        let errors = vec!["error[E0432]: unresolved import `mod_foo::bar`".to_owned()];
        assert_eq!(all_owners(&errors, &crate_owners, &injection_owners), vec!["foo"]);
    }

    #[test]
    fn attributes_by_injection_file() {
        let crate_owners = HashMap::new();
        let mut injection_owners = HashMap::new();
        injection_owners.insert("crates/horizon-client/src/game.rs".to_owned(), "foo".to_owned());
        let errors = vec![
            "error[E0425]: cannot find function `hook` in `crates/horizon-client/src/game.rs`".to_owned()
        ];
        assert_eq!(all_owners(&errors, &crate_owners, &injection_owners), vec!["foo"]);
    }

    #[test]
    fn multiple_errors_same_mod() {
        let mut crate_owners = HashMap::new();
        crate_owners.insert("mod_foo".to_owned(), "foo".to_owned());
        let injection_owners = HashMap::new();
        let errors = vec![
            "error[E0432]: unresolved import `mod_foo::bar`".to_owned(),
            "error[E0425]: cannot find function `baz` in `mod_foo`".to_owned(),
        ];
        assert_eq!(all_owners(&errors, &crate_owners, &injection_owners), vec!["foo"]);
    }
}