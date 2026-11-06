use std::{env, fs, path::Path, process::Command};
fn main() {
    let mut paths = vec![];
    for entry in fs::read_dir("src").unwrap() {
        let p = entry.unwrap().path();
        if p.extension().is_some_and(|e| e == "rs") {
            paths.push(p);
        }
    }
    paths.push(Path::new("Cargo.toml").into());
    paths.push(Path::new("Cargo.lock").into());
    paths.push(Path::new("build.rs").into());
    paths.sort();
    let mut hash = 14695981039346656037u64;
    for p in paths {
        println!("cargo:rerun-if-changed={}", p.display());
        for b in fs::read(p).unwrap() {
            hash ^= b as u64;
            hash = hash.wrapping_mul(1099511628211);
        }
    }
    let compiler = Command::new(env::var("RUSTC").unwrap())
        .arg("--version")
        .output()
        .unwrap();
    println!(
        "cargo:rustc-env=TIPE_SOURCE_ID={hash:016x}-{}-{}",
        env::var("TARGET").unwrap(),
        String::from_utf8_lossy(&compiler.stdout).trim()
    );
}
