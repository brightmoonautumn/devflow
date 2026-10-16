import { useEffect, useState } from 'react'
import { Activity, Blocks, CheckCircle2, ChevronRight, FileSearch, FolderKanban, FlaskConical, Network, RefreshCw, ServerCog, Settings2, Terminal } from 'lucide-react'
import { getWorkspace, type WorkspaceResponse } from '../lib/api'

type Tab = 'workspace' | 'runtime' | 'diagnose' | 'explorer' | 'qa'
const tabs: { key: Tab, title: string, icon: typeof Blocks }[] = [
  { key:'workspace', title:'工作区', icon:FolderKanban },
  { key:'runtime', title:'运行', icon:ServerCog },
  { key:'diagnose', title:'诊断', icon:Activity },
  { key:'explorer', title:'文件', icon:FileSearch },
  { key:'qa', title:'QA', icon:FlaskConical },
]
export function App() {
  const [tab,setTab] = useState<Tab>('workspace')
  const [data,setData] = useState<WorkspaceResponse | null>(null)
  const [error,setError] = useState('')
  const load = () => getWorkspace().then(setData).then(()=>setError('')).catch(err => setError(String(err)))
  useEffect(()=>{void load()},[])
  return <div className="shell">
    <header className="top"><div className="brand"><span className="brand-icon">◈</span><strong>DevFlow</strong><span className="edition">LOCAL WORKSPACE</span></div><div className="top-actions"><span className="dot"/> localhost:31080 <Settings2 size={16}/></div></header>
    <nav className="tabs">{tabs.map(({key,title,icon:Icon})=><button key={key} className={tab===key?'active':''} onClick={()=>setTab(key)}><Icon size={15}/>{title}</button>)}</nav>
    <main>
      <div className="headline"><div><p className="eyebrow">DEVELOPMENT CONTROL CENTER</p><h1>{tabs.find(x=>x.key===tab)?.title}</h1><p className="subtitle">{tab==='workspace'?'面向 Maven / Node 项目的本地 AI 开发工作台':'该功能的 UI 与 Rust 核心模块已预留，交由 Codex 继续实现。'}</p></div><button className="refresh" onClick={()=>void load()}><RefreshCw size={15}/>刷新</button></div>
      {error&&<div className="alert">无法获取 DevFlow API：{error}。请启动 Rust 后端。</div>}
      {tab==='workspace'?<><section className="summary"><div><span>已识别项目</span><strong>{data?.projects.length ?? '—'}</strong></div><div><span>工作区状态</span><strong className="small">{data?.workspaceRoot?'已配置':'未配置'}</strong></div><div><span>接口状态</span><strong className="small">{error?'未连接':'已连接'}</strong></div></section><div className="section-heading"><h2>项目列表</h2><span>MAVEN · NODE.JS</span></div><div className="project-list">{data?.projects.length?data.projects.map(project=><div className="project" key={project.path}><div className="project-symbol"><Blocks size={19}/></div><div className="project-info"><strong>{project.name}</strong><small title={project.path}>{project.path}</small></div><span className="kind">{project.kind.replaceAll('_',' + ')}</span><ChevronRight size={17} className="muted"/></div>):<div className="empty"><Network size={28}/><h3>尚未发现项目</h3><p>设置环境变量 <code>DEVFLOW_WORKSPACE</code> 指向包含 Maven 或 Node.js 项目的目录，然后重启后端。</p></div>}</div></>:<div className="empty feature"><Terminal size={29}/><h3>{tabs.find(x=>x.key===tab)?.title} · 待实现</h3><p>模块目录和职责边界已经创建。MVP 优先完成 JVM 服务发现、启动与文件上下文交互。</p><code>apps/web/src/features/{tab}/</code></div>}
    </main><footer><div><CheckCircle2 size={14}/> DevFlow Starter v0.1.0</div><div>Rust · Axum · React <span className="sep">/</span> MCP</div></footer>
  </div>
}
