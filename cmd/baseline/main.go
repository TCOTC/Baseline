// 基线 / Baseline —— 和之前的你对比
//
// 命令行界面。内核在 internal/ 下，HTTP 服务和 Electron 外壳用的是同一份。
// 这里做的是内核给不了的事：把「什么算推进它」这类判断讲成人话，
// 以及在缺规则、缺数据的时候直接拒绝，而不是默默画一条平线。
package main

import (
	"errors"
	"fmt"
	"os"
	"strings"
	"time"

	"github.com/spf13/cobra"

	"baseline/internal/ai"
	"baseline/internal/metrics"
	"baseline/internal/model"
	"baseline/internal/server"
	"baseline/internal/store"
	"baseline/internal/view"
)

var dbFlag string

func main() {
	if err := rootCmd().Execute(); err != nil {
		fmt.Fprintln(os.Stderr, err)
		os.Exit(1)
	}
}

func rootCmd() *cobra.Command {
	root := &cobra.Command{
		Use:   "baseline",
		Short: "基线 —— 和之前的你对比",
		Long:  "为每个目标定义「什么算推进它」，把活动归属到目标，累加成一条可以对比过去的曲线。",
	}
	root.PersistentFlags().StringVar(&dbFlag, "db", "", "数据库路径。默认 %APPDATA%\\Baseline\\baseline.db")
	root.AddCommand(
		&cobra.Command{Use: "init", Short: "初始化数据库（幂等）", RunE: runInit},
		&cobra.Command{Use: "tick", Short: "补齐每日快照（幂等；已写入的日期不会改写）", RunE: runTick},
		&cobra.Command{Use: "status", Short: "各目标的当前状态（终端速览）", RunE: runStatus},
		goalCmd(),
		sourceCmd(),
		checkinCmd(),
		aiCmd(),
		serveCmd(),
	)
	return root
}

func openStore() (*store.Store, error) {
	path := dbFlag
	if path == "" {
		path = store.DefaultPath()
	}
	return store.Open(path)
}

func dbPath() string {
	if dbFlag != "" {
		return dbFlag
	}
	return store.DefaultPath()
}

func nowParts() (time.Time, string, string) {
	now := time.Now()
	return metrics.DayStart(now), now.Format("2006-01-02"), now.Format("2006-01-02 15:04:05")
}

func runInit(*cobra.Command, []string) error {
	s, err := openStore()
	if err != nil {
		return err
	}
	defer s.Close()
	fmt.Printf("数据库就绪：%s\n", dbPath())
	fmt.Println("（桌面窗口和网页读的是同一个库）")
	return nil
}

func runTick(*cobra.Command, []string) error {
	s, err := openStore()
	if err != nil {
		return err
	}
	defer s.Close()
	today, _, _ := nowParts()
	n, err := metrics.Roll(s, today)
	if err != nil {
		return err
	}
	fmt.Printf("快照补齐完成，新写入 %d 条（已存在的日期未改写）\n", n)
	return nil
}

func runStatus(*cobra.Command, []string) error {
	s, err := openStore()
	if err != nil {
		return err
	}
	defer s.Close()
	today, _, _ := nowParts()
	// 先补快照：卡片和 status 现在都读快照（数字与曲线同源），
	// 不补的话终端显示的数会比窗口旧一天。
	if _, err := metrics.Roll(s, today); err != nil {
		return err
	}
	goals, err := s.GoalList(false)
	if err != nil {
		return err
	}
	if len(goals) == 0 {
		fmt.Println("（没有目标）")
	}
	var total float64
	for _, g := range goals {
		series, err := metrics.Series(s, g.ID, today)
		if err != nil {
			return err
		}
		total += series.DeltaWeek
		d := view.Num(series.DeltaWeek)
		if series.DeltaWeek > 0 {
			d = "+" + d
		}
		// 「规则没接线」和「接上了但没动」在这里也必须分开说：
		// 后者是事实，前者是工具还没做完。
		note := ""
		switch {
		case !series.Wired:
			note = fmt.Sprintf("（%s还没接入）", strings.Join(series.Unwired, "、"))
		case !series.HasData:
			note = "（还没有任何记录）"
		}
		fmt.Printf("%-18s 当前 %-6s 本周 %-6s %s\n",
			g.Title, view.Num(series.Current), d, note)
	}
	fmt.Println(strings.Repeat("-", 44))
	fmt.Printf("本周总位移 %s\n", view.Num(total))
	return nil
}

