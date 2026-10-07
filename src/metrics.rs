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
use crate::model::{Source, SourceKind};

/// 曲线窗口天数。
pub const WINDOW: usize = 30;

// ---------------------------------------------------------------- 判定口径
//
// **求值只有这一处。** 一个目标 = 一组计分来源，它的值就是这些来源各自贡献之和；
// 每条来源自己知道「什么算推进它」。以前这里是一个整体布尔闸门
// （「目标里有没有手工规则」→ 有就把挂上来的记录全加起来），
// 那样一来卡片底下印着的那句规则对数字没有任何影响，什么记录都算数——
// 正是这个产品要防的假曲线。所以现在逐条来源求值，没有闸门。

/// 单条计分来源在截止某天的贡献。
///
/// 没有接入的来源返回 0，但**调用方必须能分辨「0」和「没接线」**——
/// 见 [`GoalSeries::wired`]。一条只有 git 规则的目标画出一条平坦的 0，
/// 那不叫「没动」，那叫「规则还没接线」。
pub fn source_value(conn: &Connection, s: &Source, day: &str) -> Result<f64> {
    match s.kind {
        SourceKind::ManualCheckin => db::source_checkin_value(conn, s.id, day),
        // 这三种规则现在只能登记，接上数据源时在这里加分支——
        // 别在别处再写一遍「某类来源怎么算」。
        SourceKind::GitCommits | SourceKind::ExternalMetric | SourceKind::Derived => Ok(0.0),
    }
}

/// 目标在截止某天的累计值 = 它全部来源贡献之和。
pub fn value_at(conn: &Connection, goal_id: i64, day: &str) -> Result<f64> {
    let mut sum = 0.0;
    for s in db::sources_of(conn, goal_id)? {
        sum += source_value(conn, &s, day)?;
    }
    Ok(sum)
}

/// 目标当前的累计值（截止今天）。**和曲线同源**：曲线末端就是它。
///
/// 别用「把所有记录加起来」代替它——未来的日期（`checkin --date`）不会进曲线，
/// 两边一旦各算各的，卡片上的数字迟早和线的末端对不上。
pub fn value_today(conn: &Connection, goal_id: i64, today: NaiveDate) -> Result<f64> {
    value_at(conn, goal_id, &day_key(today))
}

