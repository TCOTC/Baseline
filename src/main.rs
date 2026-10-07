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

use baseline::model::SourceKind;
use baseline::{ai, db, metrics, render};

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

    /// AI 判定（拿不准「算哪条规则」时让模型在你写的规则里挑一条）
    #[command(subcommand)]
    Ai(AiCmd),

    /// 目标
    #[command(subcommand)]
    Goal(GoalCmd),

    /// 判定规则（计分来源）。一个目标 = 一组计分来源。
    #[command(subcommand)]
    Source(SourceCmd),

    /// 记一条推进
    Checkin {
        /// 目标名或 id。**不写就记一条不关联任何目标的**——它不进任何曲线，
        /// 但确实发生过。界面上的输入框默认就是这个状态。
        goal: Option<String>,
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
        /// 归到哪条判定规则（来源 id）。
        ///
        /// 目标下只有一条手工规则时会自动归属，不用写；有两条以上就必须写——
        /// 那时候「这条记录算哪一条」只有你知道，工具替你猜出来的就是假曲线。
        #[arg(long)]
        source: Option<i64>,
    },

    /// 补齐每日快照（幂等；已写入的日期不会改写）
    Tick,

    /// 渲染本地 HTML
    ///
    /// 默认出的是「导出版」：没有窗口外壳（标题栏、底部输入框），带日期。
    /// 下面两个开关是为了**在浏览器里核对窗口里真实的排版**——
    /// 无头浏览器可以按任意宽度截屏，比在真窗口上量快得多，也不抢焦点。
    Render {
        #[arg(short, long, default_value = "data/view.html")]
        out: PathBuf,
        /// 渲染后自动用默认浏览器打开
        #[arg(long)]
        open: bool,
        /// 只渲染这一个目标的详情页（目标的 id）
        #[arg(long)]
        goal: Option<i64>,
        /// 渲染设置页（AI 判定那一屏）
        #[arg(long)]
        settings: bool,
        /// 带上窗口外壳：自绘顶栏 + 底部输入框，和窗口里看到的一致
        #[arg(long)]
        window: bool,
    },

    /// 各目标的当前状态（终端速览）
    Status,
}

#[derive(Subcommand)]
enum AiCmd {
    /// 看现在的配置（密钥只显示掩码）
    Show,
    /// 配置 AI 判定。不写的项保持原样
    Set {
        /// OpenAI 兼容的接口地址，例如 https://api.deepseek.com/v1
        #[arg(long)]
        base_url: Option<String>,
        /// 模型名，例如 deepseek-chat
        #[arg(long)]
        model: Option<String>,
        /// API 密钥。传空串就是清掉
        #[arg(long)]
        key: Option<String>,
        /// 置信度门槛（0–100）。低于它的答案一律不用
        #[arg(long)]
        threshold: Option<u8>,
        /// 关掉：关掉之后拿不准就一律让你自己选
        #[arg(long)]
        off: bool,
    },
    /// 发一个最小的请求，看配好没有
    Test,
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

        Cmd::Ai(AiCmd::Show) => {
            let cfg = ai::config(&conn)?;
            let key = cfg.masked_key();
            println!("开关      {}", if cfg.enabled { "开" } else { "关" });
            println!("接口      {}", cfg.base_url);
            println!("模型      {}", cfg.model);
            println!("密钥      {}", if key.is_empty() { "（没配）" } else { &key });
            println!("自动选的把握  {}", cfg.threshold);
            if cfg.key_broken {
                println!();
                println!("⚠ 密钥读不出来了，重新填一次：baseline ai set --key <密钥>");
            }
            if !cfg.ready() {
                println!();
                println!("现在还不能用。配上密钥：baseline ai set --key <你的密钥>");
            }
        }

        Cmd::Ai(AiCmd::Set {
            base_url,
            model,
            key,
            threshold,
            off,
        }) => {
            let cur = ai::config(&conn)?;
            ai::save(
                &conn,
                if off { false } else { true },
                base_url.as_deref().unwrap_or(&cur.base_url),
                model.as_deref().unwrap_or(&cur.model),
                threshold.unwrap_or(cur.threshold),
                key.as_deref(),
            )?;
            let now_cfg = ai::config(&conn)?;
            println!("AI 判定已{}", if now_cfg.enabled { "打开" } else { "关闭" });
            println!("接口  {}", now_cfg.base_url);
            println!("模型  {}", now_cfg.model);
            println!(
                "密钥  {}",
                if now_cfg.api_key.is_empty() {
                    "（没配）"
                } else {
                    "已设置"
                }
            );
            println!("自动选的把握  {}", now_cfg.threshold);
        }

        Cmd::Ai(AiCmd::Test) => println!("{}", ai::probe(&conn)?),

