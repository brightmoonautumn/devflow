import { request } from '../../lib/api'

export type GitChange = { path: string; originalPath: string | null; status: string; kind: string; untracked: boolean; submodule: boolean; pointerChanged: boolean; submoduleDirty: boolean; selectable: boolean; reason: string | null }
export type GitRepository = { root: string; name: string; branch: string; head: string | null; parent: string | null; submodulePath: string | null; version: string; changes: GitChange[]; blocked: string | null; truncated: boolean }
export type GitStatus = { refreshing?: boolean; stale?: boolean; revision: number; repositories: GitRepository[]; warnings: string[] }
export type GitUpdateInfo = { repository: string; branch: string; upstream: string | null; version: string; reason: string | null }
export type GitUpdateProgress = { jobId: string; repository: string; phase: string; done: boolean; message: string }
export type GitMergeState = { repository: string; branch: string; active: boolean; version: string; files: { path: string; localDeleted: boolean; incomingDeleted: boolean; supported: boolean }[]; message: string; changedPaths: string[] }
export type GitConflictContent = { path: string; version: string; base: string | null; local: string | null; incoming: string | null; result: string | null; reason: string | null; localDeleted: boolean; incomingDeleted: boolean }
export function openGitUpdate(repository?: string) { window.dispatchEvent(new CustomEvent('devflow-git-update-open', { detail: repository })) }
export function notifyGitUpdated(repository: string, paths?: string[]) {
  window.dispatchEvent(new CustomEvent('devflow-git-updated', { detail: { repository, paths } }))
}
export type GitDiff = { historical?: boolean; commit?: string; parent?: string | null; repository: string; path: string; originalPath: string | null; before: string; after: string; message: string | null; submodule: boolean }
export type GitSelection = { repository: string; version: string; paths: string[] }
export type GitPreview = { planId: string; revision: number; message: string; repositories: { repository: string; branch: string; files: GitDiff[] }[] }
export type GitOutcome = { results: { repository: string; paths: string[]; commit: string | null; pushed?: boolean; pushError?: { code: string; message: string } | null; error: { code: string; message: string } | null }[] }
export function gitRequest<T>(action: string, body: object, signal?: AbortSignal) {
  return request<T>(`/api/workspace/git/${action}`, { method: 'POST', body: JSON.stringify(body), signal: signal ?? AbortSignal.timeout(32 * (action === 'commit/apply' ? 2 : 1) * 120000 + 15000) })
}

export type HistoryCommit = { id: string; parents: string[]; author: string; email: string; timestamp: number; committedTimestamp?: number; subject: string; refs: string[] }
export type GitHistory = { repository: string; version: string; references: string[]; commits: HistoryCommit[]; nextSkip: number | null }
export type HistoryFile = { path: string; status: string; submodule: boolean; available: boolean; reason: string | null; linkedRepository?: string | null; linkedCommit?: string | null }
export type GitHistoryCommit = { repository: string; commit: HistoryCommit; parent: string | null; message: string; files: HistoryFile[]; truncated: boolean }
export type OverviewCommit = HistoryCommit & { repository: string }
export type GitHistoryOverview = { commits: OverviewCommit[]; nextCursor: string | null; references: string[]; warnings: string[] }
export type GitSyncInfo = { repository: string; branch: string; upstream: string | null; ahead: number | null; behind: number | null; lastFetchedAt: number | null; version: string; reason: string | null; canFetch: boolean }
export type GitSync = { repositories: GitSyncInfo[]; warnings: string[] }

export type GitHistoryTarget = { repository: string; commit: string; sequence: number }
export type GitBlame = { contentVersion: string; repository: string; commits: { id: string; author: string; timestamp: number; timezone: string }[]; ranges: { from: number; to: number; commit: string | null }[] }
