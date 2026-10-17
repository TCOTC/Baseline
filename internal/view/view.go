// Package view 把库里的东西算成**一屏可以照着画的数据**。
//
// 这一层只出 JSON，不出 HTML。数字、位移、曲线路径、甚至「+3 / 持平 / 还没有记录」这句话，
// 都在这里定——前端一行算术都不写。两边各算一次，迟早会显示成两个数。
package view

import (
	"fmt"
	"math"
	"strings"
	"time"

	"baseline/internal/ai"
	"baseline/internal/metrics"
	"baseline/internal/model"
	"baseline/internal/store"
)

// Curve 是卡片/详情页上那条阶梯线。
//
// Path 已经是 SVG 的 d 属性，由 Go 算好——曲线怎么画是**数字的另一种说法**，
// 不该在 Vue 里再实现一遍。
type Curve struct {
	Kind   string  `json:"kind"` // line | empty | unwired
	Path   string  `json:"path"`
	BaseY  float64 `json:"baseY"`
	Range  string  `json:"range"`
	Width  float64 `json:"width"`
	Height float64 `json:"height"`
}

// Card 是左栏的一张目标卡。
type Card struct {
	ID          int64   `json:"id"`
	Title       string  `json:"title"`
	Color       string  `json:"color"`
	Current     float64 `json:"current"`
	CurrentText string  `json:"currentText"`
	DeltaText   string  `json:"deltaText"`
	// Zero 为 true 时数字画成灰色（暂无记录 / 规则未接入）。
	Zero   bool      `json:"zero"`
	Curve  Curve     `json:"curve"`
	Strip  []bool    `json:"strip"`
	Rule   RuleLine  `json:"rule"`
	Loose  int       `json:"loose"`
	Count  int       `json:"checkinCount"`
	Series []float64 `json:"series"`
	Days   []string  `json:"days"`
}

// RuleLine 是卡片底部那句「什么算推进它」。
type RuleLine struct {
	Empty bool   `json:"empty"`
	Text  string `json:"text"`
}

// Warning 是主视图顶部的一条提醒。
type Warning struct {
	Kind    string   `json:"kind"` // no-rule | unlinked
	Titles  []string `json:"titles,omitempty"`
	Count   int      `json:"count,omitempty"`
	AIRetry bool     `json:"aiRetry,omitempty"`
}

// Row 是流水里的一行：日期分隔行（k=d）或记录行（k=e）。
//
// 字段名故意保持小写：前端虚拟滚动直接按这些字段名读行数据，
// 改成驼峰要两边同时改，容易变成「长得像但不对」。
type Row struct {
	K      string    `json:"k"`
	Label  string    `json:"label,omitempty"`
	WD     string    `json:"wd,omitempty"`
	Title  string    `json:"title,omitempty"`
	ID     int64     `json:"id,omitempty"`
	Time   string    `json:"time,omitempty"`
	Text   string    `json:"text,omitempty"`
	Sub    string    `json:"sub,omitempty"`
	Goals  []RowGoal `json:"goals,omitempty"`
	AIWait bool      `json:"aiwait,omitempty"`
	AINote string    `json:"ainote,omitempty"`
}

// RowGoal 是流水行右侧的一颗目标标签。
type RowGoal struct {
	Title  string `json:"title"`
	Color  string `json:"color"`
	Counts bool   `json:"counts"`
	ByAI   bool   `json:"byAi"`
}

// Candidate 是输入框上方那颗规则气泡。
type Candidate struct {
	ID   int64  `json:"id"`
	What string `json:"what"`
}

// RulesForGoal 是一个目标下能收手工记录的规则，连会自动归属的那一条一起给界面。
//
// **这不是第二套判定。** 归属仍然由 store.ResolveLinks 定；Auto 就是
// store.PickSource 的答案，界面把它画成已经选中的那颗气泡——于是
// 「这条记录会算在哪条规则上」在按下回车之前就看得见。
type RulesForGoal struct {
	Title   string      `json:"title"`
	Color   string      `json:"color"`
	Auto    *int64      `json:"auto"`
	Sources []Candidate `json:"sources"`
}

// ComposerGoal 是输入框下面那颗可多选的目标标签。
type ComposerGoal struct {
	ID     int64  `json:"id"`
	Title  string `json:"title"`
	Color  string `json:"color"`
	Counts bool   `json:"counts"` // 这个目标的规则里有没有手工打卡
}

// Composer 是底部输入框。
type Composer struct {
	Goals []ComposerGoal          `json:"goals"`
	Rules map[string]RulesForGoal `json:"rules"`
	AIOn  bool                    `json:"aiOn"`
}

