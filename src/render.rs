//! HTML 渲染。
//!
//! 一份渲染成果同时供两条出口用：
//! - Tauri 窗口（`src-tauri` 把这份 HTML 挂在自定义协议上，窗口每次加载即重渲染）
//! - `baseline render` 落盘成自包含的单文件，方便在浏览器里看一眼或存档
//!
//! 交互（分配、打卡）暂时留在 CLI。
//!
//! 曲线按设计规范绘制：**阶梯不平滑**、零值不画线、破零点加高亮。

// `GoalSeries::days` 等字段留给后续的坐标轴与对比视图。
#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Weekday};
use rusqlite::Connection;
use serde_json::json;

use crate::db;
use crate::metrics::{self, GoalSeries};
use crate::model::Goal;

const CSS: &str = include_str!("../assets/view.css");

/// 时间线、输入框、新建目标对话的脚本。
///
/// 时间线必须是 JS 渲染的：虚拟滚动要知道滚到哪儿了，而 Rust 渲染的是一整串
/// 静态 HTML，"只画可见的那几十行"这件事插不进去。**行的结构和 class 仍然来自
/// view.css**，所以样式还是只有一份——搬走的只是行的拼装。
const VIEW_JS: &str = include_str!("../assets/view.js");

/// 底部输入框。
///
/// 结构是**上下两层**：上面写文本，下面一行左边是「这条记录推进了哪些目标」、
/// 右边「记下」。目标做成**可多选的标签**摆在这一行里，点一下选中、再点一下取消。
///
/// **默认一个都不选。** 记下来是第一步，归到哪个目标是第二步；逼着先选目标，
/// 等于在「我还不知道这算推进什么」的时候替人做决定。
///
/// 列表里是**全部目标**，不只是能手工打卡的那些。规则里没有手工打卡的目标
/// 会带一句说明——挂上去是记下「我本来想推进它」，但不进那条曲线。
/// 标签在一行里放不下时收起来，只留一个「…」，点开是同一个菜单。
fn composer_html(conn: &Connection, goals: &[Goal]) -> Result<String> {
    if goals.is_empty() {
        // 一个目标都没有，就不摆一个按下去没反应的输入框。
        return Ok(String::new());
    }
    let manual = manual_goals(conn, goals)?;

    let mut chips = String::new();
    let mut menu = String::new();
    for g in goals {
        let counts = manual.contains(&g.id);
        let tip = if counts {
            String::new()
        } else {
            "（规则里没有手工打卡，记了也不动这条线）".to_string()
        };
        chips.push_str(&format!(
            r#"<button type="button" class="gchip {color}{on}" data-goal="{id}" title="{title}{tip}"><i></i>{title}</button>"#,
            color = esc(&g.color),
            on = if counts { "" } else { " noscore" },
            id = g.id,
            title = esc(&g.title),
            tip = esc(&tip),
        ));
        menu.push_str(&format!(
            r#"<button type="button" class="gopt {color}{on}" data-goal="{id}"><i></i><span class="gn">{title}</span>{note}</button>"#,
            color = esc(&g.color),
            on = if counts { "" } else { " noscore" },
            id = g.id,
            title = esc(&g.title),
            note = if counts {
                String::new()
            } else {
                r#"<em>规则里没有手工打卡，记了也不动这条线</em>"#.to_string()
            },
        ));
    }

    Ok(format!(
        r#"
      <form class="composer" id="composer" autocomplete="off">
        <input type="hidden" id="composer-goals" value="">
        <div class="gmenu" id="gmenu" hidden>
          <div class="gmenu-h">这条记录推进了哪些目标？<span>可以多选，也可以一个都不选</span></div>
          {menu}
        </div>
        <div class="box">
          <textarea id="composer-input" rows="2" maxlength="500"
                    placeholder="刚做了什么？" aria-label="记一条"></textarea>
          <div class="crow">
            <div class="gchips" id="gchips">{chips}</div>
            <button type="button" class="gmore" id="gmore" hidden>…</button>
            <button type="submit" class="send" tabindex="-1">记下</button>
          </div>
        </div>
      </form>"#,
        menu = menu,
        chips = chips,
    ))
}