fn day_key(d: NaiveDate) -> String {
    d.format("%Y-%m-%d").to_string()
}

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
    /// 这个目标有没有至少一条已接线的来源。
    ///
    /// false 时那条平坦的 0 **不是「没动」，是「规则还没接线」**——
    /// 两者必须能分辨，否则一条 git 目标会永远显示「还没有任何记录」，
    /// 而没有任何地方告诉你它根本没算。
    pub wired: bool,
    /// 没接线的来源类型名（去重，按登记顺序）。给界面的说明用。
    pub unwired: Vec<&'static str>,
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
            let cum = value_at(conn, goal.id, &day)?;
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

    // 「规则还没接线」和「规则接线了但没动」是两件事，必须能分辨。
    let srcs = db::sources_of(conn, goal_id)?;
    let wired = srcs.iter().any(|s| s.kind.implemented());
    let mut unwired: Vec<&'static str> = Vec::new();
    for s in &srcs {
        if !s.kind.implemented() && !unwired.contains(&s.kind.label()) {
            unwired.push(s.kind.label());
        }
    }

    Ok(GoalSeries {
        goal_id,
        values,
        days,
        current,
        delta_week,
        has_data,
        wired,
        unwired,
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

// ---------------------------------------------------------------- 测试
//
// 这里守的是**判定规则与快照的语义**。它们坏掉时界面完全正常，只是数字变成另一个——
// 没有报错、没有红框，只有一条不再对应「什么算推进它」的曲线。外壳那套检查照不到这里。

#[cfg(test)]
mod tests {
    use super::*;
    use crate::model::SourceKind;

    const TODAY: &str = "2026-10-07";
    const YESTERDAY: &str = "2026-10-06";
    const THREE_AGO: &str = "2026-10-04";

    fn d(s: &str) -> NaiveDate {
        parse_day(s).unwrap()
    }

    /// 建目标 + 一条规则，返回 (目标 id, 规则 id)。
    fn goal_with(conn: &Connection, title: &str, kind: SourceKind, why: &str) -> (i64, i64) {
        let g = db::goal_add(conn, title, "", "blue", TODAY).unwrap();
        let s = db::source_add(conn, g, kind, "", "{}", why, TODAY).unwrap();
        (g, s)
    }

    /// 给同一个目标再补一条手工规则。
    ///
    /// `target` 是规则的第二个身份——同类型同 target 不允许重复登记，
    /// 所以「两条手工规则」本身就要求它们各自有名字。这也正是
    /// 「两条规则都能收这条记录」那种情形得以出现的前提。
    fn add_manual(conn: &Connection, g: i64, name: &str) -> i64 {
        db::source_add(conn, g, SourceKind::ManualCheckin, name, "{}", name, TODAY).unwrap()
    }

    /// 记一条并补快照 —— 和 CLI、窗口走的是同一条路（归属 → 落库 → 重算）。
    fn record(conn: &Connection, goal_ids: &[i64], picks: &[(i64, i64)], day: &str, note: &str) {
        let links = db::resolve_links(conn, goal_ids, picks).unwrap();
        db::checkin_add(conn, &links, day, "10:00", 1.0, note, TODAY).unwrap();
        roll(conn, d(TODAY)).unwrap();
    }

    fn snap(conn: &Connection, g: i64, day: &str) -> Option<f64> {
        db::snapshots_of(conn, g, WINDOW)
            .unwrap()
            .into_iter()
            .find(|(x, _)| x == day)
            .map(|(_, v)| v)
    }

    #[test]
    fn 唯一的手工规则自动收下记录() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章算一次");
        record(&conn, &[g], &[], TODAY, "读完第 3 章");
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 1.0);
    }

    #[test]
    fn 两条手工规则时拒绝替人猜() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "计算机基础", SourceKind::ManualCheckin, "读完一章");
        add_manual(&conn, g, "做完一章题");

        // 两条都能收它 —— 拒绝，而且一个字都不该落库。
        let err = db::resolve_links(&conn, &[g], &[]).unwrap_err().to_string();
        assert!(err.contains("读完一章"), "拒绝语里要带上候选：{err}");
        assert!(err.contains("做完一章题"), "拒绝语里要带上候选：{err}");
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 0.0);
    }

    #[test]
    fn 记录只算它归到的那一条规则() {
        let conn = db::open_memory().unwrap();
        let (g, s1) = goal_with(&conn, "计算机基础", SourceKind::ManualCheckin, "读完一章");
        let s2 = add_manual(&conn, g, "做完一章题");

        record(&conn, &[g], &[(g, s1)], TODAY, "读完第 3 章");
        record(&conn, &[g], &[(g, s2)], TODAY, "做完第 3 章题");

        // 目标上看是 2；分开看，两条规则各 1 —— 规则真正决定了数字。
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 2.0);
        let srcs = db::sources_of(&conn, g).unwrap();
        for s in &srcs {
            let want = if s.id == s1 { 1.0 } else { 1.0 };
            assert_eq!(source_value(&conn, s, TODAY).unwrap(), want);
        }
    }

    #[test]
    fn 指定了别人家的规则会被拒绝() {
        let conn = db::open_memory().unwrap();
        let (a, sa) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章");
        let (b, _) = goal_with(&conn, "计算机基础", SourceKind::ManualCheckin, "读完一章");

        // 拿 A 的规则去给挂在 B 上的记录记账 —— 不能允许。
        assert!(db::resolve_links(&conn, &[b], &[(b, sa)]).is_err());
        // 目标本身不在这次记录里，也不能给它指定。
        assert!(db::resolve_links(&conn, &[a], &[(b, sa)]).is_err());
    }

    #[test]
    fn 没接线的规则画不出曲线但不冒充没动() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "不用 AI 也能做", SourceKind::GitCommits, "标为无 AI 的仓库的提交");

        let s = series(&conn, g, d(TODAY)).unwrap();
        assert!(!s.wired, "一条 git 规则还没接线，不能说它「算得出数」");
        assert_eq!(s.current, 0.0);
        assert_eq!(s.unwired, vec!["git 提交"]);
    }

    #[test]
    fn 挂到未接线目标上的记录留痕但不算数() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "不用 AI 也能做", SourceKind::GitCommits, "标为无 AI 的仓库的提交");
        record(&conn, &[g], &[], TODAY, "今天手写了一段时间");

        // 不算数：没有手工规则收它。
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 0.0);
        // 但记录还在，流水里看得到，详情页也会说明它为什么没进曲线。
        assert_eq!(db::checkins_of(&conn, g).unwrap().len(), 1);
        assert_eq!(db::unattributed_count(&conn, g).unwrap(), 1);
    }

    #[test]
    fn 手工与未接线并存时只有手工那部分算数() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "计算机基础", SourceKind::ManualCheckin, "读完一章");
        db::source_add(&conn, g, SourceKind::GitCommits, "some/repo", "{}", "提交数", TODAY).unwrap();

        record(&conn, &[g], &[], TODAY, "读完第 3 章");
        let s = series(&conn, g, d(TODAY)).unwrap();
        assert_eq!(s.current, 1.0, "手工那部分是接线的，就该算出来");
        assert!(s.wired, "有一条接线了，这条曲线就不是「算不出数」");
        assert_eq!(s.unwired, vec!["git 提交"], "但未接线的那条要单独讲出来");
    }

    #[test]
    fn 同一个目标下的两条规则各归各的() {
        let conn = db::open_memory().unwrap();
        let (g, s1) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "复习记录");
        let s2 = add_manual(&conn, g, "读完一章");

        // 人点明了算哪一条，就不该再被拒绝。
        record(&conn, &[g], &[(g, s2)], TODAY, "读完第 3 章");
        assert_eq!(db::source_checkin_value(&conn, s2, TODAY).unwrap(), 1.0);
        assert_eq!(db::source_checkin_value(&conn, s1, TODAY).unwrap(), 0.0);
    }

    // ------------------------------------------------------------ 快照语义

    #[test]
    fn 过去的日子冻结今天每次重算() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章算一次");

        record(&conn, &[g], &[], YESTERDAY, "昨天读完第 2 章");
        assert_eq!(snap(&conn, g, YESTERDAY), Some(1.0));
        assert_eq!(snap(&conn, g, TODAY), Some(1.0), "缺日沿用前值");

        record(&conn, &[g], &[], TODAY, "今天读完第 3 章");
        assert_eq!(snap(&conn, g, YESTERDAY), Some(1.0), "昨天的快照不能被改写");
        assert_eq!(snap(&conn, g, TODAY), Some(2.0));
    }

    #[test]
    fn 补记旧账只会在今天抬一格() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章算一次");

        record(&conn, &[g], &[], TODAY, "今天读完第 3 章");
        assert_eq!(snap(&conn, g, TODAY), Some(1.0));

        // 三天前那一格已经画在屏幕上了，补一条旧账不该把它改掉——
        // 它只说明「我今天才想起来记」。
        record(&conn, &[g], &[], THREE_AGO, "三天前读完第 1 章");
        assert_eq!(snap(&conn, g, THREE_AGO), None, "过去的那一格不该被追认");
        assert_eq!(snap(&conn, g, TODAY), Some(2.0), "只会在今天抬一格");
    }

    #[test]
    fn 曲线末端和数字同源() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章算一次");
        record(&conn, &[g], &[], TODAY, "读完第 3 章");

        let s = series(&conn, g, d(TODAY)).unwrap();
        // 卡片上的数字取自快照序列的末端，不是另算一次累加——
        // 两边各算各的，迟早在界面上显示成两个数。
        assert_eq!(s.current, *s.values.last().unwrap());
        assert_eq!(s.current, snap(&conn, g, TODAY).unwrap());
    }

    // ------------------------------------------------------------ 归属回填

    #[test]
    fn 先记账后补规则时旧记录会归位() {
        let conn = db::open_memory().unwrap();
        // 建目标时先不写规则 —— CLI 允许这样（窗口里不允许，它是「先建目标、以后补规则」的反面）。
        let g = db::goal_add(&conn, "英语", "", "blue", TODAY).unwrap();
        record(&conn, &[g], &[], TODAY, "读完第 3 章");
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 0.0, "没有规则，谁也收不下它");
        assert_eq!(db::unattributed_count(&conn, g).unwrap(), 1);

        // 规则一到位就归位：不然「规则现在有了，数还是 0」会让人以为工具坏了。
        db::source_add(&conn, g, SourceKind::ManualCheckin, "", "{}", "读完一章算一次", TODAY).unwrap();
        assert_eq!(db::backfill_attribution(&conn).unwrap(), 1);
        assert_eq!(db::unattributed_count(&conn, g).unwrap(), 0);
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 1.0);
    }

    #[test]
    fn 两条规则时不回填不猜() {
        let conn = db::open_memory().unwrap();
        let g = db::goal_add(&conn, "计算机基础", "", "blue", TODAY).unwrap();
        record(&conn, &[g], &[], TODAY, "读了点东西");

        add_manual(&conn, g, "读完一章");
        add_manual(&conn, g, "做完一章题");
        // 一条变两条之前它没归过谁，现在两条都能收它 —— 保持悬着，等指认。
        assert_eq!(db::backfill_attribution(&conn).unwrap(), 0);
        assert_eq!(db::unattributed_count(&conn, g).unwrap(), 1);
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 0.0);
    }

    /// 删掉一条规则之后，原本归在它名下的记录会变成「没归到任何规则」。
    ///
    /// 这是 `ON DELETE SET NULL` 的直接后果，也是最容易变成静默丢数的一条路径：
    /// 界面上规则少了一条，数字跟着掉，而没有任何地方说为什么。
    #[test]
    fn 删掉规则后只剩两条以上时记录悬着不再算数() {
        let conn = db::open_memory().unwrap();
        let (g, s1) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "复习");
        add_manual(&conn, g, "写作");
        add_manual(&conn, g, "听力");

        record(&conn, &[g], &[(g, s1)], TODAY, "复习第 3 课");
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 1.0);

        db::source_delete(&conn, s1).unwrap();
        // 剩下的两条都能收它 —— 不猜，记录悬着，详情页会说清原因。
        assert_eq!(db::backfill_attribution(&conn).unwrap(), 0);
        assert_eq!(db::unattributed_count(&conn, g).unwrap(), 1);
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 0.0);
    }

    #[test]
    fn 删掉规则后只剩唯一一条时记录被它接手() {
        let conn = db::open_memory().unwrap();
        let (g, s1) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "复习");
        let s2 = add_manual(&conn, g, "写作");

        record(&conn, &[g], &[(g, s1)], TODAY, "复习第 3 课");
        db::source_delete(&conn, s1).unwrap();

        // 只剩一条能收它了，那就是唯一确定 —— 接手，数字不该无缘无故掉。
        assert_eq!(db::backfill_attribution(&conn).unwrap(), 1);
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 1.0);
        assert_eq!(db::source_checkin_value(&conn, s2, TODAY).unwrap(), 1.0);
    }

    /// 流水行上的「计入 / 不计入」必须来自归属本身。
    ///
    /// 曾经它是从「这个目标有没有手工规则」推出来的。删掉一条规则之后两者就分叉：
    /// 目标明明还有手工规则，那条记录却已经悬空了——于是流水上写着计入、
    /// 曲线里没有它。**界面不能说一句它自己都不信的话。**
    #[test]
    fn 流水上的计入与否来自归属而不是目标有没有规则() {
        let conn = db::open_memory().unwrap();
        let (g, s1) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "复习");
        add_manual(&conn, g, "写作");
        add_manual(&conn, g, "听力");
        record(&conn, &[g], &[(g, s1)], TODAY, "复习第 3 课");

        let counts_of = |conn: &Connection| -> bool {
            db::checkins_all(conn).unwrap()[0].links[0].counts()
        };
        assert!(counts_of(&conn), "归到了规则 #1，就是计入");

        // 删掉 #1：还剩两条手工规则（目标当然「有手工规则」），
        // 但这条记录已经没归到任何一条上了 —— 它此刻不该再自称计入。
        db::source_delete(&conn, s1).unwrap();
        assert!(db::backfill_attribution(&conn).unwrap() == 0);
        assert!(!counts_of(&conn), "悬着的记录不能在流水上自称计入");
        assert_eq!(value_today(&conn, g, d(TODAY)).unwrap(), 0.0);
    }

    #[test]
    fn 一条记录挂在两个目标上各归各的规则() {
        let conn = db::open_memory().unwrap();
        let (a, sa) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "复习记录");
        let (b, sb) = goal_with(&conn, "计算机基础", SourceKind::ManualCheckin, "读完一章");

        let links = db::resolve_links(&conn, &[a, b], &[(a, sa), (b, sb)]).unwrap();
        db::checkin_add(&conn, &links, TODAY, "10:00", 1.0, "读完 CSAPP 第 3 章", TODAY).unwrap();
        roll(&conn, d(TODAY)).unwrap();

        // 一次做的事同时推进两个目标，而两个目标各有自己的规则 ——
        // 这正是归属必须挂在「记录 × 目标」上、不能挂在记录上的原因。
        assert_eq!(value_today(&conn, a, d(TODAY)).unwrap(), 1.0);
        assert_eq!(value_today(&conn, b, d(TODAY)).unwrap(), 1.0);
    }

    // ------------------------------------------------------------ 放弃通道

    #[test]
    fn 归档必须写原因() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章");
        assert!(db::goal_archive(&conn, g, "   ", TODAY).is_err(), "原因不能是空白");
        db::goal_archive(&conn, g, "暂时不考了", TODAY).unwrap();
        let after = db::goal_by_id(&conn, g).unwrap().unwrap();
        assert_eq!(after.status, "archived");
        assert!(after.why.contains("暂时不考了"), "放弃的理由要留在 why 里");
    }

    #[test]
    fn 有记录的目标删不掉() {
        let conn = db::open_memory().unwrap();
        let (g, _) = goal_with(&conn, "英语", SourceKind::ManualCheckin, "读完一章");
        db::goal_delete(&conn, g).unwrap(); // 建错了，还没动过 —— 可以删

        let (g2, _) = goal_with(&conn, "计算机基础", SourceKind::ManualCheckin, "读完一章");
        record(&conn, &[g2], &[], TODAY, "读完第 3 章");
        assert!(db::goal_delete(&conn, g2).is_err(), "动过的历史不该被一次点击抹掉");
    }
}
