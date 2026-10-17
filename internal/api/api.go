// Package api 是 HTTP 接口层：一屏数据 + 所有写操作。
//
// 页面不再由服务端渲染成 HTML，但**判断仍然只在这里做一次**：数字、归属、
// 快照都走 store / metrics / ai，前端拿到的是一屏算好的数据。
package api

import (
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"baseline/internal/ai"
	"baseline/internal/metrics"
	"baseline/internal/model"
	"baseline/internal/store"
	"baseline/internal/view"
)

// Server 是接口层。
type Server struct {
	Store *store.Store
	// Debug 打开 /__state 和更详细的日志。
	Debug bool
	// Assets 是 SPA 的处理器（内嵌 dist，或 dev 时反向代理到 Vite）。
	Assets http.Handler
	// Logf 写一行日志（错到文件，不吞）。
	Logf func(format string, args ...any)
}

// Handler 组装路由。
func (s *Server) Handler() http.Handler {
	mux := http.NewServeMux()
	mux.HandleFunc("GET /api/view", s.handleView)
	mux.HandleFunc("POST /api/checkin", s.handleCheckin)
	mux.HandleFunc("POST /api/checkins/{id}/classify", s.handleClassify)
	mux.HandleFunc("POST /api/backlog/classify", s.handleBacklog)
	mux.HandleFunc("POST /api/goals", s.handleAddGoal)
	mux.HandleFunc("PUT /api/goals/{id}", s.handleUpdateGoal)
	mux.HandleFunc("POST /api/goals/{id}/archive", s.handleArchiveGoal)
	mux.HandleFunc("DELETE /api/goals/{id}", s.handleDeleteGoal)
	mux.HandleFunc("POST /api/goals/{id}/sources", s.handleAddSource)
	mux.HandleFunc("DELETE /api/sources/{id}", s.handleDeleteSource)
	mux.HandleFunc("PUT /api/settings", s.handleSaveSettings)
	mux.HandleFunc("POST /api/settings/test", s.handleTestAI)
	mux.HandleFunc("POST /__jslog", s.handleJSLog)
	if s.Debug {
		mux.HandleFunc("GET /__state", s.handleState)
	} else {
		// 打包后的正式程序不暴露调试口。**必须显式 404**：不注册的话会落到
		// 下面的 SPA 兜底上，返回一页 200 的 HTML——探测的人分不清「没开」和「没有」。
		mux.HandleFunc("/__state", func(w http.ResponseWriter, r *http.Request) {
			http.NotFound(w, r)
		})
	}
	// 未知的接口也回 404，不要掉进 SPA 兜底里拿一页 HTML。
	mux.HandleFunc("/api/", func(w http.ResponseWriter, r *http.Request) {
		fail(w, http.StatusNotFound, errors.New("没有这个接口"))
	})
	mux.Handle("/", s.Assets)
	return s.recoverer(mux)
}

func (s *Server) recoverer(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		defer func() {
			if v := recover(); v != nil {
				s.logf("panic · %v", v)
				fail(w, http.StatusInternalServerError, fmt.Errorf("服务内部出错：%v", v))
			}
		}()
		next.ServeHTTP(w, r)
	})
}

func (s *Server) logf(format string, args ...any) {
	if s.Logf != nil {
		s.Logf(format, args...)
	}
}

func nowParts() (time.Time, string, string) {
	now := time.Now()
	return metrics.DayStart(now),
		now.Format("2006-01-02"),
		now.Format("2006-01-02 15:04:05")
}

// ---------------------------------------------------------------- 读

func (s *Server) handleView(w http.ResponseWriter, r *http.Request) {
	q := r.URL.Query()
	goalID := int64(0)
	if raw := q.Get("goal"); raw != "" {
		goalID, _ = strconv.ParseInt(raw, 10, 64)
	}
	settings := q.Get("settings") == "1"

	today, _, _ := nowParts()
	// 每次加载补一次快照。缺了这一步，曲线会停在最后一次 tick 的位置上，
	// 而页面看起来一切正常——这种「静默地少了一段」最难发现。
	if _, err := metrics.Roll(s.Store, today); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	page, err := view.Build(s.Store, today, goalID, settings)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, page)
}

