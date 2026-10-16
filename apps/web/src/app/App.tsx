import { useEffect, useState } from 'react'
import { Activity, ArrowLeft, Box, ChevronDown, CircleHelp, FlaskConical, FolderOpen, FolderTree, MoreHorizontal, PanelBottom, Play, RefreshCw, Search, Settings2, Terminal, X } from 'lucide-react'
import { getWorkspace, type Project, type WorkspaceResponse } from '../lib/api'

type View = 'workspace' | 'explorer' | 'qa'
type Panel = { title: string, project?: string, description: string }
const entries = [
  { key: 'workspace', title: '工作区', icon: FolderTree },
  { key: 'explorer', title: '搜索与上下文', icon: Search },
  { key: 'qa', title: 'QA 场景', icon: FlaskConical },
] as const
const examples: Project[] = [
  { name: 'backend', kind: 'maven', path: '示例工作区 / backend', markers: ['pom.xml'] },
  { name: 'frontend', kind: 'node', path: '示例工作区 / frontend', markers: ['package.json'] },
]
const kindNames = { maven: 'Maven · Spring Boot', node: 'Node.js', maven_and_node: 'Maven · Node.js' }

export function App() {
  const [view, setView] = useState<View>('workspace')
  const [data, setData] = useState<WorkspaceResponse | null>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  const [demo, setDemo] = useState(false)
  const [panel, setPanel] = useState<Panel | null>(null)
  const [menu, setMenu] = useState<string | null>(null)

  async function load() {
    setLoading(true)
    try { setData(await getWorkspace()); setError('') }
    catch (err) { setError(String(err)) }
    finally { setLoading(false) }
  }
  useEffect(() => { void load() }, [])
  useEffect(() => {
    function dismiss(event: KeyboardEvent) {
      if (event.key === 'Escape') { setMenu(null); setPanel(null) }
    }
    window.addEventListener('keydown', dismiss)
    return () => window.removeEventListener('keydown', dismiss)
  }, [])

  const projects = demo ? examples : data?.projects ?? []
  const connected = !error && data !== null
  const workspaceName = demo ? '示例工作区' : data?.workspaceRoot?.split(/[\\/]/).filter(Boolean).at(-1) ?? '未选择工作区'
  const output: Panel = { title: '运行输出', description: '暂无运行输出。启动项目后，日志将在这里显示。运行功能尚未接入。' }
  function explainWorkspace() {
    setPanel({ title: '工作区设置', description: '工作区选择功能尚未接入。当前可设置环境变量 DEVFLOW_WORKSPACE 指向项目目录，然后重启后端。' })
  }
  function projectAction(project: Project, title: string) {
    setMenu(null)
    setPanel({ title, project: project.name, description: title === 'JVM 诊断' ? '诊断面板预览。后续可在这里查看 JVM 状态、线程与 Arthas 诊断结果。当前尚未接入诊断功能。' : `${title}面板预览。当前尚未接入项目运行功能，没有执行任何项目操作。` })
  }

  return <div className="shell">
    <header className="topbar">
      <a className="brand" href="#" onClick={event => { event.preventDefault(); setView('workspace') }} aria-label="DevFlow 工作区"><span className="brand-mark">◈</span><strong>DevFlow</strong></a>
      <span className="header-divider" />
      <button className="workspace-switch" onClick={explainWorkspace} title={data?.workspaceRoot ?? '配置工作区'}><FolderOpen size={14} /><span>{workspaceName}</span><ChevronDown size={12} /></button>
      <span className="local-label">LOCAL</span>
    </header>
    <div className="workbench">
      <nav className="activity-bar" aria-label="功能入口">
        <div className="activity-primary">{entries.map(({ key, title, icon: Icon }) => <button key={key} className={`activity-button ${view === key ? 'active' : ''}`} aria-label={title} title={title} aria-current={view === key ? 'page' : undefined} onClick={() => { setView(key); setMenu(null) }}><Icon size={20} strokeWidth={1.65} /></button>)}</div>
        <div className="activity-secondary"><button className="activity-button" aria-label="使用说明" title="使用说明" onClick={() => setPanel({ title: '使用说明', description: '从左侧进入工作区、搜索与 QA。在项目右侧选择运行或诊断操作，结果显示在底部面板。当前为布局预览，项目操作尚未接入。' })}><CircleHelp size={19} /></button><button className="activity-button" aria-label="设置" title="设置" onClick={explainWorkspace}><Settings2 size={19} /></button></div>
      </nav>
      <div className="editor-area">
        <main className="main-content">
          <div className="page-heading"><div className="breadcrumb">本地工作台 <span>/</span> {entries.find(entry => entry.key === view)?.title}</div><span className="preview-label">布局预览</span></div>
          {error && <div className="alert" role="alert">无法连接本地服务：{error}<button onClick={() => void load()}>重试</button></div>}
          {view === 'workspace' ? <>
            <div className="workspace-heading"><div><h1>工作区</h1><p>项目、运行与诊断，在一个地方。</p></div><button className="icon-button" aria-label="刷新项目" title="刷新项目" disabled={loading} onClick={() => void load()}><RefreshCw size={16} className={loading ? 'spinning' : ''} /></button></div>
            <div className="workspace-path"><FolderOpen size={14} /><span title={demo ? '示例工作区' : data?.workspaceRoot ?? ''}>{demo ? '示例工作区 · 仅用于查看布局' : data?.workspaceRoot ?? '配置工作区后，项目会显示在这里'}</span></div>
            <div className="list-toolbar"><h2>项目 <span>{projects.length}</span></h2><button className={`text-button ${demo ? 'selected' : ''}`} aria-pressed={demo} onClick={() => { setDemo(!demo); setMenu(null); setPanel(null) }}>{demo ? '退出演示' : '演示项目'}</button></div>
            {projects.length > 0 ? <div className="project-list">{projects.map(project => <article className="project" key={project.path}>
              <div className={`project-icon ${project.kind === 'node' ? 'node' : ''}`}><Box size={19} strokeWidth={1.5} /></div>
              <div className="project-info"><h3>{project.name}{demo && <span className="demo-badge">示例</span>}</h3><p>{kindNames[project.kind]}</p><small title={project.path}>{project.path}</small><span className="project-state"><i />{demo ? '未运行 · 演示状态' : '运行状态待接入'}</span></div>
              <div className="project-actions"><button className="run-button" onClick={() => projectAction(project, '启动配置')}><Play size={12} />启动<ChevronDown size={11} /></button><div className="menu-anchor"><button className="icon-button" aria-label={`${project.name} 更多操作`} title="更多操作" aria-expanded={menu === project.path} onClick={() => setMenu(menu === project.path ? null : project.path)}><MoreHorizontal size={17} /></button>{menu === project.path && <><button className="menu-dismiss" aria-label="关闭操作菜单" onClick={() => setMenu(null)} /><div className="project-menu" aria-label={`${project.name} 操作`}><button onClick={() => projectAction(project, '运行输出')}><Terminal size={14} />查看运行输出</button>{project.kind !== 'node' && <button onClick={() => projectAction(project, 'JVM 诊断')}><Activity size={14} />JVM 诊断</button>}<button onClick={() => projectAction(project, '调试启动')}><Play size={14} />调试启动</button><div className="menu-note">操作入口预览</div></div></>}</div></div>
            </article>)}</div> : <div className="empty-state"><div className="empty-symbol"><FolderOpen size={27} strokeWidth={1.25} /></div><h3>{loading ? '正在读取工作区…' : data?.workspaceRoot ? '还没有发现项目' : '从一个工作区开始'}</h3><p>{data?.workspaceRoot ? '支持识别目录及其直接子目录中的 Maven 和 Node.js 项目。' : '把本地项目集中在这里，随时查看运行状态、日志和诊断结果。'}</p><button className="primary-button" onClick={explainWorkspace}><FolderOpen size={14} />配置工作区</button><button className="text-button" onClick={() => setDemo(true)}>先看看演示项目 <span>→</span></button></div>}
            <div className="workspace-note"><span />支持 Maven / Spring Boot 与 Node.js 项目</div>
          </> : <div className="feature-page"><div className="workspace-heading"><div><h1>{view === 'explorer' ? '搜索与上下文' : 'QA 场景'}</h1><p>{view === 'explorer' ? '找到文件，整理本次开发需要的上下文。' : '围绕项目组织接口场景与验收结果。'}</p></div></div><div className="feature-placeholder">{view === 'explorer' ? <Search size={26} strokeWidth={1.3} /> : <FlaskConical size={26} strokeWidth={1.3} />}<h3>{view === 'explorer' ? '搜索入口已就位' : '场景入口已就位'}</h3><p>当前先确认布局，功能将在后续接入。</p><button className="text-button" onClick={() => setView('workspace')}><ArrowLeft size={13} />返回工作区</button></div></div>}
        </main>
        {panel && <section className="bottom-panel" aria-label={panel.title}><div className="panel-heading"><span><Terminal size={14} />{panel.title}{panel.project && <small>/ {panel.project}</small>}</span><button className="icon-button" aria-label="收起底部面板" title="收起面板" onClick={() => setPanel(null)}><X size={15} /></button></div><div className="panel-content"><span className="panel-caption">{panel.project ?? 'DEVFLOW'}</span><p>{panel.description}</p></div></section>}
      </div>
    </div>
    <footer className="statusbar"><span className={`connection ${connected ? 'online' : ''}`}><i />{loading ? '正在连接' : connected ? '本地已连接' : '未连接'}</span><button className={panel ? 'selected' : ''} aria-expanded={panel !== null} onClick={() => setPanel(panel ? null : output)}><PanelBottom size={13} />运行输出</button><span className="status-version">DevFlow · 0.1.0</span></footer>
  </div>
}
