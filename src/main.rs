//! 基线 / Baseline —— 和之前的你对比
//!
//! 命令行界面。内核在 `lib.rs` 导出的那几个模块里，桌面窗口（`src-tauri`）用的是同一份。
//!
//! 这里做的是内核给不了的事：把「什么算推进它」这类判断讲成人话，
//! 以及在缺规则、缺数据的时候直接拒绝，而不是默默画一条平线。

use std::path::PathBuf;

use anyhow::{Context, Result};
use clap::{Parser, Subcommand};
use chrono::{Local, NaiveDate};

use baseline::{db, metrics, render};
use baseline::model::{PALETTE, SourceKind};

#[derive(Parser)]
#[command(
    name = "baseline",
    version,
    about = "基线 —— 和之前的你对比",
    long_about = "为每个目标定义「什么算推进它」，把活动归属到目标，累加成一条可以对比过去的曲线。"
)]
struct Cli {
    /// 数据库路径。默认 %APPDATA%\Baseline\baseline.db，与桌面窗口共用同一个库。
    #[arg(long, global = true)]
    db: Option<PathBuf>,

    #[command(subcommand)]
    cmd: Cmd,
}

#[derive(Subcommand)]
enum Cmd {
    /// 初始化数据库（幂等）
    Init,

    /// 目标
    #[command(subcommand)]
    Goal(GoalCmd),

    /// 判定规则（计分来源）。一个目标 = 一组计分来源。
    #[command(subcommand)]
    Source(SourceCmd),

    /// 记一条推进
    Checkin {
        /// 目标名或 id
        goal: String,
        /// 备注，会成为时间线上的标题
        #[arg(long)]
        note: Option<String>,
        /// 计数值，默认 1
        #[arg(long, default_value_t = 1.0)]
        value: f64,
        /// 日期 YYYY-MM-DD，默认今天
        #[arg(long)]
        date: Option<String>,
        /// 时间 HH:MM，默认此刻。补记旧账时用得上。
        #[arg(long)]
        time: Option<String>,
    },

    /// 补齐每日快照（幂等；已写入的日期不会改写）
    Tick,

    /// 渲染本地 HTML
    Render {
        #[arg(short, long, default_value = "data/view.html")]
        out: PathBuf,
        /// 渲染后自动用默认浏览器打开
        #[arg(long)]
        open: bool,
    },

    /// 各目标的当前状态（终端速览）
    Status,
}

#[derive(Subcommand)]
enum GoalCmd {
    /// 新建目标
    Add {
        title: String,
        /// 为什么想做。产品里这一步是对话问出来的，CLI 里直接写。
        #[arg(long, default_value = "")]
        why: String,
        /// 色板：blue/cyan/green/amber/rose/violet/slate。不指定则自动选一个未占用的。
        #[arg(long)]
        color: Option<String>,
    },
    /// 列出目标
    List {
        #[arg(long)]
        all: bool,
    },
    /// 查看目标的详情与判定规则
    Show { goal: String },
    /// 归档（不删除）。必须写一句原因。
    Archive {
        goal: String,
        #[arg(long)]
        reason: String,
    },
}

#[derive(Subcommand)]
enum SourceCmd {
    /// 给目标加一条计分来源
    Add {
        goal: String,
        /// manual_checkin | git_commits | external_metric | derived
        #[arg(long, default_value = "manual_checkin")]
        kind: String,
        /// 仓库路径 / 外部数据标识
        #[arg(long, default_value = "")]
        target: String,
        /// 这条规则为什么这样定（供三个月后复核）
        #[arg(long, default_value = "")]
        rationale: String,
        /// JSON 参数
        #[arg(long, default_value = "{}")]
        params: String,
    },
    /// 列出目标的判定规则
    List { goal: String },
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let db_path = cli.db.clone().unwrap_or_else(db::default_path);
    let conn = db::open(&db_path)?;
    let now = Local::now();
    let today = now.date_naive();
    let now_s = now.format("%Y-%m-%d %H:%M:%S").to_string();

