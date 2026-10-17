package ai

import (
	"encoding/json"
	"strings"
	"testing"
	"time"

	"baseline/internal/metrics"
	"baseline/internal/model"
	"baseline/internal/store"
)

const today = "2026-10-07"

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

func cands() []Candidate {
	return []Candidate{
		{ID: 7, What: "读完一章"},
		{ID: 9, What: "做完一章题"},
	}
}

func reply(content string) []byte {
	b, _ := json.Marshal(map[string]any{
		"choices": []map[string]any{
			{"message": map[string]string{"content": content}},
		},
	})
	return b
}

func Test挑中的规则必须在候选里(t *testing.T) {
	p, err := ParseReply(reply(`{"id": 9, "confidence": 90}`), cands())
	if err != nil {
		t.Fatal(err)
	}
	if p.SourceID == nil || *p.SourceID != 9 || p.Confidence != 90 {
		t.Fatalf("得到 %+v", p)
	}
}

// 模型编一个没给它的 id 出来 —— 这是最危险的一种回复：
// 归到一条不存在的（或者别人家的）规则上，曲线会动，而且没人看得出为什么。
func Test候选之外的_id_一律作废(t *testing.T) {
	for _, bad := range []string{`{"id": 1, "confidence": 99}`, `{"id": 0, "confidence": 99}`} {
		p, err := ParseReply(reply(bad), cands())
		if err != nil {
			t.Fatal(err)
		}
		if p.SourceID != nil {
			t.Fatalf("不该接受候选外的 id：%s", bad)
		}
		if p.Confidence != 0 {
			t.Fatalf("没挑中的时候置信度必须归零：%d", p.Confidence)
		}
	}
}

func Test说不知道是合法回答(t *testing.T) {
	p, err := ParseReply(reply(`{"id": null, "confidence": 20}`), cands())
	if err != nil {
		t.Fatal(err)
	}
	if p.SourceID != nil || p.Confidence != 0 {
		t.Fatalf("得到 %+v", p)
	}
}

func Test代码块包着的_json_也认(t *testing.T) {
	p, err := ParseReply(reply("```json\n{\"id\": 7, \"confidence\": 85}\n```"), cands())
	if err != nil {
		t.Fatal(err)
	}
	if p.SourceID == nil || *p.SourceID != 7 {
		t.Fatalf("得到 %+v", p)
	}
}

func Test前言后语包着的_json_也认(t *testing.T) {
	p, err := ParseReply(reply(`我觉得是这样：{"id": 7, "confidence": 88} 就这样。`), cands())
	if err != nil {
		t.Fatal(err)
	}
	if p.SourceID == nil || *p.SourceID != 7 {
		t.Fatalf("得到 %+v", p)
	}
}

func Test置信度会被夹到合法区间(t *testing.T) {
	p, _ := ParseReply(reply(`{"id": 7, "confidence": 555}`), cands())
	if p.Confidence != 100 {
		t.Fatalf("得到 %d", p.Confidence)
	}
	p, _ = ParseReply(reply(`{"id": 7, "confidence": -3}`), cands())
	if p.Confidence != 0 {
		t.Fatalf("得到 %d", p.Confidence)
	}
	// 没写 confidence 就当没有把握。
	p, _ = ParseReply(reply(`{"id": 7}`), cands())
	if p.Confidence != 0 {
		t.Fatalf("得到 %d", p.Confidence)
	}
}

func Test回一段大白话就是失败而不是归属(t *testing.T) {
	if _, err := ParseReply(reply("这条记录看起来像是做完了一章题"), cands()); err == nil {
		t.Fatal("没有 JSON 就该报错")
	}
	if _, err := ParseReply([]byte(`{"error": "boom"}`), cands()); err == nil {
		t.Fatal("连 content 都没有，也该报错")
	}
}

// ------------------------------------------------------------ 密钥

func Test密钥加密后能解回来并且不是明文(t *testing.T) {
	plain := "sk-abcdef1234567890"
	blob, err := Seal(plain)
	if err != nil {
		t.Fatal(err)
	}
	if strings.Contains(blob, "sk-") || strings.Contains(blob, "abcdef") {
		t.Fatalf("密文里不该出现明文：%s", blob)
	}
	got, err := Open(blob)
	if err != nil {
		t.Fatal(err)
	}
	if got != plain {
		t.Fatalf("得到 %q", got)
	}
}

