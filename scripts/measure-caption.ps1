# 量一个真实 Windows 标题栏的窗口按钮：尺寸、图标几何、颜色。
#
# 这是 assets/view.css 里 --cap-* 那组令牌的**出处**。那些值不是设计的，是量的——
# 按设计语言自己画的按钮，一眼就能看出不是系统的东西。系统换主题、换 DPI、
# 或者大版本更新之后，重跑一遍就知道该改哪个数。
#
# 用法：
#     powershell -ExecutionPolicy Bypass -File scripts/measure-caption.ps1 -Process regedit
#     powershell -ExecutionPolicy Bypass -File scripts/measure-caption.ps1 -Process "DeepSeek Harness" -Hover
#
# -Hover 只移动光标，不点击。**永远不会去按按钮**——按到关闭键就把人家的窗口关了。
#
# ⚠ 量之前请手动把目标窗口点到前台。Windows 只在**活动窗口**上给非关闭键画悬停填充，
#   而且从后台进程调用 SetForegroundWindow 会被前台锁定拒绝。悬停测出来是底色，
#   多半就是这个原因。
#
# ⚠ 标题栏高度这个工具不测。按底色跳变推出来的值不可靠：标题栏和内容同色时
#   （Electron 的 titleBarOverlay 就是这样，这正是 WCO 的意义）推不出东西，
#   不同色时又容易被内容里的第一处变化骗到。宁可没有，也不给一个错的参考值。
#   按钮的几何和颜色才是这里要量的东西。
#
# ⚠ 本文件必须是 UTF-8 with BOM。

