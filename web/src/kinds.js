// 计分来源类型。四种，是上限——界面和 CLI 都从这一份取，避免两处各写一遍。
export const SOURCE_KINDS = [
  {
    value: 'manual_checkin',
    label: '手工打卡',
    description: '我自己记一次。比如「读完一章算一次」',
    needsTarget: false,
  },
  {
    value: 'git_commits',
    label: 'git 提交',
    description: '某个仓库的提交数。还没接入，先登记规则',
    needsTarget: true,
    targetPrompt: '哪个仓库？（先填名字，接上之后才会计数）',
  },
  {
    value: 'external_metric',
    label: '外部数据',
    description: '读别处已经记着的数，比如复习记录',
    needsTarget: true,
    targetPrompt: '读哪份数据？（先填标识，接上之后才会计数）',
  },
  {
    value: 'derived',
    label: '组合',
    description: '上面几条按公式算',
    needsTarget: false,
  },
]

export function findKind(value) {
  return SOURCE_KINDS.find((kind) => kind.value === value) || SOURCE_KINDS[0]
}
