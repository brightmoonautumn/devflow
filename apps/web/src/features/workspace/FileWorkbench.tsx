import { lazy, Suspense, useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { Check, FileText, LoaderCircle, RefreshCw, Save, Server, X } from 'lucide-react'
import { readWorkspaceFile, writeWorkspaceFile, type WorkspaceFile, type WorkspaceResponse } from '../../lib/api'
import './file-editor.css'

const SourceEditor = lazy(() => import('../../components/editor/SourceEditor'))
export type FileOpenRequest = { path: string; pinned: boolean; sequence: number; line?: number; column?: number }
type FileTab = { key: string; path: string; file?: WorkspaceFile; draft: string; preview: boolean; loading: boolean; saving: boolean; error: string; saved: boolean; line: number; column: number; location?: { line: number; column: number; sequence: number } }
type Workbench = { tabs: FileTab[]; active: string | null }
const keyFor = (path: string) => /^[a-z]:[\\/]|^\\\\/i.test(path) ? path.replace(/\\/g, '/').toLowerCase() : path
const textFor = (file: WorkspaceFile) => file.content.replace(/^\uFEFF/, '').replace(/\r\n?/g, '\n')
const dirty = (tab: FileTab) => !!tab.file && tab.draft !== textFor(tab.file)
const nameFor = (path: string) => path.split(/[\\/]/).at(-1) ?? path

export function FileWorkbench({ workspace, openRequest, children }: { workspace: WorkspaceResponse | null; openRequest: FileOpenRequest | null; children: ReactNode }) {
  const [state, setState] = useState<Workbench>({ tabs: [], active: null })
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
    change(value => ({ ...value, tabs: value.tabs.map(tab => tab.key === key ? update(tab) : tab) }))
  }, [change])

  const loadFile = useCallback(async (path: string, key: string) => {
    const workspace = currentWorkspace.current
    if (!workspace) return
    pending.current.get(key)?.abort()
    const controller = new AbortController()
    pending.current.set(key, controller)
    updateTab(key, tab => ({ ...tab, loading: true, error: '', saved: false }))
    try {
      const file = await readWorkspaceFile(path, workspace.revision, controller.signal)
      if (!controller.signal.aborted) updateTab(key, tab => ({ ...tab, path: file.path, file, draft: textFor(file), loading: false, line: 1, column: 1 }))
    } catch (err) {
      if (!controller.signal.aborted) updateTab(key, tab => ({ ...tab, loading: false, error: err instanceof Error ? err.message : String(err) }))
    } finally { if (pending.current.get(key) === controller) pending.current.delete(key) }
  }, [updateTab])

  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false; pending.current.forEach(controller => controller.abort()); pending.current.clear() }
  }, [])
  useEffect(() => {
    if (!openRequest) return
    const key = keyFor(openRequest.path)
    const existing = live.current.tabs.find(tab => tab.key === key)
    if (existing) {
      change(value => ({ tabs: value.tabs.map(tab => tab.key === key ? { ...tab, preview: openRequest.pinned ? false : tab.preview, location: openRequest.line ? { line: openRequest.line, column: openRequest.column ?? 1, sequence: openRequest.sequence } : tab.location } : tab), active: key }))
      if (existing.loading && !pending.current.has(key)) void loadFile(existing.path, key)
      return
    }
    const preview = live.current.tabs.find(tab => tab.preview && !dirty(tab) && !tab.saving)
    if (!preview && live.current.tabs.length >= 12) {
      window.alert('最多同时打开 12 个文件，请先关闭部分标签。')
      return
    }
    if (preview) { pending.current.get(preview.key)?.abort(); pending.current.delete(preview.key) }
    const tab: FileTab = { key, path: openRequest.path, draft: '', preview: !openRequest.pinned, loading: true, saving: false, error: '', saved: false, line: 1, column: 1, location: openRequest.line ? { line: openRequest.line, column: openRequest.column ?? 1, sequence: openRequest.sequence } : undefined }
    change(value => ({ active: key, tabs: preview ? value.tabs.map(current => current.key === preview.key ? tab : current) : [...value.tabs, tab] }))
    void loadFile(tab.path, key)
  }, [openRequest, change, loadFile])

  const hasUnsaved = state.tabs.some(dirty)
  useEffect(() => {
    const beforeReplace = (event: Event) => {
      const paths = new Set((event as CustomEvent<string[]>).detail.map(keyFor))
      if (live.current.tabs.some(tab => paths.has(tab.key) && (dirty(tab) || tab.saving || tab.loading))) event.preventDefault()
    }
    const replaced = (event: Event) => {
      const paths = new Set((event as CustomEvent<string[]>).detail.map(keyFor))
      for (const tab of live.current.tabs.filter(tab => paths.has(tab.key))) {
        if (dirty(tab) || tab.saving) updateTab(tab.key, current => ({ ...current, error: '磁盘文件已被搜索替换，本地草稿已保留。请重新读取并合并后再保存。' }))
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

  async function saveFile(key: string) {
    const tab = live.current.tabs.find(tab => tab.key === key)
    const workspace = currentWorkspace.current
    if (!tab?.file || tab.file.readonly || tab.saving || tab.loading || !dirty(tab) || !workspace) return
    const draft = tab.draft
    updateTab(key, current => ({ ...current, saving: true, error: '', saved: false }))
    try {
      const file = await writeWorkspaceFile(tab.path, workspace.revision, tab.file.content, draft)
      updateTab(key, current => ({ ...current, file, saving: false, saved: current.draft === draft }))
    } catch (err) { updateTab(key, current => ({ ...current, saving: false, error: err instanceof Error ? err.message : String(err) })) }
  }
  function closeFile(tab: FileTab) {
    if (tab.saving || (dirty(tab) && !window.confirm(`“${nameFor(tab.path)}”有未保存修改，确定放弃并关闭？`))) return
    pending.current.get(tab.key)?.abort(); pending.current.delete(tab.key)
    change(value => {
      const index = value.tabs.findIndex(current => current.key === tab.key)
      const tabs = value.tabs.filter(current => current.key !== tab.key)
      return { tabs, active: value.active === tab.key ? tabs[Math.min(index, tabs.length - 1)]?.key ?? null : value.active }
    })
  }
  function reloadFile(tab: FileTab) {
    if (tab.saving || (dirty(tab) && !window.confirm(`重新读取“${nameFor(tab.path)}”会放弃本地未保存修改，确定继续？`))) return
    void loadFile(tab.path, tab.key)
  }

  return <div className="file-workbench">
    <div className="file-tabbar" role="tablist" aria-label="服务与打开的文件" onKeyDown={event => {
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
        <button className="file-tab-close" disabled={tab.saving} aria-label={`关闭 ${nameFor(tab.path)}`} title="关闭文件" onClick={() => closeFile(tab)}><X size={13} /></button>
      </div>)}
    </div>
    <div id="services-pane" role="tabpanel" aria-labelledby="services-tab" className="file-services-pane" hidden={state.active !== null}>{children}</div>
    {state.tabs.map(tab => <section key={tab.key} id={`file-pane-${encodeURIComponent(tab.key)}`} role="tabpanel" aria-labelledby={`file-tab-${encodeURIComponent(tab.key)}`} hidden={state.active !== tab.key} className="file-pane">
      <header className="file-toolbar"><span className="file-path" title={tab.path}>{tab.path}</span><button className="ws-icon-button" disabled={tab.loading || tab.saving || !workspace} aria-label="重新读取文件" title="重新读取磁盘文件" onClick={() => reloadFile(tab)}><RefreshCw size={14} /></button><button className="ws-outline-button" disabled={!dirty(tab) || tab.saving || tab.loading || tab.file?.readonly || !workspace} title="保存 · Ctrl+S" onClick={() => void saveFile(tab.key)}>{tab.saving ? <LoaderCircle size={14} className="spinning" /> : <Save size={14} />}保存</button></header>
      {tab.error && <div className="file-notice is-error" role="alert">{tab.error}<button className="ws-outline-button" disabled={tab.loading || tab.saving || !workspace} onClick={() => reloadFile(tab)}>重新读取</button></div>}
      {tab.file?.readonlyReason && <p className="file-notice">{tab.file.readonlyReason}</p>}
      {tab.loading ? <div className="file-loading" role="status"><LoaderCircle size={18} className="spinning" />正在读取文件…</div> : tab.file ? <Suspense fallback={<div className="file-loading" role="status">正在加载编辑器…</div>}><SourceEditor key={tab.path} path={tab.path} value={tab.draft} readonly={tab.file.readonly} location={tab.location} onChange={draft => updateTab(tab.key, current => ({ ...current, draft, preview: false, saved: false }))} onSave={() => void saveFile(tab.key)} onPosition={(line, column) => updateTab(tab.key, current => ({ ...current, line, column }))} /></Suspense> : <div className="file-loading">无法打开此文件，请查看上方提示。</div>}
      {tab.file && <footer className="file-status"><span role="status">{tab.saving ? '保存中…' : dirty(tab) ? '未保存' : tab.saved ? <><Check size={12} />已保存</> : tab.preview ? '预览 · 双击标签固定' : '已打开'}</span><span>行 {tab.line}，列 {tab.column}</span><span>UTF-8{tab.file.bom ? ' BOM' : ''} · {tab.file.lineEnding}</span></footer>}
    </section>)}
  </div>
}