param(
    [string]$Process = 'regedit',
    [switch]$Hover,
    [string]$OutDir = 'data/_ui'
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root
Import-Module (Join-Path $PSScriptRoot 'ui.psm1') -Force
Set-UiProcess -Name $Process

if (-not (Test-Path $OutDir)) { New-Item -ItemType Directory -Force -Path $OutDir | Out-Null }

$g = Get-WindowGeometry
Write-Output ""
Write-Output "窗口  $Process  $($g.FrameW)x$($g.FrameH) 物理像素  DPI 缩放 $($g.Scale)"

$shot = Join-Path $OutDir 'caption-reference.png'
Save-Shot -Path $shot | Out-Null
$bmp = [System.Drawing.Bitmap]::FromFile((Resolve-Path $shot))
try {
    $W = $bmp.Width
    $offX = $g.OriginX - $g.FrameLeft
    $offY = $g.OriginY - $g.FrameTop

    # 扫描带：顶部 80 css（标题栏再高也不会超过），右起 400 css。
    $bandTop = $offY
    $bandBot = [math]::Min($offY + [int](80 * $g.Scale), $bmp.Height - 1)
    $scanX0 = [math]::Max(0, $W - [int](400 * $g.Scale))

    # 底色从顶栏里一处确定空着的地方取，不要取窗口下沿（那是 DWM 阴影）。
    $bgX = [math]::Min($offX + [int](20 * $g.Scale), $bmp.Width - 1)
    $bg = $bmp.GetPixel($bgX, $offY + 4)
    Write-Output ("底色         #{0:X2}{1:X2}{2:X2}  （客户区左上角起 20css 处）" -f $bg.R, $bg.G, $bg.B)

    # 标题栏高度这里不测。按底色跳变推出来的值不可靠——标题栏和内容同色时（WCO 就是这样）
    # 推不出东西，不同色时又容易被内容里的第一处变化骗到。宁可没有，也不给一个错的参考值。
    Write-Output ""

    # 图标：比底色明显深的像素。
    $bgSum = $bg.R + $bg.G + $bg.B
    $isInk = {
        param($c)
        ($c.R + $c.G + $c.B) -lt ($bgSum * 0.75)
    }

    # 先按行数墨迹，取**最上面那段连续行**——那是标题栏图标所在的竖直范围。
    # 不能在整个扫描带里直接切列：标题栏下面紧跟着页面内容，
    # 会把图标的包围盒一路撑到页面里去（关闭键量出过 56.7 css 高）。
    $rowInk = @{}
    for ($y = $bandTop; $y -lt $bandBot; $y++) {
        $n = 0
        for ($x = $scanX0; $x -lt $W - 1; $x++) {
            $c = $bmp.GetPixel($x, $y)
            if (($c.R + $c.G + $c.B) -lt ($bgSum * 0.75)) { $n++ }
        }
        $rowInk[$y] = $n
    }
    $glyphTop = -1; $glyphBot = -1; $gap = 0
    for ($y = $bandTop; $y -lt $bandBot; $y++) {
        if ($rowInk[$y] -gt 0) {
            if ($glyphTop -lt 0) { $glyphTop = $y }
            $glyphBot = $y
            $gap = 0
        } elseif ($glyphTop -ge 0) {
            $gap++
            if ($gap -gt 2) { break }
        }
    }
    if ($glyphTop -lt 0) {
        Write-Output "没找到图标。窗口可能不是活动状态，或者这一带确实没有东西。"
        exit 0
    }
    $glyphBot = $glyphBot - $gap
    Write-Output ("图标竖直范围 y={0}..{1}（{2} 物理 = {3:N1} css）" -f $glyphTop, $glyphBot,
                  ($glyphBot - $glyphTop + 1), (($glyphBot - $glyphTop + 1) / $g.Scale))
    Write-Output ""

    $cols = New-Object System.Collections.ArrayList
    for ($x = $scanX0; $x -lt $W - 1; $x++) {
        for ($y = $glyphTop; $y -le $glyphBot; $y++) {
            if (& $isInk $bmp.GetPixel($x, $y)) { [void]$cols.Add($x); break }
        }
    }
    if ($cols.Count -eq 0) {
        Write-Output "没找到图标。"
        exit 0
    }

    # 按横向间隙切成若干簇
    $groups = New-Object System.Collections.ArrayList
    $cur = New-Object System.Collections.ArrayList
    [void]$cur.Add($cols[0])
    for ($i = 1; $i -lt $cols.Count; $i++) {
        if ($cols[$i] - $cur[$cur.Count - 1] -le 6) { [void]$cur.Add($cols[$i]) }
        else {
            [void]$groups.Add($cur)
            $cur = New-Object System.Collections.ArrayList
            [void]$cur.Add($cols[$i])
        }
    }
    [void]$groups.Add($cur)

    # 最右边那几簇就是关闭 / 最大化 / 最小化
    $names = @('关闭', '最大化/还原', '最小化')
    Write-Output ("{0,-14} {1,-20} {2,-20} {3,-10} {4}" -f '按钮', '图标宽', '图标高', '墨色', '通道色差')
    $centers = @()
    $k = 0
    for ($gi = $groups.Count - 1; $gi -ge 0 -and $k -lt 3; $gi--) {
        $grp = $groups[$gi]
        $ys = New-Object System.Collections.ArrayList
        foreach ($x in $grp) {
            for ($y = $glyphTop; $y -le $glyphBot; $y++) {
                if (& $isInk $bmp.GetPixel($x, $y)) { [void]$ys.Add($y) }
            }
        }
        if ($ys.Count -eq 0) { continue }
        $minX = $grp[0]; $maxX = $grp[$grp.Count - 1]
        $minY = ($ys | Measure-Object -Minimum).Minimum
        $maxY = ($ys | Measure-Object -Maximum).Maximum
        # 最深的一个像素当墨色；通道最大差就是次像素抗锯齿留下的痕迹。
        $darkest = $null; $spread = 0
        foreach ($x in $grp) {
            for ($y = $glyphTop; $y -le $glyphBot; $y++) {
                $c = $bmp.GetPixel($x, $y)
                if (-not (& $isInk $c)) { continue }
                if ($null -eq $darkest -or ($c.R + $c.G + $c.B) -lt ($darkest.R + $darkest.G + $darkest.B)) { $darkest = $c }
                $d = [math]::Max($c.R, [math]::Max($c.G, $c.B)) - [math]::Min($c.R, [math]::Min($c.G, $c.B))
                if ($d -gt $spread) { $spread = $d }
            }
        }
        $label = '按钮'
        if ($k -lt $names.Count) { $label = $names[$k] }
        $k++
        $centers += [pscustomobject]@{ Name = $label; X = [int](($minX + $maxX) / 2); Y = [int](($minY + $maxY) / 2) }
        Write-Output ("{0,-14} {1,-20} {2,-20} {3,-10} {4}" -f
            $label,
            ("{0} 物理 ({1:N1} css)" -f ($maxX - $minX + 1), (($maxX - $minX + 1) / $g.Scale)),
            ("{0} 物理 ({1:N1} css)" -f ($maxY - $minY + 1), (($maxY - $minY + 1) / $g.Scale)),
            ("#{0:X2}{1:X2}{2:X2}" -f $darkest.R, $darkest.G, $darkest.B),
            $spread)
    }

    if ($centers.Count -ge 2) {
        $pitch = [math]::Abs($centers[0].X - $centers[1].X) / $g.Scale
        $gapRight = ($W - 1 - $centers[0].X) / $g.Scale
        Write-Output ""
        Write-Output ("按钮间距     {0:N1} css（等于按钮宽度）" -f $pitch)
        Write-Output ("关闭键中心   距窗口右边缘 {0:N1} css" -f $gapRight)
    }

    # ---- 悬停取样（只移动光标，不点击） ----
    if ($Hover) {
        Write-Output ""
        Write-Output "悬停取样（只移动光标，不点击）"
        $saved = Get-CursorPos
        foreach ($b in $centers) {
            # 按钮内部的空白处，避开图标本身
            $px = $b.X - [int](16 * $g.Scale)
            $py = $offY + [int](8 * $g.Scale)
            Set-CursorPos -X $px -Y $py
            Start-Sleep -Milliseconds 700
            $tmp = New-Object System.Drawing.Bitmap($g.FrameW, $g.FrameH)
            $gfx = [System.Drawing.Graphics]::FromImage($tmp)
            $gfx.CopyFromScreen($g.FrameLeft, $g.FrameTop, 0, 0,
                                (New-Object System.Drawing.Size($g.FrameW, $g.FrameH)))
            $c = $tmp.GetPixel(($px - $g.FrameLeft), ($py - $g.FrameTop))
            $gfx.Dispose(); $tmp.Dispose()
            Write-Output ("  {0,-14} #{1:X2}{2:X2}{3:X2}" -f $b.Name, $c.R, $c.G, $c.B)
        }
        Set-CursorPos -X $saved.X -Y $saved.Y
        Write-Output "  光标已还原"
    }
} finally { $bmp.Dispose() }

Write-Output ""
Write-Output "截图 $shot"
