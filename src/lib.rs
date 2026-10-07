//! 基线内核。
//!
//! 三个层次（设计文档 §6）：
//!
//! ```text
//! 内核      目标 → 判定规则 → 归属 → 累积 → 快照 → 曲线
//! 适配器    manual_checkin / git_commits / external_metric / derived
//! 输出      HTML（Tauri 窗口、单文件存档）、CLI 文本
//! ```
//!
//! CLI（`src/main.rs`）和桌面外壳（`src-tauri`）都只调用这一层。
//! 判定口径、累积方式、快照冻结的语义不允许有第二份实现——
//! 两份实现一旦分叉，两条曲线就会开始讲不同的故事。
//!
//! 这一层不知道窗口、不知道命令行、不知道 HTML 之外的表现形式。

pub mod ai;
pub mod db;
pub mod metrics;
pub mod model;
pub mod render;
