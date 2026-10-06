//! 生成 Windows 资源（版本信息、图标）与 capabilities 的权限 schema。
//!
//! 版本号取自本 crate 的 `version`，图标取自 `tauri.conf.json` 的 `bundle.icon`。

fn main() {
    tauri_build::build()
}
