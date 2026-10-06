//! SQLite 持久化层：连接、建表、CRUD。
//!
//! 单文件库，默认位置见 [`default_path`]。

// 部分查询函数（如 `checkins_of`）是给界面与后续 git 适配器准备的，暂未接线。
#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{Checkin, Goal, Source, SourceKind};

pub const SCHEMA_VERSION: i64 = 1;

/// 默认数据库位置。
///
/// 顺序：`BASELINE_DB` 环境变量 → `%APPDATA%\Baseline\baseline.db` → `~/.baseline/baseline.db`。
///
/// **不放在 exe 旁边。** 安装到 Program Files 后那个目录对普通用户只读，
/// 而数据库必须可写；打包分发时「程序在哪」和「数据在哪」本来就是两个问题。
/// CLI 与桌面窗口共用这一个函数，避免两边各指一个库、各讲一个故事。
pub fn default_path() -> PathBuf {
    if let Some(p) = std::env::var_os("BASELINE_DB") {
        return PathBuf::from(p);
    }
    let dir = std::env::var_os("APPDATA")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".baseline")))
        .unwrap_or_else(|| PathBuf::from("."));
    dir.join("Baseline").join("baseline.db")
}

/// 打开（必要时创建）数据库。父目录会自动创建。
pub fn open(path: &std::path::Path) -> Result<Connection> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)
            .with_context(|| format!("创建数据目录失败：{}", dir.display()))?;
    }
    let conn = Connection::open(path)
        .with_context(|| format!("打开数据库失败：{}", path.display()))?;
    conn.pragma_update(None, "journal_mode", "WAL")?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
    conn.pragma_update(None, "synchronous", "FULL")?;
    migrate(&conn)?;
    Ok(conn)
}

