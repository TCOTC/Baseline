#Requires -Version 5.1
<#
.SYNOPSIS
  一条命令起开发环境。默认起 Electron 开发窗口。

.DESCRIPTION
  缺依赖就装、内核有改动就重编、没演示库就播种、起 Vite、拉起外壳；Ctrl+C 一起收掉。
  Vite 由内核代理给外壳，所以窗口里就是热重载后的页面。

  默认用演示库 data\_demo.db——外壳不带 --db 时会落到 %APPDATA%\Baseline\baseline.db，
  那是真实数据，开发时不该碰。要看真实数据用 -Db 显式指过去。

.PARAMETER Browser
  不起外壳，改成浏览器模式：内核跑 :7717，打开 http://127.0.0.1:7717。

.PARAMETER Reset
  重建演示库（默认只在库不存在时播种）。只对演示库有效，不能和 -Db 一起用。

.PARAMETER DbPath
  指定数据库路径（相对路径按仓库根目录解析）。不能叫 -Db：那是 -Debug 的别名。

.PARAMETER NoBrowser
  浏览器模式下不自动打开浏览器。

.EXAMPLE
  .\dev.ps1

.EXAMPLE
  .\dev.ps1 -Browser

.EXAMPLE
  .\dev.ps1 -Reset
#>
[CmdletBinding()]
param(
  [switch]$Browser,
  [switch]$Reset,
  [string]$DbPath,
  [switch]$NoBrowser
)

$ErrorActionPreference = 'Stop'
$root = $PSScriptRoot
Set-Location $root

$port      = 7717
$demoDb    = Join-Path $root 'data\_demo.db'
$Db        = $DbPath
$useDemoDb = [string]::IsNullOrEmpty($Db)
if ($useDemoDb) { $Db = $demoDb }
if ($Reset -and -not $useDemoDb) { throw '-Reset 只重建演示库，不能和 -DbPath 一起用。' }

# 转绝对路径：外壳拉内核时的 cwd 是 web/，相对路径会落到 web\data\ 下。
$Db = [System.IO.Path]::GetFullPath($Db)
$dbName = Split-Path $Db -Leaf

$exe         = Join-Path $root 'bin\baseline.exe'
$log         = Join-Path $root 'data\desktop.log'
$goPidFile   = Join-Path $root 'data\_dev.pid'
$vitePidFile = Join-Path $root 'data\_vite.pid'
$viteLog     = Join-Path $root 'data\_vite.log'
$web         = Join-Path $root 'web'

$goPid   = 0
$vitePid = 0
$failed  = $false

function Step($text) { Write-Host "── $text ──" -ForegroundColor Cyan }

function Test-Port($p) {
  [bool](Get-NetTCPConnection -LocalPort $p -State Listen -ErrorAction SilentlyContinue)
}

