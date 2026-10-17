// Package model 是领域模型。
//
// 四个实体：Goal 目标 → Source 判定规则（计分来源）→ Checkin 一条推进 → Snapshot 每日快照。
// 这里不做任何 IO，也不做任何判定——判定口径在 metrics，归属在 store。
package model

import (
	"fmt"
	"strings"
)

// Palette 是目标色板的 key，对应 CSS 里的 .blue/.cyan/...
var Palette = []string{"blue", "cyan", "green", "amber", "rose", "violet", "slate"}

// SourceKind 计分来源类型。**四种，是上限。**
type SourceKind string

const (
	// KindManualCheckin 手工打卡：读完一章 / 做完一章题
	KindManualCheckin SourceKind = "manual_checkin"
	// KindGitCommits 某个 git 仓库的提交（可加「仅无 AI」）
	KindGitCommits SourceKind = "git_commits"
	// KindExternalMetric 读外部数据（如 Learn-English 的复习记录）
	KindExternalMetric SourceKind = "external_metric"
	// KindDerived 由上面几个按公式组合
	KindDerived SourceKind = "derived"
)

// Kinds 是全部来源类型，顺序即界面上的顺序。
var Kinds = []SourceKind{KindManualCheckin, KindGitCommits, KindExternalMetric, KindDerived}

func (k SourceKind) String() string { return string(k) }

// Label 是中文名，给界面和 CLI 输出用。
func (k SourceKind) Label() string {
	switch k {
	case KindManualCheckin:
		return "手工打卡"
	case KindGitCommits:
		return "git 提交"
	case KindExternalMetric:
		return "外部数据"
	case KindDerived:
		return "组合"
	}
	return string(k)
}

// Implemented 表示是否已经接线。未接线的规则允许先登记，但不产出数值。
func (k SourceKind) Implemented() bool { return k == KindManualCheckin }

// NeedsTarget 表示这条规则必须写清「算在什么上」，空着就等于「什么都算」。
func (k SourceKind) NeedsTarget() bool {
	return k == KindGitCommits || k == KindExternalMetric
}

// ParseKind 解析来源类型。
func ParseKind(s string) (SourceKind, error) {
	switch k := SourceKind(s); k {
	case KindManualCheckin, KindGitCommits, KindExternalMetric, KindDerived:
		return k, nil
	}
	return "", fmt.Errorf("未知的来源类型：%s（可用值：manual_checkin / git_commits / external_metric / derived）", s)
}

// Goal 一个目标。
type Goal struct {
	ID        int64
	Title     string
	Why       string
	Color     string
	Status    string
	CreatedAt string
}

// IsActive 目标是否还在列表里。
func (g Goal) IsActive() bool { return g.Status == "active" }

// Source 一条判定规则，也就是计分来源。
type Source struct {
	ID        int64
	GoalID    int64
	Kind      SourceKind
	Target    string
	Params    string
	Rationale string
}

// Summary 是这条规则**在界面上叫什么**。
//
// 优先 target（那是「算在什么上」），没有就退回 rationale（那句「什么算推进它」
// 才是这条规则的核心），都没有才说实话。**只有这一处**：卡片底部、流水行、
// CLI 的拒绝语都调它——三处各写一遍，迟早会变成两句不同的话。
func (s Source) Summary() string {
	if t := strings.TrimSpace(s.Target); t != "" {
		return t
	}
	if r := strings.TrimSpace(s.Rationale); r != "" {
		return r
	}
	return "未填写说明"
}

// CheckinLink 是一条记录挂到某个目标上的关联。
type CheckinLink struct {
	GoalID int64
	// SourceID 是归到**哪一条**规则。nil 就是「挂上来了，但不进这条曲线」。
	//
	// 界面直接用它说话（流水上那句「·不计入」、详情页的「没归到规则」），
	// 所以它必须来自归属本身（checkin_goals.source_id），不能由「这个目标有没有
	// 手工规则」推出来：删掉一条规则之后两者就会分叉，而分叉的表现是
	// 流水上写着「计入」、曲线里却没有它。
	SourceID *int64
	// ByAI 表示这条归属是模型挑的，不是人挑的。快照过一天就冻住，
	// 一条归错的记录事后改不回来——「谁定的」这个事实必须留下来。
	ByAI bool
}

// Counts 这条记录在这个目标下算不算数。
func (l CheckinLink) Counts() bool { return l.SourceID != nil }

// Checkin 一条推进记录。
type Checkin struct {
	ID int64
	// Links 是挂到哪些目标上，可以是空的：记下来了，但还没想好它推进什么。
	// **挂着 ≠ 计分**——要算数得归到那个目标下的一条规则上。
	Links []CheckinLink
	Day   string // YYYY-MM-DD（本地时区）
	Time  string // HH:MM
	Value float64
	Note  string
}

// Snapshot 每日快照。过去写入后不再改写；今天可以重算。
type Snapshot struct {
	GoalID     int64
	Day        string
	Cumulative float64
}
