package metrics_test

import (
	"strings"
	"testing"
	"time"

	"baseline/internal/metrics"
	"baseline/internal/model"
	"baseline/internal/store"
)

const (
	today     = "2026-10-07"
	yesterday = "2026-10-06"
	threeAgo  = "2026-10-04"
)

func d(t *testing.T, s string) time.Time {
	t.Helper()
	v, err := metrics.ParseDay(s)
	if err != nil {
		t.Fatal(err)
	}
	return v
}

func newStore(t *testing.T) *store.Store {
	t.Helper()
	s, err := store.OpenMemory()
	if err != nil {
		t.Fatal(err)
	}
	t.Cleanup(func() { s.Close() })
	return s
}

// goalWith 建目标 + 一条规则，返回 (目标 id, 规则 id)。
func goalWith(t *testing.T, s *store.Store, title string, kind model.SourceKind, why string) (int64, int64) {
	t.Helper()
	g, err := s.GoalAdd(title, "", "blue", today)
	if err != nil {
		t.Fatal(err)
	}
	src, err := s.SourceAdd(g, kind, "", "{}", why, today)
	if err != nil {
		t.Fatal(err)
	}
	return g, src
}

// addManual 给同一个目标再补一条手工规则。
//
// target 是规则的第二个身份——同类型同 target 不允许重复登记，所以「两条手工规则」
// 本身就要求它们各自有名字。这也正是「两条规则都能收这条记录」那种情形的前提。
func addManual(t *testing.T, s *store.Store, g int64, name string) int64 {
	t.Helper()
	id, err := s.SourceAdd(g, model.KindManualCheckin, name, "{}", name, today)
	if err != nil {
		t.Fatal(err)
	}
	return id
}

// record 记一条并补快照 —— 和 CLI、服务走的是同一条路（归属 → 落库 → 重算）。
//
// defer 一律给 false：测试里要的是「说不清就拒绝」这条硬规矩，
// 「挂空着等 AI」是窗口和 CLI 的选择，由它们各自传 true。
func record(t *testing.T, s *store.Store, goalIDs []int64, picks [][2]int64, day, note string) {
	t.Helper()
	links, err := s.ResolveLinks(goalIDs, picks, false)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.CheckinAdd(links, day, "10:00", 1, note, today); err != nil {
		t.Fatal(err)
	}
	if _, err := metrics.Roll(s, d(t, today)); err != nil {
		t.Fatal(err)
	}
}

func snap(t *testing.T, s *store.Store, g int64, day string) (float64, bool) {
	t.Helper()
	pts, err := s.SnapshotsOf(g, metrics.Window)
	if err != nil {
		t.Fatal(err)
	}
	for _, p := range pts {
		if p.Day == day {
			return p.Cumulative, true
		}
	}
	return 0, false
}

