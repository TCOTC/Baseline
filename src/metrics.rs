//! 累积计算与每日快照。
//!
//! # 为什么曲线读快照，而不是实时算
//!
//! 设计文档 §6：曲线靠 `snapshots` 表。两个理由——
//!
//! 1. **快**：渲染不用回扫全部打卡记录。
//! 2. **历史不会被重画**：快照一旦写入就不再改写。以后改了判定规则，
//!    只影响**未来**的取值，已经落盘的那段曲线保持原样。
//!    这是诚实性要求——不然「和之前的对比」可以随时被追溯篡改。
//!
//! # 今天是个例外
//!
//! 今天还没过完，所以**今天的快照每次重算**（`snapshot_put_today`），过去的日子才冻结。
//! 不这样的话，当天第一次渲染就把今天的值钉死了：之后每记一条打卡，
//! 卡片上的数字会涨（那是实时累加的），曲线却停在第一次渲染的位置——
//! 数字和线的末端当场对不上，而且**同一天记第二次就会撞上**。
//!
//! 这条规则还顺带定义了补记的语义：**补记一条过去的打卡，不会把曲线往回改，
//! 只会在今天抬一格。** 意思是「你今天才把它记下来」，而不是「你三天前做过」。
//!
//! # 数字和曲线必须同源
//!
//! `current` 和 `delta_week` 都取自快照序列，不再单独实时累加。
//! 一旦两边各算各的，它们迟早会显示不同的数——而用户只看得到「对不上」。
//!
//! # 缺日怎么处理
//!
//! 漏跑 `tick` 会留下空缺的日子。渲染时**沿用前一个已知值**——
//! 这对累积量是正确语义（没记录 ≠ 归零），也正好就是阶梯线的形状。

use anyhow::Result;
use chrono::{Duration, NaiveDate};
use rusqlite::Connection;

use crate::db;

/// 曲线窗口天数。
pub const WINDOW: usize = 30;

#[derive(Debug, Clone)]
pub struct GoalSeries {
    /// 目标 id。当前渲染按目标循环、用不到它；对比视图会用到。
    #[allow(dead_code)]
    pub goal_id: i64,
    /// 窗口内每一天的累积值（时间正序，长度 = WINDOW）
    pub values: Vec<f64>,
    /// 窗口内每一天的日期（YYYY-MM-DD）
    pub days: Vec<String>,
    /// 当前累计（= 曲线末端，取自快照）
    pub current: f64,
    /// 近 7 天位移（同样取自快照，和曲线是同一份数据）
    pub delta_week: f64,
    /// 是否有任何记录。false 时渲染成空状态，不画线。
    pub has_data: bool,
}

/// 补齐从「首个打卡日」到「今天」之间缺失的快照。
///
/// 幂等：**过去的日子已存在就不动；今天每次重算**。返回本次新写入或改写的条数。
pub fn roll(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let mut written = 0usize;
    for goal in db::goal_list(conn, false)? {
        let Some(first_day) = db::first_checkin_day(conn, goal.id)? else {
            continue; // 没有打卡，无需快照
        };
        let first = parse_day(&first_day)?;

        // 从「已有快照的次日」或「首个打卡日」开始补。
        // 今天要重算，所以起点不能晚于今天——否则 last_snapshot_day == today 时
        // 循环直接跳过，今天永远钉在第一次渲染的值上。
        let start = match db::last_snapshot_day(conn, goal.id)? {
            Some(last) => std::cmp::min(parse_day(&last)? + Duration::days(1), today),
            None => first,
        };
        if start > today {
            continue;
        }

        let mut d = start;
        while d <= today {
            let day = d.format("%Y-%m-%d").to_string();
            let cum = db::cumulative_until(conn, goal.id, &day)?;
            // 今天重算，过去冻结。补记因此只会在今天抬一格。
            let wrote = if d == today {
                db::snapshot_put_today(conn, goal.id, &day, cum)?
            } else {
                db::snapshot_put(conn, goal.id, &day, cum)?
            };
            if wrote {
                written += 1;
            }
            d += Duration::days(1);
        }
    }
    Ok(written)
}

/// 取某个目标窗口内的曲线序列。
pub fn series(conn: &Connection, goal_id: i64, today: NaiveDate) -> Result<GoalSeries> {
    let start = today - Duration::days(WINDOW as i64 - 1);
    let snaps = db::snapshots_of(conn, goal_id, WINDOW)?;

    // day -> cumulative
    let mut map = std::collections::HashMap::new();
    for (d, c) in &snaps {
        map.insert(d.clone(), *c);
    }

    let mut values = Vec::with_capacity(WINDOW);
    let mut days = Vec::with_capacity(WINDOW);
    let mut running = 0.0f64;
    let mut d = start;
    while d <= today {
        let key = d.format("%Y-%m-%d").to_string();
        if let Some(v) = map.get(&key) {
            running = *v;
        }
        // 缺日沿用前值 —— 阶梯线的语义
        values.push(running);
        days.push(key);
        d += Duration::days(1);
    }

    // 数字和曲线同源：都取自快照序列，不再各算各的。
    let current = values.last().copied().unwrap_or(0.0);
    let week_ago = WINDOW - 1 - 7; // 今天往前 7 天在窗口里的下标
    let delta_week = current - values.get(week_ago).copied().unwrap_or(0.0);
    let has_data = db::first_checkin_day(conn, goal_id)?.is_some();

    Ok(GoalSeries {
        goal_id,
        values,
        days,
        current,
        delta_week,
        has_data,
    })
}

/// 30 格贡献条纹：某天是否比前一天增加了。
pub fn strip_of(series: &GoalSeries) -> Vec<bool> {
    let mut out = Vec::with_capacity(series.values.len());
    let mut prev = 0.0;
    for v in &series.values {
        out.push(*v > prev);
        prev = *v;
    }
    out
}

pub fn parse_day(s: &str) -> Result<NaiveDate> {
    Ok(NaiveDate::parse_from_str(s, "%Y-%m-%d")?)
}
