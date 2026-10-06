//! 领域模型。
//!
//! 四个实体对应设计文档 §6 的数据模型：
//! `Goal` 目标 → `Source` 判定规则（计分来源）→ `Checkin` 一条推进 → `Snapshot` 每日快照。

// 模型刻意保留了后续步骤才用到的字段（git / 外部数据来源接入后会读到它们）。
// 用 allow 而不是删掉，是为了让数据结构一次定型，避免每次加来源都改表。
#![allow(dead_code)]

use anyhow::{bail, Result};
use serde::{Deserialize, Serialize};

/// 目标色板 key。对应设计规范里的 7 套色板。
pub const PALETTE: [&str; 7] = ["blue", "cyan", "green", "amber", "rose", "violet", "slate"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Goal {
    pub id: i64,
    pub title: String,
    /// 为什么想做。对应设计文档里「问目的」那一步的产物。
    pub why: String,
    pub color: String,
    /// active | archived
    pub status: String,
    pub created_at: String,
}

impl Goal {
    pub fn is_active(&self) -> bool {
        self.status == "active"
    }
}

/// 计分来源类型。设计文档 §5.2 明确：**四种，是上限**。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// 手工打卡：读完一章 / 做完一章题
    ManualCheckin,
    /// 某个 git 仓库的提交（可加 `no_ai_only` 参数）
    GitCommits,
    /// 读外部数据（如 Learn-English 的复习记录）
    ExternalMetric,
    /// 由上面几个按公式组合
    Derived,
}

impl SourceKind {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::ManualCheckin => "manual_checkin",
            Self::GitCommits => "git_commits",
            Self::ExternalMetric => "external_metric",
            Self::Derived => "derived",
        }
    }

    /// 中文名，给界面和 CLI 输出用。
    pub fn label(self) -> &'static str {
        match self {
            Self::ManualCheckin => "手工打卡",
            Self::GitCommits => "git 提交",
            Self::ExternalMetric => "外部数据",
            Self::Derived => "派生",
        }
    }

    /// 是否已经实现。未实现的来源类型允许先登记规则，但不会产出数值——
    /// 这比直接拒绝更符合「先立内核，最后接数据源」的路线。
    pub fn implemented(self) -> bool {
        matches!(self, Self::ManualCheckin)
    }

    pub fn parse(s: &str) -> Result<Self> {
        Ok(match s {
            "manual_checkin" => Self::ManualCheckin,
            "git_commits" => Self::GitCommits,
            "external_metric" => Self::ExternalMetric,
            "derived" => Self::Derived,
            _ => bail!("未知的计分来源类型：{s}（可用：manual_checkin / git_commits / external_metric / derived）"),
        })
    }
}

#[derive(Debug, Clone)]
pub struct Source {
    pub id: i64,
    pub goal_id: i64,
    pub kind: SourceKind,
    pub target: String,
    /// JSON 字符串，留给各来源类型的自定义参数
    pub params: String,
    /// 这条规则为什么这样定。对话过程本身不落库，只留这句——供三个月后复核。
    pub rationale: String,
}

/// 一条推进记录。
#[derive(Debug, Clone)]
pub struct Checkin {
    pub id: i64,
    /// 挂到哪些目标上。**可以是空的**：记下来了，但还没想好它推进什么。
    ///
    /// 多对多：一次做的事可能同时推进好几个目标。
    /// **挂着 ≠ 计分**——手工记录只推动规则里含手工打卡的目标，
    /// 挂到一条 git 提交规则的目标上只是记下「我本来想推进它」。
    /// 这不是漏洞，是诚实的中间状态：他原话就是「记录当前做的事情，
    /// 判断当前做的事情是否能让我更接近达成目标」。判断是第二步。
    pub goal_ids: Vec<i64>,
    /// YYYY-MM-DD（本地时区）
    pub day: String,
    /// HH:MM
    pub time: String,
    pub value: f64,
    pub note: String,
}

/// 每日快照。
///
/// **过去的日子写入后不再改写；今天可以重算**——见 `metrics::roll` 与
/// `db::snapshot_put_today` 的说明。今天还没过完，把它钉死会让卡片上的数字
/// 和曲线末端当场对不上。
#[derive(Debug, Clone)]
pub struct Snapshot {
    pub goal_id: i64,
    pub day: String,
    pub cumulative: f64,
}
