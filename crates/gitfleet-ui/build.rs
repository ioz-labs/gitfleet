use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=../../assets/icon.ico");

    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_windows_icon();
    }
}

fn embed_windows_icon() {
    let manifest_dir = match std::env::var("CARGO_MANIFEST_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => return,
    };
    let out_dir = match std::env::var("OUT_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => return,
    };
    let icon_path = manifest_dir.join("../../assets/icon.ico");
    if !icon_path.is_file() {
        return;
    }
    let abs_icon = icon_path.canonicalize().unwrap_or(icon_path);
    let icon_str = abs_icon.to_string_lossy();
    let clean_icon = icon_str.strip_prefix(r"\\?\").unwrap_or(&icon_str);
    let escaped = clean_icon.replace('\\', "\\\\");

    let rc_path = out_dir.join("gitfleet-icon.rc");
    let res_path = out_dir.join("gitfleet-icon.res");
    if std::fs::write(&rc_path, format!("1 ICON \"{escaped}\"\n")).is_err() {
        return;
    }

    let Some(rc_exe) = find_rc_exe() else {
        println!("cargo:warning=rc.exe not found; Windows executable icon was not embedded");
        return;
    };

    let status = Command::new(&rc_exe)
        .arg("/nologo")
        .arg("/fo")
        .arg(&res_path)
        .arg(&rc_path)
        .status();

    if let Ok(st) = status
        && st.success()
        && res_path.is_file()
    {
        println!("cargo:rustc-link-arg-bins={}", res_path.display());
    }
}

fn find_rc_exe() -> Option<PathBuf> {
    if Command::new("rc.exe")
        .arg("/?")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok()
    {
        return Some(PathBuf::from("rc.exe"));
    }

    let arch_order: &[&str] = if cfg!(target_arch = "aarch64") {
        &["arm64", "x64", "x86"]
    } else {
        &["x64", "x86", "arm64"]
    };

    let mut kit_roots: Vec<PathBuf> = Vec::new();
    if let Ok(sdk) = std::env::var("WindowsSdkDir") {
        kit_roots.push(PathBuf::from(sdk).join("bin"));
    }
    kit_roots.push(PathBuf::from(r"C:\Program Files (x86)\Windows Kits\10\bin"));
    kit_roots.push(PathBuf::from(r"C:\Program Files\Windows Kits\10\bin"));

    for bin_root in kit_roots {
        if let Some(found) = search_kit_bin(&bin_root, arch_order) {
            return Some(found);
        }
    }
    None
}

fn search_kit_bin(bin_root: &Path, arch_order: &[&str]) -> Option<PathBuf> {
    let entries = std::fs::read_dir(bin_root).ok()?;
    let mut versions: Vec<PathBuf> = entries
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    versions.sort();
    versions.reverse();

    for ver_dir in versions {
        for arch in arch_order {
            let candidate = ver_dir.join(arch).join("rc.exe");
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    for arch in arch_order {
        let candidate = bin_root.join(arch).join("rc.exe");
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}
