// Package ai 是 AI 判定：拿不准「这条记录算哪条规则」的时候，
// 让模型在**你自己写的那几条规则**里挑一条。
//
// # 它做什么，不做什么
//
// 只做一件事：从给定的候选里挑一条，或者回答「都不像」。
//
//   - **不能新增规则、不能改写规则。** 候选是调用方从库里取出来的，
//     模型的回答里出现候选之外的 id 一律作废（见 ParseReply）。
//   - **不能盖掉人已经说出口的话。** 人挑过的目标，ClassifyOne 直接跳过。
//   - **不自信就不答。** 置信度低于门槛时什么都不选，界面把气泡亮着让人自己点——
//     猜错就是一条假曲线，而假曲线正是这个产品要防的东西。
//
// # 顺序不许反过来
//
// 记录先落库（CheckinAdd 里一次网络请求都没有），AI 只负责事后补判。
// 一条已经发生的事不该因为别人的服务器慢而记不下来。
//
// # 密钥怎么存
//
// 密钥以 ChaCha20-Poly1305 加密后落库，口令编译在二进制里。**这不是安全边界，
// 是提高门槛**：能读库的人通常也能读 exe。它挡住的是按模式扫文件的窃取。
package ai

import (
	"bytes"
	"context"
	"crypto/rand"
	"encoding/hex"
	"encoding/json"
	"errors"
	"fmt"
	"io"
	"net/http"
	"strconv"
	"strings"
	"time"

	"golang.org/x/crypto/chacha20poly1305"

	"baseline/internal/metrics"
	"baseline/internal/store"
)

// DefaultBaseURL 是默认接口地址。
const DefaultBaseURL = "https://api.deepseek.com/v1"

// DefaultModel 是默认模型。
const DefaultModel = "deepseek-chat"

// DefaultThreshold 是默认置信度门槛（0–100）。定得偏高是故意的：这个数每降一点，
// 曲线里就多一分「AI 说的」，而它一旦落进快照就冻结了。
const DefaultThreshold = 80

// appSecret 编译在二进制里的加密口令。
var appSecret = []byte("baseline/local/ai-key/v1/0000000")

const (
	keyEnabled   = "ai.enabled"
	keyBaseURL   = "ai.base_url"
	keyModel     = "ai.model"
	keyAPIKey    = "ai.key"
	keyThreshold = "ai.threshold"
)

// timeout 是一个请求最多等多久。判定卡在网络上比判定失败更糟：
// 失败还能让人自己选，卡住则连「记一条」这件事都做不成。
const timeout = 20 * time.Second

// Config 是 AI 配置。
type Config struct {
	Enabled bool
	BaseURL string
	Model   string
	// APIKey 是明文，只在内存里。落库的是密文。
	APIKey    string
	Threshold int
	// KeyBroken 表示库里那把密钥解不开（换了机器、或者密文被改过）。
	KeyBroken bool
}

// Ready 表示现在能不能真的发请求。
func (c Config) Ready() bool {
	return c.Enabled &&
		strings.TrimSpace(c.APIKey) != "" &&
		strings.TrimSpace(c.BaseURL) != "" &&
		strings.TrimSpace(c.Model) != ""
}

// MaskedKey 是给界面看的密钥掩码。**明文绝不出内核**——页面是 HTML，
// 会被截图、会被复制。
func (c Config) MaskedKey() string { return Mask(c.APIKey) }

// Mask 保留最后四位，前面一律圆点。
func Mask(key string) string {
	k := strings.TrimSpace(key)
	if k == "" {
		return ""
	}
	runes := []rune(k)
	n := len(runes)
	if n <= 4 {
		return strings.Repeat("•", max(n, 4))
	}
	return "••••••••" + string(runes[n-4:])
}

func max(a, b int) int {
	if a > b {
		return a
	}
	return b
}

