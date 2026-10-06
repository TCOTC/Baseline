# 基线 UI 测试台。
#
# 窗口是「无系统边框 + 自绘顶栏」，很多行为没法靠读代码确认：
# 拖动生不生效、最大化后图标变不变、两栏是不是真的各滚各的、窗口按钮画得
# 跟系统像不像。这些只能真的动鼠标、再去量屏幕。
#
# 用法：
#     Import-Module .\scripts\ui.psm1
#     $g = Get-WindowGeometry
#     Invoke-Click -X 1211 -Y 20          # CSS 坐标，函数内部换算成屏幕像素
#
# 坐标一律用 CSS 像素（和 assets/view.css 同一套数），换算由这里负责。
# 量像素的活儿交给调用方，这个模块只负责「动」和「看」。
#
# ⚠ 本文件必须是 UTF-8 with BOM。Windows PowerShell 5.1 会把无 BOM 的脚本按
#   ANSI 读，中文注释变乱码并直接导致语法错误（scripts/demo.ps1 踩过一次）。

Add-Type -AssemblyName System.Drawing

if (-not ([System.Management.Automation.PSTypeName]'BaselineUi').Type) {
Add-Type @"
using System;
using System.Text;
using System.Runtime.InteropServices;
public class BaselineUi {
  [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
  [StructLayout(LayoutKind.Sequential)] public struct POINT { public int X, Y; }
  public delegate bool EnumProc(IntPtr h, IntPtr p);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
  [DllImport("user32.dll")] public static extern bool ShowWindow(IntPtr h, int n);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsZoomed(IntPtr h);
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool GetClientRect(IntPtr h, out RECT r);
  [DllImport("user32.dll")] public static extern bool ClientToScreen(IntPtr h, ref POINT p);
  [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
  [DllImport("user32.dll")] public static extern bool GetCursorPos(out POINT p);
  [DllImport("user32.dll")] public static extern void mouse_event(uint f, uint dx, uint dy, int d, IntPtr e);
  [DllImport("user32.dll")] public static extern uint GetDpiForWindow(IntPtr h);
  [DllImport("dwmapi.dll")] public static extern int DwmGetWindowAttribute(IntPtr h, int a, out RECT r, int s);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr p);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassNameW(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr hdc, uint flags);
  [DllImport("user32.dll")] public static extern bool AttachThreadInput(uint from, uint to, bool attach);
  [DllImport("kernel32.dll")] public static extern uint GetCurrentThreadId();

  // 把窗口提到前台，返回是否真的成功。
  //
  // 光调 SetForegroundWindow 从后台进程调用通常会被拒绝（Windows 的前台锁定）。
  // 这里先把本线程的输入队列挂到当前前台线程上，置前之后再摘掉。
  //
  // 为什么非置前不可：Windows 的鼠标滚轮消息只发给**前台窗口**。
  // 窗口不在前台时滚轮石沉大海，界面一动不动，看起来就像「独立滚动根本没实现」。
  public static bool ForceForeground(IntPtr h) {
    if (GetForegroundWindow() == h) return true;
    IntPtr fg = GetForegroundWindow();
    uint dummy;
    uint fgThread = fg == IntPtr.Zero ? 0u : GetWindowThreadProcessId(fg, out dummy);
    uint myThread = GetCurrentThreadId();
    bool attached = false;
    if (fgThread != 0 && fgThread != myThread) attached = AttachThreadInput(myThread, fgThread, true);
    ShowWindow(h, 9);   // SW_RESTORE
    SetForegroundWindow(h);
    if (attached) AttachThreadInput(myThread, fgThread, false);
    return GetForegroundWindow() == h;
  }

  // 按窗口类名找窗口。
  //
  // 绝不能用 Process.MainWindowHandle：那是「本进程第一个可见且有标题的顶层窗口」，
  // 按 Z 序取。debug 构建带一个控制台窗口，应用窗口一最小化就掉到 Z 序底部，
  // MainWindowHandle 立刻指到控制台上——于是「点最小化后窗口变成 980x511 且
  // 再也点不动」这种假 bug 就出现了，而且看起来非常像一个真 bug。查了很久。
  public static IntPtr FindByClass(uint pid, string cls) {
    IntPtr found = IntPtr.Zero;
    EnumWindows(delegate(IntPtr h, IntPtr p) {
      uint wpid; GetWindowThreadProcessId(h, out wpid);
      if (wpid != pid) return true;
      StringBuilder sb = new StringBuilder(256);
      GetClassNameW(h, sb, 256);
      if (sb.ToString() == cls) { found = h; return false; }
      return true;
    }, IntPtr.Zero);
    return found;
  }

  // 兜底：类名认不出来时，挑本进程里可见且面积最大的窗口。
  public static IntPtr LargestVisible(uint pid) {
    IntPtr best = IntPtr.Zero; long bestArea = 0;
    EnumWindows(delegate(IntPtr h, IntPtr p) {
      uint wpid; GetWindowThreadProcessId(h, out wpid);
      if (wpid != pid || !IsWindowVisible(h)) return true;
      RECT r; GetWindowRect(h, out r);
      long area = (long)(r.Right - r.Left) * (r.Bottom - r.Top);
      if (area > bestArea) { bestArea = area; best = h; }
      return true;
    }, IntPtr.Zero);
    return best;
  }
}
"@
}

# 不做 DPI 感知的话，DWM 报的是物理像素而 GDI 读写的是逻辑像素，
# 抓出来的图会整体偏移并超出屏幕边界。
[void][BaselineUi]::SetProcessDPIAware()

# 顶栏几何。必须和 assets/view.css 里的 .bar / .wbtn 保持一致——
# 改了 CSS 就要改这里，否则点击会落到空白处（第一次就踩过：顶栏移出 .wrap
# 之后按钮整体右移了一百多像素，测试却还在报「按钮没反应」）。
$script:BarHeight   = 40
$script:ButtonWidth = 46
$script:ProcessName = 'baseline-desktop'

function Set-UiProcess {
    <#  改默认目标进程。量系统标题栏（measure-caption.ps1）时要指到别的窗口上。 #>
    param([Parameter(Mandatory)][string]$Name)
    $script:ProcessName = $Name
}

function Get-UiProcessName { $script:ProcessName }

function Get-AppWindow {
    <#  定位应用窗口。返回 { Process, Id, Handle }。
        没有窗口就抛错——测试里静默失败比报错难查得多。

        必须遍历所有同名进程：Electron 应用（比如拿来当基准的 DeepSeek Harness）
        一个名字下面挂着主进程和一堆渲染进程，头一个多半是没有窗口的那个。 #>
    $procs = @(Get-Process $script:ProcessName -ErrorAction SilentlyContinue)
    if ($procs.Count -eq 0) { throw "找不到进程 $($script:ProcessName)，先把它启动起来" }
    foreach ($p in $procs) {
        $h = [BaselineUi]::FindByClass([uint32]$p.Id, 'Tauri Window')
        if ($h -eq [IntPtr]::Zero) { $h = [BaselineUi]::LargestVisible([uint32]$p.Id) }
        if ($h -ne [IntPtr]::Zero) {
            return [pscustomobject]@{ Process = $p; Id = $p.Id; Handle = $h }
        }
    }
    throw "进程 $($script:ProcessName)（$($procs.Count) 个）都没有可见的顶层窗口"
}

function Get-WindowGeometry {
    <#  窗口客户区的屏幕位置、DWM 边框、DPI 缩放、CSS 尺寸。 #>
    $h = (Get-AppWindow).Handle
    $c = New-Object BaselineUi+RECT
    [void][BaselineUi]::GetClientRect($h, [ref]$c)
    $o = New-Object BaselineUi+POINT
    [void][BaselineUi]::ClientToScreen($h, [ref]$o)
    # DWM 的可见边框才是真正的视觉边界；GetWindowRect 含不可见的调整边框。
    $f = New-Object BaselineUi+RECT
    if ([BaselineUi]::DwmGetWindowAttribute($h, 9, [ref]$f, 16) -ne 0) {
        [void][BaselineUi]::GetWindowRect($h, [ref]$f)
    }
    # 缩放只能从窗口 DPI 取。按「客户区宽 = 1280」反推的话，
    # 窗口一最大化就算错（那次的第二次点击就是这么失败的）。
    $scale = [BaselineUi]::GetDpiForWindow($h) / 96.0
    [pscustomobject]@{
        Handle    = $h
        OriginX   = $o.X
        OriginY   = $o.Y
        FrameLeft = $f.Left
        FrameTop  = $f.Top
        FrameW    = $f.Right - $f.Left
        FrameH    = $f.Bottom - $f.Top
        Scale     = $scale
        CssWidth  = ($c.Right - $c.Left) / $scale
        CssHeight = ($c.Bottom - $c.Top) / $scale
    }
}

function ConvertTo-ScreenPoint {
    <#  CSS 坐标 -> 屏幕物理坐标。窗口移动或缩放后会重新取几何。 #>
    param([Parameter(Mandatory)][double]$X, [Parameter(Mandatory)][double]$Y)
    $g = Get-WindowGeometry
    [pscustomobject]@{
        X = $g.OriginX + [int]($X * $g.Scale)
        Y = $g.OriginY + [int]($Y * $g.Scale)
    }
}

function Get-CaptionButtonPoint {
    <#  顶栏三个按钮中心的 CSS 坐标。
        顶栏横跨整个窗口，所以从客户区右边缘往里量，不受内容宽度影响。 #>
    param([Parameter(Mandatory)][ValidateSet('min', 'max', 'close')][string]$Which)
    $g = Get-WindowGeometry
    # 关闭键最靠右：中心距右边缘 23px（46 宽的一半）。
    $offset = switch ($Which) { 'close' { 23 } 'max' { 69 } 'min' { 115 } }
    [pscustomobject]@{
        X = $g.CssWidth - $offset
        Y = $script:BarHeight / 2
    }
}

function Set-CursorTo {
    param([Parameter(Mandatory)][double]$X, [Parameter(Mandatory)][double]$Y)
    $p = ConvertTo-ScreenPoint -X $X -Y $Y
    [void][BaselineUi]::SetCursorPos($p.X, $p.Y)
    $p
}

function Get-CursorPos {
    $p = New-Object BaselineUi+POINT
    [void][BaselineUi]::GetCursorPos([ref]$p)
    [pscustomobject]@{ X = $p.X; Y = $p.Y }
}

function Set-CursorPos {
    <#  直接给屏幕物理坐标。换算过的场景用 Set-CursorTo。 #>
    param([Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y)
    [void][BaselineUi]::SetCursorPos($X, $Y)
}

function Invoke-Click {
    <#  在 CSS 坐标处左键点一下。 #>
    param([Parameter(Mandatory)][double]$X, [Parameter(Mandatory)][double]$Y,
          [int]$SettleMs = 700)
    [void](Set-CursorTo -X $X -Y $Y)
    Start-Sleep -Milliseconds 200
    [BaselineUi]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)   # LEFTDOWN
    Start-Sleep -Milliseconds 60
    [BaselineUi]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)   # LEFTUP
    Start-Sleep -Milliseconds $SettleMs
}