    match cli.cmd {
        Cmd::Init => {
            println!("数据库就绪：{}", db_path.display());
            println!("（桌面窗口读的是同一个库）");
        }

        Cmd::Goal(GoalCmd::Add { title, why, color }) => {
            // 活跃目标上限 3 —— 设计文档 §5.4
            let active = db::active_goal_count(&conn)?;
            if active >= db::MAX_ACTIVE_GOALS {
                anyhow::bail!(
                    "活跃目标已达上限 {} 个。先归档一个（baseline goal archive <目标> --reason \"...\"）\n\
                     这个限制不讨好，但它是产品与聊天机器人的分界线。",
                    db::MAX_ACTIVE_GOALS
                );
            }
            let color = match color {
                Some(c) => c,
                None => next_free_color(&conn)?,
            };
            let id = db::goal_add(&conn, &title, &why, &color, &now_s)?;
            println!("已创建目标 #{id} 「{title}」（色板 {color}）");
            println!();
            println!("下一步：给它写清「什么算推进它」。没有判定规则的目标，曲线永远不会动——");
            println!("  baseline source add \"{title}\" --kind manual_checkin   --rationale \"读完一章算一次\"");
            println!("  baseline source add \"{title}\" --kind external_metric --target \"...\" --rationale \"...\"");
            println!("  baseline source add \"{title}\" --kind git_commits     --target \"...\" --rationale \"...\"");
        }

        Cmd::Goal(GoalCmd::List { all }) => {
            let goals = db::goal_list(&conn, all)?;
            if goals.is_empty() {
                println!("（没有目标。用 baseline goal add 建一个）");
            }
            for g in goals {
                let srcs = db::sources_of(&conn, g.id)?;
                let cur = db::cumulative_now(&conn, g.id)?;
                let rule = if srcs.is_empty() {
                    "  ⚠ 没有判定规则".to_string()
                } else {
                    String::new()
                };
                println!(
                    "#{:<3} {:<18} {:<8} {:<6} 当前 {}{}",
                    g.id,
                    g.title,
                    g.color,
                    g.status,
                    render::num(cur),
                    rule
                );
            }
        }

        Cmd::Goal(GoalCmd::Show { goal }) => {
            let g = db::resolve_goal(&conn, &goal)?;
            println!("目标 #{} 「{}」", g.id, g.title);
            println!("状态    {}", g.status);
            println!("色板    {}", g.color);
            println!("创建    {}", g.created_at);
            if !g.why.trim().is_empty() {
                println!("为什么  {}", g.why);
            }
            let srcs = db::sources_of(&conn, g.id)?;
            println!();
            if srcs.is_empty() {
                println!("判定规则：（无）—— 这个目标给不出「什么算推进它」，等于没有目标。");
            } else {
                println!("判定规则：");
                for s in &srcs {
                    println!(
                        "  #{} {}{}{}",
                        s.id,
                        s.kind.label(),
                        if s.target.is_empty() {
                            String::new()
                        } else {
                            format!(" · {}", s.target)
                        },
                        if s.kind.implemented() {
                            ""
                        } else {
                            "（未接入）"
                        }
                    );
                    if !s.rationale.trim().is_empty() {
                        println!("      └ {}", s.rationale);
                    }
                }
            }
            println!();
            println!("当前累计  {}", render::num(db::cumulative_now(&conn, g.id)?));
        }

        Cmd::Goal(GoalCmd::Archive { goal, reason }) => {
            let g = db::resolve_goal(&conn, &goal)?;
            db::goal_archive(&conn, g.id, &reason, &now_s)?;
            println!("已归档目标 #{} 「{}」", g.id, g.title);
            println!("原因已记入 why 字段，以后能回看当初为什么放弃。");
        }

        Cmd::Source(SourceCmd::Add {
            goal,
            kind,
            target,
            rationale,
            params,
        }) => {
            let g = db::resolve_goal(&conn, &goal)?;
            let kind = SourceKind::parse(&kind)?;
            let id = db::source_add(&conn, g.id, kind, &target, &params, &rationale, &now_s)?;
            println!("已为「{}」添加计分来源 #{}：{}", g.title, id, kind.label());
            if !kind.implemented() {
                println!("注意：{} 尚未接入，规则先登记着，暂时不产出数值。", kind.label());
            }
            if rationale.trim().is_empty() {
                println!("提示：没写 --rationale。三个月后你会想不起当初为什么这样定。");
            }
            // 立刻给出当前值 —— 「建完立刻验证」
            let cur = db::cumulative_now(&conn, g.id)?;
            println!("当前值：{}", render::num(cur));
            if cur == 0.0 {
                println!("（还是 0。要么规则写错了，要么这条线真的还没动——两种都值得知道。）");
            }
        }

        Cmd::Source(SourceCmd::List { goal }) => {
            let g = db::resolve_goal(&conn, &goal)?;
            let srcs = db::sources_of(&conn, g.id)?;
            if srcs.is_empty() {
                println!("「{}」还没有判定规则。", g.title);
            }
            for s in srcs {
                println!(
                    "#{:<3} {:<12} target={:<20} {}",
                    s.id,
                    s.kind.as_str(),
                    if s.target.is_empty() { "-" } else { &s.target },
                    s.rationale
                );
            }
        }

        Cmd::Checkin {
            goal,
            note,
            value,
            date,
            time,
        } => {
            let g = db::resolve_goal(&conn, &goal)?;
            if !db::goal_has_rule(&conn, g.id)? {
                anyhow::bail!(
                    "「{}」还没有判定规则。先写清「什么算推进它」——\n  baseline source add \"{}\" --kind manual_checkin --rationale \"...\"",
                    g.title,
                    g.title
                );
            }
            let is_backdated = date.is_some();
            let day = match date {
                Some(d) => NaiveDate::parse_from_str(&d, "%Y-%m-%d")
                    .with_context(|| format!("日期格式应为 YYYY-MM-DD，收到 {d}"))?
                    .format("%Y-%m-%d")
                    .to_string(),
                None => today.format("%Y-%m-%d").to_string(),
            };
            // 补记旧账时默认用 00:00，免得时间线上全是「此刻」
            let time = time.unwrap_or_else(|| {
                if is_backdated {
                    "00:00".to_string()
                } else {
                    now.format("%H:%M").to_string()
                }
            });
            let note = note.unwrap_or_default();
            let id = db::checkin_add(&conn, g.id, None, &day, &time, value, &note, &now_s)?;
            let cur = db::cumulative_now(&conn, g.id)?;
            println!(
                "已记录 #{} · {} · {} {} → 当前累计 {}",
                id,
                g.title,
                day,
                time,
                render::num(cur)
            );
        }

        Cmd::Tick => {
            let n = metrics::roll(&conn, today)?;
            println!("快照补齐完成，新写入 {n} 条（已存在的日期未改写）");
        }

        Cmd::Render { out, open } => {
            // 渲染前顺手补快照，保证曲线是最新的。幂等。
            metrics::roll(&conn, today)?;
            let html = render::render(&conn, today, render::Chrome::File)?;
            if let Some(dir) = out.parent() {
                std::fs::create_dir_all(dir).ok();
            }
            std::fs::write(&out, html)?;
            let abs = std::fs::canonicalize(&out).unwrap_or_else(|_| out.clone());
            // Windows 的 canonicalize 会带上 \\?\ 前缀，显示出来很难看
            let shown = abs.to_string_lossy();
            let shown = shown.strip_prefix(r"\\?\").unwrap_or(&shown);
            println!("已生成 {shown}");
            if open {
                #[cfg(target_os = "windows")]
                {
                    let _ = std::process::Command::new("cmd")
                        .args(["/C", "start", "", shown])
                        .spawn();
                }
            } else {
                println!("（加 --open 可直接用浏览器打开）");
            }
        }

        Cmd::Status => {
            let goals = db::goal_list(&conn, false)?;
            if goals.is_empty() {
                println!("（没有目标）");
            }
            let mut total = 0.0f64;
            for g in &goals {
                let s = metrics::series(&conn, g.id, today)?;
                total += s.delta_week;
                let d = if s.delta_week > 0.0 {
                    format!("+{}", render::num(s.delta_week))
                } else {
                    render::num(s.delta_week)
                };
                println!(
                    "{:<18} 当前 {:<6} 本周 {:<6} {}",
                    g.title,
                    render::num(s.current),
                    d,
                    if s.has_data { "" } else { "（还没有任何记录）" }
                );
            }
            println!("{:-<44}", "");
            println!("本周总位移 {}", render::num(total));
        }
    }

    Ok(())
}

/// 自动挑一个未被活跃目标占用的色板。
fn next_free_color(conn: &rusqlite::Connection) -> Result<String> {
    let used: Vec<String> = db::goal_list(conn, false)?
        .into_iter()
        .map(|g| g.color)
        .collect();
    Ok(PALETTE
        .iter()
        .find(|c| !used.contains(&c.to_string()))
        .unwrap_or(&PALETTE[0])
        .to_string())
}
