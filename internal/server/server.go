// Package server 起一个只绑 127.0.0.1 的 HTTP 服务：接口 + 前端。
//
// # 为什么有端口
//
// 开发时浏览器要能直接打开、AI 要能直接验证，同一份东西还要塞进 Electron——
// 一个 URL 是最短的桥。代价是常驻一个监听口，所以这里把该做的事一次做全：
// 只绑回环、随机端口、每次启动一个 token、校验 Host（防 DNS rebinding）。
package server

import (
	"crypto/subtle"
	"fmt"
	"io"
	"net"
	"net/http"
	"net/http/httputil"
	"net/url"
	"os"
	"os/signal"
	"path/filepath"
	"strings"
	"sync"
	"time"

	"baseline/internal/api"
	"baseline/internal/metrics"
	"baseline/internal/store"
	"baseline/internal/webui"
)

// Options 是启动参数。
type Options struct {
	DBPath string
	// Port 为 0 时由系统挑一个空闲端口。
	Port int
	// Dev 为 true 时把非 /api 请求反向代理到 Vite（默认 127.0.0.1:5173），
	// 于是开发时只需要一个地址：HMR 也走它。
	Dev     bool
	DevAddr string
	// Debug 打开 /__state。
	Debug bool
	// Token 为空表示不校验（仅开发用）。
	Token string
	// WatchStdin 为 true 时，stdin 关闭即退出——Electron 被强杀时不留孤儿进程。
	WatchStdin bool
	// Stdout 是启动信息的去处（测试里可以换成 buffer）。
	Stdout io.Writer
}

// Run 启动服务并阻塞到退出。
func Run(opts Options) error {
	if opts.Stdout == nil {
		opts.Stdout = os.Stdout
	}
	st, err := store.Open(opts.DBPath)
	if err != nil {
		return err
	}
	defer st.Close()

	// 启动即补一次快照：Web 页面和 CLI 读的是同一份，谁先打开都不该看到旧数字。
	if _, err := metrics.Roll(st, metrics.DayStart(time.Now())); err != nil {
		return err
	}

	logf := fileLogger(opts.DBPath)

	var assets http.Handler
	if opts.Dev {
		addr := opts.DevAddr
		if addr == "" {
			addr = "http://127.0.0.1:5173"
		}
		target, err := url.Parse(addr)
		if err != nil {
			return fmt.Errorf("开发服务器地址不合法：%w", err)
		}
		logf("开发模式 · 前端代理到 %s", addr)
		assets = httputil.NewSingleHostReverseProxy(target)
	} else {
		assets = webui.Handler()
	}

	srv := &api.Server{Store: st, Debug: opts.Debug, Assets: assets, Logf: logf}
	handler := localOnly(tokenAuth(opts.Token, srv.Handler()))

	ln, err := net.Listen("tcp", fmt.Sprintf("127.0.0.1:%d", opts.Port))
	if err != nil {
		return fmt.Errorf("端口被占了：%w", err)
	}
	addr := ln.Addr().(*net.TCPAddr)
	base := fmt.Sprintf("http://127.0.0.1:%d", addr.Port)
	visit := base + "/"
	if opts.Token != "" {
		visit += "?t=" + opts.Token
	}
	fmt.Fprintf(opts.Stdout, "BASELINE_URL=%s\n", visit)
	fmt.Fprintf(opts.Stdout, "基线服务已启动：%s\n", base)
	if opts.Dev {
		fmt.Fprintf(opts.Stdout, "前端热重载走 Vite，改完直接刷新页面。\n")
	}
	logf("服务启动 · %s · 数据库 %s", base, opts.DBPath)

	httpSrv := &http.Server{
		Handler:           handler,
		ReadHeaderTimeout: 10 * time.Second,
	}

	errc := make(chan error, 1)
	go func() { errc <- httpSrv.Serve(ln) }()

	stop := make(chan struct{})
	var once sync.Once
	shutdown := func() {
		once.Do(func() { close(stop) })
	}
	sigc := make(chan os.Signal, 1)
	signal.Notify(sigc, os.Interrupt)
	go func() {
		select {
		case <-sigc:
		case <-stop:
		}
		shutdown()
	}()
	if opts.WatchStdin {
		// 父进程（Electron）退出后 stdin 会 EOF。这比 Job Object 省事，
		// 也不需要原生模块。
		go func() {
			_, _ = io.Copy(io.Discard, os.Stdin)
			shutdown()
		}()
	}

	select {
	case err := <-errc:
		return err
	case <-stop:
		_ = httpSrv.Close()
		return nil
	}
}

// localOnly 只接受本机访问。Host 也要校验：一个恶意页面可以把域名解析到
// 127.0.0.1 再打过来（DNS rebinding），只绑回环挡不住它。
func localOnly(next http.Handler) http.Handler {
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		host := r.Host
		if h, _, err := net.SplitHostPort(r.Host); err == nil {
			host = h
		}
		host = strings.Trim(host, "[]")
		switch host {
		case "127.0.0.1", "localhost", "::1":
			next.ServeHTTP(w, r)
		default:
			http.Error(w, "只接受本机访问", http.StatusForbidden)
		}
	})
}

// tokenAuth 用一次性链接把 token 换成 HttpOnly cookie，之后同源请求自动带上。
//
// 打包后的 Electron 用随机 token；开发时 token 为空（只绑回环 + 校验 Host），
// 免得每次刷新都要带着一长串东西。
func tokenAuth(token string, next http.Handler) http.Handler {
	if token == "" {
		return next
	}
	want := []byte(token)
	return http.HandlerFunc(func(w http.ResponseWriter, r *http.Request) {
		if got := r.URL.Query().Get("t"); got != "" &&
			subtle.ConstantTimeCompare([]byte(got), want) == 1 {
			http.SetCookie(w, &http.Cookie{
				Name: "bl_token", Value: token, Path: "/",
				HttpOnly: true, SameSite: http.SameSiteStrictMode,
			})
			u := *r.URL
			q := u.Query()
			q.Del("t")
			u.RawQuery = q.Encode()
			http.Redirect(w, r, u.String(), http.StatusFound)
			return
		}
		if c, err := r.Cookie("bl_token"); err == nil &&
			subtle.ConstantTimeCompare([]byte(c.Value), want) == 1 {
			next.ServeHTTP(w, r)
			return
		}
		http.Error(w, "unauthorized", http.StatusUnauthorized)
	})
}

// fileLogger 把日志写到库旁边。release 没有控制台，诊断信息必须有地方落。
func fileLogger(dbPath string) func(string, ...any) {
	dir := filepath.Dir(dbPath)
	if dir == "" {
		dir = "."
	}
	path := filepath.Join(dir, "desktop.log")
	var mu sync.Mutex
	return func(format string, args ...any) {
		line := fmt.Sprintf("[%s] %s\n", time.Now().Format(time.RFC3339), fmt.Sprintf(format, args...))
		mu.Lock()
		defer mu.Unlock()
		if f, err := os.OpenFile(path, os.O_CREATE|os.O_APPEND|os.O_WRONLY, 0o644); err == nil {
			_, _ = f.WriteString(line)
			_ = f.Close()
		}
	}
}