// ---------------------------------------------------------------- 写

// added 是记一条的结果。**写库是同步做完的，AI 不在这儿等。**
type added struct {
	ID int64 `json:"id"`
	// NeedsAI 表示这条记录还有归属没定，回头叫 AI 补判一次。
	//
	// 两种情况都算：挂了目标但那个目标下有多条规则说不清，以及**压根没挂目标**。
	NeedsAI bool `json:"needsAi"`
}

func (s *Server) handleCheckin(w http.ResponseWriter, r *http.Request) {
	var in struct {
		GoalIDs []int64    `json:"goalIds"`
		Note    string     `json:"note"`
		Picks   [][2]int64 `json:"picks"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	// 挂到没有手工打卡规则的目标上是允许的——只记下「我本来想推进它」，
	// 不进那条曲线。所以这里只校验目标存在，不校验规则。
	for _, id := range in.GoalIDs {
		g, err := s.Store.GoalByID(id)
		if err != nil {
			fail(w, http.StatusInternalServerError, err)
			return
		}
		if g == nil {
			fail(w, http.StatusBadRequest, fmt.Errorf("目标 #%d 不存在", id))
			return
		}
	}

	cfg, err := ai.ConfigOf(s.Store)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	aiOn := cfg.Ready()

	// **归属先按机械规矩定，一次网络请求都不发。**
	// 先写下来（这一刻就能看到），再说它算哪条规则。
	links, err := s.Store.ResolveLinks(in.GoalIDs, in.Picks, aiOn)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}

	today, day, stamp := nowParts()
	clock := time.Now().Format("15:04")
	id, err := s.Store.CheckinAdd(links, day, clock, 1, in.Note, stamp)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	if _, err := metrics.Roll(s.Store, today); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}

	// 还有没有说不清的格子？两种都算，交给 classify_one 一并处理。
	needsAI := false
	if aiOn {
		if len(links) == 0 {
			// 一个目标都没选。规则是「第二步」，那就让 AI 去走它。
			goals, err := s.Store.GoalList(false)
			if err != nil {
				fail(w, http.StatusInternalServerError, err)
				return
			}
			needsAI = len(goals) > 0
		} else {
			for _, l := range links {
				if l.SourceID != nil {
					continue
				}
				srcs, err := s.Store.ManualSourcesOf(l.GoalID)
				if err != nil {
					fail(w, http.StatusInternalServerError, err)
					return
				}
				if len(srcs) > 1 {
					needsAI = true
				}
			}
		}
	}
	writeJSON(w, http.StatusOK, added{ID: id, NeedsAI: needsAI})
}

func (s *Server) handleClassify(w http.ResponseWriter, r *http.Request) {
	id, err := pathID(r)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	today, _, _ := nowParts()
	f, err := ai.ClassifyOne(s.Store, id, today)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	if f.Trouble != "" {
		fail(w, http.StatusBadGateway, errors.New(f.Trouble))
		return
	}
	if f.Decided == 0 {
		writeJSON(w, http.StatusOK, map[string]string{"message": "AI 没判出来。"})
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{"message": fmt.Sprintf("AI 归了 %d 条。", f.Decided)})
}

func (s *Server) handleBacklog(w http.ResponseWriter, r *http.Request) {
	var in struct {
		GoalID *int64 `json:"goalId"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	today, _, _ := nowParts()
	var f ai.Filled
	var err error
	if in.GoalID != nil && *in.GoalID > 0 {
		f, err = ai.ClassifyGoalBacklog(s.Store, *in.GoalID, today)
	} else {
		f, err = ai.ClassifyUnlinkedBacklog(s.Store, today)
	}
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	if f.Trouble != "" {
		fail(w, http.StatusBadGateway, errors.New(f.Trouble))
		return
	}
	msg := "AI 没判出来，这些先没归到规则。"
	if f.Decided > 0 {
		msg = fmt.Sprintf("AI 归了 %d 条。", f.Decided)
	}
	writeJSON(w, http.StatusOK, map[string]string{"message": msg})
}

// handleAddGoal 建目标，同时写下它的第一条判定规则。
//
// **目标和规则必须一起建成。** 没有规则的目标给不出「什么算推进它」，
// 它的曲线永远不会动——那不是目标，只是一句愿望。
func (s *Server) handleAddGoal(w http.ResponseWriter, r *http.Request) {
	var in struct {
		Title     string `json:"title"`
		Why       string `json:"why"`
		Rationale string `json:"rationale"`
		Kind      string `json:"kind"`
		Target    string `json:"target"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	title := strings.TrimSpace(in.Title)
	if title == "" {
		fail(w, http.StatusBadRequest, errors.New("目标得有个名字。"))
		return
	}
	kind, err := model.ParseKind(in.Kind)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	today, _, stamp := nowParts()
	color, err := s.Store.NextFreeColor()
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	id, err := s.Store.GoalAdd(title, strings.TrimSpace(in.Why), color, stamp)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if _, err := s.Store.SourceAdd(id, kind, strings.TrimSpace(in.Target), "{}",
		strings.TrimSpace(in.Rationale), stamp); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	// 规则刚落地就把归属补一遍：这条规则可能正好是某个目标下唯一能收手工记录的
	// 一条，那么它下面原来「没归到任何规则」的记录此刻就该归位。
	if err := s.Store.BackfillAttribution(); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	if _, err := metrics.Roll(s.Store, today); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	cur, err := metrics.ValueToday(s.Store, id, today)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"value": cur, "valueText": view.Num(cur)})
}

func (s *Server) handleUpdateGoal(w http.ResponseWriter, r *http.Request) {
	id, err := pathID(r)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	var in struct {
		Title string `json:"title"`
		Why   string `json:"why"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if err := s.Store.GoalUpdate(id, in.Title, in.Why); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"ok": true})
}