function Invoke-Drag {
    <#  从 (FromX,FromY) 按住拖到 (ToX,ToY)，全程用 CSS 坐标。
        分步移动而不是一步到位：一步跳过去系统不会认成拖动。 #>
    param(
        [Parameter(Mandatory)][double]$FromX, [Parameter(Mandatory)][double]$FromY,
        [Parameter(Mandatory)][double]$ToX,   [Parameter(Mandatory)][double]$ToY,
        [int]$Steps = 12, [int]$SettleMs = 700
    )
    [void](Set-CursorTo -X $FromX -Y $FromY)
    Start-Sleep -Milliseconds 250
    [BaselineUi]::mouse_event(0x0002, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds 250
    for ($i = 1; $i -le $Steps; $i++) {
        $t = $i / $Steps
        [void](Set-CursorTo -X ($FromX + ($ToX - $FromX) * $t) -Y ($FromY + ($ToY - $FromY) * $t))
        Start-Sleep -Milliseconds 40
    }
    Start-Sleep -Milliseconds 250
    [BaselineUi]::mouse_event(0x0004, 0, 0, 0, [IntPtr]::Zero)
    Start-Sleep -Milliseconds $SettleMs
}

function Invoke-Wheel {
    <#  在 CSS 坐标处滚轮。**正数向上、负数向下**（和 Win32 的 WHEEL_DELTA 约定一致）。
        滚轮只发给前台窗口，所以先确保窗口在前台，否则这一下完全没有反应。 #>
    param([Parameter(Mandatory)][double]$X, [Parameter(Mandatory)][double]$Y,
          [Parameter(Mandatory)][int]$Notches, [int]$SettleMs = 700)
    [void](Set-WindowFocus)
    [void](Set-CursorTo -X $X -Y $Y)
    Start-Sleep -Milliseconds 250
    [BaselineUi]::mouse_event(0x0800, 0, 0, (120 * $Notches), [IntPtr]::Zero)
    Start-Sleep -Milliseconds $SettleMs
}

function Save-Shot {
    <#  截窗口。**优先用 PrintWindow，不要求窗口在前台。**

        原来是屏幕取图（CopyFromScreen），因此必须先把窗口提到最前面；
        而抢前台在「他正在用电脑」的时候会失败——失败的表现是**截到别人的窗口**，
        然后一堆检查报「顶栏坏了 / 最右墨迹离边一千像素」，长得像 CSS 回归。
        这种假失败比没有测试更糟。

        当初不用 PrintWindow 是因为「WebView2 走 GPU 合成，抓出来是空白」。
        那是老经验：Windows 10 1809 之后 PrintWindow 支持 PW_RENDERFULLCONTENT(2)，
        对 GPU 合成的窗口一样能抓。这里先试它，**抓出来不是空白就用**；
        真空白（老系统）再退回屏幕取图，并在返回对象上标明，
        免得把「截了个空窗」当成「界面是空的」。#>
    param([Parameter(Mandatory)][string]$Path)
    $g = Get-WindowGeometry
    if (-not $g.FrameW) { throw "窗口尺寸为 0" }
    $dir = Split-Path -Parent $Path
    if ($dir -and -not (Test-Path $dir)) { New-Item -ItemType Directory -Force -Path $dir | Out-Null }

    $bmp = New-Object System.Drawing.Bitmap($g.FrameW, $g.FrameH)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $hdc = $gfx.GetHdc()
    $ok = [BaselineUi]::PrintWindow($g.Handle, $hdc, 2)
    $gfx.ReleaseHdc($hdc)
    $gfx.Dispose()

    if ($ok -and (Test-NotBlank -Bitmap $bmp)) {
        $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
        $bmp.Dispose()
        return Get-Item $Path
    }
    $bmp.Dispose()

    if (-not (Test-IsForeground)) {
        throw "PrintWindow 抓不到内容，而且窗口不在前台——这一张不可信，没有保存。"
    }
    $bmp = New-Object System.Drawing.Bitmap($g.FrameW, $g.FrameH)
    $gfx = [System.Drawing.Graphics]::FromImage($bmp)
    $gfx.CopyFromScreen($g.FrameLeft, $g.FrameTop, 0, 0,
                        (New-Object System.Drawing.Size($g.FrameW, $g.FrameH)))
    $bmp.Save($Path, [System.Drawing.Imaging.ImageFormat]::Png)
    $gfx.Dispose(); $bmp.Dispose()
    Get-Item $Path
}

function Test-NotBlank {
    <#  一张图是不是「基本单一颜色」。抽样即可——全白/全黑是 PrintWindow
        失败唯一的表现形式，不需要精确判断。 #>
    param([Parameter(Mandatory)]$Bitmap)
    $seen = @{}
    for ($y = 0; $y -lt $Bitmap.Height; $y += [math]::Max(1, [int]($Bitmap.Height / 40))) {
        for ($x = 0; $x -lt $Bitmap.Width; $x += [math]::Max(1, [int]($Bitmap.Width / 40))) {
            $seen[$Bitmap.GetPixel($x, $y).ToArgb()] = $true
            if ($seen.Count -gt 3) { return $true }
        }
    }
    return $false
}

function Set-WindowFocus {
    <#  把窗口提到前台，**返回是否真的成功**——滚轮需要前台，失败必须能看出来，
        否则「滚动没生效」和「窗口没在前台」会混成同一个现象。 #>
    $h = (Get-AppWindow).Handle
    for ($i = 0; $i -lt 5; $i++) {
        if ([BaselineUi]::ForceForeground($h)) {
            Start-Sleep -Milliseconds 250
            return $true
        }
        Start-Sleep -Milliseconds 200
    }
    return $false
}

function Test-IsForeground {
    [BaselineUi]::GetForegroundWindow() -eq (Get-AppWindow).Handle
}

function Test-Maximized  { [BaselineUi]::IsZoomed((Get-AppWindow).Handle) }
function Test-Minimized  { [BaselineUi]::IsIconic((Get-AppWindow).Handle) }

function Get-WindowRectCss {
    <#  窗口在屏幕上的位置与尺寸，换算成 CSS 像素。 #>
    $g = Get-WindowGeometry
    [pscustomobject]@{
        X = [math]::Round($g.FrameLeft / $g.Scale, 1)
        Y = [math]::Round($g.FrameTop / $g.Scale, 1)
        W = [math]::Round($g.FrameW / $g.Scale, 1)
        H = [math]::Round($g.FrameH / $g.Scale, 1)
    }
}

function Test-SameRegion {
    <#  比较两张截图在指定矩形内是否相同（按 Step 抽样，不是逐像素）。
        用来判断「这一栏到底动没动」——滚轮的独立滚动没法靠读代码确认。 #>
    param(
        [Parameter(Mandatory)][string]$PathA, [Parameter(Mandatory)][string]$PathB,
        [Parameter(Mandatory)][int]$X, [Parameter(Mandatory)][int]$Y,
        [Parameter(Mandatory)][int]$W, [Parameter(Mandatory)][int]$H,
        [int]$Step = 3
    )
    $a = [System.Drawing.Bitmap]::FromFile((Resolve-Path $PathA))
    $b = [System.Drawing.Bitmap]::FromFile((Resolve-Path $PathB))
    try {
        $diff = 0; $n = 0
        for ($yy = $Y; $yy -lt [math]::Min($Y + $H, $a.Height); $yy += $Step) {
            for ($xx = $X; $xx -lt [math]::Min($X + $W, $a.Width); $xx += $Step) {
                $n++
                if ($a.GetPixel($xx, $yy).ToArgb() -ne $b.GetPixel($xx, $yy).ToArgb()) { $diff++ }
            }
        }
        $ratio = 0.0
        if ($n -gt 0) { $ratio = $diff / $n }
        [pscustomobject]@{ Sampled = $n; Differ = $diff; Ratio = $ratio }
    } finally { $a.Dispose(); $b.Dispose() }
}

Export-ModuleMember -Function `
    Set-UiProcess, Get-UiProcessName,
    Get-AppWindow, Get-WindowGeometry, ConvertTo-ScreenPoint, Get-CaptionButtonPoint,
    Set-CursorTo, Set-CursorPos, Get-CursorPos, Invoke-Click, Invoke-Drag, Invoke-Wheel,
    Save-Shot, Set-WindowFocus, Test-IsForeground, Test-Maximized, Test-Minimized,
    Get-WindowRectCss, Test-SameRegion