function Kill-Tree($procId) {
  if ($procId -le 0) { return }
  Start-Process -FilePath 'taskkill.exe' -ArgumentList @('/PID', "$procId", '/T', '/F') `
    -WindowStyle Hidden -Wait -ErrorAction SilentlyContinue
}

# 等某个地址能应答。服务自己会打印 BASELINE_URL，但固定端口轮询更省事。
function Wait-Http($url, $seconds) {
  $deadline = (Get-Date).AddSeconds($seconds)
  while ((Get-Date) -lt $deadline) {
    try {
      Invoke-WebRequest $url -TimeoutSec 1 -UseBasicParsing | Out-Null
      return $true
    } catch {
      Start-Sleep -Milliseconds 250
    }
  }
  return $false
}

# 外壳拉起来的内核没法提前记 pid，所以按命令行认：--watch-stdin 只有外壳会给，
# --dev 只有开发路径会给（打包版没有）。两个都在，就是该收掉的那种。
function Get-DevKernels {
  Get-CimInstance Win32_Process -Filter "Name='baseline.exe'" -ErrorAction SilentlyContinue |
    Where-Object { $_.CommandLine -and $_.CommandLine -match '--watch-stdin' -and $_.CommandLine -match '--dev' }
}

# 上次被强杀（Ctrl+C 没走完 / 直接关了窗口）会留下内核和 Vite 占着库与端口。
function Clear-Leaked {
  $killed = @()
  foreach ($f in @($goPidFile, $vitePidFile)) {
    if (-not (Test-Path $f)) { continue }
    $old = 0
    [void][int]::TryParse((Get-Content $f -Raw).Trim(), [ref]$old)
    if ($old -gt 0 -and (Get-Process -Id $old -ErrorAction SilentlyContinue)) {
      Kill-Tree $old
      $killed += $old
    }
    Remove-Item $f -ErrorAction SilentlyContinue
  }
  foreach ($p in @(Get-DevKernels)) {
    if ($p.ProcessId -and $p.ProcessId -ne $PID) {
      Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
      $killed += $p.ProcessId
    }
  }
  if ($killed.Count -gt 0) {
    Step "收掉上次遗留的进程（$($killed -join ', ')）"
    Start-Sleep -Milliseconds 400
  }
}

try {
  foreach ($c in @('go', 'pnpm')) {
    if (-not (Get-Command $c -ErrorAction SilentlyContinue)) { throw "找不到 $c，先把它装好。" }
  }

  if (-not (Test-Path (Join-Path $web 'node_modules'))) {
    Step '装前端依赖'
    Push-Location $web
    try { pnpm install } finally { $installed = $LASTEXITCODE -eq 0; Pop-Location }
    if (-not $installed) { throw 'pnpm install 失败。' }
  }

  # 只在 .go 比 exe 新的时候重编。前端改动不需要重编——dev 下页面来自 Vite。
  $stale = $true
  if (Test-Path $exe) {
    $exeTime = (Get-Item $exe).LastWriteTime
    $src = Get-ChildItem -Path (Join-Path $root 'cmd'), (Join-Path $root 'internal') `
      -Recurse -Filter *.go -File -ErrorAction SilentlyContinue |
      Sort-Object LastWriteTime -Descending | Select-Object -First 1
    $stale = [bool]($src -and ($src.LastWriteTime -gt $exeTime))
  }
  if ($stale) {
    Step '构建内核'
    go build -o bin/baseline.exe ./cmd/baseline
    if ($LASTEXITCODE -ne 0) { throw 'go build 失败。' }
  }

  Clear-Leaked

  if ($useDemoDb -and ($Reset -or -not (Test-Path $Db))) {
    Step '演示数据'
    & powershell -ExecutionPolicy Bypass -File (Join-Path $root 'scripts\demo.ps1') | Out-Null
    if ($LASTEXITCODE -ne 0) {
      throw '演示数据生成失败。若提示演示库被占用，先把 Electron 窗口退掉（托盘右键「退出」）。'
    }
  }

  Step 'Vite'
  $vitePid = (Start-Process -FilePath 'cmd.exe' -WorkingDirectory $web -PassThru -WindowStyle Hidden `
      -ArgumentList '/c', "pnpm dev > `"$viteLog`" 2>&1").Id
  Set-Content -Path $vitePidFile -Value $vitePid -Encoding ascii
  if (-not (Wait-Http 'http://127.0.0.1:5173/' 25)) {
    $tail = Get-Content $viteLog -Tail 20 -ErrorAction SilentlyContinue
    throw "Vite 没起来（端口 5173 被占？）。data\_vite.log 末尾：`n$($tail -join "`n")"
  }

  Write-Host ''
  if ($Browser) {
    if (Test-Port $port) { throw "端口 $port 被别的进程占用了，先停掉它。" }
    Step '内核服务'
    $go = Start-Process -FilePath $exe -WorkingDirectory $root -PassThru -WindowStyle Hidden `
      -ArgumentList @('--db', $Db, 'serve', '--port', "$port", '--dev', '--debug')
    $goPid = $go.Id
    Set-Content -Path $goPidFile -Value $goPid -Encoding ascii
    if (-not (Wait-Http "http://127.0.0.1:$port/__state" 15)) {
      throw "服务没在 15 秒内就绪，看 $log。"
    }
    Write-Host "  界面  http://127.0.0.1:$port" -ForegroundColor Green
    Write-Host "  状态  http://127.0.0.1:$port/__state"
    Write-Host "  日志  data\desktop.log"
    Write-Host '  Ctrl+C 收工' -ForegroundColor DarkGray
    Write-Host ''
    if (-not $NoBrowser) { Start-Process "http://127.0.0.1:$port" }
    Wait-Process -Id $goPid
  } else {
    Write-Host "  Electron 开发窗口 · 库 data\$dbName" -ForegroundColor Green
    Write-Host '  日志  data\desktop.log（内核）· data\_vite.log（前端）'
    Write-Host '  关窗口只是收进托盘；退出走托盘右键，或在这里按 Ctrl+C' -ForegroundColor DarkGray
    Write-Host ''

    $env:BASELINE_DEV = '1'
    # 库路径走环境变量而不是 --db=：pnpm 转发 Windows 路径时会把反斜杠转义成两个，
    # 命令行上会变成 D:\\CodeProjects\\...（能用，但日志里没法看）。内核本来就读 BASELINE_DB。
    $env:BASELINE_DB = $Db
    # 有的终端/宿主会导出 ELECTRON_RUN_AS_NODE=1，Electron 就会当 node 跑，
    # 崩在 app.requestSingleInstanceLock 上。拉窗口前必须摘掉，事后还原。
    $savedRunAsNode = $env:ELECTRON_RUN_AS_NODE
    Remove-Item Env:\ELECTRON_RUN_AS_NODE -ErrorAction SilentlyContinue
    Push-Location $web
    try {
      pnpm run electron
      if ($LASTEXITCODE -ne 0) { throw "Electron 退出了（退出码 $LASTEXITCODE）。" }
    } finally {
      Remove-Item Env:\BASELINE_DEV, Env:\BASELINE_DB -ErrorAction SilentlyContinue
      if ($null -ne $savedRunAsNode) { $env:ELECTRON_RUN_AS_NODE = $savedRunAsNode }
      Pop-Location
    }
  }
} catch {
  Write-Host "✗ $($_.Exception.Message)" -ForegroundColor Red
  $failed = $true
} finally {
  Stop-Process -Id $goPid -Force -ErrorAction SilentlyContinue
  Kill-Tree $vitePid
  # 外壳可能已经把内核带走了；没带走的（Ctrl+C 打断 before-quit）在这里兜底。
  if (-not $Browser) {
    foreach ($p in @(Get-DevKernels)) {
      Stop-Process -Id $p.ProcessId -Force -ErrorAction SilentlyContinue
    }
  }
  Remove-Item $goPidFile, $vitePidFile -ErrorAction SilentlyContinue
}

if ($failed) { exit 1 }
