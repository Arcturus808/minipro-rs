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

    tauri_build::build()
}
