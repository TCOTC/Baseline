// Package metrics 是累积计算与每日快照。
//
// # 为什么曲线读快照，而不是实时算
//
//  1. 快：渲染不用回扫全部打卡记录。
//  2. **历史不会被重画**：快照一旦写入就不再改写。以后改了判定规则，只影响
//     **未来**的取值，已经落盘的那段曲线保持原样。这是诚实性要求。
//
// # 今天是个例外
//
// 今天还没过完，所以今天的快照每次重算。不这样的话，当天第一次渲染就把今天的值
// 钉死了：之后每记一条，卡片上的数字会涨（那是实时累加的），曲线却停在第一次渲染
// 的位置——数字和线的末端当场对不上，而且同一天记第二次就会撞上。
//
// 这条规则还顺带定义了补记：**补一条三天前的打卡，不会把曲线往回改，
// 只会在今天抬一格**（意思是「你今天才记下来」，而不是「你三天前做过」）。
//
// # 数字和曲线必须同源
//
// current 和 deltaWeek 都取自快照序列，不再单独实时累加。两边各算各的，
// 迟早会显示不同的数——而用户只看得到「对不上」。
package metrics

import (
	"fmt"
	"time"

	"baseline/internal/model"
	"baseline/internal/store"
)

// Window 是曲线窗口天数。
const Window = 30

// SourceValue 是单条计分来源在截止某天的贡献。
//
// **求值只有这一处。** 没有接线的来源返回 0，但调用方必须能分辨「0」和「没接线」——
// 见 GoalSeries.Wired。一条只有 git 规则的目标画出一条平坦的 0，那不叫「没动」，
// 那叫「规则还没接线」。
func SourceValue(s *store.Store, src model.Source, day string) (float64, error) {
	switch src.Kind {
	case model.KindManualCheckin:
		return s.SourceCheckinValue(src.ID, day)
	default:
		// 这三种规则现在只能登记：接上数据源时在这里加分支，
		// 别在别处再写一遍「某类来源怎么算」。
		return 0, nil
	}
}

// ValueAt 是目标在截止某天的累计值 = 它全部来源贡献之和。
func ValueAt(s *store.Store, goalID int64, day string) (float64, error) {
	srcs, err := s.SourcesOf(goalID)
	if err != nil {
		return 0, err
	}
	var sum float64
	for _, src := range srcs {
		v, err := SourceValue(s, src, day)
		if err != nil {
			return 0, err
		}
		sum += v
	}
	return sum, nil
}

// ValueToday 是目标当前的累计值（截止今天），和曲线同源：曲线末端就是它。
//
// 别用「把所有记录加起来」代替它——未来的日期不会进曲线，两边一旦各算各的，
// 卡片上的数字迟早和线的末端对不上。
func ValueToday(s *store.Store, goalID int64, today time.Time) (float64, error) {
	return ValueAt(s, goalID, DayKey(today))
}

// GoalSeries 是一个目标在窗口内的曲线。
type GoalSeries struct {
	GoalID int64
	// Values 是窗口内每一天的累积值（时间正序，长度 = Window）
	Values []float64
	// Days 是窗口内每一天的日期（YYYY-MM-DD）
	Days []string
	// Current 是当前累计（= 曲线末端，取自快照）
	Current float64
	// DeltaWeek 是近 7 天位移（同样取自快照）
	DeltaWeek float64
	// HasData 表示有没有任何记录。false 时渲染成空状态，不画线。
	HasData bool
	// Wired 表示这个目标有没有至少一条已接线的来源。false 时那条平坦的 0
	// **不是「没动」，是「规则还没接线」**——两者必须能分辨。
	Wired bool
	// Unwired 是没接线的来源类型名（去重，按登记顺序）。给界面的说明用。
	Unwired []string
}

