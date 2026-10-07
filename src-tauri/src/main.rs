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
//!
//! **记一条也走这条路**：命令写完库就 `location.reload()`，让整页按同一套逻辑重画。
//! 不做局部插入——卡片上的数字和曲线末端一旦各算各的，迟早会显示成两个数。

// release 构建不弹控制台。开发期保留它，否则诊断信息没处看。
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};

use chrono::Local;
use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{Manager as _, WebviewUrl, WebviewWindowBuilder};

use baseline::model::SourceKind;
use baseline::{ai, db, metrics, render};

/// 自定义协议名。Windows 上落在 `http://<名字>.localhost/`。
const SCHEME: &str = "baseline";

/// 窗口标签。托盘要按它把窗口找回来。
const MAIN: &str = "main";

/// 数据库路径。命令处理函数要用，建在 setup 里交给 Tauri 托管。
struct AppDb(PathBuf);

fn main() {
    let db_path = resolve_db_path();
    log(&db_path, &format!("启动 · 数据库 {}", db_path.display()));

    // 协议处理函数按请求现渲染，因此它需要自己拿到库的路径。
    let served = db_path.clone();
    tauri::Builder::default()
        .manage(AppDb(db_path.clone()))
        .invoke_handler(tauri::generate_handler![
            add_checkin,
            add_goal,
            update_goal,
            archive_goal,
            delete_goal,
            add_source,
            delete_source,
            save_settings,
            test_ai,
            classify_checkin,
            classify_backlog
        ])
        .register_uri_scheme_protocol(SCHEME, move |_ctx, request| {
            // 只有根路径给页面。favicon 之类的请求老实回 404，
            // 拿 HTML 去顶替会让浏览器缓存里塞进一堆垃圾。
            let path = request.uri().path();
            let (status, body) = if path == "/" || path == "/index.html" {
                // 详情页与设置页走服务端路由：?goal=N / ?settings=1。于是「窗口只显示
                // 这一屏」仍然由 Rust 一次渲染完成，JS 里不需要第二套模板。
                let q = request.uri().query().unwrap_or("");
                let view = if q.split('&').any(|kv| kv == "settings=1") {
                    render::View::Settings
                } else {
                    match q
                        .split('&')
                        .find_map(|kv| kv.strip_prefix("goal="))
                        .and_then(|v| v.parse::<i64>().ok())
                    {
                        Some(id) => render::View::Goal(id),
                        None => render::View::Main,
                    }
                };
                (200, page(&served, view))
            } else if path == "/__jslog" {
                // 界面上报的异常。窗口没有系统边框也就没有开发工具，
                // 异常不主动送出来就等于不存在。消息走请求体，省得再解一遍百分号转义。
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
                MAIN,
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

            build_tray(app)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            // 关掉窗口不等于退出。
            //
            // 托盘是「随手记一条」的入口，窗口收起来它才有意义；关一次就退出的话，
            // 托盘图标也跟着没了。所以这里拦下关闭，把窗口藏起来。
            // 真要退出走托盘右键菜单——留一条明的路，别让人找不到。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                let _ = window.hide();
            }
        })
        .run(tauri::generate_context!())
        .expect("启动基线窗口失败");
}

// ---------------------------------------------------------------- 托盘

fn build_tray(app: &tauri::App) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open", "打开基线", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "退出", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let mut tray = TrayIconBuilder::new()
        // 悬停提示就叫产品名。菜单和窗口标题已经说清它是什么了，
        // 再加一句「—— 记一条」是把一句话说两遍。
        .tooltip("基线")
        .menu(&menu)
        // 左键不弹菜单：左键就是「打开窗口记录」。菜单挂在右键上。
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open" => show_and_focus(app),
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                show_and_focus(tray.app_handle());
            }
        });

    if let Some(icon) = app.default_window_icon().cloned() {
        tray = tray.icon(icon);
    }
    tray.build(app)?;
    Ok(())
}

