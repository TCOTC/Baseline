# 调试桥的客户端。窗口带 --debug 起来之后，用它代替「启动 → 截图 → 人眼看图」。
#
# 用法：
#     Import-Module scripts\dbg.psm1
#     Start-Baseline -Db data/_demo.db          # 起窗口并等到真的能应答（不睡固定秒数）
#     Invoke-Js "document.querySelectorAll('.card').length"
#     Invoke-ClickOn '.send'
#     Stop-Baseline
#
# 为什么值得有：窗口没有开发工具，界面元素只能靠截图目测坐标去点。
# 一次「点错了 → 再截图看 → 再点」要好几轮往返；这里问一次坐标就够了。
# 详见 src-tauri/src/debug.rs。

$ErrorActionPreference = 'Stop'

$script:PortFile = $null
$script:Port = $null

function Get-PortFile {
    param([string]$Db)
    $dir = Split-Path -Parent (Join-Path (Get-Location) $Db)
    Join-Path $dir 'debug.port'
}

<#
  起一个带调试桥的窗口，**等到它真的能应答**再返回。
  以前是 Start-Sleep 8——那八秒是纯等的，而窗口其实两百毫秒就起来了。
#>
function Start-Baseline {
    param(
        [string]$Db = 'data/_demo.db',
        [string]$Exe = 'target\debug\baseline-desktop.exe',
        [int]$TimeoutSec = 30
    )
    Stop-Baseline
    $portFile = Get-PortFile $Db
    Remove-Item $portFile -ErrorAction SilentlyContinue

    Start-Process $Exe -ArgumentList '--db', $Db, '--debug' -WorkingDirectory (Get-Location) | Out-Null

    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        if (Test-Path $portFile) {
            $script:Port = (Get-Content $portFile -Raw).Trim()
            $script:PortFile = $portFile
            try {
                $null = Invoke-RestMethod "http://127.0.0.1:$script:Port/ping" -TimeoutSec 2
                return $script:Port
            } catch { }
        }
        Start-Sleep -Milliseconds 100
    }
    throw "窗口起来了但调试桥没应答（$TimeoutSec 秒）"
}

function Stop-Baseline {
    Get-Process baseline-desktop -ErrorAction SilentlyContinue | Stop-Process -Force
    Start-Sleep -Milliseconds 300
}

function Get-DebugPort {
    <#
      端口从哪来：先看本进程记着的，再看传进来的库，最后**去 data/ 下面找**。
      最后那一步是为了「每次调用都是一个新 pwsh 进程」这件事——
      模块变量活不过一次调用，而调试多半就是一条一条地敲。
    #>
    param([string]$Db)
    if ($script:Port) { return $script:Port }

    $file = if ($Db) { Get-PortFile $Db } else { $null }
    if (-not $file -or -not (Test-Path $file)) {
        $found = Get-ChildItem -Path 'data' -Filter 'debug.port' -Recurse -ErrorAction SilentlyContinue |
            Sort-Object LastWriteTime -Descending | Select-Object -First 1
        if ($found) { $file = $found.FullName }
    }
    if (-not $file -or -not (Test-Path $file)) {
        throw '找不到 debug.port —— 窗口是不是没带 --debug 起来？'
    }
    $script:Port = (Get-Content $file -Raw).Trim()
    $script:PortFile = $file
    $script:Port
}

<#
  在活着的页面里跑一段 JS，把值拿回来（字符串）。
  页面正好在整页重来时会超时——那是一次正常的重试，不是错误。
#>
function Invoke-Js {
    param([Parameter(Mandatory)][string]$Code, [int]$TimeoutSec = 15)
    $port = Get-DebugPort
    $r = Invoke-RestMethod "http://127.0.0.1:$port/eval" -Method Post -Body $Code -TimeoutSec $TimeoutSec
    if (-not $r.ok) { throw "页面里报错：$($r.e)" }
    # 值是对象时给回 JSON 文本，方便外面 ConvertFrom-Json。
    if ($r.v -is [string]) { return $r.v }
    return ($r.v | ConvertTo-Json -Compress -Depth 8)
}