// ---------------------------------------------------------------- goal

func goalCmd() *cobra.Command {
	goal := &cobra.Command{Use: "goal", Short: "目标"}
	var why, color string
	add := &cobra.Command{
		Use:   "add <名>",
		Short: "新建目标",
		Args:  cobra.ExactArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			_, _, stamp := nowParts()
			if color == "" {
				color, err = s.NextFreeColor()
				if err != nil {
					return err
				}
			}
			id, err := s.GoalAdd(args[0], why, color, stamp)
			if err != nil {
				return err
			}
			fmt.Printf("已创建目标 #%d 「%s」（色板 %s）\n\n", id, strings.TrimSpace(args[0]), color)
			fmt.Println("下一步：给它写清「什么算推进它」。没有判定规则的目标，曲线永远不会动——")
			fmt.Printf("  baseline source add %q --kind manual_checkin --rationale \"读完一章算一次\"\n", args[0])
			return nil
		},
	}
	add.Flags().StringVar(&why, "why", "", "为什么想做")
	add.Flags().StringVar(&color, "color", "", "色板：blue/cyan/green/amber/rose/violet/slate")

	var all bool
	list := &cobra.Command{
		Use:   "list",
		Short: "列出目标",
		RunE: func(*cobra.Command, []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			today, _, _ := nowParts()
			goals, err := s.GoalList(all)
			if err != nil {
				return err
			}
			if len(goals) == 0 {
				fmt.Println("（没有目标。用 baseline goal add 建一个）")
			}
			for _, g := range goals {
				srcs, err := s.SourcesOf(g.ID)
				if err != nil {
					return err
				}
				cur, err := metrics.ValueToday(s, g.ID, today)
				if err != nil {
					return err
				}
				rule := ""
				if len(srcs) == 0 {
					rule = "  ⚠ 没有判定规则"
				}
				fmt.Printf("#%-3d %-18s %-8s %-6s 当前 %s%s\n",
					g.ID, g.Title, g.Color, g.Status, view.Num(cur), rule)
			}
			return nil
		},
	}
	list.Flags().BoolVar(&all, "all", false, "含已归档")

	show := &cobra.Command{
		Use:   "show <目标>",
		Short: "查看目标的详情与判定规则",
		Args:  cobra.ExactArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			today, _, _ := nowParts()
			g, err := s.ResolveGoal(args[0])
			if err != nil {
				return err
			}
			fmt.Printf("目标 #%d 「%s」\n", g.ID, g.Title)
			fmt.Printf("状态    %s\n色板    %s\n创建    %s\n", g.Status, g.Color, g.CreatedAt)
			if strings.TrimSpace(g.Why) != "" {
				fmt.Printf("为什么  %s\n", g.Why)
			}
			srcs, err := s.SourcesOf(g.ID)
			if err != nil {
				return err
			}
			fmt.Println()
			if len(srcs) == 0 {
				fmt.Println("判定规则：（无）—— 这个目标给不出「什么算推进它」，等于没有目标。")
			} else {
				fmt.Println("判定规则：")
				for _, src := range srcs {
					unimpl := ""
					if !src.Kind.Implemented() {
						unimpl = "（未接入）"
					}
					fmt.Printf("  #%d %s%s%s\n", src.ID, src.Kind.Label(),
						ifStr(src.Target != "", " · "+src.Target, ""), unimpl)
					if strings.TrimSpace(src.Rationale) != "" {
						fmt.Printf("      └ %s\n", src.Rationale)
					}
				}
			}
			cur, err := metrics.ValueToday(s, g.ID, today)
			if err != nil {
				return err
			}
			fmt.Printf("\n当前累计  %s\n", view.Num(cur))
			return nil
		},
	}

	var reason string
	archive := &cobra.Command{
		Use:   "archive <目标>",
		Short: "归档（不删除）。必须写一句原因",
		Args:  cobra.ExactArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			_, _, stamp := nowParts()
			g, err := s.ResolveGoal(args[0])
			if err != nil {
				return err
			}
			if err := s.GoalArchive(g.ID, reason, stamp); err != nil {
				return err
			}
			fmt.Printf("已归档目标 #%d 「%s」\n", g.ID, g.Title)
			fmt.Println("原因已记入 why 字段，以后能回看当初为什么放弃。")
			return nil
		},
	}
	archive.Flags().StringVar(&reason, "reason", "", "为什么放弃它")

	goal.AddCommand(add, list, show, archive)
	return goal
}

