import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactElement } from 'react'
import { AlertCircle, Check, ChevronRight, ChevronsDownUp, Copy, FolderMinus, FolderPlus, LoaderCircle, MoreHorizontal, RefreshCw, Save, Trash2, X } from 'lucide-react'
import { TreeEntryIcon } from './TreeEntryIcon'
import { GitPanel } from '../git/GitPanel'
import type { GitDiff } from '../git/api'
import { WorkspaceSearch } from './WorkspaceSearch'
import { ApiError, executeWorkspace, getWorkspaceChildren, mutateWorkspaceFile, type WorkspaceChildren, type WorkspaceResponse, type WorkspaceTreeEntry } from '../../lib/api'

type NodeEntry = WorkspaceTreeEntry & { status?: string; message?: string | null }
type TreeRoot = { name: string; path: string }
type NodeMenu = { entry: NodeEntry; root: TreeRoot; level: number; revision: number; x: number; y: number; target: HTMLElement }
type NodeProps = { entry: NodeEntry; root: TreeRoot; level: number; revision: number; selected: string | null; copiedPath: string | null; onCopy: (path: string, value?: string) => Promise<void>; onSelect: (path: string) => void; onOpenFile: (path: string, pinned?: boolean) => void; onNodeMenu: (menu: NodeMenu) => void }

function relativeTreePath(path: string, root: string) {
  const normalized = path.replace(/\\/g, '/'), base = root.replace(/\\/g, '/').replace(/\/+$/, '')
  const windows = /^[a-z]:\//i.test(normalized) || normalized.startsWith('//')
  const comparable = (value: string) => windows ? value.toLowerCase() : value
  if (comparable(normalized.replace(/\/+$/, '')) === comparable(base)) return '.'
  return comparable(normalized).startsWith(`${comparable(base)}/`) ? normalized.slice(base.length + 1) : null
}

