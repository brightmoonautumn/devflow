import './tree-entry-icon.css'
import { Braces, Coffee, Database, File, FileCode2, FileImage, FileJson2, FileText, Folder, FolderCode, FolderCog, FolderOpen, Package, Settings2, Terminal, type LucideIcon } from 'lucide-react'

type IconStyle = { icon: LucideIcon; tone: string }

function fileStyle(name: string): IconStyle {
  if (name === 'pom.xml') return { icon: Package, tone: 'orange' }
  if (/^(package\.json|pnpm-lock\.yaml|package-lock\.json|yarn\.lock)$/.test(name)) return { icon: Package, tone: 'green' }
  if (name.startsWith('.env') || /^(dockerfile|docker-compose)/.test(name)) return { icon: Settings2, tone: 'teal' }
  const extension = name.split('.').pop()
  if (extension === 'java') return { icon: Coffee, tone: 'orange' }
  if (extension === 'vue') return { icon: FileCode2, tone: 'green' }
  if (['ts', 'tsx'].includes(extension ?? '')) return { icon: FileCode2, tone: 'blue' }
  if (['js', 'jsx', 'mjs', 'cjs'].includes(extension ?? '')) return { icon: FileCode2, tone: 'gold' }
  if (['json', 'jsonc', 'code-workspace'].includes(extension ?? '')) return { icon: FileJson2, tone: 'gold' }
  if (['yaml', 'yml', 'toml', 'properties', 'xml'].includes(extension ?? '')) return { icon: Settings2, tone: 'teal' }
  if (['css', 'scss', 'less'].includes(extension ?? '')) return { icon: Braces, tone: 'purple' }
  if (['html', 'htm'].includes(extension ?? '')) return { icon: FileCode2, tone: 'orange' }
  if (['md', 'mdx', 'txt'].includes(extension ?? '')) return { icon: FileText, tone: 'blue' }
  if (['svg', 'png', 'jpg', 'jpeg', 'gif', 'webp', 'ico'].includes(extension ?? '')) return { icon: FileImage, tone: 'purple' }
  if (['sh', 'bat', 'cmd', 'ps1'].includes(extension ?? '')) return { icon: Terminal, tone: 'green' }
  if (extension === 'sql') return { icon: Database, tone: 'teal' }
  return { icon: File, tone: 'muted' }
}

export function TreeEntryIcon({ name, directory, expanded, root }: { name: string; directory: boolean; expanded: boolean; root: boolean }) {
  const normalized = name.toLowerCase()
  let style: IconStyle
  if (!directory) style = fileStyle(normalized)
  else if (expanded) style = { icon: FolderOpen, tone: root ? 'blue' : 'gold' }
  else if (root) style = { icon: FolderCode, tone: 'blue' }
  else if (['src', 'java', 'components', 'pages', 'modules', 'projects'].includes(normalized)) style = { icon: FolderCode, tone: 'blue' }
  else if (['config', '.vscode', '.agent', '.agents', '.github'].includes(normalized)) style = { icon: FolderCog, tone: 'teal' }
  else if (['assets', 'images', 'public', 'resources', 'static'].includes(normalized)) style = { icon: Folder, tone: 'purple' }
  else style = { icon: Folder, tone: 'gold' }
  const Icon = style.icon
  return <Icon size={16} strokeWidth={2} aria-hidden="true" className={`ws-tree-icon ws-tree-icon--${style.tone}${directory ? ' is-folder' : ''}`} />
}