// Config 读配置。
func ConfigOf(s *store.Store) (Config, error) {
	get := func(k string) (string, bool, error) {
		v, err := s.SettingGet(k)
		if err != nil || v == nil {
			return "", false, err
		}
		return *v, true, nil
	}
	text := func(k, def string) (string, error) {
		v, ok, err := get(k)
		if err != nil {
			return "", err
		}
		if !ok {
			return def, nil
		}
		return strings.TrimSpace(v), nil
	}

	cfg := Config{}
	var broken bool
	if blob, ok, err := get(keyAPIKey); err != nil {
		return Config{}, err
	} else if ok && strings.TrimSpace(blob) != "" {
		plain, err := Open(blob)
		if err != nil {
			// 解不开就当没配：**不能因此让「记一条」失败**。
			cfg.APIKey = ""
			broken = true
		} else {
			cfg.APIKey = plain
		}
	}
	cfg.KeyBroken = broken

	if v, ok, err := get(keyEnabled); err != nil {
		return Config{}, err
	} else if ok {
		cfg.Enabled = v != "0"
	} else {
		cfg.Enabled = true
	}

	var err error
	if cfg.BaseURL, err = text(keyBaseURL, DefaultBaseURL); err != nil {
		return Config{}, err
	}
	if cfg.Model, err = text(keyModel, DefaultModel); err != nil {
		return Config{}, err
	}
	cfg.Threshold = DefaultThreshold
	if v, ok, err := get(keyThreshold); err != nil {
		return Config{}, err
	} else if ok {
		if n, err := strconv.Atoi(strings.TrimSpace(v)); err == nil {
			cfg.Threshold = min(max(n, 0), 100)
		}
	}
	return cfg, nil
}

// Save 保存设置。apiKey 为 nil 表示不动原来那把；指向空串表示清掉。
func Save(s *store.Store, enabled bool, baseURL, modelName string, threshold int, apiKey *string) error {
	en := "0"
	if enabled {
		en = "1"
	}
	if err := s.SettingSet(keyEnabled, en); err != nil {
		return err
	}
	if err := s.SettingSet(keyBaseURL, strings.TrimSpace(baseURL)); err != nil {
		return err
	}
	if err := s.SettingSet(keyModel, strings.TrimSpace(modelName)); err != nil {
		return err
	}
	if err := s.SettingSet(keyThreshold, strconv.Itoa(min(max(threshold, 0), 100))); err != nil {
		return err
	}
	if apiKey != nil {
		key := strings.TrimSpace(*apiKey)
		if key == "" {
			return s.SettingDel(keyAPIKey)
		}
		blob, err := Seal(key)
		if err != nil {
			return err
		}
		return s.SettingSet(keyAPIKey, blob)
	}
	return nil
}

// ---------------------------------------------------------------- 密钥加解密

// Seal 的密文格式：hex(nonce ‖ ciphertext)。nonce 每次重新随机——
// 同一个口令下重用 nonce 会让两把密钥的异或泄露出去。
func Seal(plain string) (string, error) {
	cipher, err := chacha20poly1305.New(appSecret)
	if err != nil {
		return "", err
	}
	nonce := make([]byte, chacha20poly1305.NonceSize)
	if _, err := io.ReadFull(rand.Reader, nonce); err != nil {
		return "", fmt.Errorf("取随机数失败：%w", err)
	}
	ct := cipher.Seal(nil, nonce, []byte(plain), nil)
	return hex.EncodeToString(append(nonce, ct...)), nil
}

// Open 解开 Seal 的密文。
func Open(blob string) (string, error) {
	raw, err := hex.DecodeString(strings.TrimSpace(blob))
	if err != nil {
		return "", errors.New("密文不是合法的十六进制")
	}
	if len(raw) < chacha20poly1305.NonceSize+chacha20poly1305.Overhead {
		return "", errors.New("密文太短")
	}
	cipher, err := chacha20poly1305.New(appSecret)
	if err != nil {
		return "", err
	}
	nonce, ct := raw[:chacha20poly1305.NonceSize], raw[chacha20poly1305.NonceSize:]
	pt, err := cipher.Open(nil, nonce, ct, nil)
	if err != nil {
		return "", errors.New("密钥解不开（换过机器，或者密文被改过）")
	}
	return string(pt), nil
}

// ---------------------------------------------------------------- 判定

// Candidate 是一条候选规则。What 是人写的那句话本身，原样交给模型。
type Candidate struct {
	ID   int64
	What string
}

// Pick 是模型的回答，**已经按候选核对过**。
type Pick struct {
	// SourceID 是挑中的规则。nil = 都不像，或者它给的 id 不在候选里。
	SourceID *int64
	// Confidence 是 0–100。SourceID 为 nil 时恒为 0。
	Confidence int
}