// Roll 补齐从「首个打卡日」到「今天」之间缺失的快照。
//
// 幂等：**过去的日子已存在就不动；今天每次重算**。返回本次新写入或改写的条数。
func Roll(s *store.Store, today time.Time) (int, error) {
	written := 0
	goals, err := s.GoalList(false)
	if err != nil {
		return 0, err
	}
	for _, g := range goals {
		firstDay, err := s.FirstCheckinDay(g.ID)
		if err != nil {
			return 0, err
		}
		if firstDay == "" {
			continue // 没有打卡，无需快照
		}
		first, err := ParseDay(firstDay)
		if err != nil {
			return 0, err
		}

		// 从「已有快照的次日」或「首个打卡日」开始补。今天要重算，所以起点
		// 不能晚于今天——否则 last == today 时循环直接跳过，今天永远钉在第一次渲染的值上。
		start := first
		if lastDay, err := s.LastSnapshotDay(g.ID); err != nil {
			return 0, err
		} else if lastDay != "" {
			last, err := ParseDay(lastDay)
			if err != nil {
				return 0, err
			}
			start = last.AddDate(0, 0, 1)
			if start.After(today) {
				start = today
			}
		}
		if start.After(today) {
			continue
		}

		for d := start; !d.After(today); d = d.AddDate(0, 0, 1) {
			day := DayKey(d)
			cum, err := ValueAt(s, g.ID, day)
			if err != nil {
				return 0, err
			}
			var wrote bool
			if d.Equal(today) {
				wrote, err = s.SnapshotPutToday(g.ID, day, cum)
			} else {
				wrote, err = s.SnapshotPut(g.ID, day, cum)
			}
			if err != nil {
				return 0, err
			}
			if wrote {
				written++
			}
		}
	}
	return written, nil
}

// Series 取某个目标窗口内的曲线序列。
func Series(s *store.Store, goalID int64, today time.Time) (GoalSeries, error) {
	start := today.AddDate(0, 0, -(Window - 1))
	snaps, err := s.SnapshotsOf(goalID, Window)
	if err != nil {
		return GoalSeries{}, err
	}
	byDay := make(map[string]float64, len(snaps))
	for _, p := range snaps {
		byDay[p.Day] = p.Cumulative
	}

	values := make([]float64, 0, Window)
	days := make([]string, 0, Window)
	var running float64
	for d := start; !d.After(today); d = d.AddDate(0, 0, 1) {
		key := DayKey(d)
		if v, ok := byDay[key]; ok {
			running = v
		}
		// 缺日沿用前值 —— 阶梯线的语义
		values = append(values, running)
		days = append(days, key)
	}

	// 数字和曲线同源：都取自快照序列，不再各算各的。
	current := 0.0
	if len(values) > 0 {
		current = values[len(values)-1]
	}
	weekAgo := Window - 1 - 7 // 今天往前 7 天在窗口里的下标
	deltaWeek := current
	if weekAgo < len(values) {
		deltaWeek = current - values[weekAgo]
	}

	firstDay, err := s.FirstCheckinDay(goalID)
	if err != nil {
		return GoalSeries{}, err
	}

	// 「规则还没接线」和「规则接线了但没动」是两件事，必须能分辨。
	srcs, err := s.SourcesOf(goalID)
	if err != nil {
		return GoalSeries{}, err
	}
	wired := false
	var unwired []string
	for _, src := range srcs {
		if src.Kind.Implemented() {
			wired = true
			continue
		}
		label := src.Kind.Label()
		if !contains(unwired, label) {
			unwired = append(unwired, label)
		}
	}

	return GoalSeries{
		GoalID:    goalID,
		Values:    values,
		Days:      days,
		Current:   current,
		DeltaWeek: deltaWeek,
		HasData:   firstDay != "",
		Wired:     wired,
		Unwired:   unwired,
	}, nil
}

// StripOf 是 30 格贡献条纹：某天是否比前一天增加了。
func StripOf(series GoalSeries) []bool {
	out := make([]bool, 0, len(series.Values))
	prev := 0.0
	for _, v := range series.Values {
		out = append(out, v > prev)
		prev = v
	}
	return out
}

// ParseDay 解析 YYYY-MM-DD，返回 UTC 零点。
//
// 全程用 UTC 只是为了「日」的加减不撞夏令时；显示的日期本身是本地日期，
// 由调用方从 time.Now() 取。
func ParseDay(s string) (time.Time, error) {
	t, err := time.Parse("2006-01-02", s)
	if err != nil {
		return time.Time{}, fmt.Errorf("日期格式应为 YYYY-MM-DD，收到 %s", s)
	}
	return t, nil
}

// DayKey 把时间截到它所在的那一天，格式化成 YYYY-MM-DD。
func DayKey(t time.Time) string {
	y, m, d := t.Date()
	return fmt.Sprintf("%04d-%02d-%02d", y, int(m), d)
}

// DayStart 把任意时间截到当天零点（UTC 表示，只用于日期加减）。
func DayStart(t time.Time) time.Time {
	y, m, d := t.Date()
	return time.Date(y, m, d, 0, 0, 0, 0, time.UTC)
}

func contains(list []string, s string) bool {
	for _, x := range list {
		if x == s {
			return true
		}
	}
	return false
}
