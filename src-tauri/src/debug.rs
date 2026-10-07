//! 调试桥：**只在 `--debug` 下开**，让外面能在活着的页面上执行任意 JS 并拿回结果。
//!
//! # 为什么需要它
//!
//! 窗口没有开发工具（无边框、无 DevTools），于是「改一行 → 构建 → 启动 → 截图 → 人眼看图」
//! 成了唯一的回路。那条回路每轮要花几十秒到几分钟，而截图只能回答「看起来对不对」：
//! 量不到一个元素的确切坐标、读不到一个变量的值、也没法问「这个按钮点下去发生了什么」。
//!
//! 这个桥把那件事换成一次 HTTP 请求：`POST /eval`，body 是 JS，返回它的值。
//! 于是「点某个选择器」变成「先问它在哪，再按坐标点」，不用再靠肉眼看图估坐标。
//!
//! # 它不是产品的一部分
//!
//! 它能在页面里执行任意 JS，等于把窗口的完全控制权交给本机上的任何进程。
//! 所以：**只绑 127.0.0.1、随机端口、显式传 `--debug` 才开**。
//! 默认开启或者绑到 0.0.0.0 的话，它就不是调试工具了。
//!
//! 端口写进 `<库所在目录>/debug.port`，脚本读那个文件就知道该连哪里——
//! 比固定端口好：不会撞端口，也不会有第二个实例抢同一个号。

use std::io::{Read as _, Write as _};
use std::net::{TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::time::Duration;

use tauri::Manager as _;

use crate::MAIN;

/// 一次 eval 最多等多久。页面正好在导航（比如刚提交完在整页重来）时是收不到回调的，
/// 这时候宁可让调用方超时重试，也不要让请求挂在那儿。
const EVAL_TIMEOUT: Duration = Duration::from_secs(8);

/// 开桥。返回端口号，同时把它写进 `debug.port`。
pub fn start(app: tauri::AppHandle, db: PathBuf) -> u16 {
    // 端口 0 = 让系统分配一个空闲的。
    let listener = TcpListener::bind(("127.0.0.1", 0)).expect("调试端口绑定失败");
    let port = listener.local_addr().expect("读不到调试端口").port();

    let file = port_file(&db);
    let _ = std::fs::write(&file, port.to_string());
    crate::log(&db, &format!("调试桥 · http://127.0.0.1:{port}（--debug，勿用于正式使用）"));

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut s) = stream else { continue };
            // 一个请求一条连接。出了错只影响这一次调用，不能把整个线程带走。
            let _ = serve(&mut s, &app);
        }
    });
    port
}

pub fn port_file(db: &Path) -> PathBuf {
    db.parent()
        .unwrap_or_else(|| Path::new("."))
        .join("debug.port")
}