// Filled 是一次补判的结果。
type Filled struct {
	// Decided 是补上了几条归属。
	Decided int
	// Trouble 是出问题时的原话（发请求失败之类）。
	Trouble string
}

// ClassifyOne 给**一条**记录补上归属。
//
// 两种情况靠同一个入口：
//
//   - 挂到某个目标上了，但没归到规则——在那个目标的规则里挑一条；
//   - 压根没挂目标——**在所有目标的规则里挑一条，目标也一并定下来**。
//
// 判得准才写；写的是 source_id IS NULL 的格子，或者一条还不存在的关联。
// 人挑过的、唯一能确定的那条，它碰不到。
func ClassifyOne(s *store.Store, checkinID int64, today time.Time) (Filled, error) {
	var out Filled
	cfg, err := ConfigOf(s)
	if err != nil {
		return out, err
	}
	if !cfg.Ready() {
		// 没配 AI 不是错误：那些记录本来就等着人指认。
		return out, nil
	}

	links, err := s.LinksOf(checkinID)
	if err != nil {
		return out, err
	}

	var cands []Candidate
	owner := map[int64]int64{}
	if len(links) == 0 {
		// 没挂目标：候选是全库的手工规则，标签带上目标名——
		// 不然「读完一章」这种规则名在几个目标下都长得一样，没法区分。
		cands, owner, err = allRules(s)
		if err != nil {
			return out, err
		}
	} else {
		// 挂了目标：只在这个目标自己的规则里挑。一个目标一条规则的时候唯一确定，
		// 轮不到模型；一条都没有时它没有资格说话（那是「我本来想推进它」）。
		for _, l := range links {
			if l.SourceID != nil {
				continue // 这一格已经有人定了
			}
			srcs, err := s.ManualSourcesOf(l.GoalID)
			if err != nil {
				return out, err
			}
			if len(srcs) < 2 {
				continue
			}
			for _, src := range srcs {
				cands = append(cands, Candidate{ID: src.ID, What: src.Summary()})
				owner[src.ID] = l.GoalID
			}
		}
	}
	if len(cands) == 0 {
		return out, nil
	}

	note, err := s.CheckinNote(checkinID)
	if err != nil {
		return out, err
	}
	pick, err := classify(&cfg, note, cands)
	if err != nil {
		out.Trouble = fmt.Sprintf("AI 判定失败：%v", err)
		return out, nil
	}
	if pick.SourceID != nil && pick.Confidence >= cfg.Threshold {
		if gid, ok := owner[*pick.SourceID]; ok {
			if err := s.Attribute(checkinID, gid, *pick.SourceID, true); err != nil {
				return out, err
			}
			out.Decided++
		}
	}

	// 补上归属之后必须重算快照：数字和曲线都读它。
	// 今天的那一格本来就每次重算，所以这条记录当场就会进曲线。
	if out.Decided > 0 {
		if _, err := metrics.Roll(s, today); err != nil {
			return out, err
		}
	}
	return out, nil
}

// allRules 是全库的手工规则，标签是「目标名 / 规则名」。
func allRules(s *store.Store) ([]Candidate, map[int64]int64, error) {
	var cands []Candidate
	owner := map[int64]int64{}
	goals, err := s.GoalList(false)
	if err != nil {
		return nil, nil, err
	}
	for _, g := range goals {
		srcs, err := s.ManualSourcesOf(g.ID)
		if err != nil {
			return nil, nil, err
		}
		for _, src := range srcs {
			cands = append(cands, Candidate{ID: src.ID, What: g.Title + " / " + src.Summary()})
			owner[src.ID] = g.ID
		}
	}
	return cands, owner, nil
}

// ClassifyGoalBacklog 是某个目标下挂空着的记录，挨条补判。
func ClassifyGoalBacklog(s *store.Store, goalID int64, today time.Time) (Filled, error) {
	var out Filled
	refs, err := s.UnattributedOf(goalID)
	if err != nil {
		return out, err
	}
	for _, r := range refs {
		one, err := ClassifyOne(s, r.ID, today)
		if err != nil {
			return out, err
		}
		merge(&out, one)
	}
	return out, nil
}