func (s *Server) handleArchiveGoal(w http.ResponseWriter, r *http.Request) {
	id, err := pathID(r)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	var in struct {
		Reason string `json:"reason"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	_, _, stamp := nowParts()
	if err := s.Store.GoalArchive(id, in.Reason, stamp); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"ok": true})
}

func (s *Server) handleDeleteGoal(w http.ResponseWriter, r *http.Request) {
	id, err := pathID(r)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if err := s.Store.GoalDelete(id); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"ok": true})
}

func (s *Server) handleAddSource(w http.ResponseWriter, r *http.Request) {
	goalID, err := pathID(r)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	var in struct {
		Kind      string `json:"kind"`
		Target    string `json:"target"`
		Rationale string `json:"rationale"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	kind, err := model.ParseKind(in.Kind)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if kind.NeedsTarget() && strings.TrimSpace(in.Target) == "" {
		fail(w, http.StatusBadRequest,
			errors.New("空着就等于「什么都算」——那条规则会把所有活动都装进来，曲线永远在涨"))
		return
	}
	today, _, stamp := nowParts()
	if _, err := s.Store.SourceAdd(goalID, kind, strings.TrimSpace(in.Target), "{}",
		strings.TrimSpace(in.Rationale), stamp); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if err := s.Store.BackfillAttribution(); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	if _, err := metrics.Roll(s.Store, today); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	cur, err := metrics.ValueToday(s.Store, goalID, today)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]any{"value": cur, "valueText": view.Num(cur)})
}

func (s *Server) handleDeleteSource(w http.ResponseWriter, r *http.Request) {
	id, err := pathID(r)
	if err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if err := s.Store.SourceDelete(id); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	// 删掉一条规则之后，原本归在它名下的记录会悬空；只剩唯一一条时自动接手。
	if err := s.Store.BackfillAttribution(); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	today, _, _ := nowParts()
	if _, err := metrics.Roll(s.Store, today); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"ok": true})
}