// RuleVM 是详情页里的一条规则。
type RuleVM struct {
	ID          int64  `json:"id"`
	Kind        string `json:"kind"`
	KindLabel   string `json:"kindLabel"`
	Target      string `json:"target"`
	Rationale   string `json:"rationale"`
	Implemented bool   `json:"implemented"`
	Summary     string `json:"summary"`
}

// GoalPage 是目标详情页。
type GoalPage struct {
	ID           int64    `json:"id"`
	Title        string   `json:"title"`
	Color        string   `json:"color"`
	Why          string   `json:"why"`
	CurrentText  string   `json:"currentText"`
	Zero         bool     `json:"zero"`
	Curve        Curve    `json:"curve"`
	FirstDay     string   `json:"firstDay"`
	LastDay      string   `json:"lastDay"`
	SpanDays     int      `json:"spanDays"`
	Rules        []RuleVM `json:"rules"`
	ManualCount  int      `json:"manualCount"`
	Loose        int      `json:"loose"`
	AIRetry      bool     `json:"aiRetry"`
	CheckinCount int      `json:"checkinCount"`
	CanDelete    bool     `json:"canDelete"`
	Timeline     []Row    `json:"timeline"`
}

// SettingsPage 是设置页。
type SettingsPage struct {
	Enabled        bool   `json:"enabled"`
	State          string `json:"state"`
	Blurb          string `json:"blurb"`
	KeyBroken      bool   `json:"keyBroken"`
	BaseURL        string `json:"baseUrl"`
	Model          string `json:"model"`
	MaskedKey      string `json:"maskedKey"`
	KeyPlaceholder string `json:"keyPlaceholder"`
	ClearDisabled  bool   `json:"clearDisabled"`
	DefaultBaseURL string `json:"defaultBaseUrl"`
	DefaultModel   string `json:"defaultModel"`
	Threshold      int    `json:"threshold"`
	Toggle         string `json:"toggle"`
}

// Page 是一屏的全部数据。
type Page struct {
	View     string        `json:"view"` // main | goal | settings
	Today    string        `json:"today"`
	Title    string        `json:"title"` // 文档标题（任务栏/Alt-Tab）
	Goals    []Card        `json:"goals"`
	Warnings []Warning     `json:"warnings"`
	Composer *Composer     `json:"composer,omitempty"`
	Timeline []Row         `json:"timeline"`
	Goal     *GoalPage     `json:"goal,omitempty"`
	Settings *SettingsPage `json:"settings,omitempty"`
	Counters Counters      `json:"counters"`
}

// Counters 是给 /__state 和日志用的一眼判断。
type Counters struct {
	Goals    int  `json:"goals"`
	Rows     int  `json:"rows"`
	Unlinked int  `json:"unlinked"`
	AIReady  bool `json:"aiReady"`
}

// Build 算出这一屏。goalID > 0 是详情页，settings 为 true 是设置页。
func Build(s *store.Store, today time.Time, goalID int64, settings bool) (Page, error) {
	page := Page{
		Today: metrics.DayKey(today), Title: "基线",
		Goals: []Card{}, Timeline: []Row{}, Warnings: []Warning{},
	}

	cfg, err := ai.ConfigOf(s)
	if err != nil {
		return Page{}, err
	}
	page.Counters.AIReady = cfg.Ready()

	goals, err := s.GoalList(false)
	if err != nil {
		return Page{}, err
	}
	page.Counters.Goals = len(goals)

	if settings {
		page.View = "settings"
		page.Title = "基线 · 设置"
		sp, err := buildSettings(cfg)
		if err != nil {
			return Page{}, err
		}
		page.Settings = sp
		return page, nil
	}

	if goalID > 0 {
		var g *model.Goal
		for i := range goals {
			if goals[i].ID == goalID {
				g = &goals[i]
			}
		}
		// 目标不存在（链接过期、被删了）就退回主视图，不要给一页空白。
		if g != nil {
			page.View = "goal"
			page.Title = "基线 · " + g.Title
			gp, err := buildGoal(s, *g, today, cfg.Ready())
			if err != nil {
				return Page{}, err
			}
			page.Goal = gp
			page.Counters.Rows = len(gp.Timeline)
			return page, nil
		}
	}

	page.View = "main"
	all, err := s.CheckinsAll()
	if err != nil {
		return Page{}, err
	}
	sources, err := s.SourcesAll()
	if err != nil {
		return Page{}, err
	}
	sourceByID := map[int64]model.Source{}
	for _, src := range sources {
		sourceByID[src.ID] = src
	}
	goalByID := map[int64]model.Goal{}
	for _, g := range goals {
		goalByID[g.ID] = g
	}
	page.Timeline = rowsOf(all, goalByID, sourceByID, today)
	page.Counters.Rows = len(page.Timeline)

	// 卡片
	for _, g := range goals {
		card, err := buildCard(s, g, today)
		if err != nil {
			return Page{}, err
		}
		page.Goals = append(page.Goals, card)
	}

	// 没有判定规则的目标要显式警告 —— 它们的曲线永远不会动。
	noRule, err := s.GoalsWithoutRule()
	if err != nil {
		return Page{}, err
	}
	if len(noRule) > 0 {
		titles := make([]string, 0, len(noRule))
		for _, g := range noRule {
			titles = append(titles, g.Title)
		}
		page.Warnings = append(page.Warnings, Warning{Kind: "no-rule", Titles: titles})
	}

	// 没关联目标的记录：它们「记下来了，但谁也不动它」。
	// 排在卡片前面：待办要看得见才有用。
	loose, err := s.UnlinkedCheckins()
	if err != nil {
		return Page{}, err
	}
	page.Counters.Unlinked = len(loose)
	if len(loose) > 0 {
		page.Warnings = append(page.Warnings, Warning{
			Kind: "unlinked", Count: len(loose), AIRetry: cfg.Ready(),
		})
	}

	composer, err := buildComposer(s, goals, cfg.Ready())
	if err != nil {
		return Page{}, err
	}
	page.Composer = composer
	return page, nil
}

