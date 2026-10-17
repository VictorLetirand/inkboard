//! Icône de l'exécutable Windows (ressource `.rc`) ; sans effet sur les autres plateformes.

fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=app.rc");
    if std::env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "windows") {
        // l'absence de compilateur de ressources (rc.exe) ne doit pas empêcher la compilation : l'icône est alors omise
        let _ = embed_resource::compile("app.rc", embed_resource::NONE);
    }
}
