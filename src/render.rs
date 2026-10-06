//! 静态 HTML 渲染。
//!
//! 阶段 A.1 不做本地服务：`baseline render` 直接吐一个自包含的 HTML 文件，
//! 浏览器打开即可。交互（分配、打卡）暂时留在 CLI。
//!
//! 曲线按设计规范绘制：**阶梯不平滑**、零值不画线、破零点加高亮。

// `GoalSeries::days` 等字段留给后续的坐标轴与对比视图。
#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::Result;
use chrono::{Datelike, NaiveDate, Weekday};
use rusqlite::{Connection, OptionalExtension, params};

use crate::db;
use crate::metrics::{self, GoalSeries};
use crate::model::{Checkin, Goal};

const CSS: &str = include_str!("../assets/view.css");

pub fn esc(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

fn weekday_cn(d: NaiveDate) -> &'static str {
    match d.weekday() {
        Weekday::Mon => "星期一",
        Weekday::Tue => "星期二",
        Weekday::Wed => "星期三",
        Weekday::Thu => "星期四",
        Weekday::Fri => "星期五",
        Weekday::Sat => "星期六",
        Weekday::Sun => "星期日",
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
    let (d, pts) = step_path(&s.values, W, H, PAD);
    let base_y = H - PAD;

    // 破零点：这条线第一次从 0 变成非 0 的位置。全产品唯一允许的强调。
    let mut extra = String::new();
    if let Some(i) = s.values.iter().position(|v| *v > 0.0) {
        if let Some(&(x, y)) = pts.get(i) {
            extra.push_str(&format!(
                r#"<circle class="cv-halo" cx="{x:.2}" cy="{y:.2}" r="6"/><circle class="cv-mark" cx="{x:.2}" cy="{y:.2}" r="3"/>"#
            ));
        }
    }
    let (lx, ly) = pts.last().copied().unwrap_or((W, base_y));

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
        + &format!(r#"<path class="cv-line" d="{d}"/>{extra}"#)
        + &format!(r#"<circle class="cv-dot" cx="{lx:.2}" cy="{ly:.2}" r="2.4"/></svg>"#)
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

fn card_html(g: &Goal, s: &GoalSeries, rule_line: &str) -> String {
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
    format!(
        r#"
    <div class="card {color}">
      <div class="chead"><span class="ct"><i></i>{title}</span><span class="{dclass}">{delta}</span></div>
      <div class="vrow"><span class="{vclass}">{cur}</span><span class="u">次</span></div>
      {curve}
      {strip}
      <div class="gmeta">{rule}</div>
    </div>"#,
        color = esc(&g.color),
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

fn timeline_html(conn: &Connection, goals: &HashMap<i64, Goal>, today: NaiveDate) -> Result<String> {
    let checkins = db::checkins_recent(conn, 300)?;
    if checkins.is_empty() {
        return Ok(String::new());
    }

    // 按天分组（已按 day DESC 排好）
    let mut groups: Vec<(String, Vec<Checkin>)> = Vec::new();
    for c in checkins {
        match groups.last_mut() {
            Some((d, v)) if *d == c.day => v.push(c),
            _ => groups.push((c.day.clone(), vec![c])),
        }
    }

    let mut out = String::new();
    for (day, items) in groups {
        let d = match metrics::parse_day(&day) {
            Ok(d) => d,
            Err(_) => continue,
        };
        let (label, wd) = day_label(&d, today);
        out.push_str(&format!(
            r#"
    <div class="day">
      <div class="dlabel"><span class="dl">{label}</span><span class="dd">{day}</span>{wd}</div>"#,
            label = esc(&label),
            day = day,
            wd = wd.map(|w| format!(r#"<span class="dw">{w}</span>"#)).unwrap_or_default(),
        ));
        for c in items {
            let g = goals.get(&c.goal_id);
            let (color, title) = match g {
                Some(g) => (g.color.clone(), g.title.clone()),
                None => ("none".into(), "（已删除）".into()),
            };
            let act = if c.note.trim().is_empty() {
                "手工打卡".to_string()
            } else {
                esc(c.note.trim())
            };
            out.push_str(&format!(
                r#"
      <div class="entry">
        <div class="time">{time}</div>
        <div class="rail"><span class="dot {color}"></span></div>
        <div class="body"><div class="act">{act}</div><div class="sub">手工打卡</div></div>
        <div class="tags"><span class="chip {color}">{title}</span></div>
      </div>"#,
                time = esc(&c.time),
                color = esc(&color),
                act = act,
                title = esc(&title),
            ));
        }
        out.push_str("\n    </div>");
    }
    Ok(out)
}

// ------------------------------------------------------------ meta 小工具

fn get_meta(conn: &Connection, k: &str) -> Result<Option<String>> {
    Ok(conn
        .query_row("SELECT v FROM meta WHERE k=?1", params![k], |r| r.get(0))
        .optional()?)
}

fn set_meta(conn: &Connection, k: &str, v: &str) -> Result<()> {
    conn.execute(
        "INSERT INTO meta(k, v) VALUES (?1, ?2)
         ON CONFLICT(k) DO UPDATE SET v = excluded.v",
        params![k, v],
    )?;
    Ok(())
}

// ------------------------------------------------------------ 主入口

pub fn render(conn: &Connection, today: NaiveDate, now: &str) -> Result<String> {
    let goals = db::goal_list(conn, false)?;
    let goal_map: HashMap<i64, Goal> = goals.iter().map(|g| (g.id, g.clone())).collect();

    // 距上次查看
    let prev_view = get_meta(conn, "last_viewed_at")?;
    let since = prev_view
        .as_deref()
        .and_then(|s| NaiveDate::parse_from_str(&s[..10.min(s.len())], "%Y-%m-%d").ok())
        .map(|d| (today - d).num_days())
        .filter(|n| *n > 0);
    set_meta(conn, "last_viewed_at", now)?;

    // 左栏
    let mut cards = String::new();
    let mut total_delta = 0.0f64;
    for g in &goals {
        let s = metrics::series(conn, g.id, today)?;
        total_delta += s.delta_week;
        cards.push_str(&card_html(g, &s, &rule_line(conn, g)?));
    }
    // 没有判定规则的目标要显式警告 —— 它们的曲线永远不会动。
    let no_rule: Vec<String> = db::goals_without_rule(conn)?
        .into_iter()
        .map(|g| g.title)
        .collect();

    let head_meta = match since {
        Some(n) => format!("{today} · {} · 距上次查看 {n} 天", weekday_cn(today)),
        None => format!("{today} · {}", weekday_cn(today)),
    };

    let body = if goals.is_empty() {
        r#"
    <div class="empty">
      还没有任何目标。<br><br>
      先建一个，并给它写清「什么算推进它」：<br><br>
      <code>baseline goal add "计算机基础" --why "基础知识匮乏" --color blue</code><br>
      <code>baseline source add "计算机基础" --kind manual_checkin --rationale "读完一章或做完一章题算一次"</code><br><br>
      然后打卡、写快照、看曲线：<br><br>
      <code>baseline checkin "计算机基础" --note "读完 CSAPP 第 3 章"</code><br>
      <code>baseline tick</code><br>
      <code>baseline render</code>
    </div>"#
            .to_string()
    } else {
        format!(
            r#"
  <div class="cols">
    <div class="left">{cards}{lsum}</div>
    <div class="right">{timeline}</div>
  </div>"#,
            cards = cards,
            lsum = if no_rule.is_empty() {
                String::new()
            } else {
                format!(
                    r#"
      <div class="warn">这些目标<b>没有判定规则</b>，曲线永远不会动：<b>{}</b><br>
        用 <code>baseline source add</code> 给它们写清「什么算推进它」。</div>"#,
                    esc(&no_rule.join("、"))
                )
            },
            timeline = timeline_html(conn, &goal_map, today)?,
        )
    };

    let total_delta_s = if total_delta > 0.0 {
        format!("+{}", num(total_delta))
    } else {
        num(total_delta)
    };

    Ok(format!(
        r#"<!DOCTYPE html>
<html lang="zh-CN"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width,initial-scale=1">
<title>基线 · {today}</title>
<style>{css}</style></head><body>
<div class="wrap">
  <header>
    <div class="brand"><i></i>基线</div>
    <div class="meta">{head_meta} · 本周总位移 {total_delta_s}</div>
  </header>
{body}
  <footer>
    数据来源：本机 SQLite · 快照写入后不再改写（历史不会因规则改动而被重画）<br>
    曲线为阶梯线，不做插值 · 缺日沿用前一个已知值 · 零值不画线
  </footer>
</div></body></html>"#,
        today = today,
        css = CSS,
        head_meta = esc(&head_meta),
        total_delta_s = total_delta_s,
        body = body,
    ))
}
