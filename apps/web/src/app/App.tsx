import { useEffect, useRef, useState } from 'react'
import { flushSync } from 'react-dom'
import './theme-transition.css'
import { ChevronDown, CircleHelp, FolderOpen, FolderTree, Moon, PanelBottom, Plus, Save, Search, Settings2, Sun, X } from 'lucide-react'
import { ResizableBottomPanel } from './ResizableBottomPanel'
import { executeNode, getNodeOptions, getNodeOutput, projectKey, metricsKey, executeJava, getJavaOptions, getJavaOutput, getRuntime, getWorkspace, type Project, type WorkspaceResponse } from '../lib/api'
import { javaStartOperation, readJavaLaunchConfig } from '../features/runtime/javaLaunchConfig'
import { NodeLaunchDialog } from '../features/runtime/NodeLaunchDialog'
import { readNodeLaunchConfig } from '../features/runtime/nodeLaunchConfig'
import { JavaLaunchDialog } from '../features/runtime/JavaLaunchDialog'
import { RuntimeConsole } from '../features/runtime/RuntimeConsole'
import { WorkspacePage } from '../features/workspace/WorkspacePage'
import { WorkspaceManager, type WorkspaceManagerMode } from '../features/workspace/WorkspaceManager'
import { WorkspaceTree } from '../features/workspace/WorkspaceTree'
import { DevFlowStatus } from '../features/runtime/DevFlowStatus'
import extremePlusLogo from '../assets/extreme-plus-logo.svg'
import extremePlusLogoDark from '../assets/extreme-plus-logo-dark.svg'

