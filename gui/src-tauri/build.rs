fn main() {
    // Copy chip database files from ../../data/ to resources/ for bundling
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap();
    let resources_dir = std::path::PathBuf::from(&manifest_dir).join("resources");
    std::fs::create_dir_all(&resources_dir).ok();

    let data_dir = std::path::PathBuf::from(&manifest_dir)
        .join("..")
        .join("..")
        .join("data");
    for filename in ["infoic.xml", "logicic.xml"] {
        let src = data_dir.join(filename);
        let dest = resources_dir.join(filename);
        if src.exists() {
            std::fs::copy(&src, &dest).ok();
        }
    }

    // Windows installers additionally bundle the standalone CLI + TUI: the
    // root-workspace release build drops them in ../../target/release/ (the
    // GitHub release job builds them before tauri-action runs). Bundled to
    // the install dir next to the XMLs → first run resolves the database via
    // the exe-dir search. Dir is always created so the resources map never
    // errors; missing exes (no prior root build) are skipped silently.
    let bin_dir = resources_dir.join("bin");
    std::fs::create_dir_all(&bin_dir).ok();
    if std::env::var("TARGET").is_ok_and(|t| t.contains("windows")) {
        let target_release = std::path::PathBuf::from(&manifest_dir)
            .join("..")
            .join("..")
            .join("target")
            .join("release");
        for filename in ["minipro.exe", "minipro-tui.exe"] {
            let src = target_release.join(filename);
            if src.exists() {
                std::fs::copy(&src, bin_dir.join(filename)).ok();
            }
        }
    }

    tauri_build::build()
}
