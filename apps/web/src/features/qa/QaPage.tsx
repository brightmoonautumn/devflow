import { useEffect, useRef, useState } from 'react'
import { ArrowDown, ArrowRight, Check, CheckCircle2, ChevronDown, ChevronRight, Circle, FlaskConical, Folder, GitBranch, ListTree, Play, RotateCcw, Search, ShieldCheck, Square } from 'lucide-react'
import { FileTreeRow, TreeChevron } from '../../components/files/FileTreeRow'
import { qaScenarios, type QaCall } from './qaDemo'
import { QaSidebar } from './QaSidebar'
import './qa.css'

export function QaPage() {
  const [scenarioId, setScenarioId] = useState('create')
  const scenario = qaScenarios.find(item => item.id === scenarioId)!
  const [actionId, setActionId] = useState<string | null>('enter')
  const [expanded, setExpanded] = useState<string | null>('list')
  const [tab, setTab] = useState('请求')
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [treeOpen, setTreeOpen] = useState(() => window.matchMedia('(min-width: 601px)').matches)
  const [view, setView] = useState<'both' | 'calls' | 'flow'>('both')
  const [done, setDone] = useState<string[]>([])
  const [running, setRunning] = useState(false)
  const [environment, setEnvironment] = useState('本地开发')
  const timer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const calls = scenario.actions.flatMap(action => action.calls)
  const selected = scenario.actions.find(action => action.id === actionId)
  useEffect(() => () => clearTimeout(timer.current), [])
  function stop() { clearTimeout(timer.current); setRunning(false) }
  function chooseScenario(id: string) { stop(); setScenarioId(id); setActionId(null); setExpanded(null); setDone([]); if (window.matchMedia('(max-width: 600px)').matches) setTreeOpen(false) }
  function selectAction(id: string) { setActionId(id); const call = scenario.actions.find(action => action.id === id)?.calls[0]; if (call) document.getElementById(`qa-call-${call.id}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' }) }
  function simulate() {
    setDone([]); setRunning(true)
    let index = 0
    const advance = () => {
      const call = calls[index++]
      if (!call) { setRunning(false); return }
      setDone(previous => [...previous, call.id])
      setActionId(scenario.actions.find(action => action.calls.some(item => item.id === call.id))!.id)
      timer.current = setTimeout(advance, 420)
    }
    timer.current = setTimeout(advance, 250)
  }
  function detail(call: QaCall) {
    if (tab === '请求') return <><div className="qa-code-caption">{environment === '本地开发' ? 'http://localhost:8080' : 'https://test.example.com'}{call.path}</div><pre>{JSON.stringify(call.body ?? { headers: { Authorization: 'Bearer {{accessToken}}' }, query: call.path.split('?')[1] ?? '无' }, null, 2)}</pre></>
    if (tab === '响应') return done.includes(call.id) ? <><div className="qa-code-caption qa-success">模拟响应 · {call.id === 'duplicate-user' ? '409 Conflict' : '200 OK'}</div><pre>{JSON.stringify(call.response, null, 2)}</pre></> : <p className="qa-detail-empty">尚未演示执行，执行后在此查看模拟响应。</p>
    if (tab === '断言') return <div className="qa-assertion"><CheckCircle2 size={15} /><span>{call.expect}</span><small>{done.includes(call.id) ? '模拟通过' : '待执行'}</small></div>
    return <div className="qa-variable"><code>{call.extract ?? (call.path.includes('{{userId}}') ? '使用 userId ← 创建用户的响应' : '当前请求无变量提取')}</code><small>场景内共享，按调用顺序传递</small></div>
  }
  return <main className="qa-page">
    <QaSidebar open={treeOpen} onToggle={() => setTreeOpen(value => !value)}>
      <label className="qa-search"><Search size={13} /><input aria-label="搜索场景" placeholder="搜索场景…" value={query} onChange={event => setQuery(event.target.value)} /></label>
      <FileTreeRow className="qa-tree-root" leading={<><TreeChevron expanded={!collapsed.includes('system')} onToggle={() => setCollapsed(items => items.includes('system') ? items.filter(id => id !== 'system') : [...items, 'system'])} label="展开或收起系统模块" /><Folder size={15} /></>}><span>系统模块</span></FileTreeRow>
      {!collapsed.includes('system') && ['用户管理', '角色管理'].map(module => <div key={module}>
        <FileTreeRow className="qa-tree-module" leading={<><TreeChevron expanded={!collapsed.includes(module)} onToggle={() => setCollapsed(items => items.includes(module) ? items.filter(id => id !== module) : [...items, module])} label={`展开或收起${module}`} /><Folder size={14} /></>}><span>{module}</span></FileTreeRow>
        {!collapsed.includes(module) && qaScenarios.filter(item => item.module === module && item.title.includes(query)).map(item => <button key={item.id} className={`qa-tree-item ${scenarioId === item.id ? 'selected' : ''}`} onClick={() => chooseScenario(item.id)}><GitBranch size={14} /><span>{item.title}</span><span className="qa-tree-dot" /></button>)}
      </div>)}
      {query && !qaScenarios.some(item => item.title.includes(query)) && <p className="qa-tree-empty">没有匹配的场景</p>}
      <div className="qa-sidebar-note"><ShieldCheck size={16} /><div>从用户行为出发<small>产品操作 → 接口编排 → 结果验证</small></div></div>
    </QaSidebar>
    <section className="qa-workspace">
      <div className="qa-title-row"><div><div className="qa-eyebrow">USER JOURNEY / API SCENARIO</div><h1>{scenario.title}</h1><p>{scenario.purpose}</p></div><span className="qa-scenario-label">{scenarioId === 'duplicate' ? '异常路径 · 业务拦截' : '正常路径'}</span></div>
      <div className="qa-toolbar"><div className="qa-toolbar-left"><label className="qa-environment"><span className="qa-tree-dot" /><select aria-label="测试环境" value={environment} onChange={event => { stop(); setDone([]); setEnvironment(event.target.value) }}><option>本地开发</option><option>测试环境</option></select></label><span className="qa-count">{scenario.actions.length} 个行为 <span>·</span> {calls.length} 个接口</span></div><div className="qa-run-actions"><button title="清除模拟结果" aria-label="清除模拟结果" onClick={() => { stop(); setDone([]) }}><RotateCcw size={14} /></button>{running ? <button className="qa-run" onClick={stop}><Square size={13} />停止演示</button> : <button className="qa-run" onClick={simulate}><Play size={13} />演示执行</button>}</div></div>
      <div className="qa-viewbar"><div className="qa-tabs">{([['both', '联动视图'], ['calls', 'API 调用'], ['flow', '行为流程']] as const).map(([id, label]) => <button key={id} className={view === id ? 'active' : ''} onClick={() => setView(id)}>{label}</button>)}</div><span>点击行为节点，定位关联接口</span></div>
      <div className={`qa-panels qa-view-${view}`}>
        {view !== 'flow' && <section className="qa-calls-panel" aria-label="API 调用链"><div className="qa-panel-heading"><span><ListTree size={15} />API 调用链</span><small>按用户操作顺序执行</small></div><div className="qa-calls-scroll">
          {scenario.actions.map((action, index) => <div className={`qa-call-group ${actionId === action.id ? 'is-selected' : ''} ${actionId && actionId !== action.id ? 'is-muted' : ''}`} key={action.id}>
            <button className="qa-group-heading" onClick={() => setActionId(actionId === action.id ? null : action.id)}><span className="qa-action-number">{String(index + 1).padStart(2, '0')}</span><strong>{action.title}</strong><small>{action.calls.length} API</small></button>
            {action.calls.map(call => <article id={`qa-call-${call.id}`} key={call.id} className={`qa-call ${expanded === call.id ? 'is-open' : ''}`}><button className="qa-call-summary" aria-expanded={expanded === call.id} onClick={() => { setExpanded(expanded === call.id ? null : call.id); setActionId(action.id) }}>{expanded === call.id ? <ChevronDown size={13} /> : <ChevronRight size={13} />}<span className={`qa-method qa-method-${call.method.toLowerCase()}`}>{call.method}</span><span className="qa-call-name">{call.name}<code>{call.path}</code></span>{done.includes(call.id) ? <span className="qa-call-result"><Check size={13} />{24 + calls.indexOf(call) * 17} ms</span> : <Circle size={12} className="qa-pending" />}</button>{expanded === call.id && <div className="qa-call-detail"><div className="qa-detail-tabs">{['请求', '响应', '断言', '变量'].map(label => <button className={tab === label ? 'active' : ''} onClick={() => setTab(label)} key={label}>{label}{label === '断言' ? ' · 1' : ''}</button>)}</div>{detail(call)}</div>}</article>)}
          </div>)}
          <div className="qa-chain-end"><CheckCircle2 size={14} />{done.length === calls.length ? '场景演示完成，全部模拟断言通过' : '调用链结束 · 业务结果验证与数据清理'}</div>
        </div></section>}
        {view !== 'calls' && <section className="qa-flow-panel" aria-label="用户行为流程"><div className="qa-panel-heading"><span><GitBranch size={15} />用户行为流程</span><button onClick={() => setActionId(null)}>取消选择</button></div><div className="qa-flow-scroll"><div className="qa-flow-start"><span />管理员 · 已登录</div>{scenario.actions.map((action, index) => <div className="qa-flow-step" key={action.id}><div className={`qa-flow-edge ${actionId === action.id ? 'active' : ''}`}><ArrowDown size={13} /></div><button className={`qa-flow-node ${actionId === action.id ? 'selected' : ''} ${action.calls.every(call => done.includes(call.id)) ? 'complete' : ''}`} aria-pressed={actionId === action.id} onClick={() => selectAction(action.id)}><div className="qa-flow-node-title"><span>{String(index + 1).padStart(2, '0')}</span><strong>{action.title}</strong>{action.calls.every(call => done.includes(call.id)) && <CheckCircle2 size={14} />}</div><p>{action.description}</p><div className="qa-flow-node-footer"><span>{action.calls.length} 个接口调用</span><span>{action.id === 'cleanup' ? '场景收尾' : '用户操作'}<ArrowRight size={12} /></span></div></button></div>)}<div className="qa-flow-edge"><ArrowDown size={13} /></div><div className="qa-flow-end"><Check size={12} />场景结束</div></div><div className="qa-flow-selection"><span className="qa-selection-dot" /><span>{selected ? `已选择「${selected.title}」· 关联 ${selected.calls.length} 个接口` : '选择一个行为，查看它触发的接口组'}</span></div></section>}
      </div>
      <footer className="qa-footer" aria-live="polite"><span><FlaskConical size={13} />{running ? `模拟执行中 ${done.length}/${calls.length}` : done.length === calls.length ? `${calls.length}/${calls.length} 模拟通过` : '示例场景 · 未连接测试服务'}</span><span>仅演示交互，不发送真实 API 请求</span></footer>
    </section>
  </main>
}
