// 接口层：所有写操作都回「已写好」，页面再整体重取一次 view model。
//
// **不做乐观更新。** 卡片上的数字、曲线末端、流水行必须来自同一次计算，
// 局部改一处就会出现「数字涨了、线没动」那种对不上的状态。

async function request(method, url, body) {
  const init = { method, headers: {} }
  if (body !== undefined) {
    init.headers['Content-Type'] = 'application/json'
    init.body = JSON.stringify(body)
  }
  const res = await fetch(url, init)
  const text = await res.text()
  let data = null
  if (text) {
    try {
      data = JSON.parse(text)
    } catch {
      data = null
    }
  }
  if (!res.ok) {
    throw new Error((data && data.error) || text || `HTTP ${res.status}`)
  }
  return data
}

export const api = {
  view(route) {
    const q = new URLSearchParams()
    if (route.settings) q.set('settings', '1')
    if (route.goal) q.set('goal', String(route.goal))
    const qs = q.toString()
    return request('GET', '/api/view' + (qs ? '?' + qs : ''))
  },

  checkin(goalIds, note, picks) {
    return request('POST', '/api/checkin', { goalIds, note, picks })
  },

  classify(id) {
    return request('POST', `/api/checkins/${id}/classify`)
  },

  backlog(goalId) {
    return request('POST', '/api/backlog/classify', { goalId: goalId ?? null })
  },

  addGoal(payload) {
    return request('POST', '/api/goals', payload)
  },

  updateGoal(id, title, why) {
    return request('PUT', `/api/goals/${id}`, { title, why })
  },

  archiveGoal(id, reason) {
    return request('POST', `/api/goals/${id}/archive`, { reason })
  },

  deleteGoal(id) {
    return request('DELETE', `/api/goals/${id}`)
  },

  addSource(goalId, kind, target, rationale) {
    return request('POST', `/api/goals/${goalId}/sources`, { kind, target, rationale })
  },

  deleteSource(id) {
    return request('DELETE', `/api/sources/${id}`)
  },

  saveSettings(payload) {
    return request('PUT', '/api/settings', payload)
  },

  testAI() {
    return request('POST', '/api/settings/test')
  },
}
