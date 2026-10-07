//! SQLite 持久化层：连接、建表、CRUD。
//!
//! 单文件库，默认位置见 [`default_path`]。

// 部分查询函数（如 `checkins_of`）是给界面与后续 git 适配器准备的，暂未接线。
#![allow(dead_code)]

use std::path::PathBuf;

use anyhow::{Context, Result, bail};
use rusqlite::{Connection, OptionalExtension, params};

use crate::model::{Checkin, CheckinLink, Goal, Source, SourceKind};

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

/// 测试用的内存库。
///
/// **判定规则和快照的语义只能靠它守住**：`scripts/ui-check.ps1` 覆盖的是外壳
/// （窗口按钮、拖动、独立滚动），而「这条记录算不算数」「过去的日子会不会被改写」
/// 在界面上看起来完全正常——它们坏掉的时候不会报错，只会悄悄给出另一个数。
#[cfg(test)]
pub(crate) fn open_memory() -> Result<Connection> {
    let conn = Connection::open_in_memory()?;
    conn.pragma_update(None, "foreign_keys", "ON")?;
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
        -- **没有 goal_id 列**：一条记录可以同时推进好几个目标，所以关联在
        -- checkin_goals 那张表里。一条都没关联也是合法的——记下来是第一步，
        -- 归到哪个目标是第二步。
        --
        -- **也没有 source_id 列**：归属属于「记录 × 目标」那个关系，不属于记录本身。
        -- 同一条记录推进两个目标时，两个目标下的规则是两条不同的话，一条记录只能
        -- 指一条来源，装不下。所以归属在 checkin_goals.source_id 上。
        CREATE TABLE IF NOT EXISTS checkins (
            id         INTEGER PRIMARY KEY,
            day        TEXT NOT NULL,
            time       TEXT NOT NULL,
            value      REAL NOT NULL DEFAULT 1,
            note       TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL
        );
        CREATE INDEX IF NOT EXISTS idx_checkins_day ON checkins(day);

        -- 记录 ↔ 目标。多对多：一次做的事可能同时推进好几个目标。
        --
        -- `source_id` 是**这条记录在这个目标下归到哪条判定规则**，也就是它算不算数的
        -- 唯一依据。NULL 表示「我本来想推进它」——不进这条曲线。
        -- 手工打卡的贡献就是「归属到这条来源的记录之和」，判定在 metrics::source_value 里，
        -- 不在这里——这样菜单能列全所有目标。
        CREATE TABLE IF NOT EXISTS checkin_goals (
            checkin_id INTEGER NOT NULL REFERENCES checkins(id) ON DELETE CASCADE,
            goal_id    INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
            source_id  INTEGER REFERENCES sources(id) ON DELETE SET NULL,
            PRIMARY KEY (checkin_id, goal_id)
        );
        CREATE INDEX IF NOT EXISTS idx_checkin_goals_goal ON checkin_goals(goal_id);

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
    split_checkin_goals(conn)?;
    move_attribution_to_links(conn)?;
    // **每次打开都补一遍**，不只在新库上做：给目标补上第一条手工规则之后，
    // 它下面那些原本「没归到任何规则」的记录就此变得唯一可归属。
    // 放在打开时做，CLI / 窗口 / 渲染三个入口就都覆盖到了，不用各自记着调。
    backfill_attribution(conn)?;
    Ok(())
}

