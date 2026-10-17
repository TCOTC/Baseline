import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

// 构建产物直接落进 Go 的 embed 目录：一次 `pnpm build` + 一次 `go build`，
// 就得到一个带前端的单文件二进制。
export default defineConfig({
  plugins: [vue()],
  base: './',
  build: {
    outDir: '../internal/webui/dist',
    emptyOutDir: true,
  },
  server: {
    // 固定 IPv4：Go 那边代理到 127.0.0.1，而 Windows 上 localhost 可能先解析到 ::1，
    // 两边一个走 v6 一个走 v4 就会 502（踩过一次）。
    host: '127.0.0.1',
    port: 5173,
    strictPort: true,
    // 直接开 :5173 时也能用；同时 Go 在 :7717 上把整个页面代理回来，
    // 给 AI 验证的是那一个地址。
    proxy: {
      '/api': 'http://127.0.0.1:7717',
      '/__state': 'http://127.0.0.1:7717',
      '/__jslog': 'http://127.0.0.1:7717',
    },
  },
})