func (s *Server) handleSaveSettings(w http.ResponseWriter, r *http.Request) {
	var in struct {
		Enabled   bool    `json:"enabled"`
		BaseURL   string  `json:"baseUrl"`
		Model     string  `json:"model"`
		Threshold int     `json:"threshold"`
		APIKey    *string `json:"apiKey"`
	}
	if err := decode(r, &in); err != nil {
		fail(w, http.StatusBadRequest, err)
		return
	}
	if strings.TrimSpace(in.BaseURL) == "" {
		fail(w, http.StatusBadRequest,
			errors.New("接口地址不能空着——填不对就关掉 AI 判定，那时拿不准会问你。"))
		return
	}
	if strings.TrimSpace(in.Model) == "" {
		fail(w, http.StatusBadRequest, errors.New("模型名不能空着。"))
		return
	}
	if err := ai.Save(s.Store, in.Enabled, in.BaseURL, in.Model, in.Threshold, in.APIKey); err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]bool{"ok": true})
}

func (s *Server) handleTestAI(w http.ResponseWriter, r *http.Request) {
	msg, err := ai.Probe(s.Store)
	if err != nil {
		fail(w, http.StatusBadGateway, err)
		return
	}
	writeJSON(w, http.StatusOK, map[string]string{"message": msg})
}

func (s *Server) handleJSLog(w http.ResponseWriter, r *http.Request) {
	body, _ := readAll(r)
	s.logf("界面 · %s", strings.TrimSpace(string(body)))
	w.WriteHeader(http.StatusNoContent)
}

// handleState 是给 AI / 自动化验证用的一眼判断。
//
// 它不做任意 JS 执行——那种能力用 Playwright 的 page.evaluate 就有，
// 不需要在服务里常开一个口子。
func (s *Server) handleState(w http.ResponseWriter, r *http.Request) {
	today, _, _ := nowParts()
	page, err := view.Build(s.Store, today, 0, false)
	if err != nil {
		fail(w, http.StatusInternalServerError, err)
		return
	}
	type goalState struct {
		ID      int64  `json:"id"`
		Title   string `json:"title"`
		Current string `json:"current"`
		Delta   string `json:"delta"`
		Wired   bool   `json:"wired"`
		HasData bool   `json:"hasData"`
	}
	goals := make([]goalState, 0, len(page.Goals))
	for _, c := range page.Goals {
		goals = append(goals, goalState{
			ID: c.ID, Title: c.Title, Current: c.CurrentText,
			Delta: c.DeltaText, Wired: c.Curve.Kind != "unwired", HasData: !c.Zero && c.Curve.Kind == "line",
		})
	}
	writeJSON(w, http.StatusOK, map[string]any{
		"goals":    goals,
		"rows":     page.Counters.Rows,
		"unlinked": page.Counters.Unlinked,
		"aiReady":  page.Counters.AIReady,
		"warnings": page.Warnings,
		"today":    page.Today,
	})
}

// ---------------------------------------------------------------- 工具

func decode(r *http.Request, into any) error {
	defer r.Body.Close()
	if err := json.NewDecoder(io.LimitReader(r.Body, 1<<20)).Decode(into); err != nil {
		return fmt.Errorf("请求内容读不出来：%w", err)
	}
	return nil
}

func readAll(r *http.Request) ([]byte, error) {
	defer r.Body.Close()
	return io.ReadAll(io.LimitReader(r.Body, 1<<20))
}

func pathID(r *http.Request) (int64, error) {
	id, err := strconv.ParseInt(r.PathValue("id"), 10, 64)
	if err != nil {
		return 0, fmt.Errorf("id 不合法：%s", r.PathValue("id"))
	}
	return id, nil
}

func writeJSON(w http.ResponseWriter, status int, v any) {
	w.Header().Set("Content-Type", "application/json; charset=utf-8")
	w.Header().Set("Cache-Control", "no-store")
	w.WriteHeader(status)
	_ = json.NewEncoder(w).Encode(v)
}

func fail(w http.ResponseWriter, status int, err error) {
	writeJSON(w, status, map[string]string{"error": err.Error()})
}
