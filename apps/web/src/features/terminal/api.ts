import { request } from '../../lib/api'

export type TerminalSession = { id: string; folder: string; shell: string; pid: number | null; exited: boolean; exitCode: number | null }
export type TerminalOperation = { type: 'list' } | { type: 'create'; folder: string; cols: number; rows: number } | { type: 'read'; id: string; after: number } | { type: 'input'; id: string; data: string } | { type: 'resize'; id: string; cols: number; rows: number } | { type: 'close'; id: string }
export type TerminalResponse = { sessions: TerminalSession[]; id?: string; data: number[]; nextCursor: number; truncated: boolean; exited: boolean; exitCode: number | null }
export function terminalRequest(expectedRevision: number, operation: TerminalOperation, signal?: AbortSignal) {
  const timeout = AbortSignal.timeout(15000)
  return request<TerminalResponse>('/api/workspace/terminal', { method: 'POST', body: JSON.stringify({ expectedRevision, operation }), signal: signal ? AbortSignal.any([signal, timeout]) : timeout })
}