/// 建表。全部 IF NOT EXISTS，可重复调用。
pub fn migrate(conn: &Connection) -> Result<()> {
    conn.execute_batch(
        r#"
        CREATE TABLE IF NOT EXISTS meta (
            k TEXT PRIMARY KEY,
            v TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS goals (
            id          INTEGER PRIMARY KEY,
            title       TEXT NOT NULL UNIQUE,
            why         TEXT NOT NULL DEFAULT '',
            color       TEXT NOT NULL DEFAULT 'blue',
            status      TEXT NOT NULL DEFAULT 'active',
            created_at  TEXT NOT NULL,
            archived_at TEXT
        );

        -- 判定规则。一个目标 = 一组计分来源。
        CREATE TABLE IF NOT EXISTS sources (
            id        INTEGER PRIMARY KEY,
            goal_id   INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
            kind      TEXT NOT NULL,
            target    TEXT NOT NULL DEFAULT '',
            params    TEXT NOT NULL DEFAULT '{}',
            rationale TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_sources_goal ON sources(goal_id);

        -- 一条推进。value 默认 1，即「一次打卡 = 一个单位」。
        -- goal_id 可空：允许「记了一条，但还没想好它推进哪个目标」。
        -- 没关联的记录不进任何一条曲线，但它确实发生过。
        CREATE TABLE IF NOT EXISTS checkins (
            id         INTEGER PRIMARY KEY,
            goal_id    INTEGER REFERENCES goals(id) ON DELETE CASCADE,
            source_id  INTEGER REFERENCES sources(id) ON DELETE SET NULL,
            day        TEXT NOT NULL,
            time       TEXT NOT NULL,
            value      REAL NOT NULL DEFAULT 1,
            note       TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_checkins_goal_day ON checkins(goal_id, day);

        -- 每日快照。**过去的日子写入后不再改写；今天可以重算**（见 snapshot_put_today）。
        CREATE TABLE IF NOT EXISTS snapshots (
            goal_id    INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
            day        TEXT NOT NULL,
            cumulative REAL NOT NULL,
            PRIMARY KEY (goal_id, day)
        );
        "#,
    )?;
    conn.execute(
        "INSERT OR IGNORE INTO meta(k, v) VALUES ('schema_version', ?1)",
        params![SCHEMA_VERSION.to_string()],
    )?;
    relax_checkin_goal(conn)?;
    Ok(())
}

/// 迁移：把 `checkins.goal_id` 的 NOT NULL 去掉（2026-10-06）。
///
/// 允许「记了一条，但还没想好它推进哪个目标」。SQLite 改不了列的约束，
/// 只能重建表再搬数据。
///
/// **判断依据是 `pragma_table_info`，不是 `meta.schema_version`。**
/// 建表全部走 `IF NOT EXISTS`，老库和新库的 schema_version 会是一样的，
/// 拿它当开关等于没写；而 `notnull` 是事实，重复调用也是安全的（已经是 0 就直接返回）。
fn relax_checkin_goal(conn: &Connection) -> Result<()> {
    // `notnull` 是 SQLite 的保留字（它是 `NOT NULL` 语法的一部分），
    // 当列名用必须加引号，否则报 "near notnull: syntax error"。
    let notnull: i64 = conn.query_row(
        r#"SELECT COALESCE(MAX("notnull"), 0) FROM pragma_table_info('checkins') WHERE name = 'goal_id'"#,
        [],
        |r| r.get(0),
    )?;
    if notnull == 0 {
        return Ok(()); // 新库，或者已经迁移过
    }
    // PRAGMA 在事务里是空操作，所以必须在 BEGIN 之前设。
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let r = conn.execute_batch(
        r#"
        BEGIN;
        CREATE TABLE checkins_new (
            id         INTEGER PRIMARY KEY,
            goal_id    INTEGER REFERENCES goals(id) ON DELETE CASCADE,
            source_id  INTEGER REFERENCES sources(id) ON DELETE SET NULL,
            day        TEXT NOT NULL,
            time       TEXT NOT NULL,
            value      REAL NOT NULL DEFAULT 1,
            note       TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        INSERT INTO checkins_new(id, goal_id, source_id, day, time, value, note, created_at)
            SELECT id, goal_id, source_id, day, time, value, note, created_at FROM checkins;
        DROP TABLE checkins;
        ALTER TABLE checkins_new RENAME TO checkins;
        CREATE INDEX IF NOT EXISTS idx_checkins_goal_day ON checkins(goal_id, day);
        COMMIT;
        "#,
    );
    conn.pragma_update(None, "foreign_keys", "ON")?;
    r.context("迁移 checkins.goal_id 失败")?;
    Ok(())
}

// ---------------------------------------------------------------- Goal

pub fn goal_add(
    conn: &Connection,
    title: &str,
    why: &str,
    color: &str,
    now: &str,
) -> Result<i64> {
    let title = title.trim();
    if title.is_empty() {
        bail!("目标名不能为空");
    }
    if !crate::model::PALETTE.contains(&color) {
        bail!(
            "未知色板 `{color}`，可用：{}",
            crate::model::PALETTE.join(" / ")
        );
    }
    if let Some(existing) = goal_by_title(conn, title)? {
        bail!("目标 `{}` 已存在（id={}）", existing.title, existing.id);
    }
    conn.execute(
        "INSERT INTO goals(title, why, color, status, created_at) VALUES (?1, ?2, ?3, 'active', ?4)",
        params![title, why, color, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn goal_by_title(conn: &Connection, title: &str) -> Result<Option<Goal>> {
    let g = conn
        .query_row(
            "SELECT id, title, why, color, status, created_at FROM goals WHERE title = ?1",
            params![title],
            row_to_goal,
        )
        .optional()?;
    Ok(g)
}

pub fn goal_by_id(conn: &Connection, id: i64) -> Result<Option<Goal>> {
    let g = conn
        .query_row(
            "SELECT id, title, why, color, status, created_at FROM goals WHERE id = ?1",
            params![id],
            row_to_goal,
        )
        .optional()?;
    Ok(g)
}

/// 解析目标名或 id。CLI 里两种都允许。
pub fn resolve_goal(conn: &Connection, key: &str) -> Result<Goal> {
    if let Ok(id) = key.parse::<i64>() {
        if let Some(g) = goal_by_id(conn, id)? {
            return Ok(g);
        }
    }
    goal_by_title(conn, key)?.ok_or_else(|| anyhow::anyhow!("找不到目标：{key}"))
}

pub fn goal_list(conn: &Connection, include_archived: bool) -> Result<Vec<Goal>> {
    let sql = if include_archived {
        "SELECT id, title, why, color, status, created_at FROM goals ORDER BY id"
    } else {
        "SELECT id, title, why, color, status, created_at FROM goals WHERE status='active' ORDER BY id"
    };
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map([], row_to_goal)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 归档，不删除。设计文档 §11：必须有正常的放弃通道，且强制留一句原因。
pub fn goal_archive(conn: &Connection, id: i64, reason: &str, now: &str) -> Result<()> {
    let reason = reason.trim();
    if reason.is_empty() {
        bail!("归档必须写一句原因（--reason）——这是为了让以后能回看当初为什么放弃");
    }
    conn.execute(
        "UPDATE goals SET status='archived', archived_at=?2, why = why || ?3 WHERE id=?1",
        params![id, now, format!("\n[归档 {now}] {reason}")],
    )?;
    Ok(())
}

/// 改目标的名字和动机。
///
/// 不在这里校验「名字唯一」之外的东西：怎么描述一个目标是他自己的事。
pub fn goal_update(conn: &Connection, id: i64, title: &str, why: &str) -> Result<()> {
    let title = title.trim();
    if title.is_empty() {
        bail!("目标得有个名字");
    }
    conn.execute(
        "UPDATE goals SET title=?2, why=?3 WHERE id=?1",
        params![id, title, why.trim()],
    )?;
    Ok(())
}

/// 删目标。**只有一条记录都没有的目标才允许删。**
///
/// 有记录就说明这条线动过——动过的历史不该被一次点击抹掉。
/// 不要了应该走归档：归档保留曲线，也保留放弃的理由。
/// 删除留给「建错了」这种情况。
pub fn goal_delete(conn: &Connection, id: i64) -> Result<()> {
    let n = checkin_count(conn, id)?;
    if n > 0 {
        bail!(
            "这个目标已经有 {n} 条记录，删不掉。\
             动过的历史不该被一次点击抹掉——不要了请走归档，它会保留曲线和放弃的理由。"
        );
    }
    conn.execute("DELETE FROM goals WHERE id=?1", params![id])?;
    Ok(())
}

pub fn checkin_count(conn: &Connection, goal_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM checkins WHERE goal_id=?1",
        params![goal_id],
        |r| r.get(0),
    )?)
}

pub fn source_delete(conn: &Connection, id: i64) -> Result<()> {
    conn.execute("DELETE FROM sources WHERE id=?1", params![id])?;
    Ok(())
}

/// 活跃目标数。**不再拿它卡上限**——上限取消了（2026-10-06，他本人的决定）。
/// 留着是因为列表和详情页还要用它显示数量。
pub fn active_goal_count(conn: &Connection) -> Result<usize> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM goals WHERE status='active'",
        [],
        |r| r.get(0),
    )?;
    Ok(n as usize)
}

/// 挑一个还没被活跃目标占用的色板。
///
/// 放在这里而不是各自复制一份：CLI 和窗口都要建目标，两边的选色逻辑一旦分叉，
/// 同一个目标在命令行里和窗口里会是两个颜色。
pub fn next_free_color(conn: &Connection) -> Result<String> {
    let used: Vec<String> = goal_list(conn, false)?.into_iter().map(|g| g.color).collect();
    Ok(crate::model::PALETTE
        .iter()
        .find(|c| !used.contains(&c.to_string()))
        .unwrap_or(&crate::model::PALETTE[0])
        .to_string())
}

fn row_to_goal(r: &rusqlite::Row) -> rusqlite::Result<Goal> {
    Ok(Goal {
        id: r.get(0)?,
        title: r.get(1)?,
        why: r.get(2)?,
        color: r.get(3)?,
        status: r.get(4)?,
        created_at: r.get(5)?,
    })
}

// ---------------------------------------------------------------- Source

pub fn source_add(
    conn: &Connection,
    goal_id: i64,
    kind: SourceKind,
    target: &str,
    params_json: &str,
    rationale: &str,
    now: &str,
) -> Result<i64> {
    // 同一目标下同类型同 target 的规则不重复登记
    let dup: Option<i64> = conn
        .query_row(
            "SELECT id FROM sources WHERE goal_id=?1 AND kind=?2 AND target=?3",
            params![goal_id, kind.as_str(), target],
            |r| r.get(0),
        )
        .optional()?;
    if let Some(id) = dup {
        bail!("这条计分来源已存在（source id={id}），未重复添加");
    }
    conn.execute(
        "INSERT INTO sources(goal_id, kind, target, params, rationale, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![goal_id, kind.as_str(), target, params_json, rationale, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn sources_of(conn: &Connection, goal_id: i64) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, goal_id, kind, target, params, rationale FROM sources
         WHERE goal_id=?1 ORDER BY id",
    )?;
    let rows = stmt.query_map(params![goal_id], |r| {
        let kind: String = r.get(2)?;
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            kind,
            r.get::<_, String>(3)?,
            r.get::<_, String>(4)?,
            r.get::<_, String>(5)?,
        ))
    })?;
    let mut out = Vec::new();
    for row in rows {
        let (id, goal_id, kind, target, params, rationale) = row?;
        out.push(Source {
            id,
            goal_id,
            kind: SourceKind::parse(&kind).unwrap_or(SourceKind::ManualCheckin),
            target,
            params,
            rationale,
        });
    }
    Ok(out)
}

/// 「没有一个目标能装下任何活动」——判定规则的守卫。
///
/// 设计文档 §5：目标坏掉不是因为太宽，而是因为给不出「什么算推进它」。
/// 所以没有计分来源的目标，产品拒绝创建/启用。
pub fn goal_has_rule(conn: &Connection, goal_id: i64) -> Result<bool> {
    let n: i64 = conn.query_row(
        "SELECT COUNT(*) FROM sources WHERE goal_id=?1",
        params![goal_id],
        |r| r.get(0),
    )?;
    Ok(n > 0)
}

/// 找出所有没有判定规则的目标。`render` 会警告它们。
pub fn goals_without_rule(conn: &Connection) -> Result<Vec<Goal>> {
    let mut stmt = conn.prepare(
        "SELECT id, title, why, color, status, created_at FROM goals g
         WHERE status='active'
           AND NOT EXISTS (SELECT 1 FROM sources s WHERE s.goal_id = g.id)
         ORDER BY id",
    )?;
    let rows = stmt.query_map([], row_to_goal)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

// ---------------------------------------------------------------- Checkin

pub fn checkin_add(
    conn: &Connection,
    goal_id: Option<i64>,
    source_id: Option<i64>,
    day: &str,
    time: &str,
    value: f64,
    note: &str,
    now: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO checkins(goal_id, source_id, day, time, value, note, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![goal_id, source_id, day, time, value, note, now],
    )?;
    Ok(conn.last_insert_rowid())
}

pub fn checkins_of(conn: &Connection, goal_id: i64) -> Result<Vec<Checkin>> {
    let mut stmt = conn.prepare(
        "SELECT id, goal_id, day, time, value, note FROM checkins
         WHERE goal_id=?1 ORDER BY day, time, id",
    )?;
    let rows = stmt.query_map(params![goal_id], row_to_checkin)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 全部打卡，倒序。
pub fn checkins_recent(conn: &Connection, limit: usize) -> Result<Vec<Checkin>> {
    let mut stmt = conn.prepare(
        "SELECT id, goal_id, day, time, value, note FROM checkins
         ORDER BY day DESC, time DESC, id DESC LIMIT ?1",
    )?;
    let rows = stmt.query_map(params![limit as i64], row_to_checkin)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

/// 全部打卡，**正序**。
///
/// 时间线是「下新上旧」的一条流水，所以按时间正序取，最新的落在最后一行。
/// 不设 LIMIT：单用户、一天几条，量级很小；真要大到需要截断，
/// 该处理的是「怎么让用户看到更早的」，而不是在这里悄悄丢数据。
pub fn checkins_all(conn: &Connection) -> Result<Vec<Checkin>> {
    let mut stmt = conn.prepare(
        "SELECT id, goal_id, day, time, value, note FROM checkins
         ORDER BY day, time, id",
    )?;
    let rows = stmt.query_map([], row_to_checkin)?;
    Ok(rows.collect::<rusqlite::Result<Vec<_>>>()?)
}

fn row_to_checkin(r: &rusqlite::Row) -> rusqlite::Result<Checkin> {
    Ok(Checkin {
        id: r.get(0)?,
        goal_id: r.get(1)?,
        day: r.get(2)?,
        time: r.get(3)?,
        value: r.get(4)?,
        note: r.get(5)?,
    })
}

/// 目标当前的累计值。= 所有打卡 value 之和。
pub fn cumulative_now(conn: &Connection, goal_id: i64) -> Result<f64> {
    let v: f64 = conn.query_row(
        "SELECT COALESCE(SUM(value), 0) FROM checkins WHERE goal_id=?1",
        params![goal_id],
        |r| r.get(0),
    )?;
    Ok(v)
}

/// 截止某天的累计值。
pub fn cumulative_until(conn: &Connection, goal_id: i64, day: &str) -> Result<f64> {
    let v: f64 = conn.query_row(
        "SELECT COALESCE(SUM(value), 0) FROM checkins WHERE goal_id=?1 AND day<=?2",
        params![goal_id, day],
        |r| r.get(0),
    )?;
    Ok(v)
}

/// 目标下最早的打卡日期。没有则 None。
pub fn first_checkin_day(conn: &Connection, goal_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT MIN(day) FROM checkins WHERE goal_id=?1",
            params![goal_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}

// ---------------------------------------------------------------- Snapshot

/// 写快照。**INSERT OR IGNORE**：已存在的日期永不改写，
/// 这是「历史不会因规则改动而被重画」的落点。
pub fn snapshot_put(conn: &Connection, goal_id: i64, day: &str, cumulative: f64) -> Result<bool> {
    let n = conn.execute(
        "INSERT OR IGNORE INTO snapshots(goal_id, day, cumulative) VALUES (?1, ?2, ?3)",
        params![goal_id, day, cumulative],
    )?;
    Ok(n > 0)
}

/// 重算**今天**的快照。
///
/// 今天还没过完。用 INSERT OR IGNORE 的话，当天第一次渲染就把今天的值钉死了——
/// 之后每记一条打卡，卡片上的数字会涨（那是实时累加的），曲线却停在第一次渲染的位置，
/// 数字和线的末端当场对不上。这不是补记旧账才会遇到的边角，**同一天记第二次就会撞上**。
///
/// 所以规则是：**过去的日子冻结，今天可以重算。**
/// 于是「补记一条三天前的打卡」只会在今天抬一格，不会把已经画出来的曲线往回改。
pub fn snapshot_put_today(
    conn: &Connection,
    goal_id: i64,
    day: &str,
    cumulative: f64,
) -> Result<bool> {
    let n = conn.execute(
        "INSERT INTO snapshots(goal_id, day, cumulative) VALUES (?1, ?2, ?3)
         ON CONFLICT(goal_id, day) DO UPDATE SET cumulative = excluded.cumulative
         WHERE cumulative <> excluded.cumulative",
        params![goal_id, day, cumulative],
    )?;
    Ok(n > 0)
}

pub fn snapshots_of(conn: &Connection, goal_id: i64, days: usize) -> Result<Vec<(String, f64)>> {
    let mut stmt = conn.prepare(
        "SELECT day, cumulative FROM snapshots WHERE goal_id=?1
         ORDER BY day DESC LIMIT ?2",
    )?;
    let rows = stmt.query_map(params![goal_id, days as i64], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, f64>(1)?))
    })?;
    let mut v = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    v.reverse(); // 变回时间正序
    Ok(v)
}

pub fn last_snapshot_day(conn: &Connection, goal_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT MAX(day) FROM snapshots WHERE goal_id=?1",
            params![goal_id],
            |r| r.get::<_, Option<String>>(0),
        )
        .optional()?
        .flatten())
}
