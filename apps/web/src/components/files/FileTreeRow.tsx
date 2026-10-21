import type { HTMLAttributes, KeyboardEvent, ReactNode } from 'react'
import { ChevronRight } from 'lucide-react'
import './file-tree-row.css'

export function FileTreeRow({ className = '', leading, trailing, children, ...props }: HTMLAttributes<HTMLDivElement> & { leading?: ReactNode; trailing?: ReactNode }) {
  return <div {...props} className={`file-tree-row ${className}`}>{leading}{children}{trailing}</div>
}

export function TreeChevron({ expanded, expandable = true, label, onToggle }: { expanded: boolean; expandable?: boolean; label?: string; onToggle?: () => void }) {
  const icon = expandable ? <ChevronRight size={13} className={expanded ? 'is-expanded' : ''} /> : null
  return onToggle && expandable
    ? <button type="button" className="file-tree-chevron" aria-label={label} aria-expanded={expanded} onClick={onToggle}>{icon}</button>
    : <span className="file-tree-chevron" aria-hidden="true">{icon}</span>
}

export function TreePathLabel<T extends { path: string; name: string }>({ segments, className = '', renderSegment }: {
  segments: readonly T[]
  className?: string
  renderSegment?: (segment: T) => ReactNode
}) {
  return <span className={`file-tree-label ${className}`}>{segments.map((segment, index) => <span key={segment.path}>{index > 0 && <span className="file-tree-separator" aria-hidden="true">/</span>}{renderSegment ? renderSegment(segment) : segment.name}</span>)}</span>
}

// Call on the treeitem itself, leaving nested buttons and checkboxes native.
export function handleTreeNavigation(event: KeyboardEvent<HTMLElement>, { expanded, expandable, onToggle, onActivate }: {
  expanded: boolean; expandable: boolean; onToggle: (expanded: boolean) => void; onActivate: () => void
}) {
  if (event.target !== event.currentTarget || event.altKey || event.ctrlKey || event.metaKey || event.shiftKey) return
  const element = event.currentTarget
  const keys = ['Enter', ' ', 'ArrowRight', 'ArrowLeft', 'ArrowUp', 'ArrowDown', 'Home', 'End']
  if (!keys.includes(event.key)) return
  event.preventDefault(); event.stopPropagation()
  if (event.key === 'Enter' || event.key === ' ') { onActivate(); return }
  if (event.key === 'ArrowRight') {
    if (expandable && !expanded) onToggle(true)
    else if (expanded) element.querySelector<HTMLElement>('[role="treeitem"]')?.focus()
  } else if (event.key === 'ArrowLeft') {
    if (expandable && expanded) onToggle(false)
    else element.parentElement?.closest<HTMLElement>('[role="treeitem"]')?.focus()
  } else {
    const nodes = Array.from(element.closest('[role="tree"]')?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? []).filter(node => !node.closest('[hidden]'))
    const index = nodes.indexOf(element)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? nodes.length - 1 : index + (event.key === 'ArrowDown' ? 1 : -1)
    nodes[next]?.focus()
  }
}
