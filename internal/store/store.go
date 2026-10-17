// Package store 是数据层：SQLite schema、CRUD、记录归属与默认库位置。
//
// # 为什么 schema 是终态
//
// 没有多版本 schema 需要兼容，所以直接建终态，省掉 pragma_table_info 那一套
// 「加过没加过」的判定。
//
// # 判定不在这一层
//
// 「一条记录算不算数」的答案在 metrics.SourceValue；这一层只负责把归属
// （checkin_goals.source_id）如实存下来，以及把「唯一能确定」的归属补上。
package store

import (
	"database/sql"
	"errors"
	"fmt"
	"os"
	"path/filepath"
	"strconv"
	"strings"

	"baseline/internal/model"

	_ "modernc.org/sqlite"
)

// Store 是数据库句柄。
//
// 单连接：单用户、每屏一次查询，连接池带来的是 PRAGMA 不一致和 SQLITE_BUSY，
// 而不是吞吐。所有入口（CLI / HTTP / 测试）都走这一个类型。
type Store struct {
	db *sql.DB
}

// DefaultPath 是数据库位置：%APPDATA%\Baseline\baseline.db（可用 BASELINE_DB 覆盖）。
//
// **不放在 exe 旁边。** 安装到 Program Files 后那个目录对普通用户只读，
// 而数据库必须可写；打包时「程序在哪」和「数据在哪」本来就是两个问题。
func DefaultPath() string {
	if p := os.Getenv("BASELINE_DB"); p != "" {
		return p
	}
	if dir := os.Getenv("APPDATA"); dir != "" {
		return filepath.Join(dir, "Baseline", "baseline.db")
	}
	if home, err := os.UserHomeDir(); err == nil {
		return filepath.Join(home, ".baseline", "baseline.db")
	}
	return "baseline.db"
}

// Open 打开（必要时创建）数据库，并保证 schema 就绪。
func Open(path string) (*Store, error) {
	if dir := filepath.Dir(path); dir != "" && dir != "." {
		if err := os.MkdirAll(dir, 0o755); err != nil {
			return nil, fmt.Errorf("创建数据目录失败：%s", dir)
		}
	}
	// PRAGMA 必须写进 DSN：database/sql 是连接池，连上之后再执行 PRAGMA
	// 只会作用到其中一条连接。
	dsn := "file:" + filepath.ToSlash(path) +
		"?_pragma=busy_timeout(5000)&_pragma=journal_mode(WAL)" +
		"&_pragma=foreign_keys(ON)&_pragma=synchronous(FULL)"
	return open(dsn)
}

// OpenMemory 是测试用的内存库。判定规则和快照的语义只能靠它守住。
func OpenMemory() (*Store, error) {
	return open("file::memory:?_pragma=foreign_keys(ON)")
}

func open(dsn string) (*Store, error) {
	db, err := sql.Open("sqlite", dsn)
	if err != nil {
		return nil, fmt.Errorf("打开数据库失败：%w", err)
	}
	db.SetMaxOpenConns(1)
	if err := db.Ping(); err != nil {
		db.Close()
		return nil, fmt.Errorf("打开数据库失败：%w", err)
	}
	s := &Store{db: db}
	if err := s.migrate(); err != nil {
		db.Close()
		return nil, err
	}
	return s, nil
}

// Close 关闭数据库。
func (s *Store) Close() error { return s.db.Close() }

