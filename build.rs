//! Compiles the Slint interface and, on Windows, gives wisp.exe its icon and version information.

use std::path::PathBuf;
use std::process::Command;
use std::{env, fs};

fn main() {
    slint_build::compile("ui/app.slint").expect("slint compile failed");
    platform_kind();
    windows_resources();
}

/// `cfg(desktop)` for computers (windows to place, folders to pick) and `cfg(mobile)` for phones.
fn platform_kind() {
    println!("cargo:rustc-check-cfg=cfg(desktop)");
    println!("cargo:rustc-check-cfg=cfg(mobile)");
    match env::var("CARGO_CFG_TARGET_OS").as_deref() {
        Ok("android" | "ios") => println!("cargo:rustc-cfg=mobile"),
        _ => println!("cargo:rustc-cfg=desktop"),
    }
}

/// The icon and the "Details" tab of wisp.exe, from `assets/icon/wisp.ico`. They are compiled with
/// `rc.exe`, which comes with the Windows SDK next to the MSVC tools; without it the program is
/// still built, only without the icon.
fn windows_resources() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") || env::var("CARGO_CFG_TARGET_ENV").as_deref() != Ok("msvc") {
        return;
    }
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon/wisp.ico");
    let Some(rc) = find_rc() else {
        println!("cargo:warning=rc.exe was not found (it comes with the Windows SDK): wisp.exe gets no icon");
        return;
    };

    let manifest = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR"));
    let out = PathBuf::from(env::var_os("OUT_DIR").expect("cargo sets OUT_DIR"));
    let version = env::var("CARGO_PKG_VERSION").expect("cargo sets CARGO_PKG_VERSION");
    let mut parts = version.split('.').map(|p| p.parse::<u32>().unwrap_or(0));
    let (major, minor, patch) = (parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0));

    // The script and the icon sit in OUT_DIR and the compiler runs there, so no path with a space
    // or an accent in it ever reaches the resource compiler.
    fs::copy(manifest.join("assets/icon/wisp.ico"), out.join("wisp.ico")).expect("assets/icon/wisp.ico is missing");
    let script = format!(
        r#"1 ICON "wisp.ico"

1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEOS 0x40004
FILETYPE 1
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904B0"
    BEGIN
      VALUE "Comments", "A featherweight, gapless desktop audio player"
      VALUE "FileDescription", "Wisp Player"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "wisp"
      VALUE "LegalCopyright", "Free software under the GNU General Public License v3"
      VALUE "OriginalFilename", "wisp.exe"
      VALUE "ProductName", "Wisp Player"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x0409, 1200
  END
END
"#
    );
    fs::write(out.join("wisp.rc"), script).expect("cannot write the resource script");
    let compiled = Command::new(&rc).current_dir(&out).args(["/nologo", "/fo", "wisp.res", "wisp.rc"]).status();
    match compiled {
        Ok(status) if status.success() => println!("cargo:rustc-link-arg-bins={}", out.join("wisp.res").display()),
        _ => println!("cargo:warning=rc.exe failed: wisp.exe gets no icon"),
    }
}

/// The newest Windows SDK's 64-bit resource compiler.
fn find_rc() -> Option<PathBuf> {
    let kits = PathBuf::from(env::var_os("ProgramFiles(x86)")?).join("Windows Kits/10/bin");
    let mut found: Vec<PathBuf> = fs::read_dir(kits).ok()?.flatten().map(|e| e.path().join("x64/rc.exe")).filter(|p| p.is_file()).collect();
    found.sort();
    found.pop()
}