/// 左栏左下角的加号。
///
/// 不再有上限（2026-10-06 取消）。原来卡 3 个的理由是「这是产品与聊天机器人的分界线」，
/// 但那条线现在由别的东西守：建目标必须一并写下判定规则，写不出就不让建；
/// 而每张卡片底部永远印着那条规则本身。数量的多少不是那条分界线。
fn addgoal_html() -> String {
    r#"
      <button type="button" class="addgoal" id="addgoal">
        <svg viewBox="0 0 12 12" aria-hidden="true"><path d="M6 1v10M1 6h10" stroke="currentColor" stroke-width="1.5" stroke-linecap="round"/></svg>
        <span>新建目标</span>
      </button>"#
        .to_string()
}


/// 页面外壳。
///
/// 同一个渲染结果有两条出口，差别只在有没有窗口顶栏：
/// **Tauri 窗口**要一条能拖拽、带窗口按钮的顶栏（系统没给边框）；
/// **导出的单文件 HTML**在浏览器里看，浏览器自己有标签栏——
/// 再画一条点了没反应的假顶栏，比没有更糟。
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Chrome {
    /// Tauri 窗口：自绘顶栏
    Window,
    /// 导出的单文件 HTML
    File,
}

/// 窗口顶栏。拖动、双击最大化由 `data-tauri-drag-region` 接管（Tauri 注入的脚本），
/// 三个按钮走 `window.__TAURI__`。
///
/// 图标是 Segoe Fluent Icons 的字符，不是画的 SVG——
/// E921 最小化 / E922 最大化 / E923 还原 / E8BB 关闭，
/// 就是 Windows 标题栏自己用的那四个字形。
///
/// 顶栏里没有产品名，也没有日期。它横跨整个窗口宽度，是窗口的边框而不是页面的头部。
///
/// 这是全项目唯一一处 JS，而且是窗口外壳而非产品逻辑——
/// 界面本身仍然由 Rust 一次渲染成字符串，不留第二份模板。
fn title_bar_html() -> &'static str {
    r#"
  <div class="bar" data-tauri-drag-region>
    <div class="drag" data-tauri-drag-region></div>
    <button type="button" class="wbtn" id="w-min" tabindex="-1" aria-label="最小化">&#xE921;</button>
    <button type="button" class="wbtn" id="w-max" tabindex="-1" aria-label="最大化">&#xE922;</button>
    <button type="button" class="wbtn close" id="w-close" tabindex="-1" aria-label="关闭">&#xE8BB;</button>
  </div>"#
}

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

/// 「周一」…「周日」。
///
/// 用「周X」不用「星期X」：流水上方那一行是**扫读用的坐标**，不是一句话。
/// 三个字里只有后一个字有信息量。
fn weekday_cn(d: NaiveDate) -> &'static str {
    match d.weekday() {
        Weekday::Mon => "周一",
        Weekday::Tue => "周二",
        Weekday::Wed => "周三",
        Weekday::Thu => "周四",
        Weekday::Fri => "周五",
        Weekday::Sat => "周六",
        Weekday::Sun => "周日",
    }
}

/// 去掉浮点尾巴：23.0 -> "23"，23.5 -> "23.5"
pub fn num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        format!("{v:.1}")
    }
}

/// 阶梯路径。**不做插值**——数据是离散累积的，平滑会伪造出一个从未发生过的渐进过程。
/// 返回 (path-d, 各点屏幕坐标)。
fn step_path(vals: &[f64], w: f64, h: f64, pad: f64) -> (String, Vec<(f64, f64)>) {
    let n = vals.len();
    if n == 0 {
        return (String::new(), vec![]);
    }
    let mx = vals.iter().cloned().fold(0.0f64, f64::max).max(1.0);
    let ih = h - pad * 2.0;
    let xf = |i: usize| {
        if n <= 1 {
            0.0
        } else {
            i as f64 / (n - 1) as f64 * w
        }
    };
    let yf = |v: f64| pad + ih - (v / mx) * ih;

    let mut d = format!("M {:.2} {:.2}", xf(0), yf(vals[0]));
    let mut pts = vec![(xf(0), yf(vals[0]))];
    for i in 1..n {
        d.push_str(&format!(" H {:.2} V {:.2}", xf(i), yf(vals[i])));
        pts.push((xf(i), yf(vals[i])));
    }
    (d, pts)
}