/// 把窗口叫回来，并且**把光标直接放进输入框**。
///
/// 托盘的意义就是「两步记一条」：点图标、打字、回车。省掉的那一步是
/// 「在窗口里再点一下输入框」——它很小，但正是这种小步骤决定了会不会真的去用。
fn show_and_focus(app: &tauri::AppHandle) {
    let Some(w) = app.get_webview_window(MAIN) else {
        return;
    };
    let _ = w.show();
    let _ = w.unminimize();
    let _ = w.set_focus();
    let _ = w.eval("window.__blFocusComposer && window.__blFocusComposer()");
}

// ---------------------------------------------------------------- 命令

fn err(e: anyhow::Error) -> String {
    format!("{e:#}")
}

/// 记一条。永远是「现在」——输入框里没有日期，时间线是一条只往后长的流水。
///
/// 补记的语义由内核定：写库之后重算**今天**的快照，过去的日子不动。
/// 所以补一条过去的记录不会把曲线往回改，只会在今天抬一格。
/// 记一条的结果。**写库是同步做完的，AI 不在这儿等。**
///
/// `rename_all = "camelCase"`：这是**返回值**，不是参数，Tauri 不会替我们做
/// 大小写转换（参数那边才会）。漏了它，界面上读到的就是 `undefined`——
/// 表现是「记录写进去了，但再也没人叫 AI 补判」，而两边都不会报错。（踩过。）
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
struct Added {
    id: i64,
    /// 这条记录还有归属没定，回头叫 AI 补判一次。
    ///
    /// 两种情况都算：挂了目标但那个目标下有多条规则说不清，以及**压根没挂目标**。
    /// 后者是「记下来是第一步，归到哪个目标是第二步」里的第二步——
    /// 它以前只能人工做，于是不选目标记下来的记录就永远躺在流水里。
    needs_ai: bool,
}

#[tauri::command]
fn add_checkin(
    state: tauri::State<'_, AppDb>,
    goal_ids: Vec<i64>,
    note: String,
    picks: Vec<(i64, i64)>,
) -> Result<Added, String> {
    let conn = db::open(&state.0).map_err(err)?;
    // 挂到没有手工打卡规则的目标上是允许的——只记下「我本来想推进它」，
    // 不进那条曲线。所以这里只校验目标存在，不校验规则。
    for id in &goal_ids {
        if db::goal_by_id(&conn, *id).map_err(err)?.is_none() {
            return Err(format!("目标 #{id} 不存在"));
        }
    }

    // **归属先按机械规矩定，一次网络请求都不发。**
    //
    // 一开始我把模型放在这一步之前，于是点「记下」之后界面要等它最长二十秒——
    // 记录没落库、屏幕上什么都没变，然后突然整页刷新。顺序反了：
    // 先写下来（这一刻就能看到），再说它算哪条规则。
    let ai_on = ai::config(&conn).map_err(err)?.ready();
    let links = db::resolve_links(&conn, &goal_ids, &picks, ai_on).map_err(err)?;

    let now = Local::now();
    let stamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let id = db::checkin_add(
        &conn,
        &links,
        &now.format("%Y-%m-%d").to_string(),
        &now.format("%H:%M").to_string(),
        1.0,
        &note,
        &stamp,
    )
    .map_err(err)?;
    metrics::roll(&conn, now.date_naive()).map_err(err)?;

    // 还有没有说不清的格子？两种都算，交给 `classify_one` 一并处理。
    let mut needs_ai = false;
    if ai_on {
        if links.is_empty() {
            // 一个目标都没选。规则是「第二步」，那就让 AI 去走它。
            needs_ai = !db::goal_list(&conn, false).map_err(err)?.is_empty();
        } else {
            for l in &links {
                if l.source_id.is_none()
                    && db::manual_sources_of(&conn, l.goal_id)
                        .map_err(err)?
                        .len()
                        > 1
                {
                    needs_ai = true;
                }
            }
        }
    }
    Ok(Added { id, needs_ai })
}

