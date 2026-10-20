export interface Project {
  name: string
  kind: 'maven' | 'node' | 'maven_and_node'
  path: string
  markers: string[]
  mainClasses?: string[]
  launchScripts?: string[]
  nodeScript?: string
}
export function projectKey(project: Pick<Project, 'path' | 'nodeScript'>) { return project.nodeScript ? `node:${project.path}#${project.nodeScript}` : project.path }
export function metricsKey(metrics: Pick<ProjectRuntime, 'projectPath' | 'nodeScript'>) { return projectKey({ path: metrics.projectPath, nodeScript: metrics.nodeScript }) }
export interface WorkspaceResponse {
  workspaceRoot: string | null
  workspaceFile: string | null
  name: string
  revision: number
  dirty: boolean
  folders: WorkspaceFolder[]
  projects: Project[]
  warnings: string[]
  hint?: string
}

export interface WorkspaceFolder { name: string; path: string; status: 'available' | 'blocked' | 'unavailable' | 'not_directory' | 'unsupported'; message: string | null }
export interface RecentWorkspace { name: string; path: string; kind: 'workspace_file' | 'folder'; available: boolean }
export interface WorkspaceCatalog { allowedRoots: string[]; workspaceFiles: string[]; warnings: string[]; recentWorkspaces?: RecentWorkspace[] }
export interface WorkspaceTreeEntry { name: string; path: string; kind: 'directory' | 'file' }
export interface WorkspaceChildren { path: string; revision: number; entries: WorkspaceTreeEntry[]; truncated: boolean }
export function getWorkspaceChildren(path: string, expectedRevision: number, signal: AbortSignal) {
  return request<WorkspaceChildren>(`/api/workspace/children?${new URLSearchParams({ path, expectedRevision: String(expectedRevision) })}`, { signal })
}
export interface WorkspaceBrowse { path: string | null; parent: string | null; entries: WorkspaceTreeEntry[]; truncated: boolean }
export function browseWorkspace(path?: string, signal?: AbortSignal) {
  return request<WorkspaceBrowse>(`/api/workspace/browse${path ? `?${new URLSearchParams({ path })}` : ''}`, { signal })
}
export function pickWorkspaceLocation(kind: 'workspace_file' | 'folder', expectedRevision: number) {
  return request<{ path: string | null }>('/api/workspace/pick', {
    method: 'POST', body: JSON.stringify({ kind, expectedRevision }), signal: AbortSignal.timeout(190000),
  })
}
export interface WorkspaceFile {
  path: string; content: string; revision: number; readonly: boolean; readonlyReason: string | null
  lineEnding: 'LF' | 'CRLF' | 'CR'; bom: boolean
}
export function readWorkspaceFile(path: string, expectedRevision: number, signal?: AbortSignal) {
  return request<WorkspaceFile>(`/api/workspace/file?${new URLSearchParams({ path, expectedRevision: String(expectedRevision) })}`, { signal })
}
export function writeWorkspaceFile(path: string, expectedRevision: number, expectedContent: string, content: string) {
  return request<WorkspaceFile>('/api/workspace/file', { method: 'PUT', body: JSON.stringify({ path, expectedRevision, expectedContent, content }) })
}
export type WorkspaceOperation = { type: 'open' | 'open_recent' | 'create' | 'remove_folder'; path: string } | { type: 'create_from_folder'; path: string; folder: string } | { type: 'close' } | { type: 'add_folder'; path: string; name?: string } | { type: 'save'; path?: string }