// buildCard 算一张卡。
func buildCard(s *store.Store, g model.Goal, today time.Time) (Card, error) {
	series, err := metrics.Series(s, g.ID, today)
	if err != nil {
		return Card{}, err
	}
	srcs, err := s.SourcesOf(g.ID)
	if err != nil {
		return Card{}, err
	}
	loose, err := s.UnattributedCount(g.ID)
	if err != nil {
		return Card{}, err
	}
	n, err := s.CheckinCount(g.ID)
	if err != nil {
		return Card{}, err
	}

	cur := metricNum(series.Current)
	delta := "暂无记录"
	has := series.HasData && series.Wired
	switch {
	case !series.Wired:
		delta = "规则未接入"
	case has:
		switch {
		case series.DeltaWeek > 0:
			delta = "+" + metricNum(series.DeltaWeek)
		case series.DeltaWeek < 0:
			delta = metricNum(series.DeltaWeek)
		default:
			delta = "持平"
		}
	}

	return Card{
		ID:          g.ID,
		Title:       g.Title,
		Color:       g.Color,
		Current:     series.Current,
		CurrentText: cur,
		DeltaText:   delta,
		Zero:        !has,
		Curve:       curveOf(series),
		Strip:       metrics.StripOf(series),
		Rule:        ruleLine(srcs),
		Loose:       int(loose),
		Count:       int(n),
		Series:      series.Values,
		Days:        series.Days,
	}, nil
}

// ruleLine 是目标的判定规则摘要，显示在卡片底部。
func ruleLine(srcs []model.Source) RuleLine {
	if len(srcs) == 0 {
		return RuleLine{Empty: true, Text: "缺少判定规则 · 无法计算推进"}
	}
	parts := make([]string, 0, len(srcs))
	for _, src := range srcs {
		detail := ""
		if strings.TrimSpace(src.Target) != "" || strings.TrimSpace(src.Rationale) != "" {
			detail = " · " + src.Summary()
		}
		note := ""
		if !src.Kind.Implemented() {
			note = "（未接入）"
		}
		parts = append(parts, src.Kind.Label()+detail+note)
	}
	return RuleLine{Text: strings.Join(parts, " + ")}
}

// curveOf 把序列画成阶梯路径。**不做插值**——数据是离散累积的，
// 平滑会伪造出一个从未发生过的渐进过程。
func curveOf(series metrics.GoalSeries) Curve {
	const w, h, pad = 300.0, 44.0, 5.0
	c := Curve{Kind: "line", Width: w, Height: h, BaseY: h - pad}

	// 先判「规则有没有接线」，再判「有没有记录」。一条只有 git 规则的目标，
	// 值恒为 0；说它「暂无记录」是把「未接入」讲成了「未动」。
	if !series.Wired {
		c.Kind = "unwired"
		return c
	}
	if !series.HasData {
		c.Kind = "empty"
		return c
	}

	vals := series.Values
	n := len(vals)
	if n == 0 {
		c.Kind = "empty"
		return c
	}
	mx := 1.0
	for _, v := range vals {
		if v > mx {
			mx = v
		}
	}
	ih := h - pad*2
	xf := func(i int) float64 {
		if n <= 1 {
			return 0
		}
		return float64(i) / float64(n-1) * w
	}
	yf := func(v float64) float64 { return pad + ih - v/mx*ih }

	var b strings.Builder
	fmt.Fprintf(&b, "M %.2f %.2f", xf(0), yf(vals[0]))
	for i := 1; i < n; i++ {
		fmt.Fprintf(&b, " H %.2f V %.2f", xf(i), yf(vals[i]))
	}
	c.Path = b.String()

	if len(series.Days) > 0 {
		c.Range = fmt.Sprintf("%s → %s · %d 天 · 阶梯线，不插值",
			series.Days[0], series.Days[len(series.Days)-1], len(series.Days))
	}
	return c
}

