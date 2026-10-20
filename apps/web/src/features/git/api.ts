import { request } from '../../lib/api'

export type GitChange = { path: string; originalPath: string | null; status: string; kind: string; untracked: boolean; submodule: boolean; pointerChanged: boolean; submoduleDirty: boolean; selectable: boolean; reason: string | null }
export type GitRepository = { root: string; name: string; branch: string; head: string | null; parent: string | null; submodulePath: string | null; version: string; changes: GitChange[]; blocked: string | null; truncated: boolean }
export type GitStatus = { revision: number; repositories: GitRepository[]; warnings: string[] }
export type GitDiff = { repository: string; path: string; originalPath: string | null; before: string; after: string; message: string | null; submodule: boolean }
export type GitSelection = { repository: string; version: string; paths: string[] }
export type GitPreview = { planId: string; revision: number; message: string; repositories: { repository: string; branch: string; files: GitDiff[] }[] }
export type GitOutcome = { results: { repository: string; paths: string[]; commit: string | null; error: { code: string; message: string } | null }[] }
export function gitRequest<T>(action: string, body: object, signal?: AbortSignal) {
  return request<T>(`/api/workspace/git/${action}`, { method: 'POST', body: JSON.stringify(body), signal: signal ?? AbortSignal.timeout(32 * 120000 + 15000) })
}