// ---------------------------------------------------------------- source

func sourceCmd() *cobra.Command {
	src := &cobra.Command{Use: "source", Short: "判定规则（计分来源）。一个目标 = 一组计分来源。"}
	var kind, target, rationale, params string
	add := &cobra.Command{
		Use:   "add <目标>",
		Short: "给目标加一条计分来源",
		Args:  cobra.ExactArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			today, _, stamp := nowParts()
			g, err := s.ResolveGoal(args[0])
			if err != nil {
				return err
			}
			k, err := model.ParseKind(kind)
			if err != nil {
				return err
			}
			id, err := s.SourceAdd(g.ID, k, target, params, rationale, stamp)
			if err != nil {
				return err
			}
			fmt.Printf("已为「%s」添加计分来源 #%d：%s\n", g.Title, id, k.Label())
			if !k.Implemented() {
				fmt.Printf("注意：%s 尚未接入，规则先登记着，暂时不产出数值。\n", k.Label())
			}
			if strings.TrimSpace(rationale) == "" {
				fmt.Println("提示：没写 --rationale。三个月后你会想不起当初为什么这样定。")
			}
			fixed, err := s.BackfillCount()
			if err != nil {
				return err
			}
			if fixed > 0 {
				fmt.Printf("已把 %d 条原本没归到规则的记录归到它名下。\n", fixed)
			}
			cur, err := metrics.ValueToday(s, g.ID, today)
			if err != nil {
				return err
			}
			fmt.Printf("当前值：%s\n", view.Num(cur))
			if cur == 0 {
				fmt.Println("（还是 0。要么规则写错了，要么这条线真的还没动——两种都值得知道。）")
			}
			return nil
		},
	}
	add.Flags().StringVar(&kind, "kind", "manual_checkin", "manual_checkin | git_commits | external_metric | derived")
	add.Flags().StringVar(&target, "target", "", "仓库路径 / 外部数据标识")
	add.Flags().StringVar(&rationale, "rationale", "", "这条规则为什么这样定（供三个月后复核）")
	add.Flags().StringVar(&params, "params", "{}", "JSON 参数")

	list := &cobra.Command{
		Use:   "list <目标>",
		Short: "列出目标的判定规则",
		Args:  cobra.ExactArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			g, err := s.ResolveGoal(args[0])
			if err != nil {
				return err
			}
			srcs, err := s.SourcesOf(g.ID)
			if err != nil {
				return err
			}
			if len(srcs) == 0 {
				fmt.Printf("「%s」还没有判定规则。\n", g.Title)
			}
			for _, x := range srcs {
				fmt.Printf("#%-3d %-12s target=%-20s %s\n",
					x.ID, x.Kind.String(), ifStr(x.Target == "", "-", x.Target), x.Rationale)
			}
			return nil
		},
	}
	src.AddCommand(add, list)
	return src
}

// ---------------------------------------------------------------- checkin

