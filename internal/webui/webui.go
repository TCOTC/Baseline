// Package webui 把前端产物（Vite build 出来的 dist）编进二进制。
//
// 单体二进制是这套东西能「双击就跑」的原因：不需要旁边放一个 assets 目录。
// dist 里那个 index.html 是占位文件——真实内容由 `pnpm build` 生成；
// 没有前端构建时 `go build` 仍然能过，只是打开来是一片「请先构建前端」。
package webui

import (
	"embed"
	"io/fs"
	"net/http"
	"strings"
)

//go:embed all:dist
var dist embed.FS

// Handler 返回 SPA 的静态文件处理器。
//
// 所有未知路径都回 index.html：路由是查询串（?goal=N / ?settings=1），
// 刷新、直接粘贴链接进来都不该 404。
func Handler() http.Handler {
	sub, err := fs.Sub(dist, "dist")
	if err != nil {
		return missing("前端资源缺失：" + err.Error())
	}
	if _, err := fs.Stat(sub, "index.html"); err != nil {
		// 没跑过 pnpm build 的时候别给一片 404——那和「服务坏了」看起来一样。
		return missing("前端还没构建。在 web/ 目录下跑 pnpm install && pnpm build，然后重新 go build。")
	}
	files := http.FileServer(http.FS(sub))
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		p := strings.TrimPrefix(r.URL.Path, "/")
		if p == "" {
			p = "index.html"
		}
		if f, err := sub.Open(p); err == nil {
			f.Close()
			// 带 hash 的资源可以长缓存；index.html 不能。
			if strings.HasPrefix(p, "assets/") {
				w.Header().Set("Cache-Control", "public, max-age=31536000, immutable")
			} else {
				w.Header().Set("Cache-Control", "no-store")
			}
			files.ServeHTTP(w, r)
			return
		}
		r.URL.Path = "/"
		w.Header().Set("Cache-Control", "no-store")
		files.ServeHTTP(w, r)
	})
}

func missing(message string) http.Handler {
	page := "<!doctype html><meta charset=\"utf-8\"><title>基线</title>" +
		"<body style=\"font-family:system-ui;padding:40px;line-height:1.8;color:#1c1a17\">" +
		"<h3>基线</h3><p>" + message + "</p></body>"
	return http.HandlerFunc(func(w http.ResponseWriter, _ *http.Request) {
		w.Header().Set("Content-Type", "text/html; charset=utf-8")
		w.Header().Set("Cache-Control", "no-store")
		_, _ = w.Write([]byte(page))
	})
}