// migrate 建表。全部 IF NOT EXISTS，可重复调用。
func (s *Store) migrate() error {
	const schema = `
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

-- 一条推进。没有 goal_id / source_id 列：
-- 一条记录可以同时推进好几个目标，归属属于「记录 × 目标」那个关系。
CREATE TABLE IF NOT EXISTS checkins (
    id         INTEGER PRIMARY KEY,
    day        TEXT NOT NULL,
    time       TEXT NOT NULL,
    value      REAL NOT NULL DEFAULT 1,
    note       TEXT NOT NULL DEFAULT '',
    created_at TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_checkins_day ON checkins(day);

-- 记录 ↔ 目标。source_id 是「这条记录在这个目标下归到哪条规则」，
-- 也就是它算不算数的唯一依据；NULL 表示「我本来想推进它」，不进这条曲线。
CREATE TABLE IF NOT EXISTS checkin_goals (
    checkin_id INTEGER NOT NULL REFERENCES checkins(id) ON DELETE CASCADE,
    goal_id    INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    source_id  INTEGER REFERENCES sources(id) ON DELETE SET NULL,
    by_ai      INTEGER NOT NULL DEFAULT 0,
    PRIMARY KEY (checkin_id, goal_id)
);
CREATE INDEX IF NOT EXISTS idx_checkin_goals_goal ON checkin_goals(goal_id);

-- 每日快照。过去的日子写入后不再改写；今天可以重算。
CREATE TABLE IF NOT EXISTS snapshots (
    goal_id    INTEGER NOT NULL REFERENCES goals(id) ON DELETE CASCADE,
    day        TEXT NOT NULL,
    cumulative REAL NOT NULL,
    PRIMARY KEY (goal_id, day)
);

CREATE TABLE IF NOT EXISTS settings (
    k TEXT PRIMARY KEY,
    v TEXT NOT NULL
);
`
	if _, err := s.db.Exec(schema); err != nil {
		return fmt.Errorf("建表失败：%w", err)
	}
	if _, err := s.db.Exec(
		"INSERT OR IGNORE INTO meta(k, v) VALUES ('schema_version', '2')",
	); err != nil {
		return fmt.Errorf("写入 schema 版本失败：%w", err)
	}
	// 每次打开都补一遍：给目标补上第一条手工规则之后，它下面那些原本
	// 「没归到任何规则」的记录就此变得唯一可归属。放在打开时做，
	// CLI / 服务 / 测试三个入口就都覆盖到了。
	return s.BackfillAttribution()
}

// ---------------------------------------------------------------- Goal

// GoalAdd 新建目标。色板必须是 model.Palette 里的一支，名字不能重复。
func (s *Store) GoalAdd(title, why, color, now string) (int64, error) {
	title = strings.TrimSpace(title)
	if title == "" {
		return 0, errors.New("目标名称不能为空。")
	}
	if !validColor(color) {
		return 0, fmt.Errorf("无效的颜色 `%s`，可用值：%s", color, strings.Join(model.Palette, " / "))
	}
	if existing, err := s.GoalByTitle(title); err != nil {
		return 0, err
	} else if existing != nil {
		return 0, fmt.Errorf("目标 `%s` 已存在（id=%d）", existing.Title, existing.ID)
	}
	res, err := s.db.Exec(
		"INSERT INTO goals(title, why, color, status, created_at) VALUES (?, ?, ?, 'active', ?)",
		title, why, color, now,
	)
	if err != nil {
		return 0, err
	}
	return res.LastInsertId()
}

func validColor(c string) bool {
	for _, p := range model.Palette {
		if p == c {
			return true
		}
	}
	return false
}

// GoalByTitle 按名字查目标。
func (s *Store) GoalByTitle(title string) (*model.Goal, error) {
	return s.oneGoal(
		"SELECT id, title, why, color, status, created_at FROM goals WHERE title = ?", title,
	)
}

// GoalByID 按 id 查目标。
func (s *Store) GoalByID(id int64) (*model.Goal, error) {
	return s.oneGoal(
		"SELECT id, title, why, color, status, created_at FROM goals WHERE id = ?", id,
	)
}

func (s *Store) oneGoal(query string, args ...any) (*model.Goal, error) {
	var g model.Goal
	err := s.db.QueryRow(query, args...).Scan(
		&g.ID, &g.Title, &g.Why, &g.Color, &g.Status, &g.CreatedAt,
	)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &g, nil
}

// ResolveGoal 解析目标名或 id。CLI 里两种都允许。
func (s *Store) ResolveGoal(key string) (*model.Goal, error) {
	if id, ok := parseInt(key); ok {
		if g, err := s.GoalByID(id); err != nil {
			return nil, err
		} else if g != nil {
			return g, nil
		}
	}
	g, err := s.GoalByTitle(key)
	if err != nil {
		return nil, err
	}
	if g == nil {
		return nil, fmt.Errorf("找不到目标：%s", key)
	}
	return g, nil
}

// GoalList 列出目标。includeArchived 为 false 时只给 active 的。
func (s *Store) GoalList(includeArchived bool) ([]model.Goal, error) {
	q := "SELECT id, title, why, color, status, created_at FROM goals ORDER BY id"
	if !includeArchived {
		q = "SELECT id, title, why, color, status, created_at FROM goals WHERE status='active' ORDER BY id"
	}
	rows, err := s.db.Query(q)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []model.Goal{}
	for rows.Next() {
		var g model.Goal
		if err := rows.Scan(&g.ID, &g.Title, &g.Why, &g.Color, &g.Status, &g.CreatedAt); err != nil {
			return nil, err
		}
		out = append(out, g)
	}
	return out, rows.Err()
}