export class ApiError extends Error {
  constructor(public code: string, message: string) { super(message) }
}
export type SearchMode = 'files' | 'content' | 'both'
export interface SearchMatch { path: string; root: string; relativePath: string; line: number | null; column: number | null; preview: string | null; previewMatches?: { start: number; end: number }[] }
export interface WorkspaceSearch { revision: number; matches: SearchMatch[]; truncated: boolean; warnings: string[] }
export function searchWorkspace(query: string, mode: SearchMode, expectedRevision: number, options: { caseSensitive: boolean; wholeWord: boolean; regex: boolean; includeGlob?: string; excludeGlob?: string }, signal: AbortSignal) {
  return request<WorkspaceSearch>('/api/workspace/search', { method: 'POST', body: JSON.stringify({ query, mode, expectedRevision, ...options }), signal })
}
export interface ReplacePreview { planId: string; revision: number; replacements: number; files: { path: string; replacements: number; before: string; after: string }[] }
export interface ReplaceOutcome { files: string[]; replacements: number; failure: { code: string; message: string } | null }
export function previewWorkspaceReplace(search: { query: string; expectedRevision: number; caseSensitive: boolean; wholeWord: boolean; regex: boolean; includeGlob: string; excludeGlob: string }, replacement: string, preserveCase: boolean, path?: string, line?: number, column?: number) {
  return request<ReplacePreview>('/api/workspace/replace/preview', { method: 'POST', body: JSON.stringify({ search, replacement, preserveCase, path, line, column }) })
}
export function applyWorkspaceReplace(planId: string, expectedRevision: number) {
  return request<ReplaceOutcome>('/api/workspace/replace/apply', { method: 'POST', body: JSON.stringify({ planId, expectedRevision }) })
}
let session: Promise<string> | null = null
function getToken() {
  if (!session) session = fetch('/api/session', { headers: { 'X-DevFlow-Client': 'web' }, cache: 'no-store', signal: AbortSignal.timeout(10000) })
    .then(async response => {
      if (!response.ok) throw new Error(`无法连接本地服务（HTTP ${response.status}）`)
      const value = await response.json() as { token: string }
      return value.token
    }).catch(error => { session = null; throw error })
  return session
}
async function request<T>(url: string, options: RequestInit = {}, retry = true): Promise<T> {
  const token = await getToken()
  const headers = new Headers(options.headers)
  headers.set('Authorization', `Bearer ${token}`)
  if (options.body) headers.set('Content-Type', 'application/json')
  const response = await fetch(url, { ...options, signal: options.signal ?? AbortSignal.timeout(15000), headers, cache: 'no-store' })
  if (response.status === 401 && retry) { session = null; return request<T>(url, options, false) }
  if (url.startsWith('/api/workspace/replace/') && (response.status === 404 || response.status === 405 || (response.ok && !response.headers.get('content-type')?.includes('application/json')))) {
    throw new ApiError('backend_outdated', '当前后端尚不支持搜索替换，请更新并重启后端后重试。')
  }
  if (url === '/api/workspace/search' && (response.status === 404 || response.status === 405 || (response.ok && !response.headers.get('content-type')?.includes('application/json')))) {
    throw new ApiError('backend_outdated', '当前后端尚不支持文件搜索，请更新并重启后端后重试。')
  }
  if (url.startsWith('/api/workspace/file') && (response.ok || response.status === 404 || response.status === 405) && !response.headers.get('content-type')?.includes('application/json')) {
    throw new ApiError('backend_outdated', '当前后端尚不支持文件编辑，请通过 DevFlow-Restart.cmd 更新并重启后重试。')
  }
  if (!response.ok) {
    if (response.status === 404 && url === '/api/workspace/pick') {
      throw new ApiError('backend_outdated', '当前运行的后端尚不支持系统选择窗口，请重新编译并重启 DevFlow 后端后重试。')
    }
    const body = await response.text()
    let value: { code?: string; message?: string } = {}
    try { value = JSON.parse(body) ?? {} } catch { /* Axum extraction failures return plain text. */ }
    if (response.status === 422 && body.includes('unknown variant') && /`(?:close|create_from_folder|remove_folder|open_recent)`/.test(body)) {
      throw new ApiError('backend_outdated', '当前运行的后端版本尚不支持此操作，请重新编译并重启 DevFlow 后端后重试。')
    }
    throw new ApiError(value.code ?? 'request_failed', value.message ?? `HTTP ${response.status}`)
  }
  return response.json() as Promise<T>
}
export function getWorkspaceCatalog(signal?: AbortSignal) { return request<WorkspaceCatalog>('/api/workspace/catalog', { signal }) }
export function executeWorkspace(operation: WorkspaceOperation, expectedRevision: number, discardChanges = false) {
  return request<WorkspaceResponse>('/api/workspace', { method: 'POST', body: JSON.stringify({ operation, expectedRevision, discardChanges }) })
}