type Panel = { title: string, project?: string, description: string, runtimeProject?: Project }
export function App() {
  const [data, setData] = useState<WorkspaceResponse | null>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  const [panel, setPanel] = useState<Panel | null>(null)
  const [launchProject, setLaunchProject] = useState<Project | null>(null)
  const pendingProjectActions = useRef(new Set<string>())
  const [managingWorkspace, setManagingWorkspace] = useState<WorkspaceManagerMode | null>(null)
  const [workspaceMenu, setWorkspaceMenu] = useState(false)
  const [query, setQuery] = useState('')
  const workspaceActions = useRef<HTMLDivElement>(null)
  const workspaceTrigger = useRef<HTMLButtonElement>(null)
  const [explorerOpen, setExplorerOpen] = useState(() => window.matchMedia('(min-width: 720px)').matches)
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
    setData(value); setError(''); setLoading(false); setQuery('')
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
      if (event.key === 'Escape') { setPanel(null); setWorkspaceMenu(false) }
    }
    window.addEventListener('keydown', dismiss)
    return () => window.removeEventListener('keydown', dismiss)
  }, [])
  useEffect(() => {
    if (!workspaceMenu) return
    function dismiss(event: PointerEvent) {
      if (!workspaceActions.current?.contains(event.target as Node)) setWorkspaceMenu(false)
    }
    document.addEventListener('pointerdown', dismiss)
    return () => document.removeEventListener('pointerdown', dismiss)
  }, [workspaceMenu])

  const connected = !error && data !== null
  const workspaceName = `${data?.name ?? '未选择工作区'}${data?.dirty ? ' · 未保存' : ''}`
  const output: Panel = { title: '运行输出', description: '在服务卡片中选择项目，或启动项目查看实时输出。' }
  function explainWorkspace() {
    setManagingWorkspace('open')
  }
  function manage(mode: WorkspaceManagerMode) { setWorkspaceMenu(false); setManagingWorkspace(mode) }
  const hasWorkspace = !!(data?.workspaceRoot || data?.workspaceFile)
  async function projectAction(project: Project, title: string) {
    if (title === '启动配置') { setLaunchProject(project); return }
    setPanel({ title: title === 'JVM 诊断' ? 'JVM 诊断 / Arthas' : '运行输出', project: project.name, runtimeProject: project, description: '' })
    if (title !== '启动项目' && title !== '停止项目' && title !== 'Arthas 挂载') return
    if (pendingProjectActions.current.has(projectKey(project)) || !data) return
    pendingProjectActions.current.add(projectKey(project))
    try {
      if (project.kind === 'node') {
        if (title === '启动项目') {
          const config = readNodeLaunchConfig(project)
          const options = await getNodeOptions(project, AbortSignal.timeout(10000))
          const script = project.nodeScript ?? (options.scripts.includes(config.script) ? config.script : options.scripts[0])
          if (!script) throw new Error('未发现可用启动脚本，请刷新工作区。')
          await executeNode(project, data.revision, { type: 'start', packageManager: config.manager === 'auto' ? undefined : config.manager, args: config.args.split(/\r?\n/).map(value => value.trim()).filter(Boolean) }, script)
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
      if (title === '启动项目') {
        const config = readJavaLaunchConfig(project)
        const options = await getJavaOptions(project.path, AbortSignal.timeout(10000))
        if (!options.mainClasses.includes(config.main)) {
          if (options.mainClasses.length !== 1) throw new Error('请从更多操作中的“启动配置”选择启动主类后重试。')
          config.main = options.mainClasses[0]
        }
        await executeJava(project.path, data.revision, javaStartOperation(config))
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
        <button ref={workspaceTrigger} className="workspace-switch" aria-expanded={workspaceMenu} aria-controls="workspace-actions" onClick={() => setWorkspaceMenu(current => !current)} title={data?.workspaceFile ?? data?.workspaceRoot ?? '工作区操作'}><FolderOpen size={14} /><span>{workspaceName}</span><ChevronDown size={12} /></button>
        {workspaceMenu && <div id="workspace-actions" className="ws-workspace-menu" role="group" aria-label="工作区操作" onKeyDown={event => { if (event.key === 'Escape') { setWorkspaceMenu(false); workspaceTrigger.current?.focus(); event.stopPropagation() } }}>
          <span className="ws-menu-label">工作区</span>
          <button autoFocus onClick={() => manage('create')}><Plus size={15} /><span>新建工作区<small>从文件夹建立或创建空工作区</small></span></button>
          <button onClick={() => manage('open')}><FolderOpen size={15} /><span>打开工作区…<small>浏览本机文件夹与工作区文件</small></span></button>
          <div className="ws-menu-separator" />
          <button disabled={!hasWorkspace} onClick={() => manage('save')}><Save size={15} /><span>保存 / 另存工作区…</span></button>
          <button disabled={!hasWorkspace} onClick={() => manage('close')}><X size={15} /><span>关闭当前工作区</span></button>
        </div>}
      </div>
      <label className="ws-top-search"><Search size={14} /><input type="search" aria-label="搜索服务" placeholder="搜索服务…" value={query} onChange={event => setQuery(event.target.value)} />{query && <button className="ws-icon-button" aria-label="清除服务搜索" onClick={() => setQuery('')}><X size={13} /></button>}</label>
      <DevFlowStatus />
      <button className="ws-icon-button" title={`切换到${theme === 'dark' ? '浅色' : '深色'}主题`} aria-label={`切换到${theme === 'dark' ? '浅色' : '深色'}主题`} onClick={toggleTheme}>{theme === 'dark' ? <Sun size={16} /> : <Moon size={16} />}</button>
    </header>
    <div className="workbench">
      <nav className="activity-bar" aria-label="功能入口">
        <div className="activity-primary"><button className={`activity-button ${explorerOpen ? 'active' : ''}`} aria-label="资源管理器" title="资源管理器" aria-expanded={explorerOpen} aria-controls="workspace-explorer" onClick={() => setExplorerOpen(current => !current)}><FolderTree size={20} strokeWidth={1.65} /></button></div>
        <div className="activity-secondary"><button className="activity-button" aria-label="使用说明" title="使用说明" onClick={() => setPanel({ title: '使用说明', description: '点击服务卡片查看运行输出，点击箭头展开目录和启动入口。点击启动直接使用已保存或默认配置，更多操作中的“启动配置”可保存 JVM 参数、程序参数和 Spring Profile；支持停止项目进程树及向运行中的 Java 应用附加 Arthas。底部面板可拖拽调整高度。Node.js 服务支持 npm/pnpm/yarn 启动、停止和运行输出。' })}><CircleHelp size={19} /></button><button className="activity-button" aria-label="设置" title="设置" onClick={explainWorkspace}><Settings2 size={19} /></button></div>
      </nav>
      {explorerOpen && <WorkspaceTree data={data} onChange={applyWorkspace} onRefresh={() => void load(true)} onConfigure={explainWorkspace} onAddFolder={() => manage('add_folder')} onSave={() => manage('save')} onClose={() => setExplorerOpen(false)} />}
      <div className="editor-area">
        <main id="projects" className="main-content">
          <WorkspacePage key={data?.folders.map(folder => folder.path).join('|') ?? 'empty'} query={query} onQueryChange={setQuery} data={data} loading={loading} monitoringEnabled={connected} onRefresh={() => void load()} onConfigure={() => manage(hasWorkspace ? 'add_folder' : 'open')} onAction={projectAction} />
        </main>
        {panel && (panel.runtimeProject && data ? <RuntimeConsole key={projectKey(panel.runtimeProject)} project={panel.runtimeProject} revision={data.revision} title={panel.title} description={panel.description} onClose={() => setPanel(null)} /> : <ResizableBottomPanel title={panel.title} project={panel.project} description={panel.description} onClose={() => setPanel(null)} />)}
      </div>
    </div>
    <footer className="statusbar"><span className={`connection ${connected ? 'online' : ''}`}><i />{loading ? '正在连接' : connected ? '本地已连接' : '未连接'}</span><button className={panel ? 'selected' : ''} aria-expanded={panel !== null} onClick={() => setPanel(panel ? null : output)}><PanelBottom size={13} />运行输出</button>{error ? <div className="service-error" title={`无法连接本地服务：${error}`}><span role="status" aria-label={`无法连接本地服务：${error}`}>本地服务未连接</span><button disabled={loading} aria-label="重试连接本地服务" onClick={() => void load()}>{loading ? '连接中…' : '重试'}</button></div> : <span className="status-version">DevFlow · 0.1.0</span>}</footer>
    {managingWorkspace && <WorkspaceManager key={managingWorkspace} mode={managingWorkspace} data={data} onChange={applyWorkspace} onRefresh={() => void load()} onClose={() => { setManagingWorkspace(null); workspaceTrigger.current?.focus() }} />}
    {launchProject && data && (launchProject.kind === 'node' ? <NodeLaunchDialog key={projectKey(launchProject)} project={launchProject} onClose={() => setLaunchProject(null)} /> : <JavaLaunchDialog key={projectKey(launchProject)} project={launchProject} onClose={() => setLaunchProject(null)} />)}
  </div>
}