func Test同一个密钥两次加密结果不同(t *testing.T) {
	a, err := Seal("sk-same")
	if err != nil {
		t.Fatal(err)
	}
	b, _ := Seal("sk-same")
	if a == b {
		t.Fatal("nonce 必须每次重新随机，否则两条密文的异或就把明文泄出去了")
	}
}

func Test密文被改过就解不开(t *testing.T) {
	blob, err := Seal("sk-abcdef1234567890")
	if err != nil {
		t.Fatal(err)
	}
	last := blob[len(blob)-1]
	repl := byte('a')
	if last == 'a' {
		repl = 'b'
	}
	if _, err := Open(blob[:len(blob)-1] + string(repl)); err == nil {
		t.Fatal("改了密文还能解开，说明没有完整性校验")
	}
	if _, err := Open("这不是十六进制"); err == nil {
		t.Fatal("非十六进制该报错")
	}
	if _, err := Open("00"); err == nil {
		t.Fatal("太短该报错")
	}
}

func Test掩码不泄露密钥本身(t *testing.T) {
	if Mask("") != "" {
		t.Fatal("空密钥的掩码应该是空")
	}
	m := Mask("sk-abcdef1234567890")
	if strings.Contains(m, "abcdef") {
		t.Fatalf("掩码里不能有中间那段：%s", m)
	}
	if !strings.HasSuffix(m, "7890") {
		t.Fatalf("留最后四位是为了让人认出是哪一把：%s", m)
	}
	if Mask("abc") != "••••" {
		t.Fatalf("得到 %q", Mask("abc"))
	}
}

// ------------------------------------------------------------ 提示词

func Test提示词里带着候选的_id_和原话(t *testing.T) {
	cfg := Config{
		Enabled: true, BaseURL: "https://example.invalid/v1", Model: "m",
		APIKey: "k", Threshold: 80,
	}
	req := BuildRequest(cfg, "做完了第 3 章习题", cands())
	msgs := req["messages"].([]map[string]string)
	user := msgs[1]["content"]
	for _, want := range []string{"id = 7", "做完一章题", "做完了第 3 章习题"} {
		if !strings.Contains(user, want) {
			t.Fatalf("提示词里少了 %q：%s", want, user)
		}
	}
	if req["temperature"] != 0 {
		t.Fatal("温度必须固定 0：同一个输入要尽量给同一个答案")
	}
}

func Test设置能存能读且密钥不明文落库(t *testing.T) {
	s := newStore(t)
	if err := Save(s, true, "https://api.deepseek.com/v1", "deepseek-chat", 85, ptr("sk-secret-1234")); err != nil {
		t.Fatal(err)
	}

	// 落库的那一份必须是密文。
	raw, err := s.SettingGet(keyAPIKey)
	if err != nil || raw == nil {
		t.Fatal(err)
	}
	if strings.Contains(*raw, "sk-secret") {
		t.Fatalf("库里存了明文：%s", *raw)
	}

	cfg, err := ConfigOf(s)
	if err != nil {
		t.Fatal(err)
	}
	if !cfg.Ready() || cfg.APIKey != "sk-secret-1234" || cfg.Threshold != 85 ||
		cfg.Model != "deepseek-chat" || cfg.KeyBroken {
		t.Fatalf("得到 %+v", cfg)
	}

	// nil = 不动原来那把。
	if err := Save(s, false, "", "", 50, nil); err != nil {
		t.Fatal(err)
	}
	cfg, _ = ConfigOf(s)
	if cfg.APIKey != "sk-secret-1234" || cfg.Enabled {
		t.Fatalf("得到 %+v", cfg)
	}

	// 指向空串 = 清掉。
	if err := Save(s, true, DefaultBaseURL, DefaultModel, 80, ptr("  ")); err != nil {
		t.Fatal(err)
	}
	cfg, _ = ConfigOf(s)
	if cfg.APIKey != "" || cfg.Ready() {
		t.Fatalf("得到 %+v", cfg)
	}
}

func ptr(s string) *string { return &s }

// ------------------------------------------------------------ 补判顺序

