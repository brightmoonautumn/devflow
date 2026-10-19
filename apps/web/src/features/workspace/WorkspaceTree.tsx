import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactElement } from 'react'
import { AlertCircle, ChevronRight, ChevronsDownUp, FolderMinus, FolderPlus, LoaderCircle, MoreHorizontal, RefreshCw, Save, X } from 'lucide-react'
import { TreeEntryIcon } from './TreeEntryIcon'
import { ApiError, executeWorkspace, getWorkspaceChildren, type WorkspaceChildren, type WorkspaceResponse, type WorkspaceTreeEntry } from '../../lib/api'

type NodeEntry = WorkspaceTreeEntry & { status?: string; message?: string | null }
type RootMenu = { entry: NodeEntry; revision: number; x: number; y: number; target: HTMLElement }
type NodeProps = { entry: NodeEntry; level: number; revision: number; selected: string | null; onSelect: (path: string) => void; onRootMenu: (menu: RootMenu) => void }

function TreeNode({ entry, level, revision, selected, onSelect, onRootMenu }: NodeProps): ReactElement {
  const directory = entry.kind === 'directory'
  const available = !entry.status || entry.status === 'available'
  const [expanded, setExpanded] = useState(false)
  const [children, setChildren] = useState<WorkspaceChildren | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const pending = useRef<AbortController | null>(null)
  useEffect(() => () => pending.current?.abort(), [])
  async function readChildren() {
    if (pending.current) return
    const controller = new AbortController()
    pending.current = controller
    setLoading(true); setError('')
    try {
      const result = await getWorkspaceChildren(entry.path, revision, controller.signal)
      if (!controller.signal.aborted) setChildren(result)
    } catch (err) {
      if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err))
    } finally {
      if (!controller.signal.aborted) setLoading(false)
      if (pending.current === controller) pending.current = null
    }
  }
  function toggle() {
    onSelect(entry.path)
    if (!directory || !available) return
    setExpanded(current => !current)
    if (!expanded && !children && !loading) void readChildren()
  }
  function onKeyDown(event: KeyboardEvent<HTMLLIElement>) {
    if (event.target !== event.currentTarget) return
    const element = event.currentTarget
    if (level === 1 && (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10'))) {
      event.preventDefault(); event.stopPropagation()
      const rect = element.querySelector('.ws-tree-row')!.getBoundingClientRect()
      onRootMenu({ entry, revision, x: rect.left + 20, y: rect.bottom, target: element }); return
    }
    if (event.key === 'Enter' || event.key === ' ') { event.preventDefault(); event.stopPropagation(); toggle(); return }
    if (event.key === 'ArrowRight') {
      event.preventDefault(); event.stopPropagation()
      if (directory && available && !expanded) toggle()
      else element.querySelector<HTMLElement>('[role="treeitem"]')?.focus()
    } else if (event.key === 'ArrowLeft') {
      event.preventDefault(); event.stopPropagation()
      if (expanded) setExpanded(false)
      else element.parentElement?.closest<HTMLElement>('[role="treeitem"]')?.focus()
    } else if (['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) {
      event.preventDefault(); event.stopPropagation()
      const nodes = Array.from(element.closest('[role="tree"]')?.querySelectorAll<HTMLElement>('[role="treeitem"]') ?? [])
      const index = nodes.indexOf(element)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? nodes.length - 1 : index + (event.key === 'ArrowDown' ? 1 : -1)
      nodes[next]?.focus()
    }
  }
  const status = entry.status === 'blocked' ? '未授权' : entry.status === 'unsupported' ? '不支持' : '不可用'
  return <li role="treeitem" tabIndex={0} aria-label={entry.name} aria-level={level} aria-selected={selected === entry.path} aria-expanded={directory && available ? expanded : undefined} aria-disabled={!available || undefined}
    onKeyDown={onKeyDown} onContextMenu={event => {
      event.stopPropagation()
      if (level !== 1) return
      event.preventDefault(); onSelect(entry.path)
      onRootMenu({ entry, revision, x: event.clientX, y: event.clientY, target: event.currentTarget })
    }} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); toggle() }}>
    <div className={`ws-tree-row ${available ? '' : 'is-unavailable'}`} style={{ paddingLeft: 9 + (level - 1) * 14 }} title={entry.message ? `${entry.path}\n${entry.message}` : entry.path}>
      <span className="ws-tree-chevron">{directory && available && <ChevronRight size={13} className={expanded ? 'is-expanded' : ''} />}</span>
      {loading ? <LoaderCircle size={16} aria-hidden="true" className="ws-tree-icon spinning" /> : <TreeEntryIcon name={entry.name} directory={directory} expanded={expanded} root={level === 1} />}
      <span className="ws-tree-label">{entry.name}</span>{!available && <span className="ws-tree-unavailable"><AlertCircle size={12} />{status}</span>}
      {level === 1 && <button className="ws-tree-more" aria-label={`${entry.name} 文件夹操作`} title="文件夹操作" onClick={event => {
        event.stopPropagation(); const rect = event.currentTarget.getBoundingClientRect(); onSelect(entry.path)
        onRootMenu({ entry, revision, x: rect.left, y: rect.bottom, target: event.currentTarget })
      }}><MoreHorizontal size={14} /></button>}
    </div>
    {expanded && <ul role="group">
      {children?.entries.map(child => <MemoTreeNode key={child.path} entry={child} level={level + 1} revision={revision} selected={selected} onSelect={onSelect} onRootMenu={onRootMenu} />)}
      {loading && <li role="none" className="ws-tree-note">正在读取…</li>}
      {error && <li role="none" className="ws-tree-note is-error" onClick={event => event.stopPropagation()}><span role="status">{error}</span><button onClick={() => void readChildren()}>重试</button></li>}
      {children && !children.entries.length && <li role="none" className="ws-tree-note">空文件夹或仅包含已忽略目录</li>}
      {children?.truncated && <li role="none" className="ws-tree-note">目录过大，仅显示前 2000 个条目范围内的内容。</li>}
    </ul>}
  </li>
}
const MemoTreeNode = memo(TreeNode)

