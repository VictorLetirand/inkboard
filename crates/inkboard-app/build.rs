//! Windows executable icon (`.rc` resource); no effect on other platforms.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=app.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        // a missing resource compiler (rc.exe) must not prevent compilation: the icon is then omitted
        let _ = embed_resource::compile("app.rc", embed_resource::NONE);
    }
}
