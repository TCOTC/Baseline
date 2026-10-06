# 端到端检查：把窗口真的操作一遍，再量屏幕确认。
#
# 用法：
#     powershell -ExecutionPolicy Bypass -File scripts/ui-check.ps1
#     powershell -ExecutionPolicy Bypass -File scripts/ui-check.ps1 -Db data/_demo.db -Keep
#
# 退出码 0 = 全过，1 = 有失败。可以直接拿来当提交前的闸门。
#
# 检查的都是「读代码看不出来」的东西：
#   窗口按钮真的生效吗 / 顶栏真的能拖吗 / 两栏真的各滚各的吗 /
#   按钮真的贴到窗口右边缘了吗 / 顶栏左边真的什么都没有吗
#
# ⚠ 本文件必须是 UTF-8 with BOM（Windows PowerShell 5.1 会把无 BOM 的脚本按 ANSI 读）。

param(
    [string]$Db = 'data/_demo.db',
    [string]$OutDir = 'data/_ui',
    [switch]$NoLaunch
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
Import-Module (Join-Path $PSScriptRoot 'ui.psm1') -Force

$script:Results = @()
function Check {
    param([string]$Name, [bool]$Ok, [string]$Detail = '')
    $script:Results += [pscustomobject]@{ Name = $Name; Ok = $Ok; Detail = $Detail }
    $mark = 'FAIL'
    if ($Ok) { $mark = 'ok  ' }
    Write-Output ("  [{0}] {1,-28} {2}" -f $mark, $Name, $Detail)
}

# ---------------------------------------------------------------- 启动

$exe = Join-Path $root 'target\debug\baseline-desktop.exe'
if (-not (Test-Path $exe)) { throw "先 cargo build（找不到 $exe）" }

$existing = Get-Process baseline-desktop -ErrorAction SilentlyContinue
if (-not $existing -and -not $NoLaunch) {
    Write-Output "启动窗口（库：$Db）"
    Start-Process $exe -ArgumentList '--db', $Db -WorkingDirectory $root | Out-Null
    Start-Sleep -Seconds 7
} elseif (-not $existing) {
    throw "窗口没在跑，去掉 -NoLaunch 让它自己启动"
}

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }
$cursor = Get-CursorPos
$geom0 = Get-WindowGeometry

Write-Output ""
Write-Output "窗口 $([int]$geom0.CssWidth)x$([int]$geom0.CssHeight) CSS，DPI 缩放 $($geom0.Scale)"

