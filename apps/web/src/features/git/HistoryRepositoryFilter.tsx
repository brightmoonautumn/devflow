import { useEffect, useRef, useState } from 'react'
import { ChevronDown } from 'lucide-react'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseButton } from '../../components/form/BaseButton'
import type { GitRepository } from './api'

export function rootColor(root: string) {
  let hash = 0
  for (const character of root) hash = (Math.imul(hash, 31) + character.charCodeAt(0)) | 0
  return `var(--tree-${['blue', 'gold', 'green', 'purple', 'orange', 'teal'][(hash >>> 0) % 6]})`
}
export function repositoryFamily(repository: string, repositories: GitRepository[]) {
  const roots = new Set([repository])
  for (let i = 0; i < repositories.length; i++) {
    const size = roots.size
    repositories.forEach(repo => { if (repo.parent && roots.has(repo.parent)) roots.add(repo.root) })
    if (roots.size === size) break
  }
  return [...roots]
}

export function HistoryRepositoryFilter({ repositories, scope, disabled, onChange }: {
  repositories: GitRepository[]; scope: string[] | null; disabled: boolean; onChange: (scope: string[] | null) => void
}) {
  const [search, setSearch] = useState('')
  const popup = useRef<HTMLDetailsElement>(null)
  const selected = new Set(scope ?? repositories.map(repo => repo.root))
  useEffect(() => {
    const close = (event: PointerEvent) => { if (popup.current && !popup.current.contains(event.target as Node)) popup.current.open = false }
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape' && popup.current?.open) { popup.current.open = false; popup.current.querySelector('summary')?.focus() } }
    document.addEventListener('pointerdown', close); document.addEventListener('keydown', escape)
    return () => { document.removeEventListener('pointerdown', close); document.removeEventListener('keydown', escape) }
  }, [])
  function focus(root: string, family: boolean) {
    onChange(family ? repositoryFamily(root, repositories) : [root])
    if (popup.current) popup.current.open = false
  }
  return <details className="history-root-filter" ref={popup}>
    <summary aria-label="筛选历史仓库">{scope === null ? '全部仓库' : selected.size === 1 ? repositories.find(repo => selected.has(repo.root))?.name ?? '1 个仓库' : `${selected.size} 个仓库`}<ChevronDown size={12} /></summary>
    <div className="history-root-popup">
      <BaseInput aria-label="搜索仓库" placeholder="搜索仓库名称或路径…" value={search} onChange={event => setSearch(event.target.value)} />
      <div className="history-root-actions"><BaseButton disabled={disabled} onClick={() => onChange(null)}>全部仓库</BaseButton><BaseButton disabled={disabled} onClick={() => onChange([])}>清空</BaseButton></div>
      <div className="history-root-options">{repositories.filter(repo => `${repo.name} ${repo.root}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())).map(repo => <div className="history-root-option" key={repo.root}>
        <label title={repo.root}><input type="checkbox" checked={selected.has(repo.root)} disabled={disabled} onChange={() => { const next = new Set(selected); if (next.has(repo.root)) next.delete(repo.root); else next.add(repo.root); onChange([...next]) }} /><i style={{ background: rootColor(repo.root) }} aria-hidden="true" /><span>{repo.name}{repo.parent && <small>子仓库</small>}</span></label>
        <button disabled={disabled} onClick={() => focus(repo.root, false)} title={`仅看 ${repo.name}`}>仅看</button>
        {repositories.some(child => child.parent === repo.root) && <button disabled={disabled} onClick={() => focus(repo.root, true)} title={`查看 ${repo.name} 及全部子仓库`}>含子仓库</button>}
      </div>)}</div>
      {!repositories.some(repo => `${repo.name} ${repo.root}`.toLocaleLowerCase().includes(search.toLocaleLowerCase())) && <p className="git-notice">没有匹配的仓库。</p>}
    </div>
  </details>
}