// ClassifyUnlinkedBacklog 是没挂到任何目标上的记录，挨条补判。
func ClassifyUnlinkedBacklog(s *store.Store, today time.Time) (Filled, error) {
	var out Filled
	refs, err := s.UnlinkedCheckins()
	if err != nil {
		return out, err
	}
	for _, r := range refs {
		one, err := ClassifyOne(s, r.ID, today)
		if err != nil {
			return out, err
		}
		merge(&out, one)
	}
	return out, nil
}

func merge(into *Filled, one Filled) {
	into.Decided += one.Decided
	if into.Trouble == "" {
		into.Trouble = one.Trouble
	}
}

// classify 把一件事和几条规则交给模型，问它落在哪一条里。
func classify(cfg *Config, note string, cands []Candidate) (Pick, error) {
	if len(cands) == 0 {
		return Pick{}, nil
	}
	url := strings.TrimRight(strings.TrimSpace(cfg.BaseURL), "/") + "/chat/completions"
	body, err := json.Marshal(BuildRequest(*cfg, note, cands))
	if err != nil {
		return Pick{}, err
	}

	ctx, cancel := context.WithTimeout(context.Background(), timeout)
	defer cancel()
	req, err := http.NewRequestWithContext(ctx, http.MethodPost, url, bytes.NewReader(body))
	if err != nil {
		return Pick{}, fmt.Errorf("请求发送失败：%w", err)
	}
	req.Header.Set("Authorization", "Bearer "+strings.TrimSpace(cfg.APIKey))
	req.Header.Set("Content-Type", "application/json")

	resp, err := http.DefaultClient.Do(req)
	if err != nil {
		if errors.Is(err, context.DeadlineExceeded) {
			return Pick{}, fmt.Errorf("请求超时（超过 %d 秒）。", int(timeout.Seconds()))
		}
		return Pick{}, fmt.Errorf("请求发送失败：%w", err)
	}
	defer resp.Body.Close()

	if resp.StatusCode != http.StatusOK {
		switch resp.StatusCode {
		case http.StatusUnauthorized, http.StatusForbidden:
			return Pick{}, fmt.Errorf("密钥无效（HTTP %d），请在设置中检查 API 密钥。", resp.StatusCode)
		case http.StatusNotFound:
			return Pick{}, errors.New("接口不存在（HTTP 404），请检查接口地址。")
		case http.StatusTooManyRequests:
			return Pick{}, errors.New("请求过于频繁（HTTP 429），请稍后重试。")
		default:
			return Pick{}, fmt.Errorf("服务返回 HTTP %d。", resp.StatusCode)
		}
	}

	raw, err := io.ReadAll(io.LimitReader(resp.Body, 1<<20))
	if err != nil {
		return Pick{}, fmt.Errorf("读取响应失败：%w", err)
	}
	return ParseReply(raw, cands)
}

// BuildRequest 是请求体。
//
// **没有用 response_format**：它虽然能逼出 JSON，但不是所有 OpenAI 兼容服务都实现了，
// 用了就会在那些服务上直接 400。与其挑服务，不如把提示词写清楚、把解析写宽容。
func BuildRequest(cfg Config, note string, cands []Candidate) map[string]any {
	list := make([]string, 0, len(cands))
	for _, c := range cands {
		list = append(list, fmt.Sprintf("- id = %d ： %s", c.ID, c.What))
	}
	return map[string]any{
		"model": cfg.Model,
		// 这是归类，不是创作。温度调 0，让同一个输入尽量给出同一个答案。
		"temperature": 0,
		"messages": []map[string]string{
			{"role": "system", "content": systemPrompt},
			{"role": "user", "content": fmt.Sprintf(
				"刚做完的事：\n%s\n\n候选规则（只能从这里挑）：\n%s",
				strings.TrimSpace(note), strings.Join(list, "\n"),
			)},
		},
	}
}

const systemPrompt = `你在帮一个人把他刚做的事归到他自己的判定规则里。这些规则是他写的，用来决定哪条曲线会动。

规矩：
1. 只能从候选里挑一条，或者回答 null。不能新增规则，不能改写规则，不能挑候选外的 id。
2. 判断标准只有一条：这件事是否落在那条规则写明的范围里。不要联想、不要类推。
3. 只要没有任何一条明确匹配，就回答 null。**宁可说不知道，也不要挑一个最像的**——
   挑错会让他以为那条线动了，而他做这个工具就是为了不被骗。
4. 只输出 JSON，不要解释，不要代码块：
{"id": 候选里的 id 或 null, "confidence": 0 到 100 的整数}

confidence 是你对这次判断的把握。不确定就写低一点，写低了不会被采用。`