fn curve_svg(s: &GoalSeries) -> String {
    const W: f64 = 300.0;
    const H: f64 = 44.0;
    const PAD: f64 = 5.0;

    if !s.has_data {
        return r#"<div class="cempty">这条线还没有任何记录</div>"#.to_string();
    }
    let (d, _pts) = step_path(&s.values, W, H, PAD);
    let base_y = H - PAD;

    // **曲线上一个圆点都没有。**
    //
    // 原来有两个：破零点（这条线第一次从 0 变正的位置）那个带光晕的点，
    // 和线尾那个「现在到哪了」的点。第一个删掉是因为线头那一段本来就是台阶，
    // 再点一个点上去读起来像一个没来由的句号；第二个我一度以为它是信息，
    // 留着了——**它不是**。阶梯线的末端本来就在那儿，点一个点只是把
    // 「最后一个数据点」画了两遍。
    //
    // 强调留给曲线本身的形状：平了多少天、哪天开始抬，线自己说得清。

    // 悬停时给出窗口范围，省得去数格子
    let range = match (s.days.first(), s.days.last()) {
        (Some(a), Some(b)) => format!("{a} → {b} · {} 天 · 阶梯线不做插值", s.days.len()),
        _ => String::new(),
    };

    format!(
        r#"<svg class="cv" viewBox="0 0 {W} {H}" preserveAspectRatio="none" xmlns="http://www.w3.org/2000/svg">"#
    ) + &format!(r#"<title>{}</title>"#, esc(&range))
        + &format!(
            r#"<line class="cv-base" x1="0" y1="{base_y:.2}" x2="{W}" y2="{base_y:.2}"/>"#
        )
        + &format!(r#"<path class="cv-line" d="{d}"/></svg>"#)
}

fn strip_html(on: &[bool]) -> String {
    let mut s = String::from(r#"<div class="strip">"#);
    for b in on {
        s.push_str(if *b {
            r#"<span class="tick on"></span>"#
        } else {
            r#"<span class="tick"></span>"#
        });
    }
    s.push_str("</div>");
    s
}

/// 一张目标卡。`clickable` 时整张卡是一个链接，点开进这个目标的详情页。
///
/// 用链接而不是 JS 点击：详情页是**服务端路由**（`?goal=N`），
/// 于是「窗口只显示这一个目标」这件事仍然由 Rust 一次渲染完成，
/// 不需要在 JS 里再维护一套详情页的模板。
fn card_html(g: &Goal, s: &GoalSeries, rule_line: &str, clickable: bool) -> String {
    let has = s.has_data;
    let (vclass, dclass) = if has { ("v", "cd") } else { ("v none", "cd zero") };
    let delta = if has {
        let d = s.delta_week;
        if d > 0.0 {
            format!("+{}", num(d))
        } else if d < 0.0 {
            num(d)
        } else {
            "持平".to_string()
        }
    } else {
        "还没有记录".to_string()
    };
    let (open, close) = if clickable {
        (format!(r#"<a class="card {color}" href="?goal={id}">"#, color = esc(&g.color), id = g.id),
         "</a>".to_string())
    } else {
        (format!(r#"<div class="card {color}">"#, color = esc(&g.color)), "</div>".to_string())
    };
    format!(
        r#"
    {open}
      <div class="chead"><span class="ct"><i></i>{title}</span><span class="{dclass}">{delta}</span></div>
      <div class="vrow"><span class="{vclass}">{cur}</span><span class="u">次</span></div>
      {curve}
      {strip}
      <div class="gmeta">{rule}</div>
    {close}"#,
        open = open,
        close = close,
        title = esc(&g.title),
        dclass = dclass,
        delta = delta,
        vclass = vclass,
        cur = num(s.current),
        curve = curve_svg(s),
        strip = strip_html(&metrics::strip_of(s)),
        rule = rule_line,
    )
}

/// 目标的判定规则摘要，显示在卡片底部。
fn rule_line(conn: &Connection, g: &Goal) -> Result<String> {
    let srcs = db::sources_of(conn, g.id)?;
    if srcs.is_empty() {
        return Ok(r#"<b>没有判定规则</b> —— 这个目标给不出「什么算推进它」"#.to_string());
    }
    let parts: Vec<String> = srcs
        .iter()
        .map(|s| {
            let label = s.kind.label();
            // 卡片底部优先显示 target；没有 target 就显示 rationale——
            // 那句「什么算推进它」才是这条规则的核心，不该被藏起来。
            let detail = if !s.target.is_empty() {
                format!(" · {}", esc(&s.target))
            } else if !s.rationale.trim().is_empty() {
                format!(" · {}", esc(s.rationale.trim()))
            } else {
                String::new()
            };
            let note = if s.kind.implemented() {
                String::new()
            } else {
                "（未接入）".to_string()
            };
            format!("{label}{detail}{note}")
        })
        .collect();
    Ok(parts.join(" + "))
}

fn day_label(day: &NaiveDate, today: NaiveDate) -> (String, Option<String>) {
    let diff = (today - *day).num_days();
    match diff {
        0 => ("今天".into(), None),
        1 => ("昨天".into(), None),
        _ => (
            day.format("%m-%d").to_string(),
            Some(weekday_cn(*day).to_string()),
        ),
    }
}

/// 时间线的行数据（JSON）。
///
/// 顺序是**下新上旧**：像一条流水，最新的一条贴着底部输入框。
/// 行的结构由 `assets/view.js` 拼，class 仍然来自 `view.css`。
fn timeline_json(
    conn: &Connection,
    goals: &HashMap<i64, Goal>,
    manual: &std::collections::HashSet<i64>,
    today: NaiveDate,
) -> Result<String> {
    Ok(rows_json(&db::checkins_all(conn)?, goals, manual, today))
}

/// 哪些目标的规则接受手工记录。
///
/// **只有这些目标的手工记录才算数。** 挂到别的目标上是允许的（记下「我本来想推进它」），
/// 但不进那条曲线——设计文档 §5.3 的例子 C：在 Learn-English 上写代码不能推动「英语」。
fn manual_goals(conn: &Connection, goals: &[Goal]) -> Result<std::collections::HashSet<i64>> {
    let mut s = std::collections::HashSet::new();
    for g in goals {
        if db::sources_of(conn, g.id)?
            .iter()
            .any(|x| x.kind == crate::model::SourceKind::ManualCheckin)
        {
            s.insert(g.id);
        }
    }
    Ok(s)
}

/// 把一串记录拼成流水行。
///
/// 主视图（全部记录）和详情页（某个目标的记录）共用这一个函数，
/// 所以两处的行结构不可能走样。
fn rows_json(
    checkins: &[crate::model::Checkin],
    goals: &HashMap<i64, Goal>,
    manual: &std::collections::HashSet<i64>,
    today: NaiveDate,
) -> String {
    let mut rows: Vec<serde_json::Value> = Vec::new();
    let mut last_day = String::new();

    for c in checkins {
        if c.day != last_day {
            if let Ok(d) = metrics::parse_day(&c.day) {
                let (label, wd) = day_label(&d, today);
                // 具体年月日**不进可见文本**，进 title：
                // 平时需要的是「哪天」和星期几，精确到日的只在真的要对账时才要。
                // 放进 title 之后，悬停就能拿到，而且不占流水的横向空间。
                rows.push(json!({
                    "k": "d",
                    "label": label,
                    "wd": wd.unwrap_or_default(),
                    "title": format!("{} {}", c.day, weekday_cn(d)),
                }));
            }
            last_day = c.day.clone();
        }
        // 一条记录可以挂多个目标。挂着的都列出来；
        // `counts` 为假表示「这条规则不接受手工记录」，标签会画得安静一点。
        let chips: Vec<serde_json::Value> = c
            .goal_ids
            .iter()
            .map(|id| match goals.get(id) {
                Some(g) => json!({
                    "title": g.title,
                    "color": g.color,
                    "counts": manual.contains(id),
                }),
                None => json!({ "title": "（已删除）", "color": "none", "counts": false }),
            })
            .collect();
        let text = if c.note.trim().is_empty() {
            "手工打卡".to_string()
        } else {
            c.note.trim().to_string()
        };
        rows.push(json!({
            "k": "e", "time": c.time, "text": text, "sub": "手工打卡",
            "goals": chips,
        }));
    }

    // 字符串里的 `<` 必须转义成 \u003c：一条备注里只要出现 `</script>`，
    // 这个 JSON 块就会被 HTML 解析器提前关掉，整页脚本全废。
    serde_json::Value::Array(rows).to_string().replace('<', "\\u003c")
}

// ------------------------------------------------------------ 目标详情

/// 一个目标的详情页。点卡片进来的。
///
/// **整个窗口只显示这一个目标**，所以不再分两栏——这一屏是「我在看这个目标」，
/// 不是「我在扫全部」。
///
/// 读的状态和编辑的表单都在这里一次渲染好，JS 只负责切换显隐。
/// 这样「目标长什么样」仍然只有一处定义。
fn detail_body(conn: &Connection, today: NaiveDate, g: &Goal) -> Result<String> {
    let s = metrics::series(conn, g.id, today)?;
    let srcs = db::sources_of(conn, g.id)?;
    let n = db::checkin_count(conn, g.id)?;

    let kind_options: String = [
        crate::model::SourceKind::ManualCheckin,
        crate::model::SourceKind::GitCommits,
        crate::model::SourceKind::ExternalMetric,
        crate::model::SourceKind::Derived,
    ]
    .iter()
    .map(|k| {
        format!(
            r#"<button type="button" class="kind" data-kind="{k}"><b>{label}</b><span>{desc}</span></button>"#,
            k = k.as_str(),
            label = k.label(),
            desc = match k {
                crate::model::SourceKind::ManualCheckin => "我自己记一次",
                crate::model::SourceKind::GitCommits => "某个仓库的提交数（还没接入）",
                crate::model::SourceKind::ExternalMetric => "读别处已经记着的数（还没接入）",
                crate::model::SourceKind::Derived => "上面几条按公式算（还没接入）",
            },
        )
    })
    .collect();

    let mut rules = String::new();
    for src in &srcs {
        rules.push_str(&format!(
            r#"
      <div class="rule">
        <div>
          <div class="rkind">{kind}{unimpl}</div>
          {target}
          {why}
        </div>
        <button type="button" class="x" data-src="{id}" title="删掉这条规则">删除规则</button>
      </div>"#,
            kind = src.kind.label(),
            unimpl = if src.kind.implemented() { "" } else { "（未接入）" },
            target = if src.target.trim().is_empty() {
                String::new()
            } else {
                format!(r#"<div class="rtarget">{}</div>"#, esc(src.target.trim()))
            },
            why = if src.rationale.trim().is_empty() {
                String::new()
            } else {
                format!(r#"<div class="rwhy">{}</div>"#, esc(src.rationale.trim()))
            },
            id = src.id,
        ));
    }
    if srcs.is_empty() {
        rules.push_str(
            r#"<div class="hint">还没有判定规则。没有它，这个目标给不出「什么算推进它」——曲线永远不会动。</div>"#,
        );
    }

    let has = s.has_data;
    let (vclass, val) = if has {
        ("dval", num(s.current))
    } else {
        ("dval zero", "0".to_string())
    };

    // 有记录就删不掉。把理由写在按钮旁边，别让人点完了才知道。
    let del_blocked = n > 0;
    let del_note = if del_blocked {
        format!(
            r#"<div class="hint">已经记了 {n} 条，所以删不掉。动过的历史不该被一次点击抹掉——
            不要了请走归档，它会保留曲线和放弃的理由。</div>"#
        )
    } else {
        r#"<div class="hint">还没有任何记录，所以可以直接删掉。删了就没了，归档则会留下痕迹。</div>"#
            .to_string()
    };

    // 布局 C：整宽头部（曲线是主角）+ 下面「规则 | 记录」两栏。
    // 曲线从卡片上的 44px 放到 116px、当前值放到 60px：窗口只放一个目标，
    // 没有理由还挤在卡片那个尺寸里。
    Ok(format!(
        r#"
<div class="detail {color}" data-goal="{id}">
  <header class="hero">
    <div class="hero-top">
      <div>
        <a class="back" href="/">← 返回主页</a>
        <div id="dview">
          <div class="dtitle"><i></i>{title}</div>
          {why}
        </div>
        <div id="deditform" hidden>
          <div class="field"><label>名字</label><input id="etitle" maxlength="40" value="{title_attr}"></div>
          <div class="field"><label>为什么想做</label><textarea id="ewhy" rows="3">{why_text}</textarea></div>
          <div class="dacts" style="border:0;padding:0;margin-top:14px">
            <button type="button" id="esave">保存</button>
            <button type="button" id="ecancel">取消</button>
          </div>
        </div>
      </div>
      <div class="dcur"><span class="{vclass}">{val}</span><span class="unit">次</span></div>
    </div>
    <div class="dcurve">{curve}</div>
    <div class="cv-foot"><span>{first_day}</span><span>{span_days} 天 · 阶梯线不做插值</span><span>{last_day}</span></div>
  </header>

  <div class="panes">
    <div class="pane rules">
      <section class="dsec">
        <h3>判定规则<span>什么算推进它</span></h3>
        <div id="rules">{rules}</div>
        <button type="button" class="addrule" id="addrbtn">＋ 加一条规则</button>
        <div id="addrform" hidden style="margin-top:12px">
          <div id="addrkinds">{kind_options}</div>
          <div class="field" id="addrtarget" hidden>
            <label>具体是哪个？<span id="addrhint"></span></label>
            <input id="artarget" maxlength="120">
          </div>
          <div class="field"><label>什么算推进它</label><input id="arrationale" maxlength="120"
            placeholder="比如：读完一章，或做完一章题，算一次"></div>
          <div class="dacts" style="border:0;padding:0;margin-top:10px">
            <button type="button" id="arsave">加上</button>
            <button type="button" id="arcancel">取消</button>
          </div>
        </div>
      </section>

      <div class="dacts">
        <button type="button" id="dedit">修改</button>
        <button type="button" id="darch">归档</button>
        <button type="button" id="ddel" class="danger"{del_disabled}>删除</button>
      </div>
      {del_note}
      <div id="derr" class="warnline" hidden></div>

      <div id="darchform" hidden style="margin-top:16px">
        <div class="field"><label>为什么放弃它？</label>
          <textarea id="areason" rows="2" placeholder="这句话三个月后你会想再看一眼"></textarea></div>
        <div class="dacts" style="border:0;padding:0;margin-top:10px">
          <button type="button" id="asave">归档</button>
          <button type="button" id="acancel">取消</button>
        </div>
      </div>
    </div>

    <div class="pane recs">
      <section class="dsec">
        <h3>记录<span>{n} 条</span></h3>
        <div class="dlog" id="log"><div class="log-canvas" id="log-canvas"><div class="log-rows" id="log-rows"></div></div></div>
        <div id="log-empty" class="hint" hidden>这个目标还没有任何记录。回主页，在底部输入框里记一条。</div>
      </section>
    </div>
  </div>
</div>"#,
        id = g.id,
        color = esc(&g.color),
        title = esc(&g.title),
        vclass = vclass,
        val = val,
        why = if g.why.trim().is_empty() {
            r#"<p class="dwhy">还没有写「为什么想做」。</p>"#.to_string()
        } else {
            format!(r#"<p class="dwhy">{}</p>"#, esc(g.why.trim()))
        },
        title_attr = esc(&g.title),
        why_text = esc(g.why.trim()),
        curve = curve_svg(&s),
        first_day = s.days.first().cloned().unwrap_or_default(),
        last_day = s.days.last().cloned().unwrap_or_default(),
        span_days = s.days.len(),
        rules = rules,
        kind_options = kind_options,
        n = n,
        del_disabled = if del_blocked { " disabled" } else { "" },
        del_note = del_note,
    ))
}

// ------------------------------------------------------------ 主入口

/// 渲染一屏。
///
/// `goal` 为 `Some(id)` 时渲染这个目标的详情页（整个窗口只显示它），
/// 否则渲染主视图。详情页走的是**服务端路由**（`?goal=N`）——
/// 于是「只显示一个目标」不需要在 JS 里再维护一套模板，Rust 仍然是唯一的渲染处。
pub fn render(
    conn: &Connection,
    today: NaiveDate,
    chrome: Chrome,
    goal: Option<i64>,
) -> Result<String> {
    let goals = db::goal_list(conn, false)?;
    let goal_map: HashMap<i64, Goal> = goals.iter().map(|g| (g.id, g.clone())).collect();
    let manual = manual_goals(conn, &goals)?;

    // 目标不存在（链接过期、被删了）就退回主视图，不要给一页空白。
    let focused = match goal {
        Some(id) => goals.iter().find(|g| g.id == id).cloned(),
        None => None,
    };

    let (bar, win_title, composer, addgoal) = match chrome {
        // 无边框窗口的标题栏、任务栏、Alt-Tab 都跟着文档标题走。
        // 那里只该出现产品名——日期是刚从页面上删掉的东西，不该从任务栏溜回来。
        Chrome::Window => (
            title_bar_html(),
            "基线".to_string(),
            composer_html(conn, &goals)?,
            addgoal_html(),
        ),
        // 导出的文件在浏览器里是一个标签页，带日期才分得清是哪天导的。
        // 没有外壳就没有提交的去处，输入框和加号都不画。
        Chrome::File => ("", format!("基线 · {today}"), String::new(), String::new()),
    };

    let (body, log_json) = if let Some(g) = &focused {
        (
            detail_body(conn, today, g)?,
            rows_json(&db::checkins_of(conn, g.id)?, &goal_map, &manual, today),
        )
    } else {
        let mut cards = String::new();
        for g in &goals {
            let s = metrics::series(conn, g.id, today)?;
            let clickable = chrome == Chrome::Window;
            cards.push_str(&card_html(g, &s, &rule_line(conn, g)?, clickable));
        }
        // 没有判定规则的目标要显式警告 —— 它们的曲线永远不会动。
        let no_rule: Vec<String> = db::goals_without_rule(conn)?
            .into_iter()
            .map(|g| g.title)
            .collect();
        let warn = if no_rule.is_empty() {
            String::new()
        } else {
            format!(
                r#"
        <div class="warn">这些目标<b>没有判定规则</b>，曲线永远不会动：<b>{}</b><br>
          点开它们，在「判定规则」里补上。</div>"#,                esc(&no_rule.join("、"))
            )
        };
        // 左栏空了的时候不留一片白：告诉他一件事该怎么做，而且这件事就在手边。
        let nogoal = if goals.is_empty() {
            r#"<div class="nogoal">还没有目标。<br>点左下角的「新建目标」——建的时候要一并写清「什么算推进它」，不然那条曲线永远不会动。</div>"#
        } else {
            ""
        };
        (
            format!(
                r#"
  <div class="cols">
    <div class="left">
      <div class="goals">{nogoal}{cards}{warn}</div>{addgoal}
    </div>
    <div class="right">
      <div class="log" id="log"><div class="log-canvas" id="log-canvas"><div class="log-rows" id="log-rows"></div></div></div>{composer}
      <div class="dlg" id="dlg" hidden></div>
    </div>
  </div>"#,
                nogoal = nogoal,
                cards = cards,
                warn = warn,
                addgoal = addgoal,
                composer = composer,
            ),
            timeline_json(conn, &goal_map, &manual, today)?,
        )
    };

    // 详情页的标题带上目标名——任务栏和 Alt-Tab 里一眼看得出在看哪个。
    let title = match (&focused, chrome) {
        (Some(g), Chrome::Window) => format!("基线 · {}", g.title),
        (Some(g), Chrome::File) => format!("基线 · {} · {today}", g.title),
        (None, _) => win_title,
    };

    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>{title}</title>
<style>{css}</style></head><body>{bar}
<div class="wrap">{body}
</div>
<div class="modal" id="modal" hidden>
  <div class="mbox" role="dialog" aria-modal="true" aria-labelledby="m-title">
    <h3 id="m-title"></h3>
    <p id="m-text"></p>
    <div class="mwhat" id="m-what" hidden></div>
    <div class="macts">
      <button type="button" id="m-cancel">取消</button>
      <button type="button" id="m-ok" class="danger">删掉</button>
    </div>
  </div>
</div>
<script type="application/json" id="log-data">{log_json}</script>
<script>{view_js}</script></body></html>"#,
        title = title,
        css = CSS,
        bar = bar,
        body = body,
        log_json = log_json,
        view_js = VIEW_JS,
    ))
}

/// 出错时给人看的一页。
///
/// 窗口外壳拿不到数据时最怕的就是一片空白——那和「今天什么都没发生」长得一模一样，
/// 而这两件事的处置完全相反。所以这里宁可难看，也要把断在哪一步写清楚。
///
/// 顶栏照给。窗口没有系统边框，这一页要是没有顶栏，就没法拖动也没法关掉。
pub fn error_page(message: &str) -> String {
    format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>基线</title>
<style>{css}</style></head><body>{bar}
<div class="wrap">
  <div class="err">
    <h1>打不开数据</h1>
    <p>窗口起来了，但读不到数据库。原始错误：</p>
    <pre>{message}</pre>
  </div>
</div>
<script>{view_js}</script></body></html>"#,
        css = CSS,
        bar = title_bar_html(),
        view_js = VIEW_JS,
        message = esc(message),
    )
}
