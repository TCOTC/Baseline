//! 基线桌面外壳。
//!
//! 窗口不自己渲染界面。内核把整页渲染成一个自包含的 HTML 串，外壳把它挂在自定义协议
//! `baseline` 上——Windows 上窗口通过 `http://baseline.localhost/` 访问它。
//!
//! # 为什么不起本地 HTTP 服务
//!
//! 单机、单窗口的程序不需要一个监听端口。起了服务就要跟着操心端口被占、防火墙弹窗，
//! 以及「服务没起来所以窗口一片白」。自定义协议没有端口，生命周期跟着窗口走。
//!
//! # 每次加载都重渲染
//!
//! 协议处理函数里现开库、现补快照、现渲染。所以按 F5 得到的就是当下最新的一屏，
//! 既不需要「刷新数据」按钮，也不会出现界面上的数字比库里旧的情况。
//! 响应必须带 `Cache-Control: no-store`，否则 WebView2 会把上一次的页面原样还回来——
//! 那样 F5 看着像生效了，其实什么都没发生。

// release 构建不弹控制台。开发期保留它，否则诊断信息没处看。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::Local;
use tauri::{WebviewUrl, WebviewWindowBuilder};

use baseline::{db, metrics, render};

/// 自定义协议名。Windows 上落在 `http://<名字>.localhost/`。
const SCHEME: &str = "baseline";

fn main() {
    let db_path = resolve_db_path();
    log(&db_path, &format!("启动 · 数据库 {}", db_path.display()));

    // 协议处理函数按请求现渲染，因此它需要自己拿到库的路径。
    let served = db_path.clone();
    tauri::Builder::default()
        .register_uri_scheme_protocol(SCHEME, move |_ctx, request| {
            // 只有根路径给页面。favicon 之类的请求老实回 404，
            // 拿 HTML 去顶替会让浏览器缓存里塞进一堆垃圾。
            let path = request.uri().path();
            let (status, body) = if path == "/" || path == "/index.html" {
                (200, page(&served))
            } else if path == "/__jslog" {
                // 界面上报的异常。窗口没有开发工具，不主动送出来就等于不存在。
                // 消息走请求体，省得在 Rust 这边再解一遍百分号转义。
                let msg = String::from_utf8_lossy(request.body()).to_string();
                log(&served, &format!("界面 · {msg}"));
                (204, String::new())
            } else {
                (404, String::from("not found"))
            };
            tauri::http::Response::builder()
                .status(status)
                .header(tauri::http::header::CONTENT_TYPE, "text/html; charset=utf-8")
                .header(tauri::http::header::CACHE_CONTROL, "no-store")
                .body(body.into_bytes())
                .expect("构造 HTTP 响应失败")
        })
        .setup(|app| {
            // 窗口在代码里建而不是写在 tauri.conf.json 里：配置里声明的窗口会在 setup
            // 之前就被创建，那样就没有机会按运行时的数据库路径决定地址了。
            WebviewWindowBuilder::new(
                app,
                "main",
                WebviewUrl::External(format!("http://{SCHEME}.localhost/").parse()?),
            )
            .title("基线")
            // 系统边框关掉，顶栏由页面自绘（见 render.rs 的 title_bar_html）。
            // 缩放拖拽和 Aero Snap 都不受影响：tao 在无边框时只接管顶边，
            // 其余交给 DWM 阴影，拖拽走的仍是 WM_NCLBUTTONDOWN/HTCAPTION。
            // 唯一损失是 Win11 悬停最大化按钮时的 Snap Layouts 弹层。
            .decorations(false)
            // 比界面内容（1120px）宽出一百多像素，两侧才留得住呼吸的余地；
            // 和窗口等宽的话，卡片会顶到边框上，和确认过的设计稿不是一回事。
            .inner_size(1280.0, 880.0)
            .min_inner_size(880.0, 600.0)
            .center()
            .build()?;
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("启动基线窗口失败");
}

/// 数据库位置：`--db <路径>` > `BASELINE_DB` 环境变量 > `%APPDATA%\Baseline\baseline.db`。
///
/// 后两级和 CLI 共用 `db::default_path`，不各写一份——两边指到不同的库，
/// 就会出现「命令行里明明有数据，窗口里是空的」。
///
/// 故意不引 clap：外壳总共就这一个参数，为它拉一整套解析器不划算。
fn resolve_db_path() -> PathBuf {
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        if arg == "--db" {
            if let Some(path) = args.next() {
                return PathBuf::from(path);
            }
        } else if let Some(path) = arg.strip_prefix("--db=") {
            return PathBuf::from(path);
        }
    }
    db::default_path()
}

/// 渲染当前这一屏。失败时返回一个能读的错误页，而不是空白。
fn page(db_path: &Path) -> String {
    match build_page(db_path) {
        Ok(html) => html,
        Err(error) => {
            let message = format!("{error:#}");
            log(db_path, &format!("渲染失败 · {message}"));
            render::error_page(&message)
        }
    }
}

fn build_page(db_path: &Path) -> anyhow::Result<String> {
    let conn = db::open(db_path)?;
    let today = Local::now().date_naive();
    // 每次加载补一次快照。缺了这一步，曲线会停在最后一次手动 tick 的位置上，
    // 而窗口看起来一切正常——这种「静默地少了一段」最难发现。
    metrics::roll(&conn, today)?;
    render::render(&conn, today, render::Chrome::Window)
}

/// 日志与数据放同一个目录。
///
/// release 构建没有控制台，`eprintln!` 会掉进虚空。「窗口起来了但读不到数据」
/// 这类问题必须留下痕迹，否则只能靠猜。
fn log(db_path: &Path, message: &str) {
    let path = db_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("desktop.log");
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let line = format!("[{}] {message}\n", Local::now().to_rfc3339());
    if let Ok(mut file) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = file.write_all(line.as_bytes());
    }
}
