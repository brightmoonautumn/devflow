import { useEffect, useState } from 'react'
import { ArrowLeft, ChevronRight, FileJson2, Folder, HardDrive, LoaderCircle, RefreshCw, Search } from 'lucide-react'
import { browseWorkspace, type WorkspaceBrowse, type WorkspaceCatalog } from '../../lib/api'

export function WorkspaceBrowser({ catalog, path, onNavigate, selected, onSelect, foldersOnly = false, disabled = false }: {
  catalog: WorkspaceCatalog | null; path: string | null; onNavigate: (path: string | null) => void
  selected: string | null; onSelect: (path: string | null) => void; foldersOnly?: boolean; disabled?: boolean
}) {
  const [listing, setListing] = useState<WorkspaceBrowse | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const [filter, setFilter] = useState('')
  const [refresh, setRefresh] = useState(0)
  useEffect(() => {
    setFilter(''); setListing(null); setError(''); onSelect(null)
    if (!path) { setLoading(false); return }
    const controller = new AbortController()
    setLoading(true)
    browseWorkspace(path, controller.signal).then(result => { if (!controller.signal.aborted) setListing(result) })
      .catch(err => { if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err)) })
      .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    return () => controller.abort()
  }, [path, refresh, onSelect])
  const entries = path ? listing?.entries ?? [] : (catalog?.allowedRoots ?? []).map(root => ({ name: root, path: root, kind: 'directory' as const }))
  const visible = entries.filter(entry => (!foldersOnly || entry.kind === 'directory') && entry.name.toLocaleLowerCase().includes(filter.toLocaleLowerCase()))
  function navigate(next: string | null) { onSelect(null); onNavigate(next) }
  return <div className="ws-browser">
    <div className="ws-browser-toolbar">
      <button type="button" className="ws-icon-button" disabled={disabled || !path || loading} aria-label="上一级目录" onClick={() => navigate(listing?.parent ?? null)}><ArrowLeft size={15} /></button>
      <button type="button" className="ws-browser-home" disabled={disabled} onClick={() => navigate(null)}><HardDrive size={14} />本机目录</button>
      <ChevronRight size={12} /><span className="ws-browser-location" title={path ?? '已授权位置'}>{path ?? '已授权位置'}</span>
      <button type="button" className="ws-icon-button" disabled={disabled || loading || !path} aria-label="刷新浏览目录" onClick={() => setRefresh(value => value + 1)}><RefreshCw size={13} /></button>
    </div>
    <label className="ws-browser-search"><Search size={14} /><input aria-label="筛选当前目录" placeholder="筛选当前目录…" value={filter} onChange={event => setFilter(event.target.value)} disabled={disabled} /></label>
    <div className="ws-browser-list" aria-busy={loading} aria-label="可选择的文件和文件夹">
      {loading ? <div className="ws-browser-empty" role="status"><LoaderCircle size={19} className="spinning" />正在读取目录…</div> : error ? <div className="ws-browser-empty" role="alert"><p>{error}</p><button type="button" className="ws-outline-button" disabled={disabled} onClick={() => setRefresh(value => value + 1)}>重试</button></div> : visible.map(entry => <button type="button" className={`ws-browser-entry ${selected === entry.path ? 'is-selected' : ''}`} key={entry.path} disabled={disabled} title={entry.path} aria-pressed={entry.kind === 'file' ? selected === entry.path : undefined} onClick={() => entry.kind === 'directory' ? navigate(entry.path) : onSelect(entry.path)}>
        {entry.kind === 'directory' ? <Folder size={19} className="ws-tree-icon--gold" /> : <FileJson2 size={19} className="ws-tree-icon--blue" />}<span>{entry.name}</span><small>{entry.kind === 'directory' ? '文件夹' : '工作区'}</small>{entry.kind === 'directory' && <ChevronRight size={13} />}
      </button>)}
      {!loading && !error && !visible.length && <div className="ws-browser-empty">{filter ? '没有匹配的条目' : path ? '此目录没有可浏览的子目录或工作区文件，可选择当前文件夹。' : '尚未配置可浏览的位置。'}</div>}
      {!path && !foldersOnly && !!catalog?.workspaceFiles.length && <><div className="ws-browser-label">已有工作区</div>{catalog.workspaceFiles.filter(file => file.toLocaleLowerCase().includes(filter.toLocaleLowerCase())).map(file => <button type="button" className={`ws-browser-entry ${selected === file ? 'is-selected' : ''}`} key={file} disabled={disabled} aria-pressed={selected === file} title={file} onClick={() => onSelect(file)}><FileJson2 size={19} className="ws-tree-icon--blue" /><span>{file.split(/[\\/]/).at(-1)}<small>{file}</small></span></button>)}</>}
    </div>
    {listing?.truncated && <p className="ws-manager-hint">目录条目过多，仅展示前 2000 项范围内的内容。</p>}
    {path && !loading && !error && <div className="ws-browser-selection"><Folder size={14} /><span title={selected ?? path}>{selected ?? path}</span></div>}
  </div>
}
