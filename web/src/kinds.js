// 计分来源类型。四种，是上限——界面和 CLI 都从这一份取，避免两处各写一遍。
export const SOURCE_KINDS = [
  {
    value: 'manual_checkin',
    label: '手工打卡',
    description: '手动记录一次，例如「读完一章计一次」',
    needsTarget: false,
  },
  {
    value: 'git_commits',
    label: 'git 提交',
    description: '统计指定仓库的提交数。尚未接入，仅登记规则',
    needsTarget: true,
    targetPrompt: '仓库名称（接入后开始计数）',
  },
  {
    value: 'external_metric',
    label: '外部数据',
    description: '读取其他来源中已有的数据，例如复习记录',
    needsTarget: true,
    targetPrompt: '数据标识（接入后开始计数）',
  },
  {
    value: 'derived',
    label: '组合',
    description: '由其他来源按公式组合计算',
    needsTarget: false,
  },
]

export function findKind(value) {
  return SOURCE_KINDS.find((kind) => kind.value === value) || SOURCE_KINDS[0]
}
