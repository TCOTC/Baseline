//! 累积计算与每日快照。
//!
//! # 为什么曲线读快照，而不是实时算
//!
//! 设计文档 §6：曲线靠 `snapshots` 表。两个理由——
//!
//! 1. **快**：渲染不用回扫全部打卡记录。
//! 2. **历史不会被重画**：快照一旦写入就不再改写（`INSERT OR IGNORE`）。
//!    以后改了判定规则，只影响**未来**的取值，已经落盘的那段曲线保持原样。
//!    这是诚实性要求——不然「和之前的对比」可以随时被追溯篡改。
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
    /// 当前累计
    pub current: f64,
    /// 近 7 天位移
    pub delta_week: f64,
    /// 是否有任何记录。false 时渲染成空状态，不画线。
    pub has_data: bool,
}

/// 补齐从「首个打卡日」到「今天」之间缺失的快照。
///
/// 幂等：已存在的日期不动。返回本次新写入的条数。
pub fn roll(conn: &Connection, today: NaiveDate) -> Result<usize> {
    let mut written = 0usize;
    for goal in db::goal_list(conn, false)? {
        let Some(first_day) = db::first_checkin_day(conn, goal.id)? else {
            continue; // 没有打卡，无需快照
        };
        let first = parse_day(&first_day)?;

        // 从「已有快照的次日」或「首个打卡日」开始补
        let start = match db::last_snapshot_day(conn, goal.id)? {
            Some(last) => parse_day(&last)? + Duration::days(1),
            None => first,
        };
        if start > today {
            continue;
        }

        let mut d = start;
        while d <= today {
            let day = d.format("%Y-%m-%d").to_string();
            let cum = db::cumulative_until(conn, goal.id, &day)?;
            if db::snapshot_put(conn, goal.id, &day, cum)? {
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

    let current = db::cumulative_now(conn, goal_id)?;
    let week_ago = (today - Duration::days(7)).format("%Y-%m-%d").to_string();
    let delta_week = current - db::cumulative_until(conn, goal_id, &week_ago)?;
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
