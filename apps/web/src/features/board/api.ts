import { request } from '../../lib/api'
import type { BoardOperation, BoardSnapshot } from './boardModel'

export function getBoard(workspaceKey: string, expectedRevision: number, signal?: AbortSignal) {
  const timeout = AbortSignal.timeout(15000)
  return request<BoardSnapshot>(`/api/workspace/board?${new URLSearchParams({ workspaceKey, expectedRevision: String(expectedRevision) })}`, { signal: signal ? AbortSignal.any([signal, timeout]) : timeout })
}
export function executeBoard(workspaceKey: string, expectedRevision: number, boardRevision: number, operation: BoardOperation) {
  return request<BoardSnapshot>('/api/workspace/board', { method: 'POST', body: JSON.stringify({ workspaceKey, expectedRevision, boardRevision, operation }) })
}