// GoalArchive 归档，不删除。必须有正常的放弃通道，且强制留一句原因。
func (s *Store) GoalArchive(id int64, reason, now string) error {
	reason = strings.TrimSpace(reason)
	if reason == "" {
		return errors.New("归档必须填写原因，便于日后回顾。")
	}
	_, err := s.db.Exec(
		"UPDATE goals SET status='archived', archived_at=?, why = why || ? WHERE id=?",
		now, fmt.Sprintf("\n[归档 %s] %s", now, reason), id,
	)
	return err
}

// GoalUpdate 改目标的名字和动机。
func (s *Store) GoalUpdate(id int64, title, why string) error {
	title = strings.TrimSpace(title)
	if title == "" {
		return errors.New("目标名称不能为空。")
	}
	_, err := s.db.Exec("UPDATE goals SET title=?, why=? WHERE id=?", title, strings.TrimSpace(why), id)
	return err
}

// GoalDelete 删目标。**只有一条记录都没有的目标才允许删。**
//
// 有记录就说明这条线动过——动过的历史不该被一次点击抹掉。不要了应该走归档：
// 归档保留曲线，也保留放弃的理由。删除留给「建错了」这种情况。
func (s *Store) GoalDelete(id int64) error {
	n, err := s.CheckinCount(id)
	if err != nil {
		return err
	}
	if n > 0 {
		return fmt.Errorf(
			"该目标已有 %d 条记录，无法删除。如需停止，请使用归档；曲线与归档原因都会保留。", n)
	}
	_, err = s.db.Exec("DELETE FROM goals WHERE id=?", id)
	return err
}

// CheckinCount 挂在某个目标上的记录条数。**算关联，不算计分**——
// 删除的判据是「有没有人声称这条线是自己的」，而不是「这条线动没动过」。
func (s *Store) CheckinCount(goalID int64) (int64, error) {
	var n int64
	err := s.db.QueryRow("SELECT COUNT(*) FROM checkin_goals WHERE goal_id=?", goalID).Scan(&n)
	return n, err
}

// ---------------------------------------------------------------- Source

// SourceAdd 加一条判定规则。同一目标下同类型同 target 不重复登记。
func (s *Store) SourceAdd(goalID int64, kind model.SourceKind, target, params, rationale, now string) (int64, error) {
	var dup int64
	err := s.db.QueryRow(
		"SELECT id FROM sources WHERE goal_id=? AND kind=? AND target=?",
		goalID, kind.String(), target,
	).Scan(&dup)
	if err == nil {
		return 0, fmt.Errorf("该来源已存在（来源 id=%d），未重复添加。", dup)
	}
	if !errors.Is(err, sql.ErrNoRows) {
		return 0, err
	}
	res, err := s.db.Exec(
		"INSERT INTO sources(goal_id, kind, target, params, rationale, created_at) VALUES (?, ?, ?, ?, ?, ?)",
		goalID, kind.String(), target, params, rationale, now,
	)
	if err != nil {
		return 0, err
	}
	return res.LastInsertId()
}

// SourcesOf 列出某个目标的规则。
func (s *Store) SourcesOf(goalID int64) ([]model.Source, error) {
	return s.querySources(
		"SELECT id, goal_id, kind, target, params, rationale FROM sources WHERE goal_id=? ORDER BY id",
		goalID,
	)
}

// SourcesAll 是全部规则，**含已归档目标的**。
//
// 流水行要说清「这条记录算在哪条规则上」，而归档目标的记录仍然留在流水里
// （归档只让它从列表消失）。按活跃目标去查会正好漏掉它们。
func (s *Store) SourcesAll() ([]model.Source, error) {
	return s.querySources(
		"SELECT id, goal_id, kind, target, params, rationale FROM sources ORDER BY id",
	)
}

func (s *Store) querySources(query string, args ...any) ([]model.Source, error) {
	rows, err := s.db.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []model.Source{}
	for rows.Next() {
		var src model.Source
		var kind string
		if err := rows.Scan(&src.ID, &src.GoalID, &kind, &src.Target, &src.Params, &src.Rationale); err != nil {
			return nil, err
		}
		// 未知类型当成手工打卡：读不出来就把整页打死，比显示错一行更糟。
		if k, err := model.ParseKind(kind); err == nil {
			src.Kind = k
		} else {
			src.Kind = model.KindManualCheckin
		}
		out = append(out, src)
	}
	return out, rows.Err()
}

