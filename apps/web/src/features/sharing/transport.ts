import { invitation, expireInvitation } from '../../lib/access'
let expired: string | null = null

/** Adapt existing read APIs to the same core services under invitation scope. */
export async function shareRequest<T>(url: string, options: RequestInit): Promise<T> {
  if (!invitation) throw new Error('请使用同事提供的完整分享链接打开。')
  if (expired) throw new Error(expired)
  const parsed = new URL(url, location.origin), path = parsed.pathname, query = parsed.searchParams
  const method = options.method ?? 'GET'
  const body = typeof options.body === 'string' ? JSON.parse(options.body) as Record<string, unknown> : {}
  let read: object
  if (method === 'GET' && ['/api/projects', '/api/workspace'].includes(path)) read = { type: 'workspace' }
  else if (method === 'GET' && path === '/api/workspace/children') read = { type: 'children', path: query.get('path') }
  else if (method === 'GET' && path === '/api/workspace/file') read = { type: 'file', path: query.get('path') }
  else if (method === 'GET' && path === '/api/runtime/projects') read = { type: 'runtime' }
  else if (method === 'GET' && path === '/api/runtime/self') read = { type: 'metrics' }
  else if (method === 'GET' && ['/api/runtime/java/output', '/api/runtime/node/output'].includes(path)) read = { type: 'output', projectPath: query.get('projectPath'), script: query.get('script'), after: Number(query.get('after') ?? 0), runId: query.get('runId') }
  else if (method === 'POST' && path === '/api/workspace/search') { const { expectedRevision: _, ...search } = body; read = { type: 'search', ...search } }
  else if (method === 'POST' && path === '/api/workspace/git/blame') read = { type: 'git_blame', path: body.path }
  else if (method === 'POST' && path === '/api/workspace/git/repositories') read = { type: 'git_repositories', force: body.force === true }
  else if (method === 'POST' && path === '/api/workspace/git/status') read = { type: 'git_status', force: body.force === true }
  else if (method === 'POST' && ['/api/workspace/git/history', '/api/workspace/git/history/commit', '/api/workspace/git/history/diff'].includes(path)) { const { expectedRevision: _, ...fields } = body; read = { ...fields, type: path.endsWith('/commit') ? 'git_history_commit' : path.endsWith('/diff') ? 'git_history_diff' : 'git_history' } }
  else if (method === 'POST' && path === '/api/workspace/git/diff') read = { type: 'git_diff', repository: body.repository, version: body.version, path: body.path }
  else throw new Error('当前分享为只读模式。')
  const response = await fetch('/api/shared/read', { method: 'POST', headers: { Authorization: `Bearer ${invitation}`, 'Content-Type': 'application/json' }, body: JSON.stringify(read), cache: 'no-store', signal: options.signal ?? AbortSignal.timeout(60000) })
  if (!response.ok) {
    const value = await response.json().catch(() => ({})) as { message?: string }
    const message = value.message ?? `读取失败（HTTP ${response.status}）`
    if (response.status === 401 && !expired) { expired = message; expireInvitation(message) }
    throw new Error(message)
  }
  const value = await response.json() as T
  if (expired) throw new Error(expired)
  return value
}