/// 给一条记录补上归属：挂空着的规则、或者压根没挂的目标。
///
/// 两个地方会叫它：刚记完一条之后的自动补判，以及列表上那些补判失败的记录重试。
/// **只有这一份实现**——补判的规矩不该有第二种说法。
#[tauri::command]
fn classify_checkin(state: tauri::State<'_, AppDb>, checkin_id: i64) -> Result<String, String> {
    let conn = db::open(&state.0).map_err(err)?;
    let today = Local::now().date_naive();
    let f = ai::classify_one(&conn, checkin_id, today).map_err(err)?;
    if let Some(t) = f.trouble {
        return Err(t);
    }
    Ok(if f.decided == 0 {
        "AI 没判出来。".to_string()
    } else {
        format!("AI 归了 {} 条。", f.decided)
    })
}

/// 把一批还没归好的记录挨条补判。
///
/// `goal_id` 给了就是那个目标下挂空着的（详情页那个按钮）；
/// 没给就是全库**没关联目标**的（主视图那条提示）。
#[tauri::command]
fn classify_backlog(
    state: tauri::State<'_, AppDb>,
    goal_id: Option<i64>,
) -> Result<String, String> {
    let conn = db::open(&state.0).map_err(err)?;
    let today = Local::now().date_naive();
    let f = match goal_id {
        Some(g) => ai::classify_goal_backlog(&conn, g, today),
        None => ai::classify_unlinked_backlog(&conn, today),
    }
    .map_err(err)?;
    if let Some(t) = f.trouble {
        return Err(t);
    }
    Ok(if f.decided == 0 {
        "AI 没判出来，这些先没归到规则。".to_string()
    } else {
        format!("AI 归了 {} 条。", f.decided)
    })
}

/// 建目标，同时写下它的第一条判定规则。
///
/// **目标和规则必须一起建成。** 没有规则的目标给不出「什么算推进它」，
/// 它的曲线永远不会动——那不是目标，只是一句愿望。所以这里不给「先建目标、
/// 以后再补规则」留口子。
///
/// 返回建好之后的当前值：规则刚写完就把数算出来，规则立刻变成可检验的。
/// 若永远是 0，他会知道要么规则写错了，要么那条线真的没动——两种都值得知道。
#[tauri::command]
fn add_goal(
    state: tauri::State<'_, AppDb>,
    title: String,
    why: String,
    rationale: String,
    kind: String,
    target: String,
) -> Result<f64, String> {
    let conn = db::open(&state.0).map_err(err)?;
    let title = title.trim().to_string();
    if title.is_empty() {
        return Err("目标得有个名字。".into());
    }
    let kind = SourceKind::parse(&kind).map_err(err)?;

    let now = Local::now();
    let stamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
    let color = db::next_free_color(&conn).map_err(err)?;
    let id = db::goal_add(&conn, &title, why.trim(), &color, &stamp).map_err(err)?;
    db::source_add(&conn, id, kind, target.trim(), "{}", rationale.trim(), &stamp).map_err(err)?;
    // 规则刚落地就把归属补一遍，而不是等下次开库：这条规则可能正好是某个目标下
    // 唯一能收手工记录的一条，那么它下面原来「没归到任何规则」的记录此刻就该归位。
    db::backfill_attribution(&conn).map_err(err)?;
    metrics::roll(&conn, now.date_naive()).map_err(err)?;
    metrics::value_today(&conn, id, now.date_naive()).map_err(err)
}

/// 改目标的名字和动机。
#[tauri::command]
fn update_goal(state: tauri::State<'_, AppDb>, id: i64, title: String, why: String) -> Result<(), String> {
    let conn = db::open(&state.0).map_err(err)?;
    db::goal_update(&conn, id, &title, &why).map_err(err)
}

/// 归档目标。**必须写一句原因**——那是三个月后唯一能回看的东西。
///
/// 归档不是删除：曲线留着、记录留着，理由接在 why 后面。放弃也是事实，值得记下来。
#[tauri::command]
fn archive_goal(state: tauri::State<'_, AppDb>, id: i64, reason: String) -> Result<(), String> {
    let conn = db::open(&state.0).map_err(err)?;
    let now = Local::now().format("%Y-%m-%d %H:%M:%S").to_string();
    db::goal_archive(&conn, id, &reason, &now).map_err(err)
}