try {
    [void](Set-WindowFocus)
    $p = Get-AppWindow
    Check '进程有窗口' ($p.Handle -ne [IntPtr]::Zero) "pid $($p.Id) hwnd 0x$([int64]$p.Handle)"

    # ------------------------------------------------------------ 顶栏几何
    # 顶栏横跨整个窗口，所以最右边的墨迹应该离窗口右边缘很近。
    # 顶栏还在 .wrap 里的时候，这个距离是一百多像素。
    $shot0 = Join-Path $OutDir 'top.png'
    Save-Shot -Path $shot0 | Out-Null
    $bmp = [System.Drawing.Bitmap]::FromFile((Resolve-Path $shot0))
    try {
        $g = Get-WindowGeometry
        $offX = $g.OriginX - $g.FrameLeft
        $offY = $g.OriginY - $g.FrameTop
        $barY0 = $offY + [int](8 * $g.Scale)
        $barY1 = $offY + [int](32 * $g.Scale)
        # 背景取顶栏里一处确定空着的地方（左边四分之一处），
        # 不要取窗口最下沿——那里是 DWM 阴影，不是页面底色。
        $bg = $bmp.GetPixel($offX + [int](300 * $g.Scale), $offY + [int](20 * $g.Scale))
        $rightmost = -1
        $leftmost = $bmp.Width
        # 从客户区左边 12px 起扫：跳开圆角本身的反锯齿。
        $scanFrom = $offX + [int](12 * $g.Scale)
        for ($y = $barY0; $y -le $barY1; $y++) {
            for ($x = $scanFrom; $x -lt $bmp.Width - 2; $x++) {
                $c = $bmp.GetPixel($x, $y)
                $d = [math]::Abs($c.R - $bg.R) + [math]::Abs($c.G - $bg.G) + [math]::Abs($c.B - $bg.B)
                if ($d -gt 30) {
                    if ($x -gt $rightmost) { $rightmost = $x }
                    if ($x -lt $leftmost) { $leftmost = $x }
                }
            }
        }
        $gapCss = ($bmp.Width - 1 - $rightmost) / $g.Scale
        Check '窗口按钮贴右边缘' ($gapCss -le 25) ("最右墨迹距边 {0:N1} css" -f $gapCss)
        Check '顶栏左半边是空的' ($leftmost -ge $bmp.Width / 2) `
              ("最左墨迹在客户区 x={0:N0} css" -f (($leftmost - $offX) / $g.Scale))
    } finally { $bmp.Dispose() }

    # ------------------------------------------------------------ 窗口按钮
    $pt = Get-CaptionButtonPoint -Which max
    Invoke-Click -X $pt.X -Y $pt.Y
    Check '最大化' (Test-Maximized)

    $pt = Get-CaptionButtonPoint -Which max
    Invoke-Click -X $pt.X -Y $pt.Y
    Check '还原' (-not (Test-Maximized))

    $pt = Get-CaptionButtonPoint -Which min
    Invoke-Click -X $pt.X -Y $pt.Y
    Check '最小化' (Test-Minimized)
    [void](Set-WindowFocus)
    Check '从最小化恢复' (-not (Test-Minimized))

    # ------------------------------------------------------------ 拖动顶栏
    $before = Get-WindowRectCss
    $g = Get-WindowGeometry
    $dragFromX = $g.CssWidth / 2
    Invoke-Drag -FromX $dragFromX -FromY 20 -ToX ($dragFromX - 60) -ToY 70
    $after = Get-WindowRectCss
    $dx = $after.X - $before.X
    $dy = $after.Y - $before.Y
    Check '顶栏可拖动' (([math]::Abs($dx + 60) -le 3) -and ([math]::Abs($dy - 50) -le 3)) "位移 dx=$dx dy=$dy"
    Check '拖动不改尺寸' (($after.W -eq $before.W) -and ($after.H -eq $before.H)) "$($after.W)x$($after.H)"

    # ------------------------------------------------------------ 独立滚动
    # 滚轮压在时间线上：右栏要动，左栏一动不动。
    # 再滚轮压在卡片上：时间线不能跟着动。
    #
    # 前提是窗口在前台——Windows 的滚轮消息只发给前台窗口。
    # 这一项单独检查，否则「没在前台」会被误读成「独立滚动没实现」。
    Check '窗口在前台（滚轮前提）' (Set-WindowFocus)

    # 区域坐标按实际客户区宽度算，和 view.css 的 --outer 是同一套算法。
    $g = Get-WindowGeometry
    $offX = $g.OriginX - $g.FrameLeft
    $offY = $g.OriginY - $g.FrameTop
    $outer = [math]::Max(0, ($g.CssWidth - 1120) / 2)
    $contentLeft = 30 + $outer
    $cardX = $offX + [int](($contentLeft + 20) * $g.Scale)
    $cardW = [int](260 * $g.Scale)
    $tlX = $offX + [int](($contentLeft + 360) * $g.Scale)
    $tlW = [int](($g.CssWidth - $contentLeft - 360 - 40) * $g.Scale)
    $regionY = $offY + [int](70 * $g.Scale)
    $regionH = [int](($g.CssHeight - 160) * $g.Scale)

    $s1 = Join-Path $OutDir 'scroll-0.png'; Save-Shot -Path $s1 | Out-Null
    Invoke-Wheel -X ($g.CssWidth / 2) -Y 400 -Notches -5
    $s2 = Join-Path $OutDir 'scroll-1.png'; Save-Shot -Path $s2 | Out-Null

    $cardsMoved = Test-SameRegion -PathA $s1 -PathB $s2 -X $cardX -Y $regionY -W $cardW -H $regionH
    $tlMoved = Test-SameRegion -PathA $s1 -PathB $s2 -X $tlX -Y $regionY -W $tlW -H $regionH
    Check '滚时间线：时间线动' ($tlMoved.Ratio -gt 0.05) ("差异 {0:P1}" -f $tlMoved.Ratio)
    Check '滚时间线：卡片不动' ($cardsMoved.Ratio -lt 0.005) ("差异 {0:P1}" -f $cardsMoved.Ratio)

    Invoke-Wheel -X ($contentLeft + 100) -Y 300 -Notches -5
    $s3 = Join-Path $OutDir 'scroll-2.png'; Save-Shot -Path $s3 | Out-Null
    $tlAfter = Test-SameRegion -PathA $s2 -PathB $s3 -X $tlX -Y $regionY -W $tlW -H $regionH
    Check '滚卡片：时间线不动' ($tlAfter.Ratio -lt 0.005) ("差异 {0:P1}" -f $tlAfter.Ratio)

    # ------------------------------------------------------------ 界面日志
    $log = Join-Path (Split-Path -Parent (Join-Path $root $Db)) 'desktop.log'
    if (Test-Path $log) {
        $lines = Get-Content $log -Encoding UTF8 -Tail 40
        $pages = @($lines | Where-Object { $_ -like '*界面 · page:*' })
        Check '界面加载有上报' ($pages.Count -gt 0) ($pages[-1] -replace '^\[[^\]]+\] ', '')
        $errors = @($lines | Where-Object { $_ -like '*onerror*' -or $_ -like '*unhandled*' })
        Check '界面没有 JS 异常' ($errors.Count -eq 0) ($errors | Select-Object -First 1)
    } else {
        Check '界面日志存在' $false $log
    }
} finally {
    Set-CursorPos -X $cursor.X -Y $cursor.Y
    [void](Set-WindowFocus)
}

# ---------------------------------------------------------------- 汇总

$failed = @($script:Results | Where-Object { -not $_.Ok })
Write-Output ""
Write-Output ("{0} 项，通过 {1}，失败 {2}" -f $script:Results.Count,
              ($script:Results.Count - $failed.Count), $failed.Count)
Write-Output "截图留在 $OutDir，可以对着看"

if ($failed.Count -gt 0) { exit 1 }
exit 0