func checkinCmd() *cobra.Command {
	var note, date, clock string
	var value float64
	var source int64
	c := &cobra.Command{
		Use:   "checkin [目标]",
		Short: "记一条推进",
		Args:  cobra.MaximumNArgs(1),
		RunE: func(_ *cobra.Command, args []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			today, todayS, stamp := nowParts()

			// 不写目标 = 记一条不关联的。不校验判定规则——没有目标就没有规则可违反。
			var goal *model.Goal
			if len(args) == 1 {
				goal, err = s.ResolveGoal(args[0])
				if err != nil {
					return err
				}
				has, err := s.GoalHasRule(goal.ID)
				if err != nil {
					return err
				}
				if !has {
					return fmt.Errorf(
						"「%s」还没有判定规则。先写清「什么算推进它」——\n  baseline source add %q --kind manual_checkin --rationale \"...\"",
						goal.Title, goal.Title)
				}
			}

			isBackdated := date != ""
			day := todayS
			if date != "" {
				parsed, err := metrics.ParseDay(date)
				if err != nil {
					return err
				}
				day = metrics.DayKey(parsed)
			}
			// 补记旧账时默认用 00:00，免得时间线上全是「此刻」。
			if clock == "" {
				if isBackdated {
					clock = "00:00"
				} else {
					clock = time.Now().Format("15:04")
				}
			}

			var picks [][2]int64
			var ids []int64
			if goal != nil {
				ids = append(ids, goal.ID)
				if source != 0 {
					picks = append(picks, [2]int64{goal.ID, source})
				}
			} else if source != 0 {
				return errors.New("--source 要配一个目标一起用")
			}

			cfg, err := ai.ConfigOf(s)
			if err != nil {
				return err
			}
			// 配了 AI 就先挂空着写下来，写完再补判——和窗口里是同一个顺序，
			// 理由也一样：一条已经发生的事不该卡在网络上。
			links, err := s.ResolveLinks(ids, picks, cfg.Ready())
			if err != nil {
				return err
			}
			id, err := s.CheckinAdd(links, day, clock, value, note, stamp)
			if err != nil {
				return err
			}

			// 补判（CLI 里等它做完就好：命令返回时就该是最终状态）。
			trouble := ""
			if cfg.Ready() {
				f, err := ai.ClassifyOne(s, id, today)
				if err != nil {
					return err
				}
				trouble = f.Trouble
			}

			// 后面这些话都要说**最终状态**，所以读库，不读上面那份 links——
			// 那份是补判之前算的，这时候已经过期了。
			nowLinks, err := s.LinksOf(id)
			if err != nil {
				return err
			}
			fmt.Printf("已记录 #%d · %s %s\n", id, day, clock)
			if len(nowLinks) == 0 {
				fmt.Println("它没有关联目标，所以不进任何曲线。")
				if !cfg.Ready() {
					fmt.Println("（在设置里打开 AI 判定，这一步可以交给它。）")
				}
			} else {
				for _, l := range nowLinks {
					g, err := s.GoalByID(l.GoalID)
					if err != nil {
						return err
					}
					title := fmt.Sprintf("#%d", l.GoalID)
					if g != nil {
						title = g.Title
					}
					if l.SourceID != nil {
						what := ""
						srcs, err := s.SourcesOf(l.GoalID)
						if err != nil {
							return err
						}
						for _, x := range srcs {
							if x.ID == *l.SourceID {
								what = x.Summary()
							}
						}
						fmt.Printf("%s → %s\n", title, what)
					} else {
						fmt.Printf("%s → 还没归到规则，所以没进这条曲线\n", title)
					}
				}
				if len(nowLinks) == 1 {
					cur, err := metrics.ValueToday(s, nowLinks[0].GoalID, today)
					if err != nil {
						return err
					}
					fmt.Printf("当前累计 %s\n", view.Num(cur))
				}
			}
			if trouble != "" {
				// 记录已经落库了，所以这不是失败，只是「这一条没能自动归」。
				fmt.Fprintln(os.Stderr, trouble)
			}
			return nil
		},
	}
	c.Flags().StringVar(&note, "note", "", "备注，会成为时间线上的标题")
	c.Flags().Float64Var(&value, "value", 1, "计数值，默认 1")
	c.Flags().StringVar(&date, "date", "", "日期 YYYY-MM-DD，默认今天")
	c.Flags().StringVar(&clock, "time", "", "时间 HH:MM，默认此刻")
	c.Flags().Int64Var(&source, "source", 0, "归到哪条判定规则（来源 id）")
	return c
}

// ---------------------------------------------------------------- ai

