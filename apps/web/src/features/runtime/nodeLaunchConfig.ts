import { projectKey, type PackageManager, type Project } from '../../lib/api'

export type NodeLaunchConfig = { script: string; manager: 'auto' | PackageManager; args: string }
export function readNodeLaunchConfig(project: Project): NodeLaunchConfig {
  try {
    const saved = JSON.parse(localStorage.getItem(`devflow.node.${projectKey(project)}`) ?? 'null') as NodeLaunchConfig | null
    if (saved && typeof saved.script === 'string' && typeof saved.args === 'string' && ['auto', 'npm', 'pnpm', 'yarn'].includes(saved.manager)) return saved
  } catch { /* Use project defaults when storage is unavailable. */ }
  return { script: project.nodeScript ?? '', manager: 'auto', args: '' }
}
export function saveNodeLaunchConfig(project: Project, config: NodeLaunchConfig) { localStorage.setItem(`devflow.node.${projectKey(project)}`, JSON.stringify(config)) }