        Cmd::Goal(GoalCmd::Add { title, why, color }) => {
            // 上限取消了（2026-10-06，他本人的决定）。原来卡 3 个的理由是
            // 「产品和聊天机器人的分界线」，但那条线现在靠别的东西守：
            // 没有判定规则就建不出目标、拒绝是机械的、卡片上永远印着规则本身。
            let color = match color {
                Some(c) => c,
                None => db::next_free_color(&conn)?,
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
                let cur = metrics::value_today(&conn, g.id, today)?;
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
            println!(
                "当前累计  {}",
                render::num(metrics::value_today(&conn, g.id, today)?)
            );
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
            // 新规则可能让这个目标下原本悬着的记录变得唯一可归属（见 db::backfill_attribution）。
            let fixed = db::backfill_attribution(&conn)?;
            if fixed > 0 {
                println!("已把 {fixed} 条原本没归到规则的记录归到它名下。");
            }
            // 立刻给出当前值 —— 「建完立刻验证」
            let cur = metrics::value_today(&conn, g.id, today)?;
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
            source,
        } => {
            // 不写目标 = 记一条不关联的。不校验判定规则——没有目标就没有规则可违反。
            let g = match &goal {
                Some(name) => {
                    let g = db::resolve_goal(&conn, name)?;
                    if !db::goal_has_rule(&conn, g.id)? {
                        anyhow::bail!(
                            "「{}」还没有判定规则。先写清「什么算推进它」——\n  baseline source add \"{}\" --kind manual_checkin --rationale \"...\"",
                            g.title,
                            g.title
                        );
                    }
                    Some(g)
                }
                None => None,
            };
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
            // 归属在这一步定：人用 --source 指认，唯一能确定的自动归，剩下的挂空着。
            let picks: Vec<(i64, i64)> = match (&g, source) {
                (Some(g), Some(s)) => vec![(g.id, s)],
                (None, Some(_)) => anyhow::bail!("--source 要配一个目标一起用"),
                _ => Vec::new(),
            };
            let ids: Vec<i64> = g.iter().map(|g| g.id).collect();
            // 配了 AI 就先挂空着写下来，写完再补判——和窗口里是同一个顺序，
            // 理由也一样：一条已经发生的事不该卡在网络上。
            let defer = ai::config(&conn)?.ready();
            let links = db::resolve_links(&conn, &ids, &picks, defer)?;
            let id = db::checkin_add(&conn, &links, &day, &time, value, &note, &now_s)?;

            // 补判（CLI 里等它做完就好：命令返回时就该是最终状态）。
            //
            // **一条记录一个入口**：挂了目标但没归规则的、压根没挂目标的，都是它。
            // 后者就是你「不选目标直接记一条」时走的那条路。
            let mut trouble = None;
            if defer {
                let f = ai::classify_one(&conn, id, today)?;
                trouble = f.trouble;
            }

            // 后面这些话都要说**最终状态**，所以读库，不读上面那份 `links`——
            // 那份是补判之前算的，这时候已经过期了。（踩过：它会让
            // 「AI 刚说归好了」和「未关联目标」同时打印出来。）
            let now_links = db::links_of(&conn, id)?;
            println!("已记录 #{id} · {day} {time}");

            if now_links.is_empty() {
                // 一个目标都没挂上，也没归成——这条记录不进任何曲线。
                println!("它没有关联目标，所以不进任何曲线。");
                if !defer {
                    println!("（在设置里打开 AI 判定，这一步可以交给它。）");
                }
            } else {
                for l in &now_links {
                    let goal = db::goal_by_id(&conn, l.goal_id)?
                        .map(|x| x.title)
                        .unwrap_or_else(|| format!("#{}", l.goal_id));
                    match l.source_id {
                        Some(sid) => {
                            let what = db::sources_of(&conn, l.goal_id)?
                                .into_iter()
                                .find(|s| s.id == sid)
                                .map(|s| s.summary())
                                .unwrap_or_default();
                            println!("{goal} → {what}");
                        }
                        None => println!("{goal} → 还没归到规则，所以没进这条曲线"),
                    }
                }
                // 「当前累计」只对唯一那个目标说得清；挂了多个就不猜。
                if let [one] = now_links.as_slice() {
                    let cur = metrics::value_today(&conn, one.goal_id, today)?;
                    println!("当前累计 {}", render::num(cur));
                }
            }
            if let Some(t) = trouble {
                // 记录已经落库了，所以这不是失败，只是「这一条没能自动归」。
                eprintln!("{t}");
            }
        }

        Cmd::Tick => {
            let n = metrics::roll(&conn, today)?;
            println!("快照补齐完成，新写入 {n} 条（已存在的日期未改写）");
        }

        Cmd::Render {
            out,
            open,
            goal,
            settings,
            window,
        } => {
            // 渲染前顺手补快照，保证曲线是最新的。幂等。
            metrics::roll(&conn, today)?;
            let chrome = if window {
                render::Chrome::Window
            } else {
                render::Chrome::File
            };
            let html = render::render(
                &conn,
                today,
                chrome,
                match (settings, goal) {
                    (true, _) => render::View::Settings,
                    (false, Some(id)) => render::View::Goal(id),
                    (false, None) => render::View::Main,
                },
            )?;
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
            // 先补快照：卡片和 status 现在都读快照（数字与曲线同源），
            // 不补的话终端显示的数会比窗口旧一天。
            metrics::roll(&conn, today)?;
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
                // 「规则没接线」和「接上了但没动」在这里也必须分开说：
                // 后者是事实，前者是工具还没做完——用同一句话讲就是把后者讲成了前者。
                let note = if !s.wired {
                    format!("（{}还没接入）", s.unwired.join("、"))
                } else if !s.has_data {
                    "（还没有任何记录）".to_string()
                } else {
                    String::new()
                };
                println!(
                    "{:<18} 当前 {:<6} 本周 {:<6} {}",
                    g.title,
                    render::num(s.current),
                    d,
                    note
                );
            }
            println!("{:-<44}", "");
            println!("本周总位移 {}", render::num(total));
        }
    }

    Ok(())
}