/// 迁移：把 `checkins.goal_id` 拆成 `checkin_goals` 关联表（2026-10-06）。
///
/// 一条记录要能同时关联多个目标，单列装不下。判据是**那一列还在不在**，
/// 不在就是迁移过了——不用版本号（建表全是 IF NOT EXISTS，版本号区分不开）。
/// SQLite 3.35+ 支持 DROP COLUMN，所以不用重建表。
fn split_checkin_goals(conn: &Connection) -> Result<()> {
    let has_col: i64 = conn.query_row(
        r#"SELECT COUNT(*) FROM pragma_table_info('checkins') WHERE name = 'goal_id'"#,
        [],
        |r| r.get(0),
    )?;
    if has_col == 0 {
        return Ok(());
    }
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let r = conn.execute_batch(
        r#"
        BEGIN;
        INSERT OR IGNORE INTO checkin_goals(checkin_id, goal_id)
            SELECT id, goal_id FROM checkins WHERE goal_id IS NOT NULL;
        DROP INDEX IF EXISTS idx_checkins_goal_day;
        ALTER TABLE checkins DROP COLUMN goal_id;
        CREATE INDEX IF NOT EXISTS idx_checkins_day ON checkins(day);
        COMMIT;
        "#,
    );
    conn.pragma_update(None, "foreign_keys", "ON")?;
    r.context("迁移 checkins.goal_id 到 checkin_goals 失败")?;
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

/// 迁移：把归属从 `checkins.source_id` 挪到 `checkin_goals.source_id`（2026-10-07）。
///
/// 原来那一列在**记录**上，可一条记录能同时推进好几个目标，而每个目标下的规则是
/// 两条不同的话——记录级的一列装不下「在这条规则下算数、在那条规则下不算」。
/// 归属本来就属于「记录 × 目标」这个关系。那一列也从没有写入点，一直是 NULL。
///
/// 判据照旧是 `pragma_table_info`，不看 `meta.schema_version`（理由见上）。
fn move_attribution_to_links(conn: &Connection) -> Result<()> {
    let col = |table: &str| -> Result<i64> {
        Ok(conn.query_row(
            &format!("SELECT COUNT(*) FROM pragma_table_info('{table}') WHERE name='source_id'"),
            [],
            |r| r.get(0),
        )?)
    };

    if col("checkin_goals")? == 0 {
        // 加列时默认值必须是 NULL，否则 SQLite 拒绝带 REFERENCES 的 ADD COLUMN。
        conn.execute_batch(
            "ALTER TABLE checkin_goals
                ADD COLUMN source_id INTEGER REFERENCES sources(id) ON DELETE SET NULL;",
        )
        .context("给 checkin_goals 加 source_id 失败")?;
    }
    if col("checkins")? == 0 {
        return Ok(()); // 新库，或者已经迁移过
    }

    // 老库里那一列有值就先搬过来再删。它一直是 NULL，但搬一次的成本是零，
    // 而「我以为它是空的」这种事不值得赌。
    conn.pragma_update(None, "foreign_keys", "OFF")?;
    let r = conn.execute_batch(
        r#"
        BEGIN;
        UPDATE checkin_goals
           SET source_id = (SELECT c.source_id FROM checkins c WHERE c.id = checkin_goals.checkin_id)
         WHERE source_id IS NULL
           AND (SELECT c.source_id FROM checkins c WHERE c.id = checkin_goals.checkin_id) IS NOT NULL;
        ALTER TABLE checkins DROP COLUMN source_id;
        COMMIT;
        "#,
    );
    conn.pragma_update(None, "foreign_keys", "ON")?;
    r.context("迁移 checkins.source_id 到 checkin_goals 失败")?;
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

/// 挂在某个目标上的记录条数。**算关联，不算计分**——
/// 删除的判据是「有没有人声称这条线是自己的」，而不是「这条线动没动过」。
pub fn checkin_count(conn: &Connection, goal_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM checkin_goals WHERE goal_id=?1",
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
    collect_sources(&mut stmt, params![goal_id])
}

/// 全部规则，**含已归档目标的**。
///
/// 流水行要说清「这条记录算在哪条规则上」，而归档目标的记录仍然留在流水里
/// （归档只让它从列表消失，曲线和记录都留着）。按活跃目标去查会正好漏掉它们，
/// 表现是那些记录底下忽然少了一句规则名。
pub fn sources_all(conn: &Connection) -> Result<Vec<Source>> {
    let mut stmt = conn.prepare(
        "SELECT id, goal_id, kind, target, params, rationale FROM sources ORDER BY id",
    )?;
    collect_sources(&mut stmt, [])
}

fn collect_sources(
    stmt: &mut rusqlite::Statement<'_>,
    args: impl rusqlite::Params,
) -> Result<Vec<Source>> {
    let rows = stmt.query_map(args, |r| {
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

/// 记一条推进。`links` 是「这条记录推进了哪些目标」，每项是 (目标 id, 归到哪条规则)。
///
/// 规则可以是 `None`：那是「我本来想推进它」——挂上去了，但不进那条曲线。
/// 归属由 [`resolve_links`] 算好再传进来，这里不做判断。
pub fn checkin_add(
    conn: &Connection,
    links: &[(i64, Option<i64>)],
    day: &str,
    time: &str,
    value: f64,
    note: &str,
    now: &str,
) -> Result<i64> {
    conn.execute(
        "INSERT INTO checkins(day, time, value, note, created_at) VALUES (?1, ?2, ?3, ?4, ?5)",
        params![day, time, value, note, now],
    )?;
    let id = conn.last_insert_rowid();
    for (g, s) in links {
        conn.execute(
            "INSERT OR IGNORE INTO checkin_goals(checkin_id, goal_id, source_id)
             VALUES (?1, ?2, ?3)",
            params![id, g, s],
        )?;
    }
    Ok(id)
}

/// checkin_id -> 挂着的关联（按 goal_id 排序，渲染顺序才稳定）。
fn links_by_checkin(conn: &Connection) -> Result<std::collections::HashMap<i64, Vec<CheckinLink>>> {
    let mut stmt = conn.prepare(
        "SELECT checkin_id, goal_id, source_id FROM checkin_goals ORDER BY checkin_id, goal_id",
    )?;
    let rows = stmt.query_map([], |r| {
        Ok((
            r.get::<_, i64>(0)?,
            CheckinLink {
                goal_id: r.get(1)?,
                source_id: r.get(2)?,
            },
        ))
    })?;
    let mut m: std::collections::HashMap<i64, Vec<CheckinLink>> = std::collections::HashMap::new();
    for row in rows {
        let (c, l) = row?;
        m.entry(c).or_default().push(l);
    }
    Ok(m)
}

fn collect_checkins(
    conn: &Connection,
    sql: &str,
    args: &[&dyn rusqlite::ToSql],
) -> Result<Vec<Checkin>> {
    let links = links_by_checkin(conn)?;
    let mut stmt = conn.prepare(sql)?;
    let rows = stmt.query_map(args, |r| {
        Ok(Checkin {
            id: r.get(0)?,
            links: Vec::new(),
            day: r.get(1)?,
            time: r.get(2)?,
            value: r.get(3)?,
            note: r.get(4)?,
        })
    })?;
    let mut out = rows.collect::<rusqlite::Result<Vec<_>>>()?;
    for c in &mut out {
        c.links = links.get(&c.id).cloned().unwrap_or_default();
    }
    Ok(out)
}

/// 挂到某个目标上的全部记录（**不管计不计分**）。详情页的流水用它。
pub fn checkins_of(conn: &Connection, goal_id: i64) -> Result<Vec<Checkin>> {
    let args: [&dyn rusqlite::ToSql; 1] = [&goal_id];
    collect_checkins(
        conn,
        "SELECT c.id, c.day, c.time, c.value, c.note FROM checkins c
         JOIN checkin_goals cg ON cg.checkin_id = c.id
         WHERE cg.goal_id = ?1 ORDER BY c.day, c.time, c.id",
        &args,
    )
}

/// 全部打卡，倒序。
pub fn checkins_recent(conn: &Connection, limit: usize) -> Result<Vec<Checkin>> {
    let lim = limit as i64;
    let args: [&dyn rusqlite::ToSql; 1] = [&lim];
    collect_checkins(
        conn,
        "SELECT id, day, time, value, note FROM checkins
         ORDER BY day DESC, time DESC, id DESC LIMIT ?1",
        &args,
    )
}

/// 全部打卡，**正序**。
///
/// 时间线是「下新上旧」的一条流水，所以按时间正序取，最新的落在最后一行。
/// 不设 LIMIT：单用户、一天几条，量级很小；真要大到需要截断，
/// 该处理的是「怎么让用户看到更早的」，而不是在这里悄悄丢数据。
pub fn checkins_all(conn: &Connection) -> Result<Vec<Checkin>> {
    collect_checkins(
        conn,
        "SELECT id, day, time, value, note FROM checkins ORDER BY day, time, id",
        &[],
    )
}

// ------------------------------------------------- 归属：记录归到哪条规则

/// **这一条规则**在截止某天的贡献：归属到它的记录之和。
///
/// 归属写在 `checkin_goals.source_id` 上。**这是判定规则与数字之间唯一的连接点**——
/// 不看这一列，卡片底下印的那句「什么算推进它」对曲线就没有任何影响，
/// 挂在这个目标上的什么记录都会让它涨一格，那就是一条假曲线。
pub fn source_checkin_value(conn: &Connection, source_id: i64, day: &str) -> Result<f64> {
    Ok(conn.query_row(
        "SELECT COALESCE(SUM(c.value), 0) FROM checkins c
          JOIN checkin_goals cg ON cg.checkin_id = c.id
         WHERE cg.source_id=?1 AND c.day<=?2",
        params![source_id, day],
        |r| r.get(0),
    )?)
}

/// 这个目标下**能收手工记录**的规则（按 id 排序，渲染顺序才稳定）。
pub fn manual_sources_of(conn: &Connection, goal_id: i64) -> Result<Vec<Source>> {
    Ok(sources_of(conn, goal_id)?
        .into_iter()
        .filter(|s| s.kind == SourceKind::ManualCheckin)
        .collect())
}

/// 这条记录该归到哪条规则。**候选唯一才自动归属**，0 条或 ≥2 条都返回 None。
///
/// - 0 条：这个目标下没有手工规则收它。挂上去是记下「我本来想推进它」，不进曲线。
/// - ≥2 条：有歧义。**不替人猜**——猜错就是一条假曲线，而假曲线正是这个产品要防的东西。
///   窗口里由选择器问一句，CLI 上用 `--source`。
pub fn pick_source(conn: &Connection, goal_id: i64) -> Result<Option<i64>> {
    let cands = manual_sources_of(conn, goal_id)?;
    Ok(if cands.len() == 1 {
        Some(cands[0].id)
    } else {
        None
    })
}

/// 把「推进了哪些目标」解析成「每条关联归到哪条规则」。
///
/// `picks` 是人点过的「谁归谁」——每项是 (目标 id, 规则 id)。两种情形会拒绝，
/// 都是**机械的拒绝**：
///
/// - 指定的规则不属于那个目标：不能靠一个 id 就把记录挂到别人家的规则上；
/// - 某个目标下有两条以上手工规则，而人没给它指定：这时候说不清它算哪条，
///   与其替你挑一条，不如让你指认——**这条记录算不算数，只有规则能回答**。
///
/// 返回的每项是 (目标 id, 归到哪条规则)。规则为 `None` 不报错：
/// 那只是「记下了，但不算数」——挂到一条 git 规则的目标上就是这个意思。
pub fn resolve_links(
    conn: &Connection,
    goal_ids: &[i64],
    picks: &[(i64, i64)],
) -> Result<Vec<(i64, Option<i64>)>> {
    for (goal_id, source_id) in picks {
        let g_title = goal_by_id(conn, *goal_id)?
            .map(|x| x.title)
            .unwrap_or_else(|| format!("#{goal_id}"));
        if !goal_ids.contains(goal_id) {
            bail!("目标「{g_title}」不在这次记录里，不能给它指定规则");
        }
        if !manual_sources_of(conn, *goal_id)?
            .iter()
            .any(|s| s.id == *source_id)
        {
            bail!("规则 #{source_id} 不是「{g_title}」下的一条手工规则，不能拿它记账");
        }
    }

    let mut out = Vec::with_capacity(goal_ids.len());
    for g in goal_ids {
        let cands = manual_sources_of(conn, *g)?;
        let picked = match picks.iter().find(|(gi, _)| gi == g) {
            Some((_, si)) => Some(*si),
            None => match cands.len() {
                1 => Some(cands[0].id),
                0 => None,
                _ => {
                    let title = goal_by_id(conn, *g)?
                        .map(|x| x.title)
                        .unwrap_or_else(|| format!("#{g}"));
                    bail!("{}", ambiguous_msg(&title, &cands))
                }
            },
        };
        out.push((*g, picked));
    }
    Ok(out)
}

/// 歧义时的拒绝语。把候选连同 id 一起列出来，让人不用回头去查就能指认。
fn ambiguous_msg(goal_title: &str, cands: &[Source]) -> String {
    let list: Vec<String> = cands
        .iter()
        .map(|s| format!("#{} {}", s.id, s.summary()))
        .collect();
    format!(
        "「{goal_title}」下有 {} 条规则都能收这条记录：{}。它算哪一条得你来定——不替你猜。",
        cands.len(),
        list.join(" / ")
    )
}

/// 把「没归到任何规则」的关联补上归属：**候选唯一才补**。返回本次补上的条数。
///
/// 每次开库都跑（见 [`migrate`]）。所以「先把账记了、后来才给目标补上规则」
/// 这种顺序不会留下一条永远不肯算数的记录——规则一到位，它就归位了。
pub fn backfill_attribution(conn: &Connection) -> Result<usize> {
    let pending: Vec<(i64, i64)> = {
        let mut stmt = conn.prepare(
            "SELECT checkin_id, goal_id FROM checkin_goals
              WHERE source_id IS NULL ORDER BY checkin_id, goal_id",
        )?;
        let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, i64>(1)?)))?;
        rows.collect::<rusqlite::Result<Vec<_>>>()?
    };
    let mut fixed = 0usize;
    for (checkin_id, goal_id) in pending {
        if let Some(sid) = pick_source(conn, goal_id)? {
            conn.execute(
                "UPDATE checkin_goals SET source_id=?1 WHERE checkin_id=?2 AND goal_id=?3",
                params![sid, checkin_id, goal_id],
            )?;
            fixed += 1;
        }
    }
    Ok(fixed)
}

/// 这个目标下「挂上来了、但没归到任何规则」的记录数。
///
/// 详情页据此说明为什么它们不计分——**别让人点完了才知道**。
pub fn unattributed_count(conn: &Connection, goal_id: i64) -> Result<i64> {
    Ok(conn.query_row(
        "SELECT COUNT(*) FROM checkin_goals cg
          JOIN checkins c ON c.id = cg.checkin_id
         WHERE cg.goal_id=?1 AND cg.source_id IS NULL",
        params![goal_id],
        |r| r.get(0),
    )?)
}

/// 目标下最早的打卡日期。没有则 None。
///
/// **这里不判「计不计分」**，只看有没有人挂上来过。`has_data` 用它决定
/// 「画曲线还是画空状态」——加了规则判断的话，一条 `external_metric` 目标
/// 会因为「手工记录不算数」而显示「还没有任何记录」，旁边却顶着一个数字，
/// 两句话当场打架。（踩过。）
pub fn first_checkin_day(conn: &Connection, goal_id: i64) -> Result<Option<String>> {
    Ok(conn
        .query_row(
            "SELECT MIN(c.day) FROM checkins c
             JOIN checkin_goals cg ON cg.checkin_id = c.id WHERE cg.goal_id=?1",
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

// ---------------------------------------------------------------- 测试

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: &str = "2026-10-07 09:00:00";
    const DAY: &str = "2026-10-07";

    /// 改归属那次（2026-10-07）之前的 schema，原样抄在这里。
    ///
    /// **照着一个真实的老库写，不照着 migration 写**——写成 migration 的镜像，
    /// 两边就会一起错，而这类错是静默的：老库打不开或者数字变了，
    /// 只在升级后第一次打开的那一刻发生一次。
    const OLD_SCHEMA: &str = r#"
        CREATE TABLE meta (k TEXT PRIMARY KEY, v TEXT NOT NULL);
        CREATE TABLE goals (
            id INTEGER PRIMARY KEY, title TEXT NOT NULL UNIQUE, why TEXT NOT NULL DEFAULT '',
            color TEXT NOT NULL DEFAULT 'blue', status TEXT NOT NULL DEFAULT 'active',
            created_at TEXT NOT NULL, archived_at TEXT);
        CREATE TABLE sources (
            id INTEGER PRIMARY KEY, goal_id INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
            kind TEXT NOT NULL, target TEXT NOT NULL DEFAULT '',
            params TEXT NOT NULL DEFAULT '{}', rationale TEXT NOT NULL DEFAULT '',
            created_at TEXT NOT NULL);
        CREATE TABLE checkins (
            id INTEGER PRIMARY KEY,
            source_id INTEGER REFERENCES sources(id) ON DELETE SET NULL,
            day TEXT NOT NULL, time TEXT NOT NULL, value REAL NOT NULL DEFAULT 1,
            note TEXT NOT NULL DEFAULT '', created_at TEXT NOT NULL);
        CREATE TABLE checkin_goals (
            checkin_id INTEGER NOT NULL REFERENCES checkins(id) ON DELETE CASCADE,
            goal_id INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
            PRIMARY KEY (checkin_id, goal_id));
        CREATE TABLE snapshots (
            goal_id INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
            day TEXT NOT NULL, cumulative REAL NOT NULL, PRIMARY KEY (goal_id, day));
    "#;

    fn old_db_one_goal(checkin_source_id: Option<i64>) -> Result<Connection> {
        let conn = Connection::open_in_memory()?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        conn.execute_batch(OLD_SCHEMA)?;
        conn.execute(
            "INSERT INTO goals(id,title,color,status,created_at) VALUES (1,'英语','blue','active',?1)",
            params![NOW],
        )?;
        conn.execute(
            "INSERT INTO sources(id,goal_id,kind,target,params,rationale,created_at)
             VALUES (1,1,'manual_checkin','','{}','读完一章算一次',?1)",
            params![NOW],
        )?;
        conn.execute(
            "INSERT INTO checkins(id,source_id,day,time,value,note,created_at)
             VALUES (1,?1,?2,'10:00',2,'读完第 3 章',?3)",
            params![checkin_source_id, DAY, NOW],
        )?;
        conn.execute("INSERT INTO checkin_goals(checkin_id,goal_id) VALUES (1,1)", [])?;
        Ok(conn)
    }

    fn columns(conn: &Connection, table: &str) -> Vec<String> {
        let mut stmt = conn
            .prepare(&format!("SELECT name FROM pragma_table_info('{table}')"))
            .unwrap();
        stmt.query_map([], |r| r.get::<_, String>(0))
            .unwrap()
            .map(|x| x.unwrap())
            .collect()
    }

    #[test]
    fn 老库打开时归属被搬到关联上且数字不变() {
        // 老库里归属写在记录上，而那一列一直是 NULL（从来没有写入点）。
        let conn = old_db_one_goal(None).unwrap();

        // 新代码打开老库 —— 这一步就是老用户升级时真实发生的事。
        migrate(&conn).unwrap();

        assert!(
            !columns(&conn, "checkins").contains(&"source_id".to_string()),
            "记录上的归属列该没了"
        );
        assert!(columns(&conn, "checkin_goals").contains(&"source_id".to_string()));

        // 唯一一条手工规则 → 老记录被回填归位，数字和升级前一样。
        assert_eq!(source_checkin_value(&conn, 1, DAY).unwrap(), 2.0);
        assert_eq!(unattributed_count(&conn, 1).unwrap(), 0);
    }

    #[test]
    fn 老库里记录上的归属有值时会被搬过来() {
        // 这一列虽然从没有写入点，但真要有值，迁移必须搬过去而不是丢掉。
        let conn = old_db_one_goal(Some(1)).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(source_checkin_value(&conn, 1, DAY).unwrap(), 2.0);
    }

    #[test]
    fn 反复打开同一个库是安全的() {
        let conn = open_memory().unwrap();
        // 每个命令都会开一次库（窗口里每按一次就是一次），迁移不能越迁越乱。
        migrate(&conn).unwrap();
        migrate(&conn).unwrap();
        let goal = goal_add(&conn, "英语", "", "blue", NOW).unwrap();
        source_add(&conn, goal, SourceKind::ManualCheckin, "", "{}", "读完一章", NOW).unwrap();
        migrate(&conn).unwrap();
        assert_eq!(sources_of(&conn, goal).unwrap().len(), 1);
        assert!(columns(&conn, "checkin_goals").contains(&"source_id".to_string()));
    }
}
