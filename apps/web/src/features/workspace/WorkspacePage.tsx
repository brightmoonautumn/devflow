import { readOnlyShare } from '../../lib/access'
import { useEffect, useRef, useState } from 'react'
import { Activity, Box, Circle, Coffee, FolderOpen, LoaderCircle, MoreHorizontal, Network, Play, PlugZap, RefreshCw, Search, Settings2, Terminal, X } from 'lucide-react'
import { projectKey, metricsKey, type Project, type WorkspaceResponse } from '../../lib/api'
import TaskRows, { type TaskRow } from '../../components/beautiful-ui/task-rows'
import { useRuntimeMetrics } from '../runtime/useRuntimeMetrics'
import { ProjectMetrics } from '../runtime/ProjectMetrics'
import '../../components/beautiful-ui/theme.css'
import './workspace.css'

const kindNames = { maven: 'Maven', node: 'Node.js', maven_and_node: 'Maven + Node.js' }
type Filter = 'all' | 'maven' | 'node'
type PendingAction = { type: 'start' | 'stop'; settledAt?: number }
type Props = {
  query: string
  onQueryChange: (query: string) => void
  data: WorkspaceResponse | null
  loading: boolean
  monitoringEnabled: boolean
  outputProjectKey?: string
  onRefresh: () => void
  onConfigure: () => void
  onAction: (project: Project, title: string) => Promise<void>
}

