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
  else if (method === 'GET' && path === '/api/workspace/image') read = { type: 'image', path: query.get('path') }
  else if (method === 'GET' && path === '/api/workspace/file') read = { type: 'file', path: query.get('path') }
  else if (method === 'GET' && path === '/api/runtime/projects') read = { type: 'runtime' }
  else if (method === 'GET' && path === '/api/runtime/self') read = { type: 'metrics' }
  else if (method === 'GET' && ['/api/runtime/java/output', '/api/runtime/node/output'].includes(path)) read = { type: 'output', projectPath: query.get('projectPath'), script: query.get('script'), after: Number(query.get('after') ?? 0), runId: query.get('runId') }
  else if (method === 'POST' && path === '/api/workspace/search') { const { expectedRevision: _, ...search } = body; read = { type: 'search', ...search } }
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
