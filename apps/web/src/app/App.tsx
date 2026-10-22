import { readViewState, writeViewState } from '../features/workspace/workspaceViewState'
import { readOnlyShare } from '../lib/access'
import { useDialogs } from '../components/form/BaseDialog'
import { useCallback, useEffect, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import './theme-transition.css'
import { ChevronDown, CircleHelp, Clock3, FolderOpen, FolderTree, Moon, PanelBottom, Plus, Save, Search, Share2, Settings2, Sun, X } from 'lucide-react'
import { ResizableBottomPanel } from './ResizableBottomPanel'
import { executeWorkspace, getWorkspaceCatalog, type RecentWorkspace, executeNode, getNodeOptions, getNodeOutput, projectKey, metricsKey, executeJava, getJavaOptions, getJavaOutput, getRuntime, getWorkspace, type Project, type WorkspaceResponse } from '../lib/api'
import { javaStartOperation, readJavaLaunchConfig } from '../features/runtime/javaLaunchConfig'
import { NodeLaunchDialog } from '../features/runtime/NodeLaunchDialog'
import { readNodeLaunchConfig } from '../features/runtime/nodeLaunchConfig'
import { JavaLaunchDialog } from '../features/runtime/JavaLaunchDialog'
import { TerminalPanel } from '../features/terminal/TerminalPanel'
import { RuntimeConsole } from '../features/runtime/RuntimeConsole'
import { WorkspacePage } from '../features/workspace/WorkspacePage'
import { WorkspaceManager, type WorkspaceManagerMode } from '../features/workspace/WorkspaceManager'
import { FileWorkbench, type FileOpenRequest, type FileSaveStatus } from '../features/workspace/FileWorkbench'
import { ShareDialog } from '../features/sharing/ShareDialog'
import { BoardPage } from '../features/board/BoardPage'
import { Kanban, ShieldCheck } from 'lucide-react'
import { QaPage } from '../features/qa/QaPage'
import { WorkspaceTree } from '../features/workspace/WorkspaceTree'
import { DevFlowStatus } from '../features/runtime/DevFlowStatus'
import extremePlusLogo from '../assets/extreme-plus-logo.svg'
import extremePlusLogoDark from '../assets/extreme-plus-logo-dark.svg'

type Panel = { title: string, project?: string, description: string, runtimeProject?: Project }
export function App() {
  const dialogs = useDialogs()
  const [editorSaveStatus, setEditorSaveStatus] = useState<FileSaveStatus | null>(null)
  const [shareError, setShareError] = useState('')
  const [sharing, setSharing] = useState(false)
  const [data, setData] = useState<WorkspaceResponse | null>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  const [panel, setPanel] = useState<Panel | null>(null)
  const [terminalOpen, setTerminalOpen] = useState(false)
  const [launchProject, setLaunchProject] = useState<Project | null>(null)
  const pendingProjectActions = useRef(new Set<string>())
  const [managingWorkspace, setManagingWorkspace] = useState<WorkspaceManagerMode | null>(null)
  const [workspaceMenu, setWorkspaceMenu] = useState(false)
  const [recentWorkspaces, setRecentWorkspaces] = useState<RecentWorkspace[]>([])
  const [recentError, setRecentError] = useState('')
  const [openingRecent, setOpeningRecent] = useState(false)
  const [query, setQuery] = useState('')
  const [taskQuery, setTaskQuery] = useState('')
  const taskSearchRef = useRef<HTMLInputElement>(null)
  const [qaOpen, setQaOpen] = useState(() => window.location.hash === '#qa')
  const [boardOpen, setBoardOpen] = useState(() => window.location.hash === '#board')
  const [savedLayout] = useState(() => readViewState('devflow.workbench-layout.v1'))
  const [explorerOpen, setExplorerOpen] = useState(() => typeof savedLayout.sidebarOpen === 'boolean' ? savedLayout.sidebarOpen : window.matchMedia('(min-width: 720px)').matches)
  const [sidebarView, setSidebarView] = useState<'explorer' | 'search'>(() => savedLayout.sidebarView === 'search' ? savedLayout.sidebarView : 'explorer')
  useEffect(() => { writeViewState('devflow.workbench-layout.v1', { sidebarOpen: explorerOpen, sidebarView }) }, [explorerOpen, sidebarView])
  const [fileRequest, setFileRequest] = useState<FileOpenRequest | null>(null)
  const [activeFilePath, setActiveFilePath] = useState<string | null>(null)
  const openFile = useCallback((path: string, pinned = false, line?: number, column?: number) => {
    setBoardOpen(false)
    setQaOpen(false)
    setFileRequest(current => ({ path, pinned, line, column, sequence: (current?.sequence ?? 0) + 1 }))
    if (window.matchMedia('(max-width: 719px)').matches) setExplorerOpen(false)
  }, [])
  useEffect(() => {
    let timer: ReturnType<typeof setTimeout> | undefined
    const search = (event: KeyboardEvent) => {
      if (!(event.ctrlKey || event.metaKey) || event.altKey || !((event.key.toLowerCase() === 'p' && !event.shiftKey) || (event.key.toLowerCase() === 'f' && event.shiftKey))) return
      event.preventDefault()
      setQaOpen(false)
      setSidebarView('search')
      setBoardOpen(false)
      setExplorerOpen(true)
      clearTimeout(timer)
      timer = setTimeout(() => window.dispatchEvent(new CustomEvent('devflow-search')), 0)
    }
    window.addEventListener('keydown', search)
    return () => { window.removeEventListener('keydown', search); clearTimeout(timer) }
  }, [])
  useEffect(() => {
    if (readOnlyShare) return
    const toggleTerminal = (event: KeyboardEvent) => {
      if ((event.ctrlKey || event.metaKey) && !event.shiftKey && !event.altKey && event.code === 'Backquote') {
        event.preventDefault(); setTerminalOpen(value => !value)
      }
    }
    window.addEventListener('keydown', toggleTerminal)
    return () => window.removeEventListener('keydown', toggleTerminal)
  }, [])
  useEffect(() => {
    if (!readOnlyShare) return
    const expired = (event: Event) => { setShareError((event as CustomEvent<string>).detail); setData(null); setFileRequest(null); setPanel(null) }
    window.addEventListener('devflow-share-expired', expired)
    return () => window.removeEventListener('devflow-share-expired', expired)
  }, [])
  const workspaceActions = useRef<HTMLDivElement>(null)
  const workspaceTrigger = useRef<HTMLButtonElement>(null)
  const loadGeneration = useRef(0)
  const [theme, setTheme] = useState<'dark' | 'light'>(() => {
    try {
      const saved = localStorage.getItem('devflow.workspace.theme')
      if (saved === 'dark' || saved === 'light') return saved
    } catch { /* Storage may be unavailable in private browser contexts. */ }
    return window.matchMedia('(prefers-color-scheme: light)').matches ? 'light' : 'dark'
  })
  const themeTransitionPending = useRef(false)
  async function toggleTheme() {
    if (themeTransitionPending.current) return
    const next = theme === 'dark' ? 'light' : 'dark'
    const applyTheme = () => {
      setTheme(next)
      try { localStorage.setItem('devflow.workspace.theme', next) } catch { /* Keep the in-memory preference. */ }
    }
    const transitionDocument = document as Document & {
      startViewTransition?: (update: () => void) => { finished: Promise<void> }
    }
    if (!transitionDocument.startViewTransition || window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
      applyTheme()
      return
    }
    const root = document.documentElement
    themeTransitionPending.current = true
    root.style.setProperty('--theme-reveal-radius', `${Math.ceil(Math.hypot(window.innerWidth, window.innerHeight) / 2)}px`)
    root.classList.add('theme-transition')
    try {
      const transition = transitionDocument.startViewTransition(() => flushSync(applyTheme))
      await transition.finished
    } catch {
      // A skipped snapshot or unavailable animation must not prevent switching.
      applyTheme()
    } finally {
      root.classList.remove('theme-transition')
      root.style.removeProperty('--theme-reveal-radius')
      themeTransitionPending.current = false
    }
  }

  function applyWorkspace(value: WorkspaceResponse) {
    loadGeneration.current += 1
    setData(value); setError(''); setLoading(false); setQuery(''); setTaskQuery('')
  }
  async function load(silent = false, signal?: AbortSignal) {
    const generation = ++loadGeneration.current
    if (!silent) setLoading(true)
    try {
      const value = await getWorkspace(signal, !silent)
      if (generation === loadGeneration.current && !signal?.aborted) { setData(value); setError('') }
    } catch (err) { if (generation === loadGeneration.current && !signal?.aborted) setError(String(err)) }
    finally { if (generation === loadGeneration.current) setLoading(false) }
  }
  useEffect(() => {
    let disposed = false
    let timer: ReturnType<typeof setTimeout> | undefined
    let pending: AbortController | undefined
    async function poll() {
      if (disposed) return
      if (!document.hidden) {
        pending = new AbortController()
        const timeout = setTimeout(() => pending?.abort(), 10000)
        await load(true, pending.signal)
        clearTimeout(timeout)
      }
      if (!disposed) timer = setTimeout(() => void poll(), 3000)
    }
    void poll()
    return () => { disposed = true; clearTimeout(timer); pending?.abort() }
  }, [])
  useEffect(() => {
    function dismiss(event: KeyboardEvent) {
      if (event.defaultPrevented) return
      if (event.key === 'Escape') { setTerminalOpen(false); setPanel(null); setWorkspaceMenu(false) }
    }
    window.addEventListener('keydown', dismiss)
    return () => window.removeEventListener('keydown', dismiss)
  }, [])
  useEffect(() => {
    if (!workspaceMenu || readOnlyShare) return
    function dismiss(event: PointerEvent) {
      if (!workspaceActions.current?.contains(event.target as Node)) setWorkspaceMenu(false)
    }
    document.addEventListener('pointerdown', dismiss)
    return () => document.removeEventListener('pointerdown', dismiss)
  }, [workspaceMenu])

  useEffect(() => {
    if (!workspaceMenu || readOnlyShare) return
    const controller = new AbortController()
    setRecentError('')
    getWorkspaceCatalog(controller.signal).then(catalog => {
      if (!controller.signal.aborted) {
        setRecentWorkspaces(catalog.recentWorkspaces ?? [])
        if (!catalog.recentWorkspaces) setRecentError('当前后端尚不支持最近打开列表，请更新并重启 DevFlow。')
      }
    }).catch(err => { if (!controller.signal.aborted) setRecentError(err instanceof Error ? err.message : String(err)) })
    return () => controller.abort()
  }, [workspaceMenu])
  async function openRecent(path: string) {
    if (!data || openingRecent) return
    const discard = data.dirty && await dialogs.confirm({ title: '放弃工作区配置更改？', description: '当前工作区配置尚未保存。打开最近工作区将放弃这些更改。', confirmText: '放弃并打开', cancelText: '留在当前工作区', tone: 'danger' })
    if (data.dirty && !discard) return
    setOpeningRecent(true); setRecentError('')
    try {
      applyWorkspace(await executeWorkspace({ type: 'open_recent', path }, data.revision, discard))
      setWorkspaceMenu(false)
    } catch (err) { setRecentError(err instanceof Error ? err.message : String(err)) }
    finally { setOpeningRecent(false) }
  }

  const connected = !error && data !== null
  const workspaceName = `${data?.name ?? '未选择工作区'}${data?.dirty ? ' · 未保存' : ''}`
  const output: Panel = { title: '运行输出', description: '在服务卡片中选择项目，或启动项目查看实时输出。' }
  function explainWorkspace() {
    if (readOnlyShare) return
    setManagingWorkspace('open')
  }
  function manage(mode: WorkspaceManagerMode) { if (readOnlyShare) return; setWorkspaceMenu(false); setManagingWorkspace(mode) }
  const hasWorkspace = !!(data?.workspaceRoot || data?.workspaceFile)
  async function projectAction(project: Project, title: string) {
    if (readOnlyShare && title !== '运行输出') return
    if (title === '启动配置') { setLaunchProject(project); return }
    setTerminalOpen(false)
    setPanel({ title: title === 'JVM 诊断' ? 'JVM 诊断 / Arthas' : '运行输出', project: project.name, runtimeProject: project, description: '' })
    if (title !== '启动项目' && title !== '重启项目' && title !== '停止项目' && title !== 'Arthas 挂载') return
    if (pendingProjectActions.current.has(projectKey(project)) || !data) return
    pendingProjectActions.current.add(projectKey(project))
    try {
      if (project.kind === 'node') {
        if (title === '启动项目' || title === '重启项目') {
          const config = readNodeLaunchConfig(project)
          const options = await getNodeOptions(project, AbortSignal.timeout(10000))
          const script = project.nodeScript ?? (options.scripts.includes(config.script) ? config.script : options.scripts[0])
          if (!script) throw new Error('未发现可用启动脚本，请刷新工作区。')
          let pid: number | undefined
          if (title === '重启项目') {
            const snapshot = await getRuntime(AbortSignal.timeout(10000))
            const metrics = snapshot.projects.find(value => metricsKey(value) === projectKey(project))
            if (!['starting', 'running'].includes(metrics?.managedState ?? '')) {
              const pids = metrics?.nodePids ?? []
              if (pids.length !== 1) throw new Error(pids.length ? '检测到多个关联进程，请先停止外部服务，再启动。' : '未检测到该服务的运行进程。')
              pid = pids[0]
            }
          }
          await executeNode(project, data.revision, { type: title === '重启项目' ? 'restart' : 'start', pid, packageManager: config.manager === 'auto' ? undefined : config.manager, args: config.args.split(/\r?\n/).map(value => value.trim()).filter(Boolean) }, script)
        } else if (title === '停止项目') {
          const snapshot = await getRuntime(AbortSignal.timeout(10000))
          const metrics = snapshot.projects.find(value => metricsKey(value) === projectKey(project))
          const output = await getNodeOutput(project, 0, '', AbortSignal.timeout(10000))
          const pids = metrics?.nodePids ?? []
          const pid = output.pid && pids.includes(output.pid) ? output.pid : pids.length === 1 ? pids[0] : undefined
          if (metrics?.managedState && ['starting', 'running'].includes(metrics.managedState)) await executeNode(project, data.revision, { type: 'stop' })
          else if (pid) await executeNode(project, data.revision, { type: 'stop', pid })
          else throw new Error(pids.length ? '请在输出工具栏选择 Node.js 服务 PID，再执行停止。' : '未检测到该服务的运行进程。')
        }
        return
      }
      if (title === '启动项目' || title === '重启项目') {
        const config = readJavaLaunchConfig(project)
        const options = await getJavaOptions(project.path, AbortSignal.timeout(10000))
        if (!options.mainClasses.includes(config.main)) {
          if (options.mainClasses.length !== 1) throw new Error('请从更多操作中的“启动配置”选择启动主类后重试。')
          config.main = options.mainClasses[0]
        }
        let pid: number | undefined
        if (title === '重启项目') {
          const snapshot = await getRuntime(AbortSignal.timeout(10000))
          const metrics = snapshot.projects.find(value => metricsKey(value) === projectKey(project))
          if (!['starting', 'running'].includes(metrics?.managedState ?? '')) {
            const pids = metrics?.javaPids ?? []
            if (pids.length !== 1) throw new Error(pids.length ? '检测到多个关联进程，请先停止外部服务，再启动。' : '未检测到该服务的运行进程。')
            pid = pids[0]
          }
        }
        await executeJava(project.path, data.revision, { ...javaStartOperation(config), type: title === '重启项目' ? 'restart' : 'start', pid })
        return
      }
      const runtime = await getRuntime(AbortSignal.timeout(10000))
      const metrics = runtime.projects.find(value => value.projectPath === project.path)
      const javaPids = metrics?.javaPids ?? metrics?.pids ?? []
      const output = title === 'Arthas 挂载' ? await getJavaOutput(project.path, 0, '', AbortSignal.timeout(10000)) : null
      const targetPid = output?.pid && javaPids.includes(output.pid) ? output.pid : javaPids.length === 1 ? javaPids[0] : undefined
      if (title === '停止项目' && metrics?.managedState && ['starting', 'running'].includes(metrics.managedState)) {
        await executeJava(project.path, data.revision, { type: 'stop' })
      } else if (targetPid) {
        await executeJava(project.path, data.revision, title === '停止项目' ? { type: 'stop', pid: targetPid } : { type: 'attach', pid: targetPid })
      } else {
        setPanel(current => current?.runtimeProject && projectKey(current.runtimeProject) === projectKey(project) ? { ...current, description: javaPids.length ? '请在输出工具栏选择应用 PID，再执行操作。' : '未检测到运行进程，请先启动 Java 服务。' } : current)
      }
    } catch (err) { setPanel(current => current?.runtimeProject && projectKey(current.runtimeProject) === projectKey(project) ? { ...current, description: err instanceof Error ? err.message : String(err) } : current) }
    finally { pendingProjectActions.current.delete(projectKey(project)) }
  }

  return <div className="shell workspace-shell" data-theme={theme}>
    <header className="topbar">
      <a className="brand" href="#projects" aria-label="DevFlow 工作区"><img className="brand-logo" src={theme === 'dark' ? extremePlusLogoDark : extremePlusLogo} alt="" width={32} height={24} /><strong>DevFlow</strong></a>
      <span className="header-divider" />
      <div className="workspace-actions" ref={workspaceActions} onBlur={event => { if (!event.currentTarget.contains(event.relatedTarget)) setWorkspaceMenu(false) }}>
        <button ref={workspaceTrigger} className="workspace-switch" aria-expanded={workspaceMenu} aria-controls="workspace-actions" onClick={() => { if (!readOnlyShare) setWorkspaceMenu(current => !current) }} title={data?.workspaceFile ?? data?.workspaceRoot ?? '工作区操作'}><FolderOpen size={14} /><span>{workspaceName}</span>{readOnlyShare ? <small>只读</small> : <ChevronDown size={12} />}</button>
        {!readOnlyShare && workspaceMenu && <div id="workspace-actions" className="ws-workspace-menu" role="group" aria-label="工作区操作" onKeyDown={event => { if (event.key === 'Escape') { setWorkspaceMenu(false); workspaceTrigger.current?.focus(); event.stopPropagation() } }}>
          <span className="ws-menu-label">工作区</span>
          <button autoFocus onClick={() => manage('create')}><Plus size={15} /><span>新建工作区<small>从文件夹建立或创建空工作区</small></span></button>
          <button onClick={() => manage('open')}><FolderOpen size={15} /><span>打开工作区…<small>浏览本机文件夹与工作区文件</small></span></button>
          <div className="ws-menu-separator" />
          <span className="ws-menu-label">最近打开</span>
          {recentWorkspaces.slice(0, 5).map(entry => <button key={entry.path} disabled={openingRecent || !data || !entry.available} title={entry.available ? entry.path : `位置不存在或已改变：${entry.path}`} onClick={() => void openRecent(entry.path)}><Clock3 size={15} /><span>{entry.name}<small>{entry.available ? entry.path : `不可用 · ${entry.path}`}</small></span></button>)}
          {!recentWorkspaces.length && <span className="ws-recent-menu-empty">打开工作区后自动记住，重启后仍保留</span>}
          <button onClick={() => manage('open')}><Clock3 size={15} /><span>查看全部最近打开…</span></button>
          {recentError && <p className="ws-recent-menu-error" role="alert">{recentError}</p>}
          <div className="ws-menu-separator" />
          <button disabled={!hasWorkspace} onClick={() => { setWorkspaceMenu(false); setSharing(true) }}><Share2 size={15} /><span>只读分享…<small>邀请局域网成员查看当前工作区</small></span></button>
          <button disabled={!hasWorkspace} onClick={() => manage('save')}><Save size={15} /><span>保存 / 另存工作区…</span></button>
          <button disabled={!hasWorkspace} onClick={() => manage('close')}><X size={15} /><span>关闭当前工作区</span></button>
        </div>}
      </div>
      <label className="ws-top-search"><Search size={14} /><input ref={taskSearchRef} type="search" aria-label={boardOpen && !readOnlyShare ? '搜索任务' : '搜索服务'} placeholder={boardOpen && !readOnlyShare ? '搜索任务…' : '搜索服务…'} value={boardOpen && !readOnlyShare ? taskQuery : query} onChange={event => boardOpen && !readOnlyShare ? setTaskQuery(event.target.value) : setQuery(event.target.value)} />{(boardOpen && !readOnlyShare ? taskQuery : query) && <button className="ws-icon-button" aria-label={boardOpen && !readOnlyShare ? '清除任务搜索' : '清除服务搜索'} onClick={() => boardOpen && !readOnlyShare ? setTaskQuery('') : setQuery('')}><X size={13} /></button>}</label>
      <div className="ws-top-status">
        {editorSaveStatus && <span className={`ws-editor-save-status ${editorSaveStatus.failed ? 'is-error' : ''}`} role="status" title={editorSaveStatus.text}>{editorSaveStatus.text}</span>}
        <DevFlowStatus />
      </div>
      <button className="ws-icon-button" title={`切换到${theme === 'dark' ? '浅色' : '深色'}主题`} aria-label={`切换到${theme === 'dark' ? '浅色' : '深色'}主题`} onClick={toggleTheme}>{theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}</button>
    </header>
    {shareError && <p className="file-notice is-error" role="alert">{shareError}</p>}
    <div className="workbench">
      <nav className="activity-bar" aria-label="功能入口">
        <div className="activity-primary">
          {!readOnlyShare && <button className={`activity-button ${boardOpen ? 'active' : ''}`} aria-label="看板" title="看板" aria-pressed={boardOpen} onClick={() => { setQaOpen(false); setBoardOpen(true); setTerminalOpen(false); setPanel(null) }}><Kanban size={20} strokeWidth={1.65} /></button>}
          {!readOnlyShare && <button className={`activity-button ${qaOpen ? 'active' : ''}`} aria-label="质量保证" title="质量保证 · API 场景测试" aria-pressed={qaOpen} onClick={() => { setQaOpen(true); setBoardOpen(false); setTerminalOpen(false); setPanel(null) }}><ShieldCheck size={20} strokeWidth={1.65} /></button>}
          <button className={`activity-button ${!qaOpen && !boardOpen && explorerOpen && sidebarView === 'explorer' ? 'active' : ''}`} aria-label="资源管理器" title="资源管理器" aria-pressed={!qaOpen && !boardOpen && explorerOpen && sidebarView === 'explorer'} aria-controls="workspace-sidebar" onClick={() => { setQaOpen(false); setBoardOpen(false); setExplorerOpen(boardOpen || sidebarView !== 'explorer' || !explorerOpen); setSidebarView('explorer') }}><FolderTree size={20} strokeWidth={1.65} /></button>
          <button className={`activity-button ${!qaOpen && !boardOpen && explorerOpen && sidebarView === 'search' ? 'active' : ''}`} aria-label="搜索文件" title="搜索 · Ctrl+Shift+F" aria-pressed={!qaOpen && !boardOpen && explorerOpen && sidebarView === 'search'} aria-controls="workspace-sidebar" onClick={() => { setQaOpen(false); setBoardOpen(false); setExplorerOpen(boardOpen || sidebarView !== 'search' || !explorerOpen); setSidebarView('search') }}><Search size={20} strokeWidth={1.65} /></button>
        </div>
        <div className="activity-secondary"><button className="activity-button" aria-label="使用说明" title="使用说明" onClick={() => setPanel({ title: '使用说明', description: '点击服务卡片查看运行输出，点击箭头展开目录和启动入口。点击启动直接使用已保存或默认配置，更多操作中的“启动配置”可保存 JVM 参数、程序参数和 Spring Profile；支持停止项目进程树及向运行中的 Java 应用附加 Arthas。底部面板可拖拽调整高度。Node.js 服务支持 npm/pnpm/yarn 启动、停止和运行输出。' })}><CircleHelp size={19} /></button>{!readOnlyShare && <button className="activity-button" aria-label="设置" title="设置" onClick={explainWorkspace}><Settings2 size={19} /></button>}</div>
      </nav>
      <WorkspaceTree activeFilePath={activeFilePath} key={shareError ? 'expired' : 'workspace'} hidden={boardOpen || qaOpen} open={explorerOpen && !boardOpen && !qaOpen} view={sidebarView} onOpenFile={openFile} data={data} onChange={applyWorkspace} onRefresh={() => void load(true)} onConfigure={explainWorkspace} onAddFolder={() => manage('add_folder')} onSave={() => manage('save')} onClose={() => setExplorerOpen(false)} onToggle={() => setExplorerOpen(value => !value)} />
      <div className="editor-area">
        {qaOpen && !readOnlyShare && <QaPage />}
        {boardOpen && !readOnlyShare && <BoardPage workspaceRevision={data?.revision ?? 0} key={data?.workspaceFile ?? data?.workspaceRoot ?? 'demo'} query={taskQuery} onQueryChange={setTaskQuery} onSearchFocus={() => taskSearchRef.current?.focus()} workspaceName={data?.name ?? 'DevFlow'} workspaceKey={data?.workspaceFile ?? data?.workspaceRoot ?? 'demo'} />}
        <div className="board-workbench-host" hidden={(boardOpen || qaOpen) && !readOnlyShare}>
        <FileWorkbench theme={theme} onActiveFileChange={setActiveFilePath} key={shareError ? 'expired' : 'workspace'} workspace={data} openRequest={fileRequest} onSaveStatusChange={setEditorSaveStatus} onOpenFile={path => openFile(path, true)}>
          <main id="projects" className="main-content">
            <WorkspacePage key={data?.folders.map(folder => folder.path).join('|') ?? 'empty'} query={query} onQueryChange={setQuery} data={data} loading={loading} monitoringEnabled={connected} outputProjectKey={panel?.runtimeProject ? projectKey(panel.runtimeProject) : undefined} onRefresh={() => void load()} onConfigure={() => manage(hasWorkspace ? 'add_folder' : 'open')} onAction={projectAction} />
          </main>
        </FileWorkbench>
        </div>
        {!readOnlyShare && <TerminalPanel workspace={data} visible={terminalOpen} theme={theme} onClose={() => setTerminalOpen(false)} />}
        {!terminalOpen && panel && (panel.runtimeProject && data ? <RuntimeConsole key={projectKey(panel.runtimeProject)} project={panel.runtimeProject} revision={data.revision} title={panel.title} description={panel.description} onClose={() => setPanel(null)} /> : <ResizableBottomPanel title={panel.title} project={panel.project} description={panel.description} onClose={() => setPanel(null)} />)}
      </div>
    </div>
    <footer className="statusbar"><span className={`connection ${connected ? 'online' : ''}`}><i />{loading ? '正在连接' : connected ? readOnlyShare ? '分享已连接 · 只读' : '本地已连接' : '未连接'}</span><button className={panel && !terminalOpen ? 'selected' : ''} aria-expanded={panel !== null && !terminalOpen} onClick={() => { setTerminalOpen(false); setPanel(panel && !terminalOpen ? null : output) }}><PanelBottom size={13} />运行输出</button>{!readOnlyShare && <button className={terminalOpen ? 'selected' : ''} aria-expanded={terminalOpen} title="终端 · Ctrl+`" onClick={() => setTerminalOpen(value => !value)}><PanelBottom size={13} />终端</button>}{error ? <div className="service-error" title={`连接失败：${error}`}><span role="status" aria-label={`连接失败：${error}`}>本地服务未连接</span><button disabled={loading} aria-label="重试连接本地服务" onClick={() => void load()}>{loading ? '连接中…' : '重试'}</button></div> : <span className="status-version">DevFlow · 0.1.0</span>}</footer>
    {managingWorkspace && <WorkspaceManager key={managingWorkspace} mode={managingWorkspace} data={data} onChange={applyWorkspace} onRefresh={() => void load()} onClose={() => { setManagingWorkspace(null); workspaceTrigger.current?.focus() }} />}
    {launchProject && data && (launchProject.kind === 'node' ? <NodeLaunchDialog key={projectKey(launchProject)} project={launchProject} onClose={() => setLaunchProject(null)} /> : <JavaLaunchDialog key={projectKey(launchProject)} project={launchProject} onClose={() => setLaunchProject(null)} />)}
    {sharing && <ShareDialog workspace={data} onClose={() => setSharing(false)} />}
  </div>
}
