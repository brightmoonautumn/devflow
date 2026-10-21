import { readViewState, viewPathKey, workspaceViewKey, writeViewState } from './workspaceViewState'
import { copyText } from '../../lib/clipboard'
import { readOnlyShare } from '../../lib/access'
import { useDialogs } from '../../components/form/BaseDialog'
import { memo, useCallback, useEffect, useMemo, useRef, useState, type CSSProperties, type KeyboardEvent, type ReactElement } from 'react'
import { AlertCircle, Check, ChevronsDownUp, Copy, FolderMinus, FolderPlus, LoaderCircle, MoreHorizontal, RefreshCw, Save, Settings2, Trash2, X } from 'lucide-react'
import { TreeEntryIcon } from '../../components/files/TreeEntryIcon'
import { FileTreeRow, TreeChevron, TreePathLabel, handleTreeNavigation } from '../../components/files/FileTreeRow'
import { compactFolderChild, MAX_COMPACT_FOLDER_SEGMENTS } from '../../components/files/compactFolders'
import { useCompactFolders } from '../../components/files/useCompactFolders'
import { WorkspaceSearch } from './WorkspaceSearch'
import { ApiError, executeWorkspace, getWorkspaceChildren, mutateWorkspaceFile, type WorkspaceChildren, type WorkspaceResponse, type WorkspaceTreeEntry } from '../../lib/api'

type NodeEntry = WorkspaceTreeEntry & { status?: string; message?: string | null }
type TreeRoot = { name: string; path: string }
type NodeMenu = { entry: NodeEntry; root: TreeRoot; level: number; revision: number; x: number; y: number; target: HTMLElement }
type NodeProps = { compact: boolean; scope: string | null; entry: NodeEntry; root: TreeRoot; level: number; revision: number; selected: string | null; copiedPath: string | null; onCopy: (path: string, value?: string) => Promise<void>; onSelect: (path: string) => void; onOpenFile: (path: string, pinned?: boolean) => void; onNodeMenu: (menu: NodeMenu) => void }

function relativeTreePath(path: string, root: string) {
  const normalized = path.replace(/\\/g, '/'), base = root.replace(/\\/g, '/').replace(/\/+$/, '')
  const windows = /^[a-z]:\//i.test(normalized) || normalized.startsWith('//')
  const comparable = (value: string) => windows ? value.toLowerCase() : value
  if (comparable(normalized.replace(/\/+$/, '')) === comparable(base)) return '.'
  return comparable(normalized).startsWith(`${comparable(base)}/`) ? normalized.slice(base.length + 1) : null
}

