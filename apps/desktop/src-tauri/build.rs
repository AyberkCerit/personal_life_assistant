fn main() {
    // The notification app id is the bundle identifier the installer gives the Start menu
    // shortcut (AppUserModelID); one source, so the two never drift apart.
    let conf: serde_json::Value = serde_json::from_str(&std::fs::read_to_string("tauri.conf.json").expect("tauri.conf.json")).expect("tauri.conf.json is JSON");
    let id = conf["identifier"].as_str().expect("tauri.conf.json > identifier");
    println!("cargo:rustc-env=PLA_IDENTIFIER={id}");
    println!("cargo:rerun-if-changed=tauri.conf.json");
    tauri_build::build()
}