fn serve(s: &mut TcpStream, app: &tauri::AppHandle) -> std::io::Result<()> {
    s.set_read_timeout(Some(Duration::from_secs(10)))?;
    let mut buf = Vec::new();
    let mut chunk = [0u8; 8192];

    // 先把头读完，才知道 body 有多长。
    let head_end = loop {
        if let Some(p) = find(&buf, b"\r\n\r\n") {
            break p;
        }
        let n = s.read(&mut chunk)?;
        if n == 0 {
            return Ok(());
        }
        buf.extend_from_slice(&chunk[..n]);
    };

    let head = String::from_utf8_lossy(&buf[..head_end]).to_string();
    let mut lines = head.lines();
    let request_line = lines.next().unwrap_or("").to_string();
    let len: usize = lines
        .find_map(|l| {
            let (k, v) = l.split_once(':')?;
            if k.eq_ignore_ascii_case("content-length") {
                v.trim().parse().ok()
            } else {
                None
            }
        })
        .unwrap_or(0);

    // 再按 Content-Length 把 body 读齐。
    let body_at = head_end + 4;
    while buf.len() < body_at + len {
        let n = s.read(&mut chunk)?;
        if n == 0 {
            break;
        }
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = String::from_utf8_lossy(&buf[body_at..]).to_string();

    let (status, json) = route(&request_line, &body, app);
    let payload = json.into_bytes();
    let head = format!(
        "HTTP/1.1 {status}\r\n\
         Content-Type: application/json; charset=utf-8\r\n\
         Content-Length: {}\r\n\
         Cache-Control: no-store\r\n\
         Connection: close\r\n\r\n",
        payload.len()
    );
    s.write_all(head.as_bytes())?;
    s.write_all(&payload)?;
    s.flush()
}

fn route(request_line: &str, body: &str, app: &tauri::AppHandle) -> (&'static str, String) {
    let mut it = request_line.split_whitespace();
    let method = it.next().unwrap_or("");
    let path = it.next().unwrap_or("");

    match (method, path) {
        // 就绪探询。脚本用它取代「睡八秒看看起来没有」——那八秒是纯等的。
        ("GET", "/ping") => ("200 OK", r#"{"ok":true}"#.to_string()),
        ("POST", "/reload") => match app.get_webview_window(MAIN) {
            Some(w) => match w.eval("location.reload()") {
                Ok(()) => ("200 OK", r#"{"ok":true}"#.to_string()),
                Err(e) => (status_err(), err_json(&e.to_string())),
            },
            None => (status_err(), err_json("窗口不在")),
        },
        ("POST", "/eval") => match app.get_webview_window(MAIN) {
            Some(w) => ("200 OK", eval(&w, body)),
            None => (status_err(), err_json("窗口不在")),
        },
        _ => ("404 Not Found", err_json("只有 GET /ping 和 POST /eval、/reload")),
    }
}

fn status_err() -> &'static str {
    "500 Internal Server Error"
}

fn err_json(msg: &str) -> String {
    serde_json::json!({ "ok": false, "e": msg }).to_string()
}

/// 在页面里跑一段 JS，把它的值拿回来。
///
/// WebView2 的 `ExecuteScript` 给回的是**最后一个表达式的 JSON 表示**，
/// 所以注入的片段本身返回一个 `JSON.stringify(...)` 出来的字符串：
/// 这样返回值长什么样由我们说了算，而不是由 WebView2 的序列化规则说了算
/// （比如 `DOMRect` 直接序列化会变成 `{}`——它的属性在原型上）。
fn eval(w: &tauri::WebviewWindow, js: &str) -> String {
    let src = serde_json::to_string(js).unwrap_or_else(|_| "\"\"".to_string());
    let script = format!(
        r#"(function(){{
  var __out;
  try {{
    var __v = (0, eval)({src});
    try {{
      __out = {{ ok: true, v: JSON.parse(JSON.stringify(__v === undefined ? null : __v)) }};
    }} catch (__e2) {{
      __out = {{ ok: true, v: String(__v) }};
    }}
  }} catch (__e) {{
    __out = {{ ok: false, e: String((__e && __e.message) || __e) }};
  }}
  return JSON.stringify(__out);
}})()"#
    );

    let (tx, rx) = std::sync::mpsc::channel();
    if let Err(e) = w.eval_with_callback(script, move |v| {
        let _ = tx.send(v);
    }) {
        return err_json(&e.to_string());
    }

    match rx.recv_timeout(EVAL_TIMEOUT) {
        Ok(raw) => {
            // 回调给的是「那个字符串」的 JSON 表示，所以先解一层字符串，再解内容。
            let inner = serde_json::from_str::<String>(&raw).unwrap_or(raw);
            match serde_json::from_str::<serde_json::Value>(&inner) {
                Ok(v) => v.to_string(),
                Err(e) => err_json(&format!("返回的不是 JSON（{e}）：{inner}")),
            }
        }
        Err(_) => err_json("等页面回话超时（可能正在整页重来）"),
    }
}

fn find(hay: &[u8], needle: &[u8]) -> Option<usize> {
    hay.windows(needle.len()).position(|w| w == needle)
}
