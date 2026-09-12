fn main() {
    // Windows MSVC：test 二进制需要内嵌 Common-Controls manifest，
    // 否则 comctl32.dll 5.x 缺少 tauri 引用的入口点（STATUS_ENTRYPOINT_NOT_FOUND）。
    if std::env::var("CARGO_CFG_WINDOWS").is_ok() {
        println!("cargo:rustc-link-arg-tests=/MANIFEST:EMBED");
        println!(
            "cargo:rustc-link-arg-tests=/MANIFESTDEPENDENCY:type='Win32' \
name='Microsoft.Windows.Common-Controls' version='6.0.0.0' publicKeyToken='6595b64144ccf1df' \
language='*' processorArchitecture='*'"
        );
    }
    tauri_build::build();
}