// buildGoal 算详情页。
func buildGoal(s *store.Store, g model.Goal, today time.Time, aiReady bool) (*GoalPage, error) {
	series, err := metrics.Series(s, g.ID, today)
	if err != nil {
		return nil, err
	}
	srcs, err := s.SourcesOf(g.ID)
	if err != nil {
		return nil, err
	}
	rules := make([]RuleVM, 0, len(srcs))
	for _, src := range srcs {
		rules = append(rules, RuleVM{
			ID:          src.ID,
			Kind:        src.Kind.String(),
			KindLabel:   src.Kind.Label(),
			Target:      strings.TrimSpace(src.Target),
			Rationale:   strings.TrimSpace(src.Rationale),
			Implemented: src.Kind.Implemented(),
			Summary:     src.Summary(),
		})
	}

	manualN := 0
	for _, src := range srcs {
		if src.Kind == model.KindManualCheckin {
			manualN++
		}
	}
	loose, err := s.UnattributedCount(g.ID)
	if err != nil {
		return nil, err
	}
	n, err := s.CheckinCount(g.ID)
	if err != nil {
		return nil, err
	}

	checkins, err := s.CheckinsOf(g.ID)
	if err != nil {
		return nil, err
	}
	allGoals, err := s.GoalList(true)
	if err != nil {
		return nil, err
	}
	goalByID := map[int64]model.Goal{}
	for _, x := range allGoals {
		goalByID[x.ID] = x
	}
	sources, err := s.SourcesAll()
	if err != nil {
		return nil, err
	}
	sourceByID := map[int64]model.Source{}
	for _, src := range sources {
		sourceByID[src.ID] = src
	}

	has := series.HasData && series.Wired
	cur := "0"
	if has {
		cur = metricNum(series.Current)
	}
	first, last := "", ""
	if len(series.Days) > 0 {
		first, last = series.Days[0], series.Days[len(series.Days)-1]
	}

	return &GoalPage{
		ID:           g.ID,
		Title:        g.Title,
		Color:        g.Color,
		Why:          strings.TrimSpace(g.Why),
		CurrentText:  cur,
		Zero:         !has,
		Curve:        curveOf(series),
		FirstDay:     first,
		LastDay:      last,
		SpanDays:     len(series.Days),
		Rules:        rules,
		ManualCount:  manualN,
		Loose:        int(loose),
		AIRetry:      aiReady && manualN > 0 && loose > 0,
		CheckinCount: int(n),
		CanDelete:    n == 0,
		Timeline:     rowsOf(checkins, goalByID, sourceByID, today),
	}, nil
}

func buildSettings(cfg ai.Config) (*SettingsPage, error) {
	masked := cfg.MaskedKey()
	keyPlaceholder := "未填写"
	if masked != "" {
		keyPlaceholder = "当前密钥：" + masked
	}
	state, blurb := "已停用", "记录仍会保存；归属需要手动选择。"
	toggle := "启用 AI 判定"
	if cfg.Enabled {
		state = "已启用"
		blurb = "当一条记录同时符合多条规则时，由 AI 在已登记规则中选择归属；置信度低于阈值时保留为待归属。"
		toggle = "停用 AI 判定"
	}
	return &SettingsPage{
		Enabled:        cfg.Enabled,
		State:          state,
		Blurb:          blurb,
		KeyBroken:      cfg.KeyBroken,
		BaseURL:        cfg.BaseURL,
		Model:          cfg.Model,
		MaskedKey:      masked,
		KeyPlaceholder: keyPlaceholder,
		ClearDisabled:  masked == "",
		DefaultBaseURL: ai.DefaultBaseURL,
		DefaultModel:   ai.DefaultModel,
		Threshold:      cfg.Threshold,
		Toggle:         toggle,
	}, nil
}

