import { readOnlyShare } from '../../lib/access'
import type { WorkspaceResponse } from '../../lib/api'

export type SavedFileTab = { path: string; preview: boolean; line: number; column: number }
export type WorkspaceViewState = { tabs?: SavedFileTab[]; active?: string | null; expanded?: string[]; selected?: string | null; sidebarOpen?: boolean; sidebarView?: 'explorer' | 'search' }
export const viewPathKey = (path: string) => /^[a-z]:[\\/]|^\\\\/i.test(path) ? path.replace(/\\/g, '/').toLowerCase() : path

export function workspaceViewKey(workspace: WorkspaceResponse | null): string | null {
  if (!workspace || (!workspace.workspaceFile && !workspace.workspaceRoot && !workspace.folders.length)) return null
  const identity = workspace.workspaceFile ?? workspace.workspaceRoot ?? workspace.folders.map(folder => viewPathKey(folder.path)).sort().join('|')
  return `devflow.workspace-view.v1:${readOnlyShare ? 'shared' : 'local'}:${viewPathKey(identity)}`
}

function storage() { return readOnlyShare ? sessionStorage : localStorage }
export function readViewState(key: string): WorkspaceViewState {
  try { const value: unknown = JSON.parse(storage().getItem(key) ?? '{}'); return value && typeof value === 'object' && !Array.isArray(value) ? value as WorkspaceViewState : {} } catch { return {} }
}
export function writeViewState(key: string, patch: WorkspaceViewState) {
  try { storage().setItem(key, JSON.stringify({ ...readViewState(key), ...patch })) } catch { /* Storage is optional; keep the live view working. */ }
}

export function restoredTabs(value: WorkspaceViewState): SavedFileTab[] {
  if (!Array.isArray(value.tabs)) return []
  const seen = new Set<string>()
  return value.tabs.filter(tab => {
    if (!tab || typeof tab.path !== 'string' || tab.path.length > 32768 || !/^(?:[a-z]:[\\/]|\\\\|\/)/i.test(tab.path)) return false
    const key = viewPathKey(tab.path)
    if (seen.has(key)) return false
    seen.add(key); return true
  }).slice(0, 12).map(tab => ({ path: tab.path, preview: tab.preview === true, line: Number.isSafeInteger(tab.line) && tab.line > 0 ? tab.line : 1, column: Number.isSafeInteger(tab.column) && tab.column > 0 ? tab.column : 1 }))
}