func Test唯一的手工规则自动收下记录(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章算一次")
	record(t, s, []int64{g}, nil, today, "读完第 3 章")

	v, err := metrics.ValueToday(s, g, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if v != 1 {
		t.Fatalf("当前值 = %v，期望 1", v)
	}
}

func Test两条手工规则时拒绝替人猜(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")
	addManual(t, s, g, "做完一章题")

	_, err := s.ResolveLinks([]int64{g}, nil, false)
	if err == nil {
		t.Fatal("两条都能收它，应该拒绝")
	}
	if !strings.Contains(err.Error(), "读完一章") || !strings.Contains(err.Error(), "做完一章题") {
		t.Fatalf("拒绝语里要带上候选：%v", err)
	}
	v, err := metrics.ValueToday(s, g, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if v != 0 {
		t.Fatalf("拒绝时一个字都不该落库，当前值 = %v", v)
	}
}

// 说不清时还有另一种处置：**先挂空着写下来**。
func Test说不清也可以先挂空着写下来(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")
	addManual(t, s, g, "做完一章题")

	links, err := s.ResolveLinks([]int64{g}, nil, true)
	if err != nil {
		t.Fatal(err)
	}
	if len(links) != 1 || links[0].SourceID != nil {
		t.Fatalf("应该挂空着，得到 %+v", links)
	}
	if links[0].ByAI {
		t.Fatal("还没判呢，不能先说是 AI 定的")
	}
	if _, err := s.CheckinAdd(links, today, "10:00", 1, "读了点东西", today); err != nil {
		t.Fatal(err)
	}
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 0 {
		t.Fatalf("挂空着的不该进曲线，当前值 = %v", v)
	}
	if n, _ := s.UnattributedCount(g); n != 1 {
		t.Fatalf("应该留一条待办，得到 %d", n)
	}
}

func Test记录只算它归到的那一条规则(t *testing.T) {
	s := newStore(t)
	g, s1 := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")
	s2 := addManual(t, s, g, "做完一章题")

	record(t, s, []int64{g}, [][2]int64{{g, s1}}, today, "读完第 3 章")
	record(t, s, []int64{g}, [][2]int64{{g, s2}}, today, "做完第 3 章题")

	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 2 {
		t.Fatalf("目标上应该是 2，得到 %v", v)
	}
	srcs, err := s.SourcesOf(g)
	if err != nil {
		t.Fatal(err)
	}
	for _, src := range srcs {
		v, err := metrics.SourceValue(s, src, today)
		if err != nil {
			t.Fatal(err)
		}
		if v != 1 {
			t.Fatalf("规则 #%d 应该是 1，得到 %v", src.ID, v)
		}
	}
}

func Test指定了别人家的规则会被拒绝(t *testing.T) {
	s := newStore(t)
	_, sa := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章")
	b, _ := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")

	// 拿 A 的规则去给挂在 B 上的记录记账 —— 不能允许。
	if _, err := s.ResolveLinks([]int64{b}, [][2]int64{{b, sa}}, false); err == nil {
		t.Fatal("别的目标的规则不该被接受")
	}
}

func Test没接线的规则画不出曲线但不冒充没动(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "不用 AI 也能做", model.KindGitCommits, "标为无 AI 的仓库的提交")

	series, err := metrics.Series(s, g, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if series.Wired {
		t.Fatal("一条 git 规则还没接线，不能说它「算得出数」")
	}
	if series.Current != 0 {
		t.Fatalf("当前值 = %v", series.Current)
	}
	if len(series.Unwired) != 1 || series.Unwired[0] != "git 提交" {
		t.Fatalf("未接线列表 = %v", series.Unwired)
	}
}

func Test挂到未接线目标上的记录留痕但不算数(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "不用 AI 也能做", model.KindGitCommits, "标为无 AI 的仓库的提交")
	record(t, s, []int64{g}, nil, today, "今天手写了一段时间")

	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 0 {
		t.Fatalf("不算数，当前值 = %v", v)
	}
	if cs, _ := s.CheckinsOf(g); len(cs) != 1 {
		t.Fatalf("记录还在，得到 %d 条", len(cs))
	}
	if n, _ := s.UnattributedCount(g); n != 1 {
		t.Fatalf("应该留一条待办，得到 %d", n)
	}
}

func Test手工与未接线并存时只有手工那部分算数(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")
	if _, err := s.SourceAdd(g, model.KindGitCommits, "some/repo", "{}", "提交数", today); err != nil {
		t.Fatal(err)
	}
	record(t, s, []int64{g}, nil, today, "读完第 3 章")

	series, err := metrics.Series(s, g, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if series.Current != 1 {
		t.Fatalf("手工那部分是接线的，就该算出来，得到 %v", series.Current)
	}
	if !series.Wired {
		t.Fatal("有一条接线了，这条曲线就不是「算不出数」")
	}
	if len(series.Unwired) != 1 || series.Unwired[0] != "git 提交" {
		t.Fatalf("未接线的那条要单独讲出来，得到 %v", series.Unwired)
	}
}

func Test同一个目标下的两条规则各归各的(t *testing.T) {
	s := newStore(t)
	g, s1 := goalWith(t, s, "英语", model.KindManualCheckin, "复习记录")
	s2 := addManual(t, s, g, "读完一章")

	record(t, s, []int64{g}, [][2]int64{{g, s2}}, today, "读完第 3 章")
	if v, _ := s.SourceCheckinValue(s2, today); v != 1 {
		t.Fatalf("s2 = %v", v)
	}
	if v, _ := s.SourceCheckinValue(s1, today); v != 0 {
		t.Fatalf("s1 = %v", v)
	}
}

// ------------------------------------------------------------ 快照语义

func Test过去的日子冻结今天每次重算(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章算一次")

	record(t, s, []int64{g}, nil, yesterday, "昨天读完第 2 章")
	if v, _ := snap(t, s, g, yesterday); v != 1 {
		t.Fatalf("昨天 = %v", v)
	}
	if v, _ := snap(t, s, g, today); v != 1 {
		t.Fatalf("缺日沿用前值，今天 = %v", v)
	}

	record(t, s, []int64{g}, nil, today, "今天读完第 3 章")
	if v, _ := snap(t, s, g, yesterday); v != 1 {
		t.Fatalf("昨天的快照不能被改写，得到 %v", v)
	}
	if v, _ := snap(t, s, g, today); v != 2 {
		t.Fatalf("今天 = %v", v)
	}
}

func Test补记旧账只会在今天抬一格(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章算一次")

	record(t, s, []int64{g}, nil, today, "今天读完第 3 章")
	if v, _ := snap(t, s, g, today); v != 1 {
		t.Fatalf("今天 = %v", v)
	}

	record(t, s, []int64{g}, nil, threeAgo, "三天前读完第 1 章")
	if _, ok := snap(t, s, g, threeAgo); ok {
		t.Fatal("过去的那一格不该被追认")
	}
	if v, _ := snap(t, s, g, today); v != 2 {
		t.Fatalf("只会在今天抬一格，得到 %v", v)
	}
}

func Test曲线末端和数字同源(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章算一次")
	record(t, s, []int64{g}, nil, today, "读完第 3 章")

	series, err := metrics.Series(s, g, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if series.Current != series.Values[len(series.Values)-1] {
		t.Fatal("current 必须取自快照序列的末端")
	}
	if v, _ := snap(t, s, g, today); series.Current != v {
		t.Fatalf("current = %v，快照 = %v", series.Current, v)
	}
}

// ------------------------------------------------------------ 归属回填

func Test先记账后补规则时旧记录会归位(t *testing.T) {
	s := newStore(t)
	// 建目标时先不写规则 —— CLI 允许这样。
	g, err := s.GoalAdd("英语", "", "blue", today)
	if err != nil {
		t.Fatal(err)
	}
	record(t, s, []int64{g}, nil, today, "读完第 3 章")
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 0 {
		t.Fatalf("没有规则，谁也收不下它，得到 %v", v)
	}
	if n, _ := s.UnattributedCount(g); n != 1 {
		t.Fatalf("待办数 = %d", n)
	}

	if _, err := s.SourceAdd(g, model.KindManualCheckin, "", "{}", "读完一章算一次", today); err != nil {
		t.Fatal(err)
	}
	n, err := s.BackfillCount()
	if err != nil {
		t.Fatal(err)
	}
	if n != 1 {
		t.Fatalf("应该补上 1 条，得到 %d", n)
	}
	if c, _ := s.UnattributedCount(g); c != 0 {
		t.Fatalf("待办数 = %d", c)
	}
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 1 {
		t.Fatalf("当前值 = %v", v)
	}
}

func Test两条规则时不回填不猜(t *testing.T) {
	s := newStore(t)
	g, err := s.GoalAdd("计算机基础", "", "blue", today)
	if err != nil {
		t.Fatal(err)
	}
	record(t, s, []int64{g}, nil, today, "读了点东西")

	addManual(t, s, g, "读完一章")
	addManual(t, s, g, "做完一章题")
	n, err := s.BackfillCount()
	if err != nil {
		t.Fatal(err)
	}
	if n != 0 {
		t.Fatalf("两条都能收，不该补，得到 %d", n)
	}
	if c, _ := s.UnattributedCount(g); c != 1 {
		t.Fatalf("待办数 = %d", c)
	}
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 0 {
		t.Fatalf("当前值 = %v", v)
	}
}

// 删掉一条规则之后，原本归在它名下的记录会变成「没归到任何规则」。
func Test删掉规则后只剩两条以上时记录悬着不再算数(t *testing.T) {
	s := newStore(t)
	g, s1 := goalWith(t, s, "英语", model.KindManualCheckin, "复习")
	addManual(t, s, g, "写作")
	addManual(t, s, g, "听力")

	record(t, s, []int64{g}, [][2]int64{{g, s1}}, today, "复习第 3 课")
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 1 {
		t.Fatalf("当前值 = %v", v)
	}

	if err := s.SourceDelete(s1); err != nil {
		t.Fatal(err)
	}
	n, err := s.BackfillCount()
	if err != nil {
		t.Fatal(err)
	}
	if n != 0 {
		t.Fatalf("不猜，得到 %d", n)
	}
	if c, _ := s.UnattributedCount(g); c != 1 {
		t.Fatalf("待办数 = %d", c)
	}
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 0 {
		t.Fatalf("当前值 = %v", v)
	}
}

func Test删掉规则后只剩唯一一条时记录被它接手(t *testing.T) {
	s := newStore(t)
	g, s1 := goalWith(t, s, "英语", model.KindManualCheckin, "复习")
	s2 := addManual(t, s, g, "写作")

	record(t, s, []int64{g}, [][2]int64{{g, s1}}, today, "复习第 3 课")
	if err := s.SourceDelete(s1); err != nil {
		t.Fatal(err)
	}
	if n, err := s.BackfillCount(); err != nil || n != 1 {
		t.Fatalf("只剩一条能收它了，应该接手：n=%d err=%v", n, err)
	}
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 1 {
		t.Fatalf("当前值 = %v", v)
	}
	if v, _ := s.SourceCheckinValue(s2, today); v != 1 {
		t.Fatalf("s2 = %v", v)
	}
}

// 流水行上的「计入 / 不计入」必须来自归属本身。
func Test流水上的计入与否来自归属而不是目标有没有规则(t *testing.T) {
	s := newStore(t)
	g, s1 := goalWith(t, s, "英语", model.KindManualCheckin, "复习")
	addManual(t, s, g, "写作")
	addManual(t, s, g, "听力")
	record(t, s, []int64{g}, [][2]int64{{g, s1}}, today, "复习第 3 课")

	all, err := s.CheckinsAll()
	if err != nil {
		t.Fatal(err)
	}
	if !all[0].Links[0].Counts() {
		t.Fatal("归到了规则 #1，就是计入")
	}

	if err := s.SourceDelete(s1); err != nil {
		t.Fatal(err)
	}
	if n, _ := s.BackfillCount(); n != 0 {
		t.Fatalf("不猜，补了 %d 条", n)
	}
	all, _ = s.CheckinsAll()
	if all[0].Links[0].Counts() {
		t.Fatal("悬着的记录不能在流水上自称计入")
	}
	if v, _ := metrics.ValueToday(s, g, d(t, today)); v != 0 {
		t.Fatalf("当前值 = %v", v)
	}
}

func Test一条记录挂在两个目标上各归各的规则(t *testing.T) {
	s := newStore(t)
	a, sa := goalWith(t, s, "英语", model.KindManualCheckin, "复习记录")
	b, sb := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")

	links, err := s.ResolveLinks([]int64{a, b}, [][2]int64{{a, sa}, {b, sb}}, false)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.CheckinAdd(links, today, "10:00", 1, "读完 CSAPP 第 3 章", today); err != nil {
		t.Fatal(err)
	}
	if _, err := metrics.Roll(s, d(t, today)); err != nil {
		t.Fatal(err)
	}
	if v, _ := metrics.ValueToday(s, a, d(t, today)); v != 1 {
		t.Fatalf("a = %v", v)
	}
	if v, _ := metrics.ValueToday(s, b, d(t, today)); v != 1 {
		t.Fatalf("b = %v", v)
	}
}

// ------------------------------------------------------------ 放弃通道

func Test归档必须写原因(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章")
	if err := s.GoalArchive(g, "   ", today); err == nil {
		t.Fatal("原因不能是空白")
	}
	if err := s.GoalArchive(g, "暂时不考了", "2026-10-07 09:00:00"); err != nil {
		t.Fatal(err)
	}
	after, err := s.GoalByID(g)
	if err != nil || after == nil {
		t.Fatal(err)
	}
	if after.Status != "archived" {
		t.Fatalf("状态 = %s", after.Status)
	}
	if !strings.Contains(after.Why, "暂时不考了") {
		t.Fatalf("放弃的理由要留在 why 里：%q", after.Why)
	}
}

func Test有记录的目标删不掉(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章")
	if err := s.GoalDelete(g); err != nil {
		t.Fatalf("建错了，还没动过，应该可以删：%v", err)
	}

	g2, _ := goalWith(t, s, "计算机基础", model.KindManualCheckin, "读完一章")
	record(t, s, []int64{g2}, nil, today, "读完第 3 章")
	if err := s.GoalDelete(g2); err == nil {
		t.Fatal("动过的历史不该被一次点击抹掉")
	}
}

// 删目标会级联删掉它的规则和快照，但**不会**删掉记录本身（记录不属于某个目标）。
func Test删目标只删自己的规则和快照(t *testing.T) {
	s := newStore(t)
	g, _ := goalWith(t, s, "英语", model.KindManualCheckin, "读完一章")
	if _, err := s.SourceAdd(g, model.KindGitCommits, "repo", "{}", "提交", today); err != nil {
		t.Fatal(err)
	}
	if err := s.GoalDelete(g); err != nil {
		t.Fatal(err)
	}
	srcs, err := s.SourcesOf(g)
	if err != nil {
		t.Fatal(err)
	}
	if len(srcs) != 0 {
		t.Fatalf("规则该跟着目标一起没了，剩 %d 条", len(srcs))
	}
}
