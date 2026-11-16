use std::{env, fs, path::{Path, PathBuf}, process::Command};

#[path = "src/version.rs"]
mod version;

// The next commit after this one is the first versioned commit (0.0.1).
const BASE: &str = "1670e5586f72abfb8284777332b0fb7b9644e09e";

fn git(args: &[&str]) -> Option<String> {
    let output = Command::new("git").args(args).output().ok()?;
    if !output.status.success() { return None; }
    Some(String::from_utf8(output.stdout).ok()?.trim().to_owned())
}

fn app_version() -> String {
    println!("cargo:rerun-if-env-changed=MYDESKTOP_BUILD_VERSION");
    if let Ok(value) = env::var("MYDESKTOP_BUILD_VERSION") {
        return version::validate(&value).expect("MYDESKTOP_BUILD_VERSION must be A.B.C with each part from 0 to 99");
    }
    if let Some(head) = git(&["rev-parse", "--git-path", "HEAD"]) {
        println!("cargo:rerun-if-changed={head}");
    }
    if let Some(reference) = git(&["symbolic-ref", "-q", "HEAD"]) {
        if let Some(path) = git(&["rev-parse", "--git-path", &reference]) {
            println!("cargo:rerun-if-changed={path}");
        }
    }
    if let Some(path) = git(&["rev-parse", "--git-path", "packed-refs"]) {
        println!("cargo:rerun-if-changed={path}");
    }
    assert!(git(&["merge-base", "--is-ancestor", BASE, "HEAD"]).is_some(),
        "Cannot determine version from Git history. Use a full clone containing {BASE}, or set MYDESKTOP_BUILD_VERSION=A.B.C for a source archive.");
    let range = format!("{BASE}..HEAD");
    let count: u32 = git(&["rev-list", "--first-parent", "--count", &range])
        .expect("Cannot count Git commits").parse().expect("Invalid Git commit count");
    version::from_count(count).expect("Version exceeds 99.99.99")
}

fn main() {
    println!("cargo:rerun-if-changed=icon.ico");
    let value = app_version();
    println!("cargo:rustc-env=MYDESKTOP_VERSION={value}");
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") { return; }
    let parts: Vec<u32> = value.split('.').map(|v| v.parse().unwrap()).collect();
    let out_dir = PathBuf::from(env::var_os("OUT_DIR").unwrap());
    let icon = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").unwrap()).join("icon.ico");
    let icon = icon.to_string_lossy().replace('\\', "/");
    let resource = format!(r#"1 ICON "{icon}"
1 VERSIONINFO
 FILEVERSION {a},{b},{c},0
 PRODUCTVERSION {a},{b},{c},0
 FILEOS 0x40004
 FILETYPE 0x1
BEGIN
 BLOCK "StringFileInfo"
 BEGIN
  BLOCK "040904b0"
  BEGIN
   VALUE "FileDescription", "myDesktop\0"
   VALUE "FileVersion", "{value}\0"
   VALUE "ProductName", "myDesktop\0"
   VALUE "ProductVersion", "{value}\0"
  END
 END
 BLOCK "VarFileInfo"
 BEGIN
  VALUE "Translation", 0x409, 1200
 END
END
"#, a=parts[0], b=parts[1], c=parts[2]);
    let rc_input = out_dir.join("mydesktop.rc");
    fs::write(&rc_input, resource).expect("Cannot generate Windows resources");
    let out = out_dir.join("mydesktop.res");
    let rc = find_rc().expect("Windows resource compiler rc.exe not found. Install the Windows SDK.");
    let status = Command::new(rc).arg("/nologo").arg(format!("/fo{}", out.display())).arg(&rc_input).status().expect("Cannot run rc.exe");
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
