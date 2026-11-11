import { request } from '../../lib/api'
import type { IdeasOperation, IdeasSnapshot } from './model'
export function getIdeas(workspaceKey: string, expectedRevision: number, signal: AbortSignal, ifRevision?: number) {
  const params = new URLSearchParams({ workspaceKey, expectedRevision: String(expectedRevision), includeMedia: 'true' })
  if (ifRevision !== undefined) params.set('ifRevision', String(ifRevision))
  return request<IdeasSnapshot | null>(`/api/workspace/ideas?${params}`, { signal: AbortSignal.any([signal, AbortSignal.timeout(15000)]) })
}
export function saveIdeas(workspaceKey: string, expectedRevision: number, ideasRevision: number, operation: IdeasOperation) {
  return request<IdeasSnapshot>('/api/workspace/ideas', { method: 'POST', body: JSON.stringify({ workspaceKey, expectedRevision, ideasRevision, operation }), signal: AbortSignal.timeout(20000) })
}