function TreeNode({ compact, scope, entry, root, level, revision, selected, copiedPath, onCopy, onSelect, onOpenFile, onNodeMenu }: NodeProps): ReactElement {
  const directory = entry.kind === 'directory'
  const available = !entry.status || entry.status === 'available'
  const [expanded, setExpanded] = useState(() => {
    const saved = scope ? readViewState(scope).expanded : []
    return Array.isArray(saved) && saved.some(path => typeof path === 'string' && viewPathKey(path) === viewPathKey(entry.path))
  })
  function expand(next: boolean) {
    setExpanded(next)
    if (!scope) return
    const saved = readViewState(scope).expanded
    const paths = new Set(Array.isArray(saved) ? saved.filter(path => typeof path === 'string').slice(0, 200) : [])
    for (const path of paths) if (segments.some(segment => viewPathKey(path) === viewPathKey(segment.path))) paths.delete(path)
    if (next) for (const segment of segments) paths.add(segment.path)
    writeViewState(scope, { expanded: [...paths].slice(-200) })
  }
  const [segments, setSegments] = useState<NodeEntry[]>([entry])
  const [children, setChildren] = useState<WorkspaceChildren | null>(null)
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState('')
  const pending = useRef<AbortController | null>(null)
  const cache = useRef(new Map<string, WorkspaceChildren>())
  const activeEntry = segments.find(segment => segment.path === selected) ?? segments[segments.length - 1]
  const empty = !!children && !children.truncated && children.entries.length === 0 && !error && !loading
  const expandable = directory && available && !empty
  const label = segments.map(segment => segment.name).join('/')
  const readChildren = useCallback(async (refresh = false) => {
    pending.current?.abort()
    const controller = new AbortController()
    pending.current = controller
    if (refresh) cache.current.clear()
    setLoading(true); setError('')
    const chain: NodeEntry[] = [entry]
    let current = entry
    let resolved: WorkspaceChildren | null = null
    try {
      // Probe only visible folders and their single-child chains, never entire subtrees.
      for (let depth = 0; depth < MAX_COMPACT_FOLDER_SEGMENTS; depth += 1) {
        const result = cache.current.get(current.path) ?? await getWorkspaceChildren(current.path, revision, controller.signal)
        if (controller.signal.aborted) return
        cache.current.set(current.path, result)
        resolved = result
        const child = compactFolderChild(chain, result.entries, {
          enabled: compact && level > 1, truncated: result.truncated,
          key: segment => viewPathKey(segment.path), isDirectory: segment => segment.kind === 'directory',
        })
        if (!child) break
        chain.push(child); current = child
      }
      // Publish the chain atomically so intermediate folders do not mount and probe again.
      setSegments([...chain]); setChildren(resolved)
    } catch (err) {
      if (!controller.signal.aborted) {
        // Keep the last confirmed chain; the failed folder remains explicitly retryable.
        setSegments([...chain]); setChildren(null)
        setError(err instanceof Error ? err.message : String(err))
      }
    } finally {
      if (!controller.signal.aborted) setLoading(false)
      if (pending.current === controller) pending.current = null
    }
  }, [entry, revision, compact, level])
  useEffect(() => {
    if (directory && available && (expanded || (compact && level > 1))) void readChildren()
    return () => { pending.current?.abort(); pending.current = null }
  }, [expanded, directory, available, compact, level, readChildren])
  useEffect(() => {
    const changed = (event: Event) => {
      const path = (event as CustomEvent<string>).detail.replace(/\\/g, '/')
      const parent = path.slice(0, path.lastIndexOf('/'))
      if (segments.some(segment => viewPathKey(segment.path.replace(/\\/g, '/').replace(/\/+$/, '')) === viewPathKey(parent)) && directory && available) void readChildren(true)
    }
    window.addEventListener('devflow-tree-file-changed', changed)
    return () => window.removeEventListener('devflow-tree-file-changed', changed)
  }, [segments, directory, available, readChildren])
  useEffect(() => {
    if (!scope || !expanded) return
    const saved = readViewState(scope).expanded
    const paths = new Set(Array.isArray(saved) ? saved.filter(path => typeof path === 'string') : [])
    for (const segment of segments) paths.add(segment.path)
    writeViewState(scope, { expanded: [...paths].slice(-200) })
  }, [scope, expanded, segments])
  function toggle(target = activeEntry) {
    onSelect(target.path)
    if (!directory) { if (available) onOpenFile(entry.path); return }
    if (!expandable) return
    expand(!expanded)
  }
  function onKeyDown(event: KeyboardEvent<HTMLLIElement>) {
    if (event.target !== event.currentTarget) return
    const element = event.currentTarget
    if (segments.length > 1 && event.altKey && ['ArrowLeft', 'ArrowRight'].includes(event.key)) {
      event.preventDefault(); event.stopPropagation()
      const index = segments.indexOf(activeEntry) + (event.key === 'ArrowRight' ? 1 : -1)
      if (segments[index]) onSelect(segments[index].path)
      return
    }
    if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
      event.preventDefault(); event.stopPropagation()
      const rect = element.querySelector('.ws-tree-row')!.getBoundingClientRect()
      onNodeMenu({ entry: activeEntry, root, level, revision, x: rect.left + 20, y: rect.bottom, target: element }); return
    }
    handleTreeNavigation(event, { expanded, expandable, onToggle: next => { if (next) onSelect(activeEntry.path); expand(next) }, onActivate: toggle })
  }
  const status = entry.status === 'blocked' ? '未授权' : entry.status === 'unsupported' ? '不支持' : '不可用'
  return <li role="treeitem" tabIndex={0} aria-label={label} aria-level={level} aria-selected={segments.some(segment => segment.path === selected)} aria-expanded={expandable ? expanded : undefined} aria-disabled={!available || undefined}
    onDoubleClick={event => { event.stopPropagation(); if (!(event.target as HTMLElement).closest('button') && !directory && available) onOpenFile(entry.path, true) }}
    onKeyDown={onKeyDown} onContextMenu={event => {
      event.stopPropagation()
      event.preventDefault(); onSelect(activeEntry.path)
      onNodeMenu({ entry: activeEntry, root, level, revision, x: event.clientX, y: event.clientY, target: event.currentTarget })
    }} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); toggle() }}>
    <FileTreeRow className={`ws-tree-row ${available ? '' : 'is-unavailable'}`} data-copy-path={activeEntry.path} style={{ paddingLeft: 9 + (level - 1) * 14 }} title={`${activeEntry.path}${entry.message ? `\n${entry.message}` : ''}\n悬停时按 C 复制绝对路径`}>
      <TreeChevron expanded={expanded} expandable={expandable} />
      {loading ? <LoaderCircle size={16} aria-hidden="true" className="ws-tree-icon spinning" /> : <TreeEntryIcon name={segments[segments.length - 1].name} directory={directory} expanded={expanded} root={level === 1} />}
      <TreePathLabel segments={segments} renderSegment={segments.length === 1 ? undefined : segment => <button type="button" tabIndex={-1} className={`ws-tree-segment ${segment.path === selected ? 'is-selected' : ''}`} data-copy-path={segment.path} title={`${segment.path}\nAlt+←/→ 切换路径段；悬停时按 C 复制`} onClick={event => { event.stopPropagation(); event.currentTarget.closest<HTMLElement>('[role="treeitem"]')?.focus(); toggle(segment) }} onContextMenu={event => {
        event.preventDefault(); event.stopPropagation(); onSelect(segment.path)
        onNodeMenu({ entry: segment, root, level, revision, x: event.clientX, y: event.clientY, target: event.currentTarget.closest<HTMLElement>('[role="treeitem"]')! })
      }}>{segment.name}</button>} />{!available && <span className="ws-tree-unavailable"><AlertCircle size={12} />{status}</span>}
      <button className={`ws-tree-copy ${copiedPath === activeEntry.path ? 'is-copied' : ''}`} aria-label={`复制 ${activeEntry.name} 的绝对路径`} title={copiedPath === activeEntry.path ? '路径已复制' : '复制绝对路径 · 悬停时按 C'} onClick={event => { event.stopPropagation(); void onCopy(activeEntry.path) }}>
        {copiedPath === activeEntry.path ? <Check size={14} /> : <Copy size={14} />}
      </button>
      {(level === 1 || !directory) && <button className="ws-tree-more" aria-label={`${entry.name} 操作`} title="更多操作" onClick={event => {
        event.stopPropagation(); const rect = event.currentTarget.getBoundingClientRect(); onSelect(entry.path)
        onNodeMenu({ entry, root, level, revision, x: rect.left, y: rect.bottom, target: event.currentTarget })
      }}><MoreHorizontal size={14} /></button>}
    </FileTreeRow>
    {((expanded && !empty) || !!error) && <ul role="group">
      {expanded && children?.entries.map(child => <MemoTreeNode compact={compact} scope={scope} key={child.path} entry={child} root={root} level={level + 1} revision={revision} selected={selected} copiedPath={copiedPath} onCopy={onCopy} onSelect={onSelect} onOpenFile={onOpenFile} onNodeMenu={onNodeMenu} />)}
      {expanded && loading && <li role="none" className="ws-tree-note">正在读取…</li>}
      {error && <li role="none" className="ws-tree-note is-error" onClick={event => event.stopPropagation()}><span role="status">{error}</span><button onClick={() => void readChildren(true)}>重试</button></li>}
      {expanded && children?.truncated && <li role="none" className="ws-tree-note">目录过大，仅显示前 2000 个条目范围内的内容。</li>}
    </ul>}
  </li>
}
const MemoTreeNode = memo(TreeNode)