export function WorkspacePage({ query, onQueryChange: setQuery, data, loading, monitoringEnabled, outputProjectKey, onRefresh, onConfigure, onAction }: Props) {
  const [filter, setFilter] = useState<Filter>('all')
  const [menu, setMenu] = useState<string | null>(null)
  const [pendingActions, setPendingActions] = useState<Record<string, PendingAction>>({})
  const submitting = useRef(new Set<string>())
  const menuTrigger = useRef<HTMLButtonElement | null>(null)
  useEffect(() => {
    if (!menu) return
    function dismiss(event: PointerEvent) { if (!(event.target as Element).closest('.ws-menu-anchor')) setMenu(null) }
    document.addEventListener('pointerdown', dismiss)
    return () => document.removeEventListener('pointerdown', dismiss)
  }, [menu])
  const projects = data?.projects ?? []
  const runtime = useRuntimeMetrics(monitoringEnabled && projects.length > 0)
  useEffect(() => {
    // Wait for a sample taken after the mutation, rather than showing cached state.
    setPendingActions(current => {
      const completed = Object.keys(current).filter(path => {
        const settledAt = current[path].settledAt
        return settledAt !== undefined && (!monitoringEnabled || !!runtime.error || (runtime.snapshot?.sampledAt ?? 0) * 1000 > settledAt)
      })
      if (!completed.length) return current
      const next = { ...current }
      completed.forEach(path => { delete next[path] })
      return next
    })
  }, [runtime.snapshot, runtime.error, monitoringEnabled, pendingActions])
  async function changeProjectState(project: Project, type: 'start' | 'stop') {
    if (submitting.current.has(projectKey(project)) || pendingActions[projectKey(project)]) return
    submitting.current.add(projectKey(project))
    setPendingActions(current => ({ ...current, [projectKey(project)]: { type } }))
    try {
      await onAction(project, type === 'start' ? '启动项目' : '停止项目')
    } finally {
      submitting.current.delete(projectKey(project))
      setPendingActions(current => ({ ...current, [projectKey(project)]: { type, settledAt: Date.now() } }))
    }
  }
  const metricsByPath = new Map(runtime.snapshot?.projects.map(metrics => [metricsKey(metrics), metrics]))
  const filtered = projects.filter(project => {
    const matchesKind = filter === 'all' || project.kind === filter || project.kind === 'maven_and_node'
    return matchesKind && `${project.name} ${project.path}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase())
  })
  const rows = filtered.map((project): TaskRow => {
    const metrics = metricsByPath.get(projectKey(project))
    const observed = !!metrics?.pids.length || metrics?.managedState === 'starting' || metrics?.managedState === 'running'
    const caption = !monitoringEnabled || runtime.error ? '运行指标暂不可用' : !metrics ? '正在读取运行状态…' : metrics.managedState === 'starting' ? '构建 / 启动中' : observed ? undefined : metrics.managedState === 'failed' ? '启动失败或异常退出' : '未检测到进程'
    const listeningPorts = metrics?.listeningPorts
    const portError = metrics && listeningPorts === undefined
      ? '当前后端未返回端口字段，请重新编译并重启 DevFlow 后端。'
      : metrics?.portError ?? (listeningPorts === null ? '后端未提供端口采集结果，请更新并重启 DevFlow 后端后重试。' : undefined)
    const portStatus = portError ? listeningPorts === undefined ? '后端需更新' : '端口采集失败'
      : metrics?.managedState === 'starting' ? '等待监听' : '暂无监听端口'
    const showPorts = observed && monitoringEnabled && !runtime.error
    return {
      key: projectKey(project),
      label: project.name,
      amount: kindNames[project.kind],
      status: observed ? 'running' : metrics && monitoringEnabled && !runtime.error ? 'idle' : 'unknown',
      caption,
      metadata: showPorts ? <span className="ws-service-ports" title={listeningPorts?.length ? `TCP 监听端口：${listeningPorts.join('、')}。包括本机关联进程的业务、调试及诊断端口，与启动工具无关。` : portError}>
        {listeningPorts?.length ? <><Network size={11} aria-hidden="true" />{listeningPorts.slice(0, 3).map(port => <span key={port} className="ws-port-badge" aria-label={`TCP 端口 ${port}`}>{port}</span>)}{listeningPorts.length > 3 && <span className="ws-port-more" aria-label={`另外 ${listeningPorts.length - 3} 个端口`}>+{listeningPorts.length - 3}</span>}</> : <span className="ws-port-empty">{portStatus}</span>}
      </span> : undefined,
      content: metrics && observed ? <ProjectMetrics metrics={metrics} history={runtime.history} /> : undefined,
      icon: <span className={`ws-project-symbol ${project.kind === 'node' ? 'is-node' : ''} ${observed && monitoringEnabled && !runtime.error ? metrics?.managedState === 'starting' ? 'is-starting' : 'is-running' : ''}`} title={observed && monitoringEnabled && !runtime.error ? metrics?.managedState === 'starting' ? '构建 / 启动中' : '运行中' : undefined}>{project.kind === 'node' ? <Box size={19} strokeWidth={1.6} /> : <Coffee size={19} strokeWidth={1.6} />}</span>,
      details: [
        { label: '项目目录', meta: project.path },
        ...(showPorts && listeningPorts?.length ? [{ label: 'TCP 监听端口', meta: listeningPorts.join(' · ') }] : []),
        ...(showPorts && portError ? [{ label: '端口检测', meta: portError }] : []),
        ...(metrics?.pids.length ? [{ label: '关联 PID', meta: metrics.pids.join(' · ') }] : []),
        { label: '识别文件', meta: project.markers.join(' · ') },
        ...(project.mainClasses?.length ? [{ label: '启动主类', meta: project.mainClasses.join(' · ') }] : []),
        ...(project.launchScripts?.length ? [{ label: '启动脚本', meta: project.launchScripts.join(' · ') }] : []),
      ],
    }
  })
  // Stable sorting keeps the workspace order within each runtime group.
  rows.sort((left, right) => Number(right.status === 'running') - Number(left.status === 'running'))
  const projectsByPath = new Map(filtered.map(project => [projectKey(project), project]))
  function renderProjectActions(row: TaskRow) {
    const project = projectsByPath.get(row.key)
    if (!project) return null
    const running = row.status === 'running'
    const pending = pendingActions[projectKey(project)]
    const starting = pending?.type === 'start' || metricsByPath.get(projectKey(project))?.managedState === 'starting'
    const stopping = pending?.type === 'stop'
    const transitioning = starting || stopping
    const ready = monitoringEnabled && !runtime.error && metricsByPath.has(projectKey(project))
    const action = stopping ? '停止中…' : starting ? '启动中…' : ready ? running ? '停止' : '启动' : runtime.error || !monitoringEnabled ? '状态未知' : '读取中'
    const buttonTone = ready ? stopping || (running && !starting) ? 'is-stop' : 'is-start' : ''
    const actionTitle = stopping ? '正在停止项目进程树' : starting ? pending ? '正在提交启动请求' : '构建 / 启动中，点击取消启动' : !ready ? '等待有效运行状态' : running ? '停止项目进程树' : '使用已保存配置或默认配置启动'
    return (<div className="ws-row-actions">
      {!readOnlyShare && <button className={`ws-launch ${transitioning ? 'is-pending' : ''} ${buttonTone}`} disabled={!ready || !!pending} aria-busy={transitioning} aria-label={`${project.name} ${action}${starting && !pending ? '，点击取消启动' : ''}`} title={actionTitle} onClick={() => void changeProjectState(project, running || starting ? 'stop' : 'start')}>
        {transitioning ? <LoaderCircle size={13} className="spinning" aria-hidden="true" /> : running ? <Circle size={10} fill="currentColor" aria-hidden="true" /> : <Play size={12} fill="currentColor" aria-hidden="true" />}
        <span>{action}</span>
      </button>}<div className="ws-menu-anchor">
          <button className="ws-icon-button" aria-label={`${project.name} 更多操作`} title="更多操作" aria-expanded={menu === projectKey(project)} aria-controls={menu === projectKey(project) ? 'workspace-project-actions' : undefined} onClick={event => { menuTrigger.current = event.currentTarget; setMenu(menu === projectKey(project) ? null : projectKey(project)) }}><MoreHorizontal size={17} /></button>
          {menu === projectKey(project) && <div id="workspace-project-actions" className="ws-menu" aria-label={`${project.name} 操作`} onKeyDown={event => { if (event.key === 'Escape') { setMenu(null); menuTrigger.current?.focus(); event.stopPropagation() } }}>
            <button autoFocus onClick={() => { setMenu(null); onAction(project, '运行输出') }}><Terminal size={14} />查看运行输出</button>
            {!readOnlyShare && <button onClick={() => { setMenu(null); onAction(project, '启动配置') }}><Settings2 size={14} />启动配置</button>}
            {!readOnlyShare && project.kind !== 'node' && <button onClick={() => { setMenu(null); onAction(project, 'JVM 诊断') }}><Activity size={14} />JVM 诊断</button>}
            {!readOnlyShare && project.kind !== 'node' && <button disabled={!running || !ready} onClick={() => { setMenu(null); onAction(project, 'Arthas 挂载') }}><PlugZap size={14} />快速挂载 Arthas</button>}
          </div>}
        </div></div>)
  }

  return <section className="workspace-page" aria-label="工作区项目">
    <div className="ws-section-bar"><div className="ws-section-title"><h1>服务</h1><span className="ws-count">{projects.length}</span><button className="ws-icon-button" title="刷新服务" aria-label="刷新服务" disabled={loading} onClick={onRefresh}><RefreshCw size={14} className={loading ? 'spinning' : ''} /></button></div>
      <div className="ws-filters" role="group" aria-label="项目类型">{(['all', 'maven', 'node'] as const).map(kind => <button key={kind} aria-pressed={filter === kind} onClick={() => setFilter(kind)}>{kind === 'all' ? '全部' : kind === 'maven' ? 'Maven' : 'Node.js'}</button>)}</div>
    </div>
    <div className="ws-project-list" aria-busy={loading}>
      {rows.length > 0 && <TaskRows variant="Cards" className="ws-service-cards" rows={rows} activeRowKey={outputProjectKey} labels={{ completed: '已完成', failed: '失败' }} renderActions={renderProjectActions} onActivateRow={row => { const project = projectsByPath.get(row.key); if (project) onAction(project, '运行输出') }} />}
      {filtered.length === 0 && <div className="ws-empty"><div className="ws-empty-icon">{projects.length ? <Search size={24} strokeWidth={1.4} /> : <FolderOpen size={26} strokeWidth={1.4} />}</div><h2>{projects.length ? '没有匹配的服务' : loading ? '正在读取工作区…' : data?.workspaceFile && !data.folders.length ? '工作区还没有文件夹' : data?.workspaceRoot ? '还没有发现可启动服务' : '从一个工作区开始'}</h2><p>{projects.length ? '试试其他名称，或调整项目类型。' : data?.workspaceFile && !data.folders.length ? '添加本地文件夹到工作区，保存后也可以在 VS Code 中打开。' : data?.workspaceRoot ? '只列出具有真实 Java 启动入口或 Node.js 启动脚本的工程。' : '配置本地项目目录，让项目井然有序。'}</p>{projects.length ? <button className="ws-outline-button" onClick={() => { setQuery(''); setFilter('all') }}><X size={13} />清除筛选</button> : !loading && !readOnlyShare && <button className="ws-outline-button" onClick={onConfigure}><FolderOpen size={14} />{data?.workspaceFile && !data.folders.length ? '添加文件夹' : '配置工作区'}</button>}</div>}
    </div>
  </section>
}
