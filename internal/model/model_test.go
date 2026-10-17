package model_test

import (
	"testing"

	"baseline/internal/model"
)

// 规则叫什么只有这一处定义：卡片底部、流水行、CLI 的拒绝语都读它。
// 三处各写一遍的话，「这条记录归到了哪条规则」和「卡片上印着哪句话」
// 迟早变成两句不同的话——而用户只看得到「对不上」。
func Test规则的名字优先取_target_再退回_rationale(t *testing.T) {
	src := func(target, rationale string) model.Source {
		return model.Source{Kind: model.KindManualCheckin, Target: target, Rationale: rationale}
	}
	cases := []struct {
		got, want string
	}{
		{src("做题", "做完一章题算一次").Summary(), "做题"},
		{src("", "读完一章算一次").Summary(), "读完一章算一次"},
		{src("  ", "  读完一章  ").Summary(), "读完一章"},
		// 都没写就说实话，不编一个出来。
		{src("", "").Summary(), "没写说明"},
	}
	for _, c := range cases {
		if c.got != c.want {
			t.Fatalf("得到 %q，期望 %q", c.got, c.want)
		}
	}
}

func Test来源类型只有四种(t *testing.T) {
	if len(model.Kinds) != 4 {
		t.Fatalf("四种，是上限；得到 %d", len(model.Kinds))
	}
	if _, err := model.ParseKind("nope"); err == nil {
		t.Fatal("未知类型该报错")
	}
	for _, k := range model.Kinds {
		if _, err := model.ParseKind(k.String()); err != nil {
			t.Fatalf("%s 应该能解析回来：%v", k, err)
		}
	}
	if !model.KindManualCheckin.Implemented() {
		t.Fatal("手工打卡已经接线")
	}
	if model.KindGitCommits.Implemented() {
		t.Fatal("git 还没接线")
	}
}
