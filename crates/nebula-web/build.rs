//! Embed the built web app (`app/dist`, what `pnpm build` writes) in the
//! binary: a table of every file in it, each `include_bytes!`ed, in
//! `$OUT_DIR/app.rs`. With no build of the app there, the table is empty
//! and the server says how to make one, so cargo needs no Node.
//!
//! Cargo reruns this when the build changes, a new hashed file name
//! included, and when a first build appears in the app's folder.

use std::path::{Path, PathBuf};

fn main() {
    let manifest = PathBuf::from(std::env::var_os("CARGO_MANIFEST_DIR").unwrap());
    let dist = manifest.join("app").join("dist");
    let mut files = Vec::new();
    if dist.join("index.html").is_file() {
        println!("cargo:rerun-if-changed={}", dist.display());
        collect(&dist, &dist, &mut files);
        files.sort();
    } else {
        println!("cargo:rerun-if-changed={}", manifest.join("app").display());
    }
    let mut table = String::from("pub static FILES: &[(&str, &[u8])] = &[\n");
    for (path, file) in &files {
        table += &format!("    ({path:?}, include_bytes!({file:?})),\n");
    }
    table += "];\n";
    let out = PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("app.rs");
    std::fs::write(out, table).unwrap();
}

/// Every file under `dir`, by its path under `root` and its own.
fn collect(root: &Path, dir: &Path, files: &mut Vec<(String, String)>) {
    for entry in std::fs::read_dir(dir).unwrap().flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect(root, &path, files);
        } else {
            let under = path.strip_prefix(root).unwrap();
            let under = under
                .components()
                .map(|c| c.as_os_str().to_string_lossy())
                .collect::<Vec<_>>()
                .join("/");
            files.push((under, path.display().to_string()));
        }
    }
}
