export interface Project {
  name: string
  kind: 'maven' | 'node' | 'maven_and_node'
  path: string
  markers: string[]
}
export interface WorkspaceResponse {
  workspaceRoot: string | null
  projects: Project[]
  hint?: string
}

export async function getWorkspace(): Promise<WorkspaceResponse> {
  const response = await fetch('/api/projects')
  if (!response.ok) throw new Error(`HTTP ${response.status}`)
  return response.json() as Promise<WorkspaceResponse>
}