/// 删目标。**只有一条记录都没有的才删得掉**——动过的历史不该被一次点击抹掉，
/// 所以判定放在 db 层，界面上那个按钮置灰只是提前说明，不是唯一的防线。
#[tauri::command]
fn delete_goal(state: tauri::State<'_, AppDb>, id: i64) -> Result<(), String> {
    let conn = db::open(&state.0).map_err(err)?;
    db::goal_delete(&conn, id).map_err(err)
}

/// 给已有的目标补一条判定规则。
#[tauri::command]
fn add_source(
    state: tauri::State<'_, AppDb>,
    goal_id: i64,
    kind: String,
    target: String,
    rationale: String,
) -> Result<f64, String> {
    let conn = db::open(&state.0).map_err(err)?;
    let kind = SourceKind::parse(&kind).map_err(err)?;
    let now = Local::now();
    let stamp = now.format("%Y-%m-%d %H:%M:%S").to_string();
    db::source_add(&conn, goal_id, kind, target.trim(), "{}", rationale.trim(), &stamp).map_err(err)?;
    // 见 add_goal：新规则可能让这个目标下原本悬着的记录变得唯一可归属。
    db::backfill_attribution(&conn).map_err(err)?;
    metrics::roll(&conn, now.date_naive()).map_err(err)?;
    metrics::value_today(&conn, goal_id, now.date_naive()).map_err(err)
}

#[tauri::command]
fn delete_source(state: tauri::State<'_, AppDb>, id: i64) -> Result<(), String> {
    let conn = db::open(&state.0).map_err(err)?;
    db::source_delete(&conn, id).map_err(err)
}

/// 保存设置。**密钥的明文只在这一趟里存在**：收下来、加密、落库，不回读、不回显。
///
/// `api_key` 为 `None` 表示「不动现在这把」——界面上的密码框是空的，
/// 空着不该等于清掉。清掉要显式地给一个空串。
#[tauri::command]
fn save_settings(
    state: tauri::State<'_, AppDb>,
    enabled: bool,
    base_url: String,
    model: String,
    threshold: u8,
    api_key: Option<String>,
) -> Result<(), String> {
    let conn = db::open(&state.0).map_err(err)?;
    if base_url.trim().is_empty() {
        return Err("接口地址不能空着——填不对就关掉 AI 判定，那时拿不准会问你。".into());
    }
    if model.trim().is_empty() {
        return Err("模型名不能空着。".into());
    }
    ai::save(
        &conn,
        enabled,
        &base_url,
        &model,
        threshold,
        api_key.as_deref(),
    )
    .map_err(err)
}

/// 设置页上那个「测一下」。真发一个最小的请求，把结果原话带回去。
#[tauri::command]
fn test_ai(state: tauri::State<'_, AppDb>) -> Result<String, String> {
    let conn = db::open(&state.0).map_err(err)?;
    ai::probe(&conn).map_err(err)
}

// ---------------------------------------------------------------- 路径与日志

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
fn page(db_path: &Path, view: render::View) -> String {
    match build_page(db_path, view) {
        Ok(html) => html,
        Err(error) => {
            let message = format!("{error:#}");
            log(db_path, &format!("渲染失败 · {message}"));
            render::error_page(&message)
        }
    }
}

fn build_page(db_path: &Path, view: render::View) -> anyhow::Result<String> {
    let conn = db::open(db_path)?;
    let today = Local::now().date_naive();
    // 每次加载补一次快照。缺了这一步，曲线会停在最后一次手动 tick 的位置上，
    // 而窗口看起来一切正常——这种「静默地少了一段」最难发现。
    metrics::roll(&conn, today)?;
    render::render(&conn, today, render::Chrome::Window, view)
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
