// Vite 的 emptyOutDir 会把 dist 整个清掉，包括让 `//go:embed all:dist` 成立的那个
// 占位文件。构建完把它补回来：没有前端构建时 `go build` 仍然要能过。
import { writeFileSync } from 'node:fs'

writeFileSync(new URL('../../internal/webui/dist/.gitkeep', import.meta.url), '')
