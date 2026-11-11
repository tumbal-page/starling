// SPDX-License-Identifier: MIT
use std::{io, path::PathBuf};

fn main() -> io::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let home = args.next().map(PathBuf::from).ok_or_else(|| {
        io::Error::other(
            "Usage: create-assets GAME_INSTANCE RESOURCE_FORMAT_MAJOR RESOURCE_FORMAT_MINOR",
        )
    })?;
    let parse = |arg: Option<std::ffi::OsString>| {
        arg.and_then(|s| s.to_str()?.parse().ok())
            .ok_or_else(|| io::Error::other("Expected the Horizon resource format major and minor"))
    };
    let format = (parse(args.next())?, parse(args.next())?);
    if args.next().is_some() {
        return Err(io::Error::other("Too many arguments"));
    }
    match create_assets::prepare(&home, format)? {
        Some(imported) => println!(
            "{}",
            serde_json::json!({
                "status": "ready", "pack": imported.pack, "files": imported.files, "cached": imported.cached,
                "sha256": create_assets::JAR_SHA256
            })
        ),
        None => println!(
            "{}",
            serde_json::json!({"status": "missing", "input": home.join(create_assets::INPUT_DIR)})
        ),
    }
    Ok(())
}
