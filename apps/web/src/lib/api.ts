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
export interface WorkspaceCatalog { allowedRoots: string[]; workspaceFiles: string[]; warnings: string[] }
export interface WorkspaceTreeEntry { name: string; path: string; kind: 'directory' | 'file' }
export interface WorkspaceChildren { path: string; revision: number; entries: WorkspaceTreeEntry[]; truncated: boolean }
export function getWorkspaceChildren(path: string, expectedRevision: number, signal: AbortSignal) {
  return request<WorkspaceChildren>(`/api/workspace/children?${new URLSearchParams({ path, expectedRevision: String(expectedRevision) })}`, { signal })
}
export interface WorkspaceBrowse { path: string | null; parent: string | null; entries: WorkspaceTreeEntry[]; truncated: boolean }
export function browseWorkspace(path?: string, signal?: AbortSignal) {
  return request<WorkspaceBrowse>(`/api/workspace/browse${path ? `?${new URLSearchParams({ path })}` : ''}`, { signal })
}
export type WorkspaceOperation = { type: 'open' | 'create' | 'remove_folder'; path: string } | { type: 'create_from_folder'; path: string; folder: string } | { type: 'close' } | { type: 'add_folder'; path: string; name?: string } | { type: 'save'; path?: string }

export class ApiError extends Error {
  constructor(public code: string, message: string) { super(message) }
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
  if (!response.ok) {
    const body = await response.text()
    let value: { code?: string; message?: string } = {}
    try { value = JSON.parse(body) ?? {} } catch { /* Axum extraction failures return plain text. */ }
    if (response.status === 422 && body.includes('unknown variant') && /`(?:close|create_from_folder|remove_folder)`/.test(body)) {
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