function maxSidebarWidth() { return window.innerWidth < 720 ? Math.max(200, Math.min(360, window.innerWidth - (window.innerWidth <= 550 ? 45 : 51) - 28)) : Math.max(200, Math.min(520, window.innerWidth - 420)) }
function clampWidth(width: number) { return Math.round(Math.max(200, Math.min(maxSidebarWidth(), width))) }

export function WorkspaceTree({ data, open, view, onChange, onRefresh, onConfigure, onAddFolder, onSave, onClose, onOpenFile }: { data: WorkspaceResponse | null; open: boolean; view: 'explorer' | 'search'; onChange: (data: WorkspaceResponse) => void; onRefresh: () => void; onConfigure: () => void; onAddFolder: () => void; onSave: () => void; onClose: () => void; onOpenFile: (path: string, pinned?: boolean, line?: number, column?: number) => void }) {
  const dialogs = useDialogs()
  const scope = workspaceViewKey(data)
  const [compact, setCompact] = useCompactFolders()
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
      await copyText(value)
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
  useEffect(() => { const saved = scope ? readViewState(scope).selected : null; setSelected(typeof saved === 'string' ? saved : null); setMenu(null) }, [scope, data?.revision])
  const selectPath = useCallback((path: string) => { setSelected(path); if (scope) writeViewState(scope, { selected: path }) }, [scope])
  useEffect(() => { setMenu(null); finishResize() }, [open, view])
  useEffect(() => {
    setCopyNotice(null)
    const copyHoveredPath = (event: globalThis.KeyboardEvent) => {
      if (event.defaultPrevented || event.repeat || event.isComposing || event.ctrlKey || event.metaKey || event.altKey || event.shiftKey || event.key.toLowerCase() !== 'c') return
      if (!document.hasFocus() || menuRef.current || document.querySelector('dialog[open], [role="dialog"][aria-modal="true"]')) return
      const editable = (target: EventTarget | null) => target instanceof HTMLElement && (target.isContentEditable || !!target.closest('input, textarea, select, [role="textbox"], [role="combobox"]'))
      if (editable(event.target) || editable(document.activeElement)) return
      const path = sidebarRef.current?.querySelector<HTMLElement>('.ws-tree-segment:hover')?.dataset.copyPath ?? sidebarRef.current?.querySelector<HTMLElement>('.ws-tree-row:hover')?.dataset.copyPath
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
      if (!await dialogs.confirm({ title: '永久删除文件？', description: `“${entry.name}”及其未保存的修改都将删除，此操作无法撤销。`, confirmText: '永久删除', tone: 'danger' })) return
      if (mutating.current) return
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
  return <aside ref={sidebarRef} hidden={!open} className={`workspace-sidebar ${resizing ? 'is-resizing' : ''}`} style={{ '--explorer-width': `${width}px` } as CSSProperties} id="workspace-sidebar" aria-label={view === 'search' ? '工作区搜索' : '工作区资源管理器'}>
    <div className="ws-explorer-view" hidden={view !== 'explorer'}>
    <div className="ws-explorer-heading"><h2 className="ws-explorer-title" title={data?.workspaceFile ?? data?.workspaceRoot ?? ''}><span className="ws-explorer-name">{data?.name ?? '未选择工作区'}</span>{data?.dirty && <small className="ws-explorer-unsaved">未保存</small>}</h2><div>
      {!readOnlyShare && <button className="ws-icon-button" disabled={!hasWorkspace} aria-label="添加文件夹到工作区" title="添加文件夹到工作区" onClick={onAddFolder}><FolderPlus size={15} /></button>}
      {!readOnlyShare && data?.dirty && <button className="ws-icon-button" aria-label="保存工作区更改" title="保存工作区更改" onClick={onSave}><Save size={14} /></button>}
      <button className="ws-icon-button" aria-label="紧凑文件夹" title="紧凑文件夹：合并连续单子目录" aria-pressed={compact} onClick={() => {
        const next = !compact; setCompact(next); setMenu(null)
      }}><Settings2 size={14} /></button>
      <button className="ws-icon-button" aria-label="刷新目录树" title="刷新目录树" onClick={() => setGeneration(current => current + 1)}><RefreshCw size={14} /></button>
      <button className="ws-icon-button" aria-label="折叠所有文件夹" title="折叠所有文件夹" onClick={() => { if (scope) writeViewState(scope, { expanded: [] }); setGeneration(current => current + 1) }}><ChevronsDownUp size={14} /></button>
      <button className="ws-icon-button" aria-label="收起资源管理器" title="收起资源管理器" onClick={onClose}><X size={14} /></button>
    </div></div>
    <div className="ws-tree-scroll">
      <ul role="tree" aria-label="工作区文件夹" key={`${scope}:${data?.revision ?? 0}:${generation}:${compact}`}>
        {roots.map(folder => <MemoTreeNode compact={compact} scope={scope} key={folder.path} entry={folder} root={folder} level={1} revision={data!.revision} selected={selected} copiedPath={copyNotice?.path ?? null} onCopy={copyPath} onSelect={selectPath} onOpenFile={onOpenFile} onNodeMenu={openNodeMenu} />)}
      </ul>
      {!data?.folders.length && <div className="ws-tree-empty"><p>{hasWorkspace ? '工作区还没有文件夹' : '打开文件夹或工作区'}</p>{!readOnlyShare && <button className="ws-outline-button" onClick={hasWorkspace ? onAddFolder : onConfigure}><FolderPlus size={14} />{hasWorkspace ? '添加文件夹' : '打开工作区'}</button>}</div>}
      {data?.warnings.map(warning => <p className="ws-tree-note is-error" role="status" key={warning}>{warning}</p>)}
      {operationError && <p className="ws-tree-note is-error" role="alert">{operationError}</p>}
    </div>
    <div className={`ws-tree-footer ${copyNotice?.error ? 'is-error' : ''}`} title={copyNotice?.message ?? selected ?? data?.workspaceFile ?? ''}><span role="status" aria-live="polite" aria-atomic="true">{copyNotice?.message}</span>{!copyNotice && (selected ?? data?.workspaceFile ?? '悬停文件或文件夹，按 C 复制绝对路径')}</div>
    </div>
    <div className="ws-search-view" hidden={view !== 'search'}><WorkspaceSearch key={data?.revision ?? 0} workspace={data} active={open && view === 'search'} onOpenFile={onOpenFile} onClose={onClose} /></div>
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
      {!readOnlyShare && menu.entry.kind === 'file' && <>
        <button role="menuitem" disabled={removing} title="将磁盘文件复制为同目录副本" onClick={() => void mutateFile('copy')}><Copy size={15} />复制文件</button>
        <button className="ws-context-remove" role="menuitem" disabled={removing} onClick={() => void mutateFile('delete')}><Trash2 size={15} />删除文件</button>
      </>}
      {!readOnlyShare && menu.level === 1 && <><button className="ws-context-remove" role="menuitem" disabled={removing} onClick={() => void removeRoot()}><FolderMinus size={15} />{removing ? '正在移除…' : '从工作区移除'}</button>
      <small>仅移除引用，不删除文件；保存后持久化。</small></>}
    </div>}
  </aside>
}
