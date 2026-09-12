//! 绑定门禁：tauri-specta 生成的 TS 与仓库内 generated.ts 逐字节比对。
//! 设 `UPDATE_BINDINGS=1` 重新写入。

use std::path::Path;

#[test]
fn generated_bindings_match() {
    let builder = yohaku_companion_lib::command_builder();
    let temp = tempfile::tempdir().unwrap();
    let out = temp.path().join("generated.ts");
    builder
        .export(specta_typescript::Typescript::default(), &out)
        .expect("export bindings");
    let generated = std::fs::read_to_string(&out).unwrap();

    let checked = Path::new(env!("CARGO_MANIFEST_DIR")).join("../src/api/generated.ts");
    if std::env::var("UPDATE_BINDINGS").is_ok() {
        std::fs::create_dir_all(checked.parent().unwrap()).unwrap();
        std::fs::write(&checked, generated).unwrap();
        return;
    }
    let existing = std::fs::read_to_string(&checked).unwrap_or_else(|_| {
        panic!(
            "missing {} — run UPDATE_BINDINGS=1 cargo test",
            checked.display()
        )
    });
    assert_eq!(
        existing, generated,
        "generated.ts is stale — run `UPDATE_BINDINGS=1 cargo test -p yohaku-companion`"
    );
}
