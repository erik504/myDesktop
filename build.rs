use std::{env, fs, path::{Path, PathBuf}, process::Command};

fn main() {
    println!("cargo:rerun-if-changed=icon.rc");
    println!("cargo:rerun-if-changed=icon.ico");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let out = PathBuf::from(env::var_os("OUT_DIR").unwrap()).join("icon.res");
    let rc = find_rc().expect("Windows resource compiler rc.exe not found. Install the Windows SDK.");
    let status = Command::new(rc).arg("/nologo").arg(format!("/fo{}", out.display())).arg("icon.rc").status().expect("Cannot run rc.exe");
    assert!(status.success(), "Resource compilation failed");
    println!("cargo:rustc-link-arg={}", out.display());
}

fn find_rc() -> Option<PathBuf> {
    if let Some(paths) = env::var_os("PATH") {
        for dir in env::split_paths(&paths) {
            let candidate = dir.join("rc.exe");
            if candidate.is_file() { return Some(candidate); }
        }
    }
    let kits = Path::new(r"C:\Program Files (x86)\Windows Kits\10\bin");
    let mut versions: Vec<_> = fs::read_dir(kits).ok()?.flatten().map(|e| e.path()).collect();
    versions.sort();
    versions.reverse();
    versions.into_iter().map(|p| p.join("x64").join("rc.exe")).find(|p| p.is_file())
}
