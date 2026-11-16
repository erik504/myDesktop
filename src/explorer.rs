//! Explorer folder discovery and launching.
//!
//! Explorer owns the navigation state, not the window manager. Shell.Application
//! exposes it together with the top-level HWND, including for separate windows.
use std::collections::HashMap;
use std::os::windows::process::CommandExt;
use std::process::Command;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn capture() -> Result<HashMap<isize, String>, String> {
    // Explicit UTF-8 output avoids mangling non-ASCII folder names under Windows
    // PowerShell 5.1's console code pages. @() and -InputObject preserve arrays.
    let script = r#"
$ErrorActionPreference = 'Stop'
[Console]::OutputEncoding = New-Object System.Text.UTF8Encoding($false)
$shell = New-Object -ComObject Shell.Application
$items = @($shell.Windows() | ForEach-Object {
    try {
        $p = $_.Document.Folder.Self.Path
        if ($p) { [pscustomobject]@{ handle = [long]$_.HWND; path = [string]$p } }
    } catch { }
})
ConvertTo-Json -InputObject $items -Compress
"#;
    let output = Command::new("powershell.exe")
        .args(["-NoProfile", "-NonInteractive", "-Sta", "-Command", script])
        .creation_flags(CREATE_NO_WINDOW)
        .output().map_err(|e| format!("Impossibile leggere Esplora file: {e}"))?;
    if !output.status.success() {return Err(format!("Impossibile leggere Esplora file: {}", String::from_utf8_lossy(&output.stderr)));}
    #[derive(serde::Deserialize)]
    struct Item { handle: i64, path: String }
    Ok(serde_json::from_slice::<Vec<Item>>(&output.stdout)
        .map_err(|e| format!("Risposta di Esplora file non valida: {e}"))?
        .into_iter()
        .map(|item| (item.handle as isize, item.path))
        .collect())
}

pub fn launch(path: &str) -> std::io::Result<()> {
    // The path is an argument, never interpolated into a shell command.
    Command::new("explorer.exe").arg(path).spawn().map(|_| ())
}