type chatReply struct {
	Choices []struct {
		Message struct {
			Content string `json:"content"`
		} `json:"message"`
	} `json:"choices"`
}

// ParseReply 解析模型的回答。
//
// **核对是这里做的**：模型可能编一个没给它的 id、可能把 JSON 包在代码块里、
// 可能干脆回一段话。这三种都不能变成一条归属。
func ParseReply(body []byte, cands []Candidate) (Pick, error) {
	var reply chatReply
	if err := json.Unmarshal(body, &reply); err != nil {
		return Pick{}, fmt.Errorf("服务返回内容不是 JSON：%s", brief(string(body)))
	}
	if len(reply.Choices) == 0 || reply.Choices[0].Message.Content == "" {
		return Pick{}, errors.New("响应缺少 choices[0].message.content。")
	}
	content := reply.Choices[0].Message.Content

	raw, ok := extractJSON(content)
	if !ok {
		return Pick{}, fmt.Errorf("响应中未找到 JSON：%s", brief(content))
	}
	var v struct {
		ID         *int64 `json:"id"`
		Confidence *int   `json:"confidence"`
	}
	if err := json.Unmarshal([]byte(raw), &v); err != nil {
		return Pick{}, fmt.Errorf("响应不是合法 JSON（%v）：%s", err, brief(content))
	}

	conf := 0
	if v.Confidence != nil {
		conf = min(max(*v.Confidence, 0), 100)
	}

	// 候选之外的 id 一律作废。这一条比什么都重要：归属是写进库、并且会冻住的东西。
	var sourceID *int64
	if v.ID != nil {
		for _, c := range cands {
			if c.ID == *v.ID {
				id := c.ID
				sourceID = &id
				break
			}
		}
	}
	pick := Pick{SourceID: sourceID}
	if sourceID != nil {
		pick.Confidence = conf
	}
	return pick, nil
}

// extractJSON 从一段可能带代码块、带前言后语的文本里抠出那个 JSON 对象。
func extractJSON(s string) (string, bool) {
	start := strings.Index(s, "{")
	end := strings.LastIndex(s, "}")
	if start < 0 || end <= start {
		return "", false
	}
	return s[start : end+1], true
}

func brief(s string) string {
	t := strings.TrimSpace(strings.ReplaceAll(s, "\n", " "))
	runes := []rune(t)
	if len(runes) > 120 {
		return string(runes[:120]) + "…"
	}
	return t
}

// Probe 是设置页上那个「测试连接」。它要回答的是「配好没有」，所以真发一个最小的
// 请求，并且把**用的是哪个模型**报回来——配错了模型是最常见的一种「看起来配好了」。
func Probe(s *store.Store) (string, error) {
	cfg, err := ConfigOf(s)
	if err != nil {
		return "", err
	}
	if cfg.KeyBroken {
		return "", errors.New("密钥读取失败，请重新填写。")
	}
	if !cfg.Ready() {
		return "", errors.New("尚未填写 API 密钥。")
	}
	cands := []Candidate{
		{ID: 1, What: "读完一章书"},
		{ID: 2, What: "做完一章习题"},
	}
	start := time.Now()
	pick, err := classify(&cfg, "把第 3 章的习题做完了", cands)
	if err != nil {
		return "", err
	}
	ms := time.Since(start).Milliseconds()
	if pick.SourceID == nil {
		// 说「不知道」是规矩允许的答案，不是失败——这一条也要讲清楚，
		// 否则第一次用的人会以为配坏了。
		return fmt.Sprintf("连接正常。模型：%s，耗时 %d 毫秒。", cfg.Model, ms), nil
	}
	what := "读完一章书"
	if *pick.SourceID == 2 {
		what = "做完一章习题"
	}
	return fmt.Sprintf(
		"连接正常。模型：%s，耗时 %d 毫秒。测试记录「把第 3 章的习题做完了」被归入「%s」，置信度 %d。",
		cfg.Model, ms, what, pick.Confidence,
	), nil
}
