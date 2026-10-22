import { readOnlyShare } from '../../lib/access'
import { pathWithin, readViewState, restoredTabs, viewPathKey, workspaceViewKey, writeViewState, type WorkspaceViewState } from './workspaceViewState'
import { useDialogs } from '../../components/form/BaseDialog'
import { lazy, Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { Check, FileText, LoaderCircle, RefreshCw, Server, X } from 'lucide-react'
import { readWorkspaceFile, writeWorkspaceFile, type WorkspaceFile, type WorkspaceResponse } from '../../lib/api'
import './file-editor.css'

const FileEditor = lazy(() => import('../../components/editor/FileEditor'))
export type FileOpenRequest = { path: string; pinned: boolean; sequence: number; line?: number; column?: number }
export type FileSaveStatus = { text: string; failed: boolean }
type FileTab = { key: string; path: string; file?: WorkspaceFile; draft: string; preview: boolean; loading: boolean; saving: boolean; error: string; savedAt?: number; deleting?: boolean; line: number; column: number; location?: { line: number; column: number; sequence: number } }
type Workbench = { tabs: FileTab[]; active: string | null }
const snapshotFor = (value: Workbench): WorkspaceViewState => ({
  tabs: value.tabs.map(tab => ({ path: tab.path, preview: tab.preview, line: tab.line, column: tab.column })),
  active: value.tabs.find(tab => tab.key === value.active)?.path ?? null,
})
const keyFor = viewPathKey
const textFor = (file: WorkspaceFile) => file.content.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n')
const dirty = (tab: FileTab) => !!tab.file && tab.draft !== textFor(tab.file)
const nameFor = (path: string) => path.split(/[\\/]/).at(-1) ?? path

export function FileWorkbench({ theme, workspace, openRequest, onOpenFile, onSaveStatusChange, onActiveFileChange, children }: { theme: 'dark' | 'light'; workspace: WorkspaceResponse | null; openRequest: FileOpenRequest | null; onOpenFile?: (path: string) => void; onSaveStatusChange?: (status: FileSaveStatus | null) => void; onActiveFileChange?: (path: string | null) => void; children: ReactNode }) {
  const dialogs = useDialogs()
  const confirmations = useRef(new Set<string>())
  const [state, setState] = useState<Workbench>({ tabs: [], active: null })
  const scope = workspaceViewKey(workspace)
  const [restoredScope, setRestoredScope] = useState<string | null>(null)
  const [editorActions, setEditorActions] = useState<HTMLDivElement | null>(null)
  const activeTab = state.tabs.find(tab => tab.key === state.active)
  const activePath = scope === restoredScope ? activeTab?.path ?? null : null
  useEffect(() => { onActiveFileChange?.(activePath) }, [activePath, onActiveFileChange])
  useEffect(() => () => onActiveFileChange?.(null), [onActiveFileChange])
  const saveText = !activeTab ? '' : activeTab.deleting ? '删除中…' : activeTab.loading ? '读取中…' : activeTab.saving ? '保存中…' : activeTab.error && dirty(activeTab) ? '保存失败' : activeTab.error && !activeTab.file ? '读取失败' : dirty(activeTab) ? '等待自动保存…' : activeTab.savedAt ? `保存于 ${new Date(activeTab.savedAt).toLocaleTimeString('zh-CN', { hour12: false, hour: '2-digit', minute: '2-digit', second: '2-digit' })}` : activeTab.file?.readonly ? '只读' : '自动保存'
  const saveFailed = !!activeTab?.error
  useEffect(() => { onSaveStatusChange?.(saveText ? { text: saveText, failed: saveFailed } : null) }, [saveText, saveFailed, onSaveStatusChange])
  useEffect(() => () => onSaveStatusChange?.(null), [onSaveStatusChange])
  const live = useRef(state)
  const currentWorkspace = useRef(workspace)
  currentWorkspace.current = workspace
  const pending = useRef(new Map<string, AbortController>())
  const mounted = useRef(true)
  const change = useCallback((update: (value: Workbench) => Workbench) => {
    if (!mounted.current) return
    live.current = update(live.current)
    setState(live.current)
  }, [])
  const updateTab = useCallback((key: string, update: (tab: FileTab) => FileTab) => {
    change(value => {
      let changed = false
      const tabs = value.tabs.map(tab => {
        const next = tab.key === key ? update(tab) : tab
        if (next !== tab) changed = true
        return next
      })
      return changed ? { ...value, tabs } : value
    })
  }, [change])
  const loadFile = useCallback(async (path: string, key: string) => {
    const workspace = currentWorkspace.current
    if (!workspace) return
    pending.current.get(key)?.abort()
    const controller = new AbortController()
    pending.current.set(key, controller)
    updateTab(key, tab => ({ ...tab, loading: true, error: '', savedAt: undefined }))
    try {
      const file = await readWorkspaceFile(path, workspace.revision, controller.signal)
      if (!controller.signal.aborted) updateTab(key, tab => ({ ...tab, path: file.path, file, draft: textFor(file), loading: false }))
    } catch (err) {
      if (!controller.signal.aborted) updateTab(key, tab => ({ ...tab, loading: false, error: err instanceof Error ? err.message : String(err) }))
    } finally { if (pending.current.get(key) === controller) pending.current.delete(key) }
  }, [updateTab])

  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false; pending.current.forEach(controller => controller.abort()); pending.current.clear() }
  }, [])
  useEffect(() => {
    if (scope === restoredScope) return
    if (restoredScope) writeViewState(restoredScope, snapshotFor(live.current))
    pending.current.forEach(controller => controller.abort()); pending.current.clear()
    const saved: WorkspaceViewState = scope ? readViewState(scope) : {}
    const tabs: FileTab[] = restoredTabs(saved).map(tab => ({ ...tab, key: keyFor(tab.path), draft: '', loading: false, saving: false, error: '', location: { line: tab.line, column: tab.column, sequence: 0 } }))
    const savedActive = typeof saved.active === 'string' ? keyFor(saved.active) : null
    const active = tabs.some(tab => tab.key === savedActive) ? savedActive : null
    change(() => ({ tabs, active }))
    setRestoredScope(scope)
  }, [scope, restoredScope, change])
  useEffect(() => {
    if (scope !== restoredScope || !scope) return
    const tab = live.current.tabs.find(tab => tab.key === live.current.active)
    if (tab && !tab.file && !tab.error && !pending.current.has(tab.key)) void loadFile(tab.path, tab.key)
  }, [scope, restoredScope, state.active, loadFile])
  const persistView = useCallback(() => {
    if (!scope || scope !== restoredScope || workspaceViewKey(currentWorkspace.current) !== scope) return
    writeViewState(scope, snapshotFor(live.current))
  }, [scope, restoredScope])
  useEffect(() => { const timer = setTimeout(persistView, 250); return () => clearTimeout(timer) }, [state, persistView])
  useEffect(() => {
    const hidden = () => { if (document.hidden) persistView() }
    window.addEventListener('pagehide', persistView); document.addEventListener('visibilitychange', hidden)
    return () => { window.removeEventListener('pagehide', persistView); document.removeEventListener('visibilitychange', hidden) }
  }, [persistView])

  useEffect(() => {
    if (!openRequest) return
    const key = keyFor(openRequest.path)
    const existing = live.current.tabs.find(tab => tab.key === key)
    if (existing) {
      change(value => ({ tabs: value.tabs.map(tab => tab.key === key ? { ...tab, preview: openRequest.pinned ? false : tab.preview, location: openRequest.line ? { line: openRequest.line, column: openRequest.column ?? 1, sequence: openRequest.sequence } : tab.location } : tab), active: key }))
      if (existing.loading && !pending.current.has(key)) void loadFile(existing.path, key)
      return
    }
    const preview = live.current.tabs.find(tab => tab.preview && !dirty(tab) && !tab.saving && !tab.deleting)
    if (!preview && live.current.tabs.length >= 12) {
      void dialogs.alert({ title: '打开标签已达上限', description: '最多同时打开 12 个文件，请先关闭部分标签。' })
      return
    }
    if (preview) { pending.current.get(preview.key)?.abort(); pending.current.delete(preview.key) }
    const tab: FileTab = { key, path: openRequest.path, draft: '', preview: !openRequest.pinned, loading: true, saving: false, error: '', savedAt: undefined, line: 1, column: 1, location: openRequest.line ? { line: openRequest.line, column: openRequest.column ?? 1, sequence: openRequest.sequence } : undefined }
    change(value => ({ active: key, tabs: preview ? value.tabs.map(current => current.key === preview.key ? tab : current) : [...value.tabs, tab] }))
    void loadFile(tab.path, key)
  }, [openRequest, change, loadFile, dialogs])

  const hasUnsaved = state.tabs.some(dirty)
  useEffect(() => {
    const beforeReplace = (event: Event) => {
      const paths = new Set((event as CustomEvent<string[]>).detail.map(keyFor))
      if (live.current.tabs.some(tab => paths.has(tab.key) && (dirty(tab) || tab.saving || tab.loading || tab.deleting))) event.preventDefault()
    }
    const replaced = (event: Event) => {
      const paths = new Set((event as CustomEvent<string[]>).detail.map(keyFor))
      for (const tab of live.current.tabs.filter(tab => paths.has(tab.key))) {
        if (dirty(tab) || tab.saving) updateTab(tab.key, current => ({ ...current, error: '磁盘文件已被外部操作修改，本地草稿已保留。请重新读取并合并后再保存。' }))
        else void loadFile(tab.path, tab.key)
      }
    }
    window.addEventListener('devflow-before-replace', beforeReplace)
    window.addEventListener('devflow-files-replaced', replaced)
    return () => { window.removeEventListener('devflow-before-replace', beforeReplace); window.removeEventListener('devflow-files-replaced', replaced) }
  }, [loadFile, updateTab])
  useEffect(() => {
    if (!hasUnsaved) return
    const beforeUnload = (event: BeforeUnloadEvent) => { event.preventDefault(); event.returnValue = '' }
    window.addEventListener('beforeunload', beforeUnload)
    return () => window.removeEventListener('beforeunload', beforeUnload)
  }, [hasUnsaved])

  useEffect(() => {
    if (!readOnlyShare || !scope || scope !== restoredScope || !state.active) return
    let disposed = false
    let controller: AbortController | null = null
    let timer: ReturnType<typeof setTimeout> | undefined
    let lastAttempt = 0
    let delay = 15000
    const key = state.active
    const schedule = () => { clearTimeout(timer); if (!disposed && !document.hidden) timer = setTimeout(() => void check(), delay) }
    async function check() {
      if (disposed || document.hidden || controller) return
      const tab = live.current.tabs.find(tab => tab.key === key)
      const workspace = currentWorkspace.current
      if (!tab || !workspace) return
      if (!tab.file || tab.loading || tab.saving || dirty(tab) || !navigator.onLine) { schedule(); return }
      lastAttempt = Date.now()
      const request = new AbortController()
      controller = request
      const timeout = setTimeout(() => request.abort(), 10000)
      try {
        const file = await readWorkspaceFile(tab.path, workspace.revision, request.signal)
        if (disposed || request.signal.aborted) return
        delay = 15000
        updateTab(key, current => {
          if (dirty(current) || current.saving || current.file !== tab.file) return current
          if (current.file && file.content === current.file.content && file.readonly === current.file.readonly && file.readonlyReason === current.file.readonlyReason && !current.error) return current
          return { ...current, file, draft: textFor(file), error: '' }
        })
      } catch (err) {
        if (!disposed && !document.hidden) {
          delay = Math.min(delay * 2, 60000)
          updateTab(key, current => ({ ...current, error: `自动同步暂不可用：${err instanceof Error ? err.message : String(err)}` }))
        }
      } finally {
        clearTimeout(timeout)
        if (controller === request) controller = null
        schedule()
      }
    }
    const wake = () => {
      clearTimeout(timer)
      if (document.hidden) { controller?.abort(); return }
      if (Date.now() - lastAttempt >= 5000) void check()
      else timer = setTimeout(() => void check(), 5000 - (Date.now() - lastAttempt))
    }
    schedule()
    document.addEventListener('visibilitychange', wake); window.addEventListener('focus', wake); window.addEventListener('online', wake)
    return () => { disposed = true; clearTimeout(timer); controller?.abort(); document.removeEventListener('visibilitychange', wake); window.removeEventListener('focus', wake); window.removeEventListener('online', wake) }
  }, [scope, restoredScope, state.active, workspace?.revision, updateTab])

  const saveFile = useCallback(async (key: string) => {
    const tab = live.current.tabs.find(tab => tab.key === key)
    const workspace = currentWorkspace.current
    if (!tab?.file || tab.file.readonly || tab.saving || tab.loading || tab.deleting || confirmations.current.has(key) || !dirty(tab) || !workspace) return
    const draft = tab.draft
    const savingScope = workspaceViewKey(workspace)
    updateTab(key, current => ({ ...current, saving: true, error: '', savedAt: undefined }))
    try {
      const file = await writeWorkspaceFile(tab.path, workspace.revision, tab.file.content, draft)
      if (workspaceViewKey(currentWorkspace.current) === savingScope) updateTab(key, current => ({ ...current, file, saving: false, savedAt: Date.now() }))
    } catch (err) { if (workspaceViewKey(currentWorkspace.current) === savingScope) updateTab(key, current => ({ ...current, saving: false, error: err instanceof Error ? err.message : String(err) })) }
  }, [updateTab])

  const autosaves = useRef(new Map<string, { draft: string; timer: ReturnType<typeof setTimeout> }>())
  useEffect(() => {
    const eligible = state.tabs.filter(tab => dirty(tab) && !tab.saving && !tab.loading && !tab.deleting && !tab.error && !tab.file?.readonly && !!workspace)
    for (const [key, pending] of autosaves.current) {
      if (!eligible.some(tab => tab.key === key && tab.draft === pending.draft)) { clearTimeout(pending.timer); autosaves.current.delete(key) }
    }
    for (const tab of eligible) {
      if (autosaves.current.has(tab.key)) continue
      autosaves.current.set(tab.key, { draft: tab.draft, timer: setTimeout(() => { autosaves.current.delete(tab.key); void saveFile(tab.key) }, 700) })
    }
  }, [state.tabs, workspace, saveFile])
  useEffect(() => () => { autosaves.current.forEach(pending => clearTimeout(pending.timer)); autosaves.current.clear() }, [])
  useEffect(() => {
    const beforeDelete = (event: Event) => {
      const path = (event as CustomEvent<string>).detail
      const tabs = live.current.tabs.filter(tab => pathWithin(tab.path, path))
      if (tabs.some(tab => tab.saving || tab.loading || tab.deleting)) { event.preventDefault(); return }
      for (const tab of tabs) updateTab(tab.key, current => ({ ...current, deleting: true }))
    }
    const finished = (event: Event) => {
      const { path, deleted } = (event as CustomEvent<{ path: string; deleted: boolean }>).detail
      const affected = live.current.tabs.filter(tab => pathWithin(tab.path, path))
      if (!deleted) { for (const tab of affected) updateTab(tab.key, current => ({ ...current, deleting: false })); return }
      for (const tab of affected) { pending.current.get(tab.key)?.abort(); pending.current.delete(tab.key) }
      change(value => {
        const index = value.tabs.findIndex(tab => tab.key === value.active)
        const tabs = value.tabs.filter(tab => !pathWithin(tab.path, path))
        return { tabs, active: tabs.some(tab => tab.key === value.active) ? value.active : tabs[Math.min(index, tabs.length - 1)]?.key ?? null }
      })
    }
    window.addEventListener('devflow-before-file-delete', beforeDelete)
    window.addEventListener('devflow-file-delete-finished', finished)
    return () => { window.removeEventListener('devflow-before-file-delete', beforeDelete); window.removeEventListener('devflow-file-delete-finished', finished) }
  }, [change, updateTab])
  async function confirmDiscard(key: string, reload: boolean) {
    const tab = live.current.tabs.find(current => current.key === key)
    if (!tab || tab.deleting || tab.saving || confirmations.current.has(key)) return false
    if (!dirty(tab)) return true
    confirmations.current.add(key)
    try {
      return await dialogs.confirm({
        title: reload ? '放弃修改并重新读取？' : '放弃未保存的修改？',
        description: `“${nameFor(tab.path)}”仍有未保存内容。${reload ? '重新读取将使用磁盘内容替换当前草稿。' : '关闭后将丢失当前草稿。'}`,
        confirmText: reload ? '放弃并重新读取' : '放弃并关闭', cancelText: '继续编辑', tone: 'danger',
      })
    } finally {
      confirmations.current.delete(key)
      // Resume an autosave timer that expired while the confirmation was open.
      updateTab(key, current => ({ ...current }))
    }
  }
  async function closeFile(tab: FileTab) {
    if (!await confirmDiscard(tab.key, false) || !mounted.current) return
    const current = live.current.tabs.find(item => item.key === tab.key)
    if (!current || current.saving || current.deleting) return
    pending.current.get(tab.key)?.abort(); pending.current.delete(tab.key)
    change(value => {
      const index = value.tabs.findIndex(current => current.key === tab.key)
      const tabs = value.tabs.filter(current => current.key !== tab.key)
      return { tabs, active: value.active === tab.key ? tabs[Math.min(index, tabs.length - 1)]?.key ?? null : value.active }
    })
  }
  async function reloadFile(tab: FileTab) {
    if (!await confirmDiscard(tab.key, true) || !mounted.current) return
    const current = live.current.tabs.find(item => item.key === tab.key)
    if (current && !current.saving && !current.deleting) void loadFile(current.path, current.key)
  }

  return <div className="file-workbench">
    <div className="file-tabbar">
    <div className="file-tabs" role="tablist" aria-label="服务与打开的文件" onKeyDown={event => {
      if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key) || (event.target as HTMLElement).getAttribute('role') !== 'tab') return
      event.preventDefault()
      const tabs = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('[role="tab"]'))
      const index = tabs.indexOf(event.target as HTMLButtonElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? tabs.length - 1 : (index + (event.key === 'ArrowLeft' ? -1 : 1) + tabs.length) % tabs.length
      tabs[next]?.click(); tabs[next]?.focus()
    }}>
      <button id="services-tab" role="tab" aria-selected={state.active === null} aria-controls="services-pane" tabIndex={state.active === null ? 0 : -1} className={`file-tab file-services-tab ${state.active === null ? 'is-active' : ''}`} onClick={() => change(value => ({ ...value, active: null }))}><Server size={14} />服务</button>
      {state.tabs.map(tab => <div key={tab.key} className={`file-tab-group ${state.active === tab.key ? 'is-active' : ''}`}>
        <button id={`file-tab-${encodeURIComponent(tab.key)}`} className={`file-tab ${tab.preview ? 'is-preview' : ''}`} role="tab" tabIndex={state.active === tab.key ? 0 : -1} aria-selected={state.active === tab.key} aria-controls={`file-pane-${encodeURIComponent(tab.key)}`} title={`${tab.path}${dirty(tab) ? '\n未保存' : ''}`} onClick={() => change(value => ({ ...value, active: tab.key }))} onDoubleClick={() => updateTab(tab.key, current => ({ ...current, preview: false }))}>
          {tab.loading || tab.saving ? <LoaderCircle size={14} className="spinning" /> : <FileText size={14} />}<span>{nameFor(tab.path)}</span>{dirty(tab) && <i className="file-dirty" aria-label="未保存" />}
        </button>
        <button className="file-tab-close" disabled={tab.saving || tab.deleting} aria-label={`关闭 ${nameFor(tab.path)}`} title="关闭文件" onClick={() => closeFile(tab)}><X size={13} /></button>
      </div>)}
    </div>
    {readOnlyShare && <span className="file-share-status">只读分享</span>}
    <div className="file-tab-actions" hidden={!activeTab}>
      <div ref={setEditorActions} className="file-editor-actions" />
      <button className="ws-icon-button" disabled={!activeTab || activeTab.loading || activeTab.saving || activeTab.deleting || !workspace} aria-label="重新读取文件" title="重新读取磁盘文件" onClick={() => { if (activeTab) void reloadFile(activeTab) }}><RefreshCw size={14} /></button>
    </div>
    </div>
    <div id="services-pane" role="tabpanel" aria-labelledby="services-tab" className="file-services-pane" hidden={state.active !== null}>{children}</div>
    {state.tabs.map(tab => <section key={tab.key} id={`file-pane-${encodeURIComponent(tab.key)}`} role="tabpanel" aria-labelledby={`file-tab-${encodeURIComponent(tab.key)}`} hidden={state.active !== tab.key} className="file-pane">
      {tab.error && <div className="file-notice is-error" role="alert">{tab.error}<button className="ws-outline-button" disabled={tab.loading || tab.saving || tab.deleting || !workspace} onClick={() => reloadFile(tab)}>重新读取</button></div>}
      {tab.file?.readonlyReason && !(readOnlyShare && tab.file.readonlyReason === '只读分享') && <p className="file-notice">{tab.file.readonlyReason}</p>}
      {tab.loading ? <div className="file-loading" role="status"><LoaderCircle size={18} className="spinning" />正在读取文件…</div> : tab.file ? <Suspense fallback={<div className="file-loading" role="status">正在加载编辑器…</div>}><FileEditor theme={theme} key={tab.path} viewStateKey={scope ? `${scope}:editor:${keyFor(tab.path)}` : undefined} active={state.active === tab.key} toolbarContainer={state.active === tab.key ? editorActions : null} onOpenFile={onOpenFile} path={tab.path} value={tab.draft} readonly={tab.file.readonly || !!tab.deleting} location={tab.location} onChange={draft => updateTab(tab.key, current => ({ ...current, draft, preview: false }))} onSave={() => void saveFile(tab.key)} onPosition={(line, column) => updateTab(tab.key, current => ({ ...current, line, column }))} /></Suspense> : <div className="file-loading">无法打开此文件，请查看上方提示。</div>}
      {tab.file && <footer className="file-status"><span role="status">{tab.saving ? '保存中…' : dirty(tab) ? '未保存' : tab.savedAt ? <><Check size={12} />已保存</> : tab.preview ? '预览 · 双击标签固定' : '已打开'}</span><span>行 {tab.line}，列 {tab.column}</span><span>UTF-8{tab.file.bom ? ' BOM' : ''} · {tab.file.lineEnding}</span></footer>}
    </section>)}
  </div>
}