function Invoke-JsJson {
    param([Parameter(Mandatory)][string]$Code, [int]$TimeoutSec = 15)
    (Invoke-Js -Code $Code -TimeoutSec $TimeoutSec) | ConvertFrom-Json
}

<#
  按选择器点。**先问坐标，再按坐标点**——不再靠看截图估。
  返回它点到哪儿了，这样可以自己核对一眼。
#>
function Invoke-ClickOn {
    param(
        [Parameter(Mandatory)][string]$Selector,
        [int]$Index = 0,
        [int]$SettleMs = 300,
        [switch]$NoWait
    )
    $sel = $Selector | ConvertTo-Json -Compress
    $code = @"
(function(){
  var els = document.querySelectorAll($sel);
  if (!els.length) return { found: 0 };
  var el = els[$Index];
  if (!el) return { found: els.length, error: 'index out of range' };
  var r = el.getBoundingClientRect();
  return { found: els.length, x: r.left + r.width / 2, y: r.top + r.height / 2, w: r.width, h: r.height };
})()
"@
    $box = Invoke-JsJson -Code $code
    if ($box.found -eq 0) { throw "找不到元素：$Selector" }
    if ($null -eq $box.x) { throw "选中了但拿不到坐标：$Selector（$($box.error)）" }

    Import-Module (Join-Path $PSScriptRoot 'ui.psm1') -Force
    Invoke-Click -X $box.x -Y $box.y -SettleMs $SettleMs
    [pscustomobject]@{ Selector = $Selector; Index = $Index; X = [int]$box.x; Y = [int]$box.y }
}

function Invoke-TypeInto {
    <#
      往输入框里写值。**不经过键盘**：键盘会撞上输入法（打过「a summary」变成「啊」），
      而中文经过命令行又会被代码页吃掉（打过「读完 CSAPP 第 9 章」，库里存成
      「?? CSAPP ? 9 ?」）。所以：

      - 要写中文就用 -TextFile 从文件读（文件是 UTF-8，命令行上只出现一个 ASCII 路径）；
      - 文本一律 base64 编码之后再进页面，HTTP body 全程是 ASCII，不赌任何一层的字符集。
    #>
    param(
        [Parameter(Mandatory)][string]$Selector,
        [string]$Text,
        [string]$TextFile
    )
    if ($TextFile) { $Text = Get-Content -LiteralPath $TextFile -Raw -Encoding UTF8 }
    if (-not $Text) { throw '要写点什么：-Text 或者 -TextFile' }

    $sel = $Selector | ConvertTo-Json -Compress
    $b64 = [Convert]::ToBase64String([System.Text.Encoding]::UTF8.GetBytes($Text))
    $js = "var el=document.querySelector($sel); el.focus(); el.value=decodeURIComponent(escape(atob('$b64'))); el.dispatchEvent(new Event('input')); 'ok'"
    Invoke-Js -Code $js | Out-Null
}

function Get-UiLog {
    param([int]$Tail = 20)
    $log = 'data/desktop.log'
    if (-not (Test-Path $log)) { return @() }
    Get-Content $log -Encoding UTF8 -Tail $Tail
}

<# 窗口等的就是这一句：整页重来之后，能按选择器干活了才算真的就绪。 #>
function Wait-Page {
    param([string]$Selector = '.composer', [int]$TimeoutSec = 15)
    $sel = $Selector | ConvertTo-Json -Compress
    $deadline = (Get-Date).AddSeconds($TimeoutSec)
    while ((Get-Date) -lt $deadline) {
        try {
            if ((Invoke-Js "!!document.querySelector($sel)") -eq 'True') { return $true }
        } catch { }
        Start-Sleep -Milliseconds 150
    }
    return $false
}

Export-ModuleMember -Function Start-Baseline, Stop-Baseline, Invoke-Js, Invoke-JsJson,
    Invoke-ClickOn, Invoke-TypeInto, Get-UiLog, Wait-Page, Get-DebugPort
