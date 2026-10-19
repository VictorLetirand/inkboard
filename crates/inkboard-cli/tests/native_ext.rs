//! `ink convert` writes the native `.inkboard` extension and reads the three native names.

use inkboard_core::serial;
use std::path::PathBuf;
use std::process::Command;

const INK: &str = env!("CARGO_BIN_EXE_ink");

fn dir(tag: &str) -> PathBuf {
    let d = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join(format!("native-ext-{tag}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

fn convert(input: &PathBuf, output: &PathBuf) -> bool {
    Command::new(INK).arg("convert").arg(input).arg(output).output().expect("run ink").status.success()
}

#[test]
fn convert_writes_inkboard_and_reads_every_native_name() {
    let d = dir("convert");
    let mmd = d.join("flow.mmd");
    std::fs::write(&mmd, "flowchart TD\n  A --> B\n").unwrap();
    let native = d.join("flow.inkboard");
    assert!(convert(&mmd, &native));
    let doc = serial::load(&native).expect("native file written under the .inkboard extension");
    assert!(!doc.pages[0].objects.is_empty());
    // reading: the same content under the older names, and re-saving under each of the three names
    for old in ["old.inkboard.json", "older.tableau.json"] {
        let p = d.join(old);
        std::fs::copy(&native, &p).unwrap();
        let again = d.join(format!("again-{old}"));
        assert!(convert(&p, &again), "{old}");
        assert_eq!(serial::load(&again).unwrap().pages[0].objects.len(), doc.pages[0].objects.len(), "{old}");
    }
    let back = d.join("back.inkboard");
    assert!(convert(&d.join("old.inkboard.json"), &back));
    assert!(serial::load(&back).is_ok());
    std::fs::remove_dir_all(&d).ok();
}