func aiCmd() *cobra.Command {
	cmd := &cobra.Command{Use: "ai", Short: "AI 判定（拿不准「算哪条规则」时让模型在你写的规则里挑一条）"}
	cmd.AddCommand(&cobra.Command{
		Use:   "show",
		Short: "看现在的配置（密钥只显示掩码）",
		RunE: func(*cobra.Command, []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			cfg, err := ai.ConfigOf(s)
			if err != nil {
				return err
			}
			key := cfg.MaskedKey()
			if key == "" {
				key = "（没配）"
			}
			on := "关"
			if cfg.Enabled {
				on = "开"
			}
			fmt.Printf("开关      %s\n接口      %s\n模型      %s\n密钥      %s\n自动选的把握  %d\n",
				on, cfg.BaseURL, cfg.Model, key, cfg.Threshold)
			if cfg.KeyBroken {
				fmt.Println("\n⚠ 密钥读不出来了，重新填一次：baseline ai set --key <密钥>")
			}
			if !cfg.Ready() {
				fmt.Println("\n现在还不能用。配上密钥：baseline ai set --key <你的密钥>")
			}
			return nil
		},
	})

	var baseURL, modelName, key string
	var threshold int
	var off bool
	set := &cobra.Command{
		Use:   "set",
		Short: "配置 AI 判定。不写的项保持原样",
		RunE: func(*cobra.Command, []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			cur, err := ai.ConfigOf(s)
			if err != nil {
				return err
			}
			next := cur
			if baseURL != "" {
				next.BaseURL = baseURL
			}
			if modelName != "" {
				next.Model = modelName
			}
			if threshold >= 0 {
				next.Threshold = threshold
			}
			var keyPtr *string
			if key != "" {
				keyPtr = &key
			}
			if err := ai.Save(s, !off, next.BaseURL, next.Model, next.Threshold, keyPtr); err != nil {
				return err
			}
			nowCfg, err := ai.ConfigOf(s)
			if err != nil {
				return err
			}
			state := "关闭"
			if nowCfg.Enabled {
				state = "打开"
			}
			keyState := "已设置"
			if nowCfg.APIKey == "" {
				keyState = "（没配）"
			}
			fmt.Printf("AI 判定已%s\n接口  %s\n模型  %s\n密钥  %s\n自动选的把握  %d\n",
				state, nowCfg.BaseURL, nowCfg.Model, keyState, nowCfg.Threshold)
			return nil
		},
	}
	set.Flags().StringVar(&baseURL, "base-url", "", "OpenAI 兼容的接口地址")
	set.Flags().StringVar(&modelName, "model", "", "模型名")
	set.Flags().StringVar(&key, "key", "", "API 密钥。传空串就是清掉")
	set.Flags().IntVar(&threshold, "threshold", -1, "置信度门槛（0–100）")
	set.Flags().BoolVar(&off, "off", false, "关掉：关掉之后拿不准就一律让你自己选")

	cmd.AddCommand(set)
	cmd.AddCommand(&cobra.Command{
		Use:   "test",
		Short: "发一个最小的请求，看配好没有",
		RunE: func(*cobra.Command, []string) error {
			s, err := openStore()
			if err != nil {
				return err
			}
			defer s.Close()
			msg, err := ai.Probe(s)
			if err != nil {
				return err
			}
			fmt.Println(msg)
			return nil
		},
	})
	return cmd
}

// ---------------------------------------------------------------- serve

func serveCmd() *cobra.Command {
	var port int
	var dev, debug, watchStdin bool
	var token, devAddr string
	c := &cobra.Command{
		Use:   "serve",
		Short: "起本地服务（网页 + API）。开发时配 --dev 走 Vite 热重载",
		RunE: func(*cobra.Command, []string) error {
			return server.Run(server.Options{
				DBPath:     dbPath(),
				Port:       port,
				Dev:        dev,
				DevAddr:    devAddr,
				Debug:      debug,
				Token:      token,
				WatchStdin: watchStdin,
			})
		},
	}
	c.Flags().IntVar(&port, "port", 7717, "监听端口（0 = 系统挑）")
	c.Flags().BoolVar(&dev, "dev", false, "把前端代理到 Vite（127.0.0.1:5173）")
	c.Flags().StringVar(&devAddr, "dev-addr", "", "Vite 地址，默认 http://127.0.0.1:5173")
	c.Flags().BoolVar(&debug, "debug", false, "打开 /__state 等调试接口")
	c.Flags().StringVar(&token, "token", "", "访问令牌；设置后首访要带 ?t=")
	c.Flags().BoolVar(&watchStdin, "watch-stdin", false, "stdin 关闭即退出（给 Electron 当子进程用）")
	return c
}

func ifStr(cond bool, a, b string) string {
	if cond {
		return a
	}
	return b
}