// buildComposer 算输入框。
func buildComposer(s *store.Store, goals []model.Goal, aiOn bool) (*Composer, error) {
	if len(goals) == 0 {
		// 一个目标都没有，就不摆一个按下去没反应的输入框。
		return nil, nil
	}
	c := &Composer{Goals: []ComposerGoal{}, Rules: map[string]RulesForGoal{}, AIOn: aiOn}
	for _, g := range goals {
		srcs, err := s.ManualSourcesOf(g.ID)
		if err != nil {
			return nil, err
		}
		c.Goals = append(c.Goals, ComposerGoal{
			ID: g.ID, Title: g.Title, Color: g.Color, Counts: len(srcs) > 0,
		})
		if len(srcs) == 0 {
			continue
		}
		list := make([]Candidate, 0, len(srcs))
		for _, src := range srcs {
			list = append(list, Candidate{ID: src.ID, What: src.Summary()})
		}
		auto, err := s.PickSource(g.ID)
		if err != nil {
			return nil, err
		}
		c.Rules[fmt.Sprint(g.ID)] = RulesForGoal{
			Title: g.Title, Color: g.Color, Auto: auto, Sources: list,
		}
	}
	return c, nil
}

// rowsOf 把一串记录拼成流水行。主视图和详情页共用这一个函数，
// 所以两处的行结构不可能走样。
func rowsOf(checkins []model.Checkin, goals map[int64]model.Goal, sources map[int64]model.Source, today time.Time) []Row {
	rows := []Row{}
	lastDay := ""
	for _, c := range checkins {
		if c.Day != lastDay {
			if day, err := metrics.ParseDay(c.Day); err == nil {
				label, wd := dayLabel(day, today)
				// 具体年月日**不进可见文本**，进 title：平时需要的是「哪天」和星期几，
				// 精确到日的只在真的要对账时才要。
				rows = append(rows, Row{
					K: "d", Label: label, WD: wd,
					Title: c.Day + " " + weekdayCN(day),
				})
			}
			lastDay = c.Day
		}

		chips := make([]RowGoal, 0, len(c.Links))
		for _, l := range c.Links {
			if g, ok := goals[l.GoalID]; ok {
				chips = append(chips, RowGoal{
					Title: g.Title, Color: g.Color, Counts: l.Counts(), ByAI: l.ByAI,
				})
			} else {
				chips = append(chips, RowGoal{Title: "（目标已删除）", Color: "none"})
			}
		}

		text := "手工打卡"
		if strings.TrimSpace(c.Note) != "" {
			text = strings.TrimSpace(c.Note)
		}

		// 那句「手工打卡」下面要写出**这条记录算在哪条规则上**：
		// 卡片底部只说明「这个目标怎么算」，不说明这一条记录究竟归到了哪一句。
		var ruleNames []string
		for _, l := range c.Links {
			if l.SourceID == nil {
				continue
			}
			if src, ok := sources[*l.SourceID]; ok {
				ruleNames = append(ruleNames, src.Summary())
			}
		}
		sub := "手工打卡"
		if len(ruleNames) > 0 {
			sub = "手工打卡 · " + strings.Join(ruleNames, "、")
		}

		rows = append(rows, Row{
			K: "e", ID: c.ID, Time: c.Time, Text: text, Sub: sub, Goals: chips,
		})
	}
	return rows
}

func dayLabel(day, today time.Time) (string, string) {
	diff := int(today.Sub(day).Hours() / 24)
	switch diff {
	case 0:
		return "今天", ""
	case 1:
		return "昨天", ""
	default:
		return day.Format("01-02"), weekdayCN(day)
	}
}

// weekdayCN 是「周一」…「周日」。
//
// 用「周X」不用「星期X」：流水上方那一行是**扫读用的坐标**，不是一句话。
func weekdayCN(d time.Time) string {
	switch d.Weekday() {
	case time.Monday:
		return "周一"
	case time.Tuesday:
		return "周二"
	case time.Wednesday:
		return "周三"
	case time.Thursday:
		return "周四"
	case time.Friday:
		return "周五"
	case time.Saturday:
		return "周六"
	default:
		return "周日"
	}
}

// metricNum 去掉浮点尾巴：23.0 -> "23"，23.5 -> "23.5"。
func metricNum(v float64) string {
	if math.Abs(v-math.Round(v)) < 1e-9 {
		return fmt.Sprintf("%d", int64(math.Round(v)))
	}
	return fmt.Sprintf("%.1f", v)
}

// Num 是给 CLI 用的同一份格式化。
func Num(v float64) string { return metricNum(v) }