function TreeNode({ entry, root, level, revision, selected, copiedPath, onCopy, onSelect, onOpenFile, onNodeMenu }: NodeProps): ReactElement {
  const directory = entry.kind === 'directory'
  const available = !entry.status || entry.status === 'available'
  const [expanded, setExpanded] = useState(false)
  const [children, setChildren] = useState<WorkspaceChildren | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const pending = useRef<AbortController | null>(null)
  useEffect(() => () => pending.current?.abort(), [])
  useEffect(() => {
    const changed = (event: Event) => {
      const path = (event as CustomEvent<string>).detail.replace(/\\/g, '/')
      const directory = entry.path.replace(/\\/g, '/').replace(/\/+$/, '')
      if (expanded && path.slice(0, path.lastIndexOf('/')).toLowerCase() === directory.toLowerCase()) void readChildren()
    }
    window.addEventListener('devflow-tree-file-changed', changed)
    return () => window.removeEventListener('devflow-tree-file-changed', changed)
  }, [expanded, entry.path, revision])
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
    if (!directory) { if (available) onOpenFile(entry.path); return }
    if (!available) return
    setExpanded(current => !current)
    if (!expanded && !children && !loading) void readChildren()
  }
  function onKeyDown(event: KeyboardEvent<HTMLLIElement>) {
    if (event.target !== event.currentTarget) return
    const element = event.currentTarget
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault(); event.stopPropagation()
      const rect = element.querySelector('.ws-tree-row')!.getBoundingClientRect()
      onNodeMenu({ entry, root, level, revision, x: rect.left + 20, y: rect.bottom, target: element }); return
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
    onDoubleClick={event => { event.stopPropagation(); if (!(event.target as HTMLElement).closest('button') && !directory && available) onOpenFile(entry.path, true) }}
    onKeyDown={onKeyDown} onContextMenu={event => {
      event.stopPropagation()
      event.preventDefault(); onSelect(entry.path)
      onNodeMenu({ entry, root, level, revision, x: event.clientX, y: event.clientY, target: event.currentTarget })
    }} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); toggle() }}>
    <div className={`ws-tree-row ${available ? '' : 'is-unavailable'}`} data-copy-path={entry.path} style={{ paddingLeft: 9 + (level - 1) * 14 }} title={`${entry.path}${entry.message ? `\n${entry.message}` : ''}\n悬停时按 C 复制绝对路径`}>
      <span className="ws-tree-chevron">{directory && available && <ChevronRight size={13} className={expanded ? 'is-expanded' : ''} />}</span>
      {loading ? <LoaderCircle size={16} aria-hidden="true" className="ws-tree-icon spinning" /> : <TreeEntryIcon name={entry.name} directory={directory} expanded={expanded} root={level === 1} />}
      <span className="ws-tree-label">{entry.name}</span>{!available && <span className="ws-tree-unavailable"><AlertCircle size={12} />{status}</span>}
      <button className={`ws-tree-copy ${copiedPath === entry.path ? 'is-copied' : ''}`} aria-label={`复制 ${entry.name} 的绝对路径`} title={copiedPath === entry.path ? '路径已复制' : '复制绝对路径 · 悬停时按 C'} onClick={event => { event.stopPropagation(); void onCopy(entry.path) }}>
        {copiedPath === entry.path ? <Check size={14} /> : <Copy size={14} />}
      </button>
      {(level === 1 || !directory) && <button className="ws-tree-more" aria-label={`${entry.name} 操作`} title="更多操作" onClick={event => {
        event.stopPropagation(); const rect = event.currentTarget.getBoundingClientRect(); onSelect(entry.path)
        onNodeMenu({ entry, root, level, revision, x: rect.left, y: rect.bottom, target: event.currentTarget })
      }}><MoreHorizontal size={14} /></button>}
    </div>
    {expanded && <ul role="group">
      {children?.entries.map(child => <MemoTreeNode key={child.path} entry={child} root={root} level={level + 1} revision={revision} selected={selected} copiedPath={copiedPath} onCopy={onCopy} onSelect={onSelect} onOpenFile={onOpenFile} onNodeMenu={onNodeMenu} />)}
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

export function WorkspaceTree({ data, open, view, onChange, onRefresh, onConfigure, onAddFolder, onSave, onClose, onOpenFile, onOpenDiff }: { data: WorkspaceResponse | null; open: boolean; view: 'explorer' | 'search' | 'git'; onOpenDiff: (diff: GitDiff) => void; onChange: (data: WorkspaceResponse) => void; onRefresh: () => void; onConfigure: () => void; onAddFolder: () => void; onSave: () => void; onClose: () => void; onOpenFile: (path: string, pinned?: boolean, line?: number, column?: number) => void }) {
  const [generation, setGeneration] = useState(0)
  const [selected, setSelected] = useState<string | null>(null)
  const [menu, setMenu] = useState<NodeMenu | null>(null)
  const [removing, setRemoving] = useState(false)
  const mutating = useRef(false)
  const [operationError, setOperationError] = useState('')
  const [copyNotice, setCopyNotice] = useState<{ message: string; error: boolean; path?: string } | null>(null)
  const copyScope = useRef(0)
  const copying = useRef(false)
  const copyTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const sidebarRef = useRef<HTMLElement>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const roots = useMemo(() => data?.folders.map(folder => ({ ...folder, kind: 'directory' as const })) ?? [], [data?.folders])
  const openNodeMenu = useCallback((next: NodeMenu) => { setMenu(next); setOperationError('') }, [])
  const copyPath = useCallback(async (path: string, value = path) => {
    if (copying.current) return
    copying.current = true
    const scope = copyScope.current
    let notice: { message: string; error: boolean; path?: string }
    try {
      await navigator.clipboard.writeText(value)
      notice = { message: `路径已复制：${value}`, error: false, path }
    } catch {
      notice = { message: '复制失败，请允许浏览器访问剪贴板后重试', error: true }
    } finally { copying.current = false }
    if (scope !== copyScope.current) return
    setCopyNotice(notice)
    clearTimeout(copyTimer.current)
    copyTimer.current = setTimeout(() => setCopyNotice(null), 3000)
  }, [])
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
  useEffect(() => { setMenu(null); finishResize() }, [open, view])
  useEffect(() => {
    setCopyNotice(null)
    const copyHoveredPath = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || event.key.toLowerCase() !== 'c') return
      if (!document.hasFocus() || menuRef.current || document.querySelector('dialog[open], [role="dialog"][aria-modal="true"]')) return
      const editable = (target: EventTarget | null) => target instanceof HTMLElement && (target.isContentEditable || !!target.closest('input, textarea, select, [role="textbox"], [role="combobox"]'))
      if (editable(event.target) || editable(document.activeElement)) return
      const path = sidebarRef.current?.querySelector<HTMLElement>('.ws-tree-row:hover')?.dataset.copyPath
      if (!path) return
      event.preventDefault()
      void copyPath(path)
    }
    document.addEventListener('keydown', copyHoveredPath)
    return () => { copyScope.current += 1; clearTimeout(copyTimer.current); document.removeEventListener('keydown', copyHoveredPath) }
  }, [data?.revision, generation, copyPath])
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
    if (!menu || menu.level !== 1 || removing) return
    setRemoving(true); setOperationError('')
    try { const result = await executeWorkspace({ type: 'remove_folder', path: menu.entry.path }, menu.revision); onChange(result); setMenu(null) }
    catch (err) { setOperationError(err instanceof Error ? err.message : String(err)); if (err instanceof ApiError && err.code === 'conflict') onRefresh() }
    finally { setRemoving(false) }
  }
  async function mutateFile(operation: 'copy' | 'delete') {
    if (!menu || menu.entry.kind !== 'file' || mutating.current) return
    const { entry, revision } = menu
    if (operation === 'delete') {
      if (!window.confirm(`确定永久删除“${entry.name}”？文件和此文件未保存的修改都将删除，无法撤销。`)) return
      if (!window.dispatchEvent(new CustomEvent('devflow-before-file-delete', { detail: entry.path, cancelable: true }))) {
        setOperationError('文件正在读取、保存或删除，请稍后重试。'); return
      }
    }
    mutating.current = true; setRemoving(true); setOperationError('')
    let deleted = false
    try {
      await mutateWorkspaceFile(entry.path, revision, operation)
      deleted = operation === 'delete'
      setMenu(null)
      if (deleted) setSelected(current => current === entry.path ? null : current)
      window.dispatchEvent(new CustomEvent('devflow-tree-file-changed', { detail: entry.path }))
    } catch (err) {
      setOperationError(err instanceof Error ? err.message : String(err))
      if (err instanceof ApiError && err.code === 'conflict') onRefresh()
    } finally {
      if (operation === 'delete') window.dispatchEvent(new CustomEvent('devflow-file-delete-finished', { detail: { path: entry.path, deleted } }))
      mutating.current = false; setRemoving(false)
    }
  }
  return <aside ref={sidebarRef} hidden={!open} className={`workspace-sidebar ${resizing ? 'is-resizing' : ''}`} style={{ '--explorer-width': `${width}px` } as CSSProperties} id="workspace-sidebar" aria-label={view === 'git' ? '工作区 Git 管理' : view === 'search' ? '工作区搜索' : '工作区资源管理器'}>
    <div className="ws-explorer-view" hidden={view !== 'explorer'}>
    <div className="ws-explorer-heading"><h2 className="ws-explorer-title" title={data?.workspaceFile ?? data?.workspaceRoot ?? ''}><span className="ws-explorer-name">{data?.name ?? '未选择工作区'}</span>{data?.dirty && <small className="ws-explorer-unsaved">未保存</small>}</h2><div>
      <button className="ws-icon-button" disabled={!hasWorkspace} aria-label="添加文件夹到工作区" title="添加文件夹到工作区" onClick={onAddFolder}><FolderPlus size={15} /></button>
      {data?.dirty && <button className="ws-icon-button" aria-label="保存工作区更改" title="保存工作区更改" onClick={onSave}><Save size={14} /></button>}
      <button className="ws-icon-button" aria-label="刷新目录树" title="刷新目录树" onClick={() => setGeneration(current => current + 1)}><RefreshCw size={14} /></button>
      <button className="ws-icon-button" aria-label="折叠所有文件夹" title="折叠所有文件夹" onClick={() => setGeneration(current => current + 1)}><ChevronsDownUp size={14} /></button>
      <button className="ws-icon-button" aria-label="收起资源管理器" title="收起资源管理器" onClick={onClose}><X size={14} /></button>
    </div></div>
    <div className="ws-tree-scroll">
      <ul role="tree" aria-label="工作区文件夹" key={`${data?.revision ?? 0}:${generation}`}>
        {roots.map(folder => <MemoTreeNode key={folder.path} entry={folder} root={folder} level={1} revision={data!.revision} selected={selected} copiedPath={copyNotice?.path ?? null} onCopy={copyPath} onSelect={setSelected} onOpenFile={onOpenFile} onNodeMenu={openNodeMenu} />)}
      </ul>
      {!data?.folders.length && <div className="ws-tree-empty"><p>{hasWorkspace ? '工作区还没有文件夹' : '打开文件夹或工作区'}</p><button className="ws-outline-button" onClick={hasWorkspace ? onAddFolder : onConfigure}><FolderPlus size={14} />{hasWorkspace ? '添加文件夹' : '打开工作区'}</button></div>}
      {data?.warnings.map(warning => <p className="ws-tree-note is-error" role="status" key={warning}>{warning}</p>)}
      {operationError && <p className="ws-tree-note is-error" role="alert">{operationError}</p>}
    </div>
    <div className={`ws-tree-footer ${copyNotice?.error ? 'is-error' : ''}`} title={copyNotice?.message ?? selected ?? data?.workspaceFile ?? ''}><span role="status" aria-live="polite" aria-atomic="true">{copyNotice?.message}</span>{!copyNotice && (selected ?? data?.workspaceFile ?? '悬停文件或文件夹，按 C 复制绝对路径')}</div>
    </div>
    <div className="ws-search-view" hidden={view !== 'search'}><WorkspaceSearch key={data?.revision ?? 0} workspace={data} active={open && view === 'search'} onOpenFile={onOpenFile} onClose={onClose} /></div>
    <div className="ws-search-view" hidden={view !== 'git'}><GitPanel key={data?.revision ?? 0} workspace={data} active={open && view === 'git'} onOpenDiff={onOpenDiff} onClose={onClose} /></div>
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
    {menu && <div ref={menuRef} className="ws-tree-context" role="menu" aria-label={`${menu.entry.name} 操作`} style={{ left: Math.max(8, Math.min(menu.x, window.innerWidth - 250)), top: Math.max(8, Math.min(menu.y, window.innerHeight - 270)) }} onKeyDown={event => {
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
      event.preventDefault(); event.stopPropagation()
      const items = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'))
      const index = items.indexOf(document.activeElement as HTMLButtonElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? items.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + items.length) % items.length
      items[next]?.focus()
    }}>
      <span className="ws-context-name" title={menu.entry.path}>{menu.entry.name}</span>
      <button role="menuitem" autoFocus onClick={() => { void copyPath(menu.entry.path); menu.target.focus(); setMenu(null) }}><Copy size={15} />复制绝对路径</button>
      <button role="menuitem" disabled={relativeTreePath(menu.entry.path, menu.root.path) === null} title={`相对于 ${menu.root.name}；根文件夹自身为 .`} onClick={() => {
        const relative = relativeTreePath(menu.entry.path, menu.root.path)
        if (relative === null) return
        void copyPath(menu.entry.path, relative); menu.target.focus(); setMenu(null)
      }}><Copy size={15} /><span>复制相对路径<small className="ws-context-relative">相对 {menu.root.name}</small></span></button>
      {menu.entry.kind === 'file' && <>
        <button role="menuitem" disabled={removing} title="将磁盘文件复制为同目录副本" onClick={() => void mutateFile('copy')}><Copy size={15} />复制文件</button>
        <button className="ws-context-remove" role="menuitem" disabled={removing} onClick={() => void mutateFile('delete')}><Trash2 size={15} />删除文件</button>
      </>}
      {menu.level === 1 && <><button className="ws-context-remove" role="menuitem" disabled={removing} onClick={() => void removeRoot()}><FolderMinus size={15} />{removing ? '正在移除…' : '从工作区移除'}</button>
      <small>仅移除引用，不删除文件；保存后持久化。</small></>}
    </div>}
  </aside>
}