func Test没配密钥时什么都不做也不算错(t *testing.T) {
	s := newStore(t)
	g, _ := twoRuleGoal(t, s)
	id := looseRecord(t, s, []int64{g}, "读了点东西")

	f, err := ClassifyOne(s, id, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if f.Decided != 0 || f.Trouble != "" {
		t.Fatalf("没配 AI 不算「出问题」：%+v", f)
	}
	if n, _ := s.UnattributedCount(g); n != 1 {
		t.Fatalf("还是那条待办，得到 %d", n)
	}
}

func Test全库没有手工规则时不发请求(t *testing.T) {
	s := newStore(t)
	if _, err := s.GoalAdd("英语", "", "blue", today); err != nil {
		t.Fatal(err)
	}
	id, err := s.CheckinAdd(nil, today, "10:00", 1, "随便记一句", today)
	if err != nil {
		t.Fatal(err)
	}
	// 指向一个连不上的地址：真发了请求就会留下 trouble。
	if err := Save(s, true, "http://127.0.0.1:1/v1", "m", 80, ptr("k")); err != nil {
		t.Fatal(err)
	}
	f, err := ClassifyOne(s, id, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if f.Decided != 0 || f.Trouble != "" {
		t.Fatalf("没有候选就不该发请求：%+v", f)
	}
}

// 没关联目标的记录，哪怕全库只有一条手工规则，**也要问模型**。
//
// 这和「挂了目标、目标下只有一条规则」不是一回事：那时候规则是唯一确定的；
// 这里要判的是「这件事到底算不算那条规则」——而「都不像」只有模型能回答。
func Test没关联目标时哪怕只有一条候选也要问(t *testing.T) {
	s := newStore(t)
	g, err := s.GoalAdd("英语", "", "blue", today)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.SourceAdd(g, model.KindManualCheckin, "复习", "{}", "复习一轮算一次", today); err != nil {
		t.Fatal(err)
	}
	id, err := s.CheckinAdd(nil, today, "10:00", 1, "写了点代码", today)
	if err != nil {
		t.Fatal(err)
	}
	if err := Save(s, true, "http://127.0.0.1:1/v1", "m", 80, ptr("k")); err != nil {
		t.Fatal(err)
	}
	f, err := ClassifyOne(s, id, d(t, today))
	if err != nil {
		t.Fatal(err)
	}
	if f.Decided != 0 || f.Trouble == "" {
		t.Fatalf("这一条必须问模型，不该被本地规则挡掉：%+v", f)
	}
}

func Test补判只填挂空的那一格(t *testing.T) {
	s := newStore(t)
	g, s1 := twoRuleGoal(t, s)
	links, err := s.ResolveLinks([]int64{g}, [][2]int64{{g, s1}}, false)
	if err != nil {
		t.Fatal(err)
	}
	id, err := s.CheckinAdd(links, today, "10:00", 1, "复习第 3 课", today)
	if err != nil {
		t.Fatal(err)
	}

	// 指向一个连不上的地址：真去补判会失败，于是「有没有动过原来那条」才验得出来。
	if err := Save(s, true, "http://127.0.0.1:1/v1", "m", 80, ptr("k")); err != nil {
		t.Fatal(err)
	}
	if _, err := ClassifyOne(s, id, d(t, today)); err != nil {
		t.Fatal(err)
	}

	if v, _ := s.SourceCheckinValue(s1, today); v != 1 {
		t.Fatalf("人挑的那条不能被改写：%v", v)
	}
	if refs, _ := s.UnattributedOf(g); len(refs) != 0 {
		t.Fatal("本来就没人挂空")
	}
}

// ------------------------------------------------------------ 夹具

func twoRuleGoal(t *testing.T, s *store.Store) (int64, int64) {
	t.Helper()
	g, err := s.GoalAdd("计算机基础", "", "blue", today)
	if err != nil {
		t.Fatal(err)
	}
	s1, err := s.SourceAdd(g, model.KindManualCheckin, "读完一章", "{}", "读完一章", today)
	if err != nil {
		t.Fatal(err)
	}
	if _, err := s.SourceAdd(g, model.KindManualCheckin, "做完一章题", "{}", "做完一章题", today); err != nil {
		t.Fatal(err)
	}
	return g, s1
}

// looseRecord 是一条「挂了目标但没归到规则」的记录 —— 说不清时 defer = true 的产物。
func looseRecord(t *testing.T, s *store.Store, goals []int64, note string) int64 {
	t.Helper()
	links, err := s.ResolveLinks(goals, nil, true)
	if err != nil {
		t.Fatal(err)
	}
	id, err := s.CheckinAdd(links, today, "10:00", 1, note, today)
	if err != nil {
		t.Fatal(err)
	}
	return id
}
