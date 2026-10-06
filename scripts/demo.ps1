# 用演示数据跑通全流程，用来验证渲染结果。
# 用法：pwsh -File scripts/demo.ps1
# 会写 data/_demo.db（data/ 已被 gitignore）

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

$exe = Join-Path $root 'target\debug\baseline.exe'
if (-not (Test-Path $exe)) { throw "先 cargo build" }

$db = 'data/_demo.db'
if (Test-Path $db) { Remove-Item $db -Force }
Remove-Item "$db-wal", "$db-shm" -Force -ErrorAction SilentlyContinue

function B { & $exe --db $db @args }

Write-Output "── init ──" 
B init

Write-Output "`n── 建目标 ──" 
B goal add "计算机基础" --why "基础知识匮乏，想打扎实" --color blue
B goal add "英语"       --why "想看懂英文文档，以后可能考研" --color green
B goal add "不用 AI 也能做" --why "自己的能力不能完全依赖 AI" --color amber

Write-Output "`n── 写判定规则（什么算推进它）──" 
B source add "计算机基础" --kind manual_checkin --rationale "读完一章，或做完一章题，算一次"
B source add "英语" --kind external_metric --target "Learn-English 复习记录" --rationale "只算复习记录；在上面写代码的提交不算"
B source add "不用 AI 也能做" --kind git_commits --target "标为无 AI 的仓库" --rationale "从某个提交起禁用 AI，那个点之后的提交才算"

Write-Output "`n── 打卡（计算机基础 23 次，散布在 30 天内）──" 
$today = Get-Date
$notes = @(
  "读完 CSAPP 第 3 章", "做完王道数据结构第 2 章习题", "整理红黑树笔记",
  "读完 CSAPP 第 4 章", "做完王道第 3 章习题", "手写一遍二叉搜索树",
  "读完 CSAPP 第 5 章", "复习内存对齐", "做完第 4 章习题",
  "手写 LRU 缓存", "读完 CSAPP 第 6 章", "整理缓存一致性笔记",
  "做完第 5 章习题", "手写跳表", "读完 CSAPP 第 7 章",
  "复习链接与装载", "手写简单分配器", "做完第 6 章习题",
  "读完 CSAPP 第 8 章", "复习信号与进程", "手写 mini shell",
  "做完第 7 章习题", "整理进程调度笔记"
)
$offsets = @(29,28,27,26,25,23,22,21,19,18,17,15,14,12,11,10,8,7,5,4,3,1,0)
for ($i = 0; $i -lt $offsets.Count; $i++) {
  $d = $today.AddDays(-$offsets[$i]).ToString('yyyy-MM-dd')
  B checkin "计算机基础" --date $d --note $notes[$i] | Out-Null
}
Write-Output "  已写入 $($offsets.Count) 条"

Write-Output "`n── 打卡（英语 5 次）──" 
$en = @(@(9,'复习 12 张卡片'), @(8,'新增 30 词'), @(5,'复习 8 张卡片'), @(2,'复习 15 张卡片'), @(0,'复习 20 张卡片'))
foreach ($e in $en) {
  $d = $today.AddDays(-$e[0]).ToString('yyyy-MM-dd')
  B checkin "英语" --date $d --note $e[1] | Out-Null
}
Write-Output "  已写入 $($en.Count) 条"

Write-Output "`n── 「不用 AI 也能做」故意不打任何卡 ──" 

Write-Output "`n── tick（补快照）──" 
B tick

Write-Output "`n── status ──" 
B status

Write-Output "`n── render ──" 
B render --out data/_demo.html

Write-Output "`n完成。用浏览器打开 data/_demo.html" 