function maxSidebarWidth() { return window.innerWidth < 720 ? Math.max(200, Math.min(360, window.innerWidth - (window.innerWidth <= 550 ? 45 : 51) - 28)) : Math.max(200, Math.min(520, window.innerWidth - 420)) }
function clampWidth(width: number) { return Math.round(Math.max(200, Math.min(maxSidebarWidth(), width))) }

export function WorkspaceTree({ data, onChange, onRefresh, onConfigure, onAddFolder, onSave, onClose }: { data: WorkspaceResponse | null; onChange: (data: WorkspaceResponse) => void; onRefresh: () => void; onConfigure: () => void; onAddFolder: () => void; onSave: () => void; onClose: () => void }) {
  const [generation, setGeneration] = useState(0)
  const [selected, setSelected] = useState<string | null>(null)
  const [menu, setMenu] = useState<RootMenu | null>(null)
  const [removing, setRemoving] = useState(false)
  const [operationError, setOperationError] = useState('')
  const menuRef = useRef<HTMLDivElement>(null)
  const roots = useMemo(() => data?.folders.map(folder => ({ ...folder, kind: 'directory' as const })) ?? [], [data?.folders])
  const openRootMenu = useCallback((next: RootMenu) => { setMenu(next); setOperationError('') }, [])
  const [width, setWidth] = useState(() => {
    try { const saved = Number(localStorage.getItem('devflow.explorer.width') ?? 260); return clampWidth(Number.isFinite(saved) ? saved : 260) } catch { return clampWidth(260) }
  })
  const [resizing, setResizing] = useState(false)
  const drag = useRef<{ x: number; width: number; pointerId: number } | null>(null)
  const dragCleanup = useRef<(() => void) | null>(null)
  const liveWidth = useRef(width)
  const resizeHandle = useRef<HTMLDivElement>(null)
  function resize(next: number, persist = false) {
    const value = clampWidth(next); liveWidth.current = value; setWidth(value)
    if (persist) { try { localStorage.setItem('devflow.explorer.width', String(value)) } catch { /* Keep the current width in memory. */ } }
  }
  function finishResize() { drag.current = null; dragCleanup.current?.(); dragCleanup.current = null; setResizing(false); resize(liveWidth.current, true) }
  useEffect(() => () => { dragCleanup.current?.() }, [])
  useEffect(() => {
    const fit = () => { const value = clampWidth(liveWidth.current); liveWidth.current = value; setWidth(value) }
    window.addEventListener('resize', fit)
    return () => window.removeEventListener('resize', fit)
  }, [])
  const hasWorkspace = !!(data?.workspaceRoot || data?.workspaceFile)
  useEffect(() => { setSelected(null); setMenu(null) }, [data?.revision])
  useEffect(() => {
    if (!menu) return
    const dismiss = (event: PointerEvent) => { if (!menuRef.current?.contains(event.target as Node)) setMenu(null) }
    const escape = (event: globalThis.KeyboardEvent) => { if (event.key === 'Escape') { event.stopPropagation(); setMenu(null); menu.target.focus() } }
    const dismissOnLayoutChange = () => setMenu(null)
    document.addEventListener('pointerdown', dismiss)
    document.addEventListener('keydown', escape)
    window.addEventListener('resize', dismissOnLayoutChange)
    window.addEventListener('scroll', dismissOnLayoutChange, true)
    return () => { document.removeEventListener('pointerdown', dismiss); document.removeEventListener('keydown', escape); window.removeEventListener('resize', dismissOnLayoutChange); window.removeEventListener('scroll', dismissOnLayoutChange, true) }
  }, [menu])
  async function removeRoot() {
    if (!menu || removing) return
    setRemoving(true); setOperationError('')
    try { const result = await executeWorkspace({ type: 'remove_folder', path: menu.entry.path }, menu.revision); onChange(result); setMenu(null) }
    catch (err) { setOperationError(err instanceof Error ? err.message : String(err)); if (err instanceof ApiError && err.code === 'conflict') onRefresh() }
    finally { setRemoving(false) }
  }
  return <aside className={`workspace-sidebar ${resizing ? 'is-resizing' : ''}`} style={{ '--explorer-width': `${width}px` } as CSSProperties} id="workspace-explorer" aria-label="工作区资源管理器">
    <div className="ws-explorer-heading"><h2>资源管理器</h2><div>
      <button className="ws-icon-button" disabled={!hasWorkspace} aria-label="添加文件夹到工作区" title="添加文件夹到工作区" onClick={onAddFolder}><FolderPlus size={15} /></button>
      {data?.dirty && <button className="ws-icon-button" aria-label="保存工作区更改" title="保存工作区更改" onClick={onSave}><Save size={14} /></button>}
      <button className="ws-icon-button" aria-label="刷新目录树" title="刷新目录树" onClick={() => setGeneration(current => current + 1)}><RefreshCw size={14} /></button>
      <button className="ws-icon-button" aria-label="折叠所有文件夹" title="折叠所有文件夹" onClick={() => setGeneration(current => current + 1)}><ChevronsDownUp size={14} /></button>
      <button className="ws-icon-button" aria-label="收起资源管理器" title="收起资源管理器" onClick={onClose}><X size={14} /></button>
    </div></div>
    <div className="ws-tree-workspace" title={data?.workspaceFile ?? data?.workspaceRoot ?? ''}><span>{data?.name ?? '未选择工作区'}</span>{data?.dirty && <small>未保存</small>}</div>
    <div className="ws-tree-scroll">
      <ul role="tree" aria-label="工作区文件夹" key={`${data?.revision ?? 0}:${generation}`}>
        {roots.map(folder => <MemoTreeNode key={folder.path} entry={folder} level={1} revision={data!.revision} selected={selected} onSelect={setSelected} onRootMenu={openRootMenu} />)}
      </ul>
      {!data?.folders.length && <div className="ws-tree-empty"><p>{hasWorkspace ? '工作区还没有文件夹' : '打开文件夹或工作区'}</p><button className="ws-outline-button" onClick={hasWorkspace ? onAddFolder : onConfigure}><FolderPlus size={14} />{hasWorkspace ? '添加文件夹' : '打开工作区'}</button></div>}
      {data?.warnings.map(warning => <p className="ws-tree-note is-error" role="status" key={warning}>{warning}</p>)}
      {operationError && <p className="ws-tree-note is-error" role="alert">{operationError}</p>}
    </div>
    <div className="ws-tree-footer" title={selected ?? data?.workspaceFile ?? ''}>{selected ?? data?.workspaceFile ?? '按需展开目录 · 不扫描构建产物'}</div>
    <div ref={resizeHandle} className="ws-sidebar-resizer" role="separator" aria-label="调整资源管理器宽度" aria-orientation="vertical" aria-valuemin={200} aria-valuemax={maxSidebarWidth()} aria-valuenow={width} tabIndex={0} title="拖拽调整宽度 · 双击恢复默认" onPointerDown={event => {
      if (event.button !== 0) return
      event.preventDefault(); event.stopPropagation(); event.currentTarget.focus()
      dragCleanup.current?.()
      const handle = event.currentTarget, pointerId = event.pointerId
      try { handle.setPointerCapture(pointerId) } catch { /* Window listeners support embedded browsers without pointer capture. */ }
      drag.current = { x: event.clientX, width: liveWidth.current, pointerId }; setResizing(true)
      const move = (next: PointerEvent) => { if (drag.current?.pointerId === next.pointerId) resize(drag.current.width + next.clientX - drag.current.x) }
      const end = (next: PointerEvent) => { if (next.pointerId === pointerId) finishResize() }
      const blur = () => finishResize()
      window.addEventListener('pointermove', move); window.addEventListener('pointerup', end); window.addEventListener('pointercancel', end); window.addEventListener('blur', blur)
      dragCleanup.current = () => {
        window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', end); window.removeEventListener('pointercancel', end); window.removeEventListener('blur', blur)
        try { if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId) } catch { /* The element may have been removed. */ }
      }
    }} onLostPointerCapture={() => { if (drag.current) finishResize() }} onDoubleClick={() => resize(260, true)} onKeyDown={event => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
      event.preventDefault()
      resize(event.key === 'Home' ? 200 : event.key === 'End' ? maxSidebarWidth() : width + (event.key === 'ArrowLeft' ? -1 : 1) * (event.shiftKey ? 40 : 10), true)
    }}><span className="ws-resize-grip" aria-hidden="true" /></div>
    {menu && <div ref={menuRef} className="ws-tree-context" role="menu" aria-label={`${menu.entry.name} 文件夹操作`} style={{ left: Math.max(8, Math.min(menu.x, window.innerWidth - 250)), top: Math.max(8, Math.min(menu.y, window.innerHeight - 130)) }}>
      <span className="ws-context-name" title={menu.entry.path}>{menu.entry.name}</span>
      <button role="menuitem" autoFocus disabled={removing} onClick={() => void removeRoot()}><FolderMinus size={15} />{removing ? '正在移除…' : '从工作区移除'}</button>
      <small>仅移除引用，不删除文件；保存后持久化。</small>
    </div>}
  </aside>
}
