# 全量检查：构建前端 + 构建 Go + 重置演示库 + 起服务 + 浏览器端到端 + Electron 外壳。
# 退出码 0/1。用法：powershell -ExecutionPolicy Bypass -File scripts/check.ps1
#
# 前端必须编进二进制，所以这里的顺序是 pnpm build → go build，不能反。

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

Write-Output "── 构建前端 ──"
Push-Location web
if (-not (Test-Path node_modules)) {
  pnpm install
  if ($LASTEXITCODE -ne 0) { throw "pnpm install 失败" }
}
pnpm build
if ($LASTEXITCODE -ne 0) { throw "pnpm build 失败" }
Pop-Location

Write-Output "── 构建 Go ──"
go build -o bin/baseline.exe ./cmd/baseline
if ($LASTEXITCODE -ne 0) { throw "go build 失败" }

# 端口检查必须在重置演示库之前：还开着的服务会锁住 _demo.db，
# 那时候报「文件被占用」看不出真正的原因。
$port = 7717
if (Get-NetTCPConnection -LocalPort $port -State Listen -ErrorAction SilentlyContinue) {
  throw "端口 $port 已被占用——先停掉那个服务再跑"
}

# 开发窗口（Electron + 随机端口的 Go）不占 7717，但会锁住演示库。
# 独占打开一次，能打开才说明没人用着。
$dbFile = Join-Path $root 'data\_demo.db'
if (Test-Path $dbFile) {
  try {
    $fs = [System.IO.File]::Open($dbFile, 'Open', 'ReadWrite', 'None')
    $fs.Close()
  } catch {
    throw "演示库正被占用（开发窗口还开着？）——先关掉它再跑"
  }
}

Write-Output "── 演示数据 ──"
powershell -ExecutionPolicy Bypass -File scripts/demo.ps1 | Out-Null
if ($LASTEXITCODE -ne 0) { throw "演示数据生成失败" }

$srv = Start-Process -FilePath (Join-Path $root 'bin\baseline.exe') `
  -ArgumentList @('--db', 'data\_demo.db', 'serve', '--port', "$port", '--debug') `
  -PassThru -WindowStyle Hidden

try {
  $ready = $false
  for ($i = 0; $i -lt 40; $i++) {
    try {
      Invoke-RestMethod "http://127.0.0.1:$port/__state" -TimeoutSec 1 | Out-Null
      $ready = $true
      break
    } catch {
      Start-Sleep -Milliseconds 250
    }
  }
  if (-not $ready) { throw "服务没在 10 秒内起来" }

  Push-Location web
  Write-Output "── 浏览器端到端 ──"
  node scripts/check-ui.mjs
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }

  Write-Output "── Electron 外壳 ──"
  node scripts/check-electron.mjs
  if ($LASTEXITCODE -ne 0) { exit $LASTEXITCODE }
  Pop-Location
} finally {
  Stop-Process -Id $srv.Id -Force -ErrorAction SilentlyContinue
}

Write-Output "全部通过。"