export interface ProjectRuntime {
  nodeScript?: string
  nodePids?: number[]
  javaPids?: number[]
  listeningPorts?: number[] | null
  portError?: string | null
  managedState?: 'starting' | 'running' | 'stopped' | 'failed'
  projectPath: string
  pids: number[]
  memoryBytes: number | null
  cpuPercent: number | null
  uptimeSeconds: number | null
}

export interface RuntimeSnapshot {
  sampledAt: number
  projects: ProjectRuntime[]
}

export async function getRuntime(signal: AbortSignal): Promise<RuntimeSnapshot> {
  return request<RuntimeSnapshot>('/api/runtime/projects', { signal })
}

export async function getWorkspace(signal?: AbortSignal, refresh = false): Promise<WorkspaceResponse> {
  return request<WorkspaceResponse>(refresh ? '/api/projects?refresh=true' : '/api/projects', { signal })
}

export interface DevFlowMetrics {
  sampledAt: number; pid: number; memoryBytes: number | null; cpuPercent: number | null; uptimeSeconds: number | null
}
export function getDevFlowMetrics(signal: AbortSignal) { return request<DevFlowMetrics>('/api/runtime/self', { signal }) }

export interface JavaOptions { mainClasses: string[]; profiles: string[] }
export interface JavaLogLine { sequence: number; stream: string; text: string }
export interface JavaOutput { runId: string; state: string; pid: number | null; exitCode: number | null; lines: JavaLogLine[]; nextCursor: number; truncated: boolean; arthasUrl: string | null; attaching: boolean }
export type JavaOperation = { type: 'start'; jvmArgs: string[]; args: string[]; profiles: string[]; mainClass: string } | { type: 'stop'; pid?: number } | { type: 'attach'; pid: number }
export function getJavaOptions(projectPath: string, signal?: AbortSignal) { return request<JavaOptions>(`/api/runtime/java/options?${new URLSearchParams({ projectPath })}`, { signal }) }
export function getJavaOutput(projectPath: string, after: number, runId: string, signal: AbortSignal) { return request<JavaOutput>(`/api/runtime/java/output?${new URLSearchParams({ projectPath, after: String(after), runId })}`, { signal }) }
export function executeJava(projectPath: string, expectedRevision: number, operation: JavaOperation) { return request<JavaOutput>('/api/runtime/java', { method: 'POST', body: JSON.stringify({ projectPath, expectedRevision, operation }) }) }

export type PackageManager = 'npm' | 'pnpm' | 'yarn'
export interface NodeOptions { scripts: string[]; packageManager: PackageManager }
export type NodeOperation = { type: 'start'; packageManager?: PackageManager; args: string[] } | { type: 'stop'; pid?: number }
function nodeParams(project: Project) { return { projectPath: project.path, ...(project.nodeScript ? { script: project.nodeScript } : {}) } }
export function getNodeOptions(project: Project, signal?: AbortSignal) { return request<NodeOptions>(`/api/runtime/node/options?${new URLSearchParams(nodeParams(project))}`, { signal }) }
export function getNodeOutput(project: Project, after: number, runId: string, signal: AbortSignal) { return request<JavaOutput>(`/api/runtime/node/output?${new URLSearchParams({ ...nodeParams(project), after: String(after), runId })}`, { signal }) }
export function executeNode(project: Project, expectedRevision: number, operation: NodeOperation, script?: string) { return request<JavaOutput>('/api/runtime/node', { method: 'POST', body: JSON.stringify({ ...nodeParams(project), ...(script ? { script } : {}), expectedRevision, operation }) }) }