// SourceDelete 删一条规则。原本归在它名下的记录会悬空（ON DELETE SET NULL）。
func (s *Store) SourceDelete(id int64) error {
	_, err := s.db.Exec("DELETE FROM sources WHERE id=?", id)
	return err
}

// GoalHasRule 目标有没有至少一条规则。
func (s *Store) GoalHasRule(goalID int64) (bool, error) {
	var n int64
	err := s.db.QueryRow("SELECT COUNT(*) FROM sources WHERE goal_id=?", goalID).Scan(&n)
	return n > 0, err
}

// GoalsWithoutRule 找出所有没有判定规则的目标。主视图会警告它们。
func (s *Store) GoalsWithoutRule() ([]model.Goal, error) {
	rows, err := s.db.Query(
		`SELECT id, title, why, color, status, created_at FROM goals g
          WHERE status='active'
            AND NOT EXISTS (SELECT 1 FROM sources s WHERE s.goal_id = g.id)
          ORDER BY id`,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []model.Goal{}
	for rows.Next() {
		var g model.Goal
		if err := rows.Scan(&g.ID, &g.Title, &g.Why, &g.Color, &g.Status, &g.CreatedAt); err != nil {
			return nil, err
		}
		out = append(out, g)
	}
	return out, rows.Err()
}

// NextFreeColor 挑一个还没被活跃目标占用的色板。
//
// 放在这里而不是各自复制一份：CLI 和窗口都要建目标，两边的选色逻辑一旦分叉，
// 同一个目标在命令行里和窗口里会是两个颜色。
func (s *Store) NextFreeColor() (string, error) {
	goals, err := s.GoalList(false)
	if err != nil {
		return "", err
	}
	used := map[string]bool{}
	for _, g := range goals {
		used[g.Color] = true
	}
	for _, c := range model.Palette {
		if !used[c] {
			return c, nil
		}
	}
	return model.Palette[0], nil
}

// ---------------------------------------------------------------- Checkin

// CheckinAdd 记一条推进。links 是「这条记录推进了哪些目标，各归到哪条规则」。
// 归属由 ResolveLinks 算好再传进来，这里不做判断。
func (s *Store) CheckinAdd(links []model.CheckinLink, day, clock string, value float64, note, now string) (int64, error) {
	tx, err := s.db.Begin()
	if err != nil {
		return 0, err
	}
	defer tx.Rollback()

	res, err := tx.Exec(
		"INSERT INTO checkins(day, time, value, note, created_at) VALUES (?, ?, ?, ?, ?)",
		day, clock, value, note, now,
	)
	if err != nil {
		return 0, err
	}
	id, err := res.LastInsertId()
	if err != nil {
		return 0, err
	}
	for _, l := range links {
		if _, err := tx.Exec(
			"INSERT OR IGNORE INTO checkin_goals(checkin_id, goal_id, source_id, by_ai) VALUES (?, ?, ?, ?)",
			id, l.GoalID, nullableID(l.SourceID), l.ByAI,
		); err != nil {
			return 0, err
		}
	}
	if err := tx.Commit(); err != nil {
		return 0, err
	}
	return id, nil
}

func nullableID(p *int64) any {
	if p == nil {
		return nil
	}
	return *p
}

func (s *Store) linksByCheckin() (map[int64][]model.CheckinLink, error) {
	rows, err := s.db.Query(
		"SELECT checkin_id, goal_id, source_id, by_ai FROM checkin_goals ORDER BY checkin_id, goal_id",
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := map[int64][]model.CheckinLink{}
	for rows.Next() {
		var cid int64
		var l model.CheckinLink
		var sid sql.NullInt64
		var byAI int64
		if err := rows.Scan(&cid, &l.GoalID, &sid, &byAI); err != nil {
			return nil, err
		}
		if sid.Valid {
			v := sid.Int64
			l.SourceID = &v
		}
		l.ByAI = byAI != 0
		out[cid] = append(out[cid], l)
	}
	return out, rows.Err()
}

func (s *Store) queryCheckins(query string, args ...any) ([]model.Checkin, error) {
	links, err := s.linksByCheckin()
	if err != nil {
		return nil, err
	}
	rows, err := s.db.Query(query, args...)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []model.Checkin{}
	for rows.Next() {
		var c model.Checkin
		if err := rows.Scan(&c.ID, &c.Day, &c.Time, &c.Value, &c.Note); err != nil {
			return nil, err
		}
		c.Links = links[c.ID]
		if c.Links == nil {
			c.Links = []model.CheckinLink{}
		}
		out = append(out, c)
	}
	return out, rows.Err()
}

// CheckinsOf 挂到某个目标上的全部记录（**不管计不计分**）。详情页的流水用它。
func (s *Store) CheckinsOf(goalID int64) ([]model.Checkin, error) {
	return s.queryCheckins(
		`SELECT c.id, c.day, c.time, c.value, c.note FROM checkins c
           JOIN checkin_goals cg ON cg.checkin_id = c.id
          WHERE cg.goal_id = ? ORDER BY c.day, c.time, c.id`, goalID,
	)
}

// CheckinsAll 是全部打卡，**正序**。
//
// 时间线是「下新上旧」的一条流水，所以按时间正序取，最新的落在最后一行。
// 不设 LIMIT：单用户、一天几条，量级很小；真要大到需要截断，该处理的是
// 「怎么让用户看到更早的」，而不是在这里悄悄丢数据。
func (s *Store) CheckinsAll() ([]model.Checkin, error) {
	return s.queryCheckins("SELECT id, day, time, value, note FROM checkins ORDER BY day, time, id")
}

// SourceCheckinValue 是**这一条规则**在截止某天的贡献：归属到它的记录之和。
//
// 归属写在 checkin_goals.source_id 上。**这是判定规则与数字之间唯一的连接点**——
// 不看这一列，卡片底下印的那句「什么算推进它」对曲线就没有任何影响，
// 挂在这个目标上的什么记录都会让它涨一格，那就是一条假曲线。
func (s *Store) SourceCheckinValue(sourceID int64, day string) (float64, error) {
	var v float64
	err := s.db.QueryRow(
		`SELECT COALESCE(SUM(c.value), 0) FROM checkins c
           JOIN checkin_goals cg ON cg.checkin_id = c.id
          WHERE cg.source_id=? AND c.day<=?`, sourceID, day,
	).Scan(&v)
	return v, err
}

// ManualSourcesOf 是这个目标下**能收手工记录**的规则（按 id 排序）。
func (s *Store) ManualSourcesOf(goalID int64) ([]model.Source, error) {
	srcs, err := s.SourcesOf(goalID)
	if err != nil {
		return nil, err
	}
	out := []model.Source{}
	for _, src := range srcs {
		if src.Kind == model.KindManualCheckin {
			out = append(out, src)
		}
	}
	return out, nil
}

// PickSource 是「这条记录该归到哪条规则」。**候选唯一才自动归属**，
// 0 条或 ≥2 条都返回 nil——不替人猜，猜错就是一条假曲线。
func (s *Store) PickSource(goalID int64) (*int64, error) {
	cands, err := s.ManualSourcesOf(goalID)
	if err != nil {
		return nil, err
	}
	if len(cands) == 1 {
		id := cands[0].ID
		return &id, nil
	}
	return nil, nil
}

// ResolveLinks 把「推进了哪些目标」解析成「每条关联归到哪条规则」。
//
//   - picks 是人点过的「谁归谁」（目标 id, 规则 id）。
//   - defer 是说不清时怎么办：false 现在就拒绝并列出候选；true 先挂空着写下来，
//     之后由 AI 补判或者由人指认。**记录先落库是硬要求**：卡在网络上等着，
//     等于让一条已经发生的事因为别人的服务器慢而记不下来。
//
// 两种情形会拒绝，都是机械的：指定的规则不属于那个目标；某个目标下有多条手工规则、
// 而人没指定，且 defer 是 false。
func (s *Store) ResolveLinks(goalIDs []int64, picks [][2]int64, deferAmbiguous bool) ([]model.CheckinLink, error) {
	for _, p := range picks {
		goalID, sourceID := p[0], p[1]
		g, err := s.GoalByID(goalID)
		if err != nil {
			return nil, err
		}
		title := fmt.Sprintf("#%d", goalID)
		if g != nil {
			title = g.Title
		}
		found := false
		for _, id := range goalIDs {
			if id == goalID {
				found = true
			}
		}
		if !found {
			return nil, fmt.Errorf("目标「%s」不在本次记录中，无法指定规则。", title)
		}
		cands, err := s.ManualSourcesOf(goalID)
		if err != nil {
			return nil, err
		}
		ok := false
		for _, c := range cands {
			if c.ID == sourceID {
				ok = true
			}
		}
		if !ok {
			return nil, fmt.Errorf("规则 #%d 不属于「%s」的手工规则，无法用于归属。", sourceID, title)
		}
	}

	out := make([]model.CheckinLink, 0, len(goalIDs))
	for _, g := range goalIDs {
		cands, err := s.ManualSourcesOf(g)
		if err != nil {
			return nil, err
		}
		var link model.CheckinLink
		link.GoalID = g
		picked := false
		for _, p := range picks {
			if p[0] == g {
				id := p[1]
				link.SourceID = &id
				picked = true
				break
			}
		}
		if !picked {
			switch {
			case len(cands) == 1:
				id := cands[0].ID
				link.SourceID = &id
			case len(cands) == 0:
				// 挂上去是记下「我本来想推进它」，不进曲线。
			case deferAmbiguous:
				// 挂空着，等 AI 或者等人。
			default:
				title := fmt.Sprintf("#%d", g)
				if goal, err := s.GoalByID(g); err == nil && goal != nil {
					title = goal.Title
				}
				return nil, fmt.Errorf("%s", ambiguousMsg(title, cands))
			}
		}
		out = append(out, link)
	}
	return out, nil
}

// ambiguousMsg 是歧义时的拒绝语。把候选连同 id 一起列出来，让人不用回头去查
// 就能指认。**只陈述事实**，不写行动指引——同一句话在 CLI 和窗口里都会出现，
// 而那两个地方该做的事不一样。
func ambiguousMsg(goalTitle string, cands []model.Source) string {
	list := make([]string, 0, len(cands))
	for _, c := range cands {
		list = append(list, fmt.Sprintf("#%d %s", c.ID, c.Summary()))
	}
	// 用「；」分隔：规则原话里本来就有「，」「、」，再拿它们分隔就分不清哪到哪了。
	return fmt.Sprintf("「%s」下有 %d 条规则均匹配该记录：%s。",
		goalTitle, len(cands), strings.Join(list, "；"))
}

// CheckinNote 读一条记录的备注。
func (s *Store) CheckinNote(checkinID int64) (string, error) {
	var note string
	err := s.db.QueryRow("SELECT note FROM checkins WHERE id=?", checkinID).Scan(&note)
	return note, err
}

// LinksOf 是一条记录**现在**挂着哪些目标、各归到哪条规则。
//
// 和 ResolveLinks 的区别：那个算的是「准备怎么写」，这个是「库里现在是什么」。
// 补判之后再想说话就得读这一份。
func (s *Store) LinksOf(checkinID int64) ([]model.CheckinLink, error) {
	rows, err := s.db.Query(
		"SELECT goal_id, source_id, by_ai FROM checkin_goals WHERE checkin_id=? ORDER BY goal_id",
		checkinID,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []model.CheckinLink{}
	for rows.Next() {
		var l model.CheckinLink
		var sid sql.NullInt64
		var byAI int64
		if err := rows.Scan(&l.GoalID, &sid, &byAI); err != nil {
			return nil, err
		}
		if sid.Valid {
			v := sid.Int64
			l.SourceID = &v
		}
		l.ByAI = byAI != 0
		out = append(out, l)
	}
	return out, rows.Err()
}

// CheckinRef 是一条待办记录的引用：id 与备注。
type CheckinRef struct {
	ID   int64
	Note string
}

// UnattributedOf 是这个目标下「挂上来了、还没归到任何规则」的记录。
// 这是**待办**，不是垃圾：它们等着被 AI 判，或者被人指认。
func (s *Store) UnattributedOf(goalID int64) ([]CheckinRef, error) {
	rows, err := s.db.Query(
		`SELECT c.id, c.note FROM checkin_goals cg
           JOIN checkins c ON c.id = cg.checkin_id
          WHERE cg.goal_id=? AND cg.source_id IS NULL
          ORDER BY c.day, c.time, c.id`, goalID,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []CheckinRef{}
	for rows.Next() {
		var r CheckinRef
		if err := rows.Scan(&r.ID, &r.Note); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// UnlinkedCheckins 是压根没挂到任何目标上的记录：(记录 id, 备注)。
//
// 「记下来是第一步，归到哪个目标是第二步」——默认不选目标的记录会一直躺在流水里，
// 需要一条查询把它们捞出来，交给人工或 AI 补判。
func (s *Store) UnlinkedCheckins() ([]CheckinRef, error) {
	rows, err := s.db.Query(
		`SELECT c.id, c.note FROM checkins c
          WHERE NOT EXISTS (SELECT 1 FROM checkin_goals g WHERE g.checkin_id = c.id)
          ORDER BY c.day, c.time, c.id`,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []CheckinRef{}
	for rows.Next() {
		var r CheckinRef
		if err := rows.Scan(&r.ID, &r.Note); err != nil {
			return nil, err
		}
		out = append(out, r)
	}
	return out, rows.Err()
}

// Attribute 把一条记录归到某个目标的某条规则上。byAI 记下这是谁定的。
//
// **upsert**：挂空着的那种是改写已有的一行，压根没挂目标的那些是新建一行。
// 两者都是「把归属写上」，分成两个函数迟早会有人只调用其中一个，
// 而漏掉的那一半表现是「判完了但没写进去」——静默、且看起来像模型没答上来。
func (s *Store) Attribute(checkinID, goalID, sourceID int64, byAI bool) error {
	_, err := s.db.Exec(
		`INSERT INTO checkin_goals(checkin_id, goal_id, source_id, by_ai) VALUES (?, ?, ?, ?)
         ON CONFLICT(checkin_id, goal_id)
         DO UPDATE SET source_id = excluded.source_id, by_ai = excluded.by_ai`,
		checkinID, goalID, sourceID, byAI,
	)
	return err
}

// BackfillAttribution 把「没归到任何规则」的关联补上归属：**候选唯一才补**。
// 返回本次补上的条数。
//
// 每次开库都跑，所以「先把账记了、后来才给目标补上规则」这种顺序不会留下一条
// 永远不肯算数的记录——规则一到位，它就归位了。
func (s *Store) BackfillAttribution() error {
	rows, err := s.db.Query(
		"SELECT checkin_id, goal_id FROM checkin_goals WHERE source_id IS NULL ORDER BY checkin_id, goal_id",
	)
	if err != nil {
		return err
	}
	type pair struct{ checkin, goal int64 }
	pending := []pair{}
	for rows.Next() {
		var p pair
		if err := rows.Scan(&p.checkin, &p.goal); err != nil {
			rows.Close()
			return err
		}
		pending = append(pending, p)
	}
	rows.Close()
	if err := rows.Err(); err != nil {
		return err
	}

	for _, p := range pending {
		sid, err := s.PickSource(p.goal)
		if err != nil {
			return err
		}
		if sid == nil {
			continue
		}
		if _, err := s.db.Exec(
			"UPDATE checkin_goals SET source_id=? WHERE checkin_id=? AND goal_id=?",
			*sid, p.checkin, p.goal,
		); err != nil {
			return err
		}
	}
	return nil
}

// BackfillCount 是 BackfillAttribution 的「补了几条」版本，给 CLI 的提示用。
func (s *Store) BackfillCount() (int, error) {
	before, err := s.unattributedTotal()
	if err != nil {
		return 0, err
	}
	if err := s.BackfillAttribution(); err != nil {
		return 0, err
	}
	after, err := s.unattributedTotal()
	if err != nil {
		return 0, err
	}
	return before - after, nil
}

func (s *Store) unattributedTotal() (int, error) {
	var n int
	err := s.db.QueryRow("SELECT COUNT(*) FROM checkin_goals WHERE source_id IS NULL").Scan(&n)
	return n, err
}

// UnattributedCount 是这个目标下「挂上来了、但没归到任何规则」的记录数。
func (s *Store) UnattributedCount(goalID int64) (int64, error) {
	var n int64
	err := s.db.QueryRow(
		`SELECT COUNT(*) FROM checkin_goals cg
           JOIN checkins c ON c.id = cg.checkin_id
          WHERE cg.goal_id=? AND cg.source_id IS NULL`, goalID,
	).Scan(&n)
	return n, err
}

// FirstCheckinDay 是目标下最早的打卡日期。没有则 nil。
//
// **这里不判「计不计分」**，只看有没有人挂上来过。has_data 用它决定
// 「画曲线还是画空状态」——加了规则判断的话，一条 external_metric 目标
// 会因为「手工记录不算数」而显示「还没有任何记录」，旁边却顶着一个数字。
func (s *Store) FirstCheckinDay(goalID int64) (string, error) {
	var day sql.NullString
	err := s.db.QueryRow(
		`SELECT MIN(c.day) FROM checkins c
           JOIN checkin_goals cg ON cg.checkin_id = c.id WHERE cg.goal_id=?`, goalID,
	).Scan(&day)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	return day.String, nil
}

// ---------------------------------------------------------------- Snapshot

// SnapshotPut 写快照。**INSERT OR IGNORE**：已存在的日期永不改写，
// 这是「历史不会因规则改动而被重画」的落点。
func (s *Store) SnapshotPut(goalID int64, day string, cumulative float64) (bool, error) {
	res, err := s.db.Exec(
		"INSERT OR IGNORE INTO snapshots(goal_id, day, cumulative) VALUES (?, ?, ?)",
		goalID, day, cumulative,
	)
	if err != nil {
		return false, err
	}
	n, err := res.RowsAffected()
	return n > 0, err
}

// SnapshotPutToday 重算**今天**的快照。
//
// 今天还没过完。用 INSERT OR IGNORE 的话，当天第一次渲染就把今天的值钉死了——
// 之后每记一条，卡片上的数字会涨（那是实时累加的），曲线却停在第一次渲染的位置，
// 数字和线的末端当场对不上。所以：**过去的日子冻结，今天可以重算。**
func (s *Store) SnapshotPutToday(goalID int64, day string, cumulative float64) (bool, error) {
	res, err := s.db.Exec(
		`INSERT INTO snapshots(goal_id, day, cumulative) VALUES (?, ?, ?)
         ON CONFLICT(goal_id, day) DO UPDATE SET cumulative = excluded.cumulative
         WHERE cumulative <> excluded.cumulative`,
		goalID, day, cumulative,
	)
	if err != nil {
		return false, err
	}
	n, err := res.RowsAffected()
	return n > 0, err
}

// SnapshotPoint 是某一天已落盘的累计值。
type SnapshotPoint struct {
	Day        string
	Cumulative float64
}

// SnapshotsOf 取最近 days 天的快照，时间正序。
func (s *Store) SnapshotsOf(goalID int64, days int) ([]SnapshotPoint, error) {
	rows, err := s.db.Query(
		`SELECT day, cumulative FROM snapshots WHERE goal_id=?
          ORDER BY day DESC LIMIT ?`, goalID, days,
	)
	if err != nil {
		return nil, err
	}
	defer rows.Close()
	out := []SnapshotPoint{}
	for rows.Next() {
		var p SnapshotPoint
		if err := rows.Scan(&p.Day, &p.Cumulative); err != nil {
			return nil, err
		}
		out = append(out, p)
	}
	if err := rows.Err(); err != nil {
		return nil, err
	}
	// 变回时间正序
	for i, j := 0, len(out)-1; i < j; i, j = i+1, j-1 {
		out[i], out[j] = out[j], out[i]
	}
	return out, nil
}

// LastSnapshotDay 是最后一个已写入快照的日期。
func (s *Store) LastSnapshotDay(goalID int64) (string, error) {
	var day sql.NullString
	err := s.db.QueryRow("SELECT MAX(day) FROM snapshots WHERE goal_id=?", goalID).Scan(&day)
	if errors.Is(err, sql.ErrNoRows) {
		return "", nil
	}
	if err != nil {
		return "", err
	}
	return day.String, nil
}

// ---------------------------------------------------------------- Settings

// SettingGet 读一条设置。没有就返回 nil——**不要在这里编默认值**：
// 默认值属于用它的那一项（AI 的默认在 ai.Config），塞在这里会变成两份。
func (s *Store) SettingGet(key string) (*string, error) {
	var v string
	err := s.db.QueryRow("SELECT v FROM settings WHERE k=?", key).Scan(&v)
	if errors.Is(err, sql.ErrNoRows) {
		return nil, nil
	}
	if err != nil {
		return nil, err
	}
	return &v, nil
}

// SettingSet 写一条设置。
func (s *Store) SettingSet(key, value string) error {
	_, err := s.db.Exec(
		"INSERT INTO settings(k, v) VALUES (?, ?) ON CONFLICT(k) DO UPDATE SET v = excluded.v",
		key, value,
	)
	return err
}

// SettingDel 删一条设置。
func (s *Store) SettingDel(key string) error {
	_, err := s.db.Exec("DELETE FROM settings WHERE k=?", key)
	return err
}

func parseInt(s string) (int64, bool) {
	n, err := strconv.ParseInt(s, 10, 64)
	return n, err == nil
}
