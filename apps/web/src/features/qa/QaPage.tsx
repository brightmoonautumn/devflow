import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { ArrowDown, ArrowRight, Check, CheckCircle2, ChevronDown, ChevronRight, Circle, FlaskConical, LoaderCircle, GitBranch, ListTree, PanelsTopLeft, Play, Search, Square, Trash2, XCircle } from 'lucide-react'
import { FileTreeRow, TreeChevron, handleTreeNavigation } from '../../components/files/FileTreeRow'
import { TreeEntryIcon } from '../../components/files/TreeEntryIcon'
import { executeQa, getQa, qaExecutionPlan, type QaCall, type QaOperation, type QaSnapshot } from './qaApi'
import { getToken, type WorkspaceResponse } from '../../lib/api'
import { QaPanelDivider } from './QaPanelDivider'
import { QaSidebar } from './QaSidebar'
import { BaseSelect } from '../../components/form/BaseSelect'
import { useDialogs } from '../../components/form/BaseDialog'
import { BaseToast, type ToastMessage } from '../../components/form/BaseToast'
import { copyText } from '../../lib/clipboard'
import { QaCurl, QaJson, formatQaJson } from './QaJson'
import './qa.css'

export function QaPage({ workspace }: { workspace: WorkspaceResponse | null }) {
  const [snapshot, setSnapshot] = useState<QaSnapshot | null>(null)
  const [error, setError] = useState('')
  const [loadError, setLoadError] = useState('')
  const [pending, setPending] = useState(false)
  const workspaceKey = workspace?.workspaceFile ?? workspace?.workspaceRoot
  const revision = workspace?.revision
  const generation = useRef(0)
  const snapshotSequence = useRef(0)
  const refreshQa = useRef<() => void>(() => {})
  useEffect(() => {
    const current = ++generation.current
    setSnapshot(null); setError(''); setLoadError('')
    if (!workspaceKey || revision === undefined) return
    const controller = new AbortController()
    let reading = false, dirty = false
    async function refresh() {
      dirty = true
      const seq = ++snapshotSequence.current
      if (reading) return
      reading = true
      let currentSeq = seq
      try {
        do {
          dirty = false
          currentSeq = snapshotSequence.current
          try {
            const data = await getQa(workspaceKey!, revision!, controller.signal)
            if (generation.current === current && !controller.signal.aborted && currentSeq === snapshotSequence.current) { setSnapshot(data); setLoadError('') }
          } catch (e) { if (!controller.signal.aborted && generation.current === current && currentSeq === snapshotSequence.current) setLoadError(e instanceof Error ? e.message : '读取 QA 失败') }
        } while (dirty && !controller.signal.aborted)
      } finally { reading = false }
    }
    refreshQa.current = () => { void refresh() }
    async function subscribe() {
      let refreshToken = false
      while (!controller.signal.aborted) {
        try {
          const token = await getToken(refreshToken)
          refreshToken = false
          const params = new URLSearchParams({ workspaceKey: workspaceKey!, expectedRevision: String(revision!) })
          const response = await fetch(`/api/qa/events?${params}`, { headers: { Authorization: `Bearer ${token}` }, signal: controller.signal, cache: 'no-store' })
          if (response.status === 401) { refreshToken = true; throw new Error('QA 状态连接认证失败') }
          if (!response.ok || !response.body || !response.headers.get('content-type')?.includes('text/event-stream')) throw new Error('QA 实时连接不可用，请更新并重启后端')
          const reader = response.body.getReader(), decoder = new TextDecoder()
          let buffer = ''
          try {
            while (!controller.signal.aborted) {
              const chunk = await reader.read()
              if (chunk.done) break
              buffer += decoder.decode(chunk.value, { stream: true })
              let boundary: number
              while ((boundary = buffer.indexOf('\n\n')) !== -1) {
                const event = buffer.slice(0, boundary)
                buffer = buffer.slice(boundary + 2)
                if (event.split('\n').some(line => line.startsWith('data:'))) void refresh()
              }
            }
          } finally { reader.releaseLock() }
        } catch (e) { if (!controller.signal.aborted && generation.current === current) setLoadError(e instanceof Error ? e.message : 'QA 实时连接中断') }
        if (!controller.signal.aborted) await new Promise<void>(resolve => { const finish = () => { clearTimeout(timer); controller.signal.removeEventListener('abort', finish); resolve() }; const timer = setTimeout(finish, 2000); controller.signal.addEventListener('abort', finish, { once: true }) })
      }
    }
    void refresh()
    void subscribe()
    return () => { controller.abort(); snapshotSequence.current++; refreshQa.current = () => {} }
  }, [workspaceKey, revision])
  async function execute(operation: QaOperation) {
    if (!snapshot || !workspaceKey || revision === undefined || pending) return false
    const current = generation.current
    const sequence = ++snapshotSequence.current
    setPending(true); setError('')
    try { const data = await executeQa(workspaceKey, revision, snapshot.revision, operation); if (generation.current === current && sequence === snapshotSequence.current) setSnapshot(data); if (generation.current === current) refreshQa.current(); return true }
    catch (e) { if (generation.current === current) { setError(e instanceof Error ? e.message : 'QA 操作失败'); refreshQa.current() }; return false }
    finally { setPending(false) }
  }
  if (!workspaceKey) return <div className="qa-empty">请先打开工作区，再使用质量保证。</div>
  if (!snapshot?.suite) return <div className="qa-empty"><h2>质量保证 · API 场景测试</h2>{(error || loadError) && <p role="alert">{error || loadError}</p>}{snapshot ? <><p>初始化 Extreme 200 环境的公共授权与新账号验收场景。初始化只保存配置，不发送测试请求。</p><button className="ws-primary-button" disabled={pending} onClick={() => void execute({ type: 'seed' })}>初始化 Extreme 200 示例</button></> : <p>正在读取场景…</p>}</div>
  if (!snapshot.suite.scenarios.length) return <div className="qa-empty"><h2>质量保证</h2><p>暂无场景</p>{(error || loadError) && <p role="alert">{error || loadError}</p>}</div>
  return <QaWorkspace key={workspaceKey} snapshot={snapshot} error={error || loadError} pending={pending} execute={execute} />
}

function QaWorkspace({ snapshot, error, pending, execute }: { snapshot: QaSnapshot; error: string; pending: boolean; execute: (operation: QaOperation) => Promise<boolean> }) {
  const dialogs = useDialogs()
  const suite = snapshot.suite!
  const qaScenarios = suite.scenarios
  const qaActors = suite.actors
  const [scenarioId, setScenarioId] = useState('admin-login')
  const scenario = qaScenarios.find(item => item.id === scenarioId) ?? qaScenarios[0]
  const [actionId, setActionId] = useState<string | null>('admin-signin')
  const [expanded, setExpanded] = useState<string | null>('admin-login-call')
  const [tab, setTab] = useState('请求与响应')
  const [copyMessage, setCopyMessage] = useState<ToastMessage | null>(null)
  const toastSequence = useRef(0)
  useEffect(() => {
    if (!copyMessage) return
    const timer = setTimeout(() => setCopyMessage(null), 2200)
    return () => clearTimeout(timer)
  }, [copyMessage])
  const [query, setQuery] = useState('')
  const [collapsed, setCollapsed] = useState<string[]>([])
  const [treeOpen, setTreeOpen] = useState(() => window.matchMedia('(min-width: 601px)').matches)
  const [view, setView] = useState<'both' | 'calls' | 'flow'>('both')
  const [flowPercent, setFlowPercent] = useState(35)
  const [flowHeight, setFlowHeight] = useState(245)
  const [adminPassword, setAdminPassword] = useState('admin')
  const [historyId, setHistoryId] = useState('')
  const history = snapshot.runs.filter(item => item.scenarioId === scenario.id || item.results.some(result => result.scenarioId === scenario.id))
  const run = history.find(item => item.id === historyId) ?? history[0]
  const activeRun = snapshot.runs.find(item => ['running', 'cancelling'].includes(item.status))
  const running = !!run && ['running', 'cancelling'].includes(run.status)
  const plan = qaExecutionPlan(scenario, qaScenarios)
  const prerequisites = plan.filter(item => item.id !== scenario.id)
  const calls = plan.flatMap(item => item.actions.flatMap(action => action.calls))
  const actualDone = run?.results.filter(item => item.status === 'passed' && calls.some(call => call.id === item.callId)).map(item => item.callId) ?? []
  const ownCalls = scenario.actions.flatMap(action => action.calls)
  const prerequisiteCalls = prerequisites.flatMap(item => item.actions.flatMap(action => action.calls))
  const visibleResults = run?.results ?? []
  const done = actualDone
  const [following, setFollowing] = useState(true)
  const [celebration, setCelebration] = useState<{ runId: string; sequence: number } | null>(null)
  const celebrationSequence = useRef(0)
  const pendingCelebration = useRef<{ scenarioId: string; existingIds: Set<string> } | null>(null)
  const currentCall = running ? run?.currentCall : null
  const cursorAction = scenario.actions.find(action => action.calls.some(call => call.id === currentCall))
  const selectedActionId = following && running ? cursorAction?.id ?? null : actionId
  const selected = scenario.actions.find(action => action.id === selectedActionId)
  const failed = run?.results.find(result => result.status === 'failed' && calls.some(call => call.id === result.callId))
  const ownFailure = failed && ownCalls.some(call => call.id === failed.callId)
  const showFailure = !!failed && !running
  const panelsRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const pendingRun = pendingCelebration.current
    if (!pendingRun) return
    const completed = snapshot.runs.find(item => item.scenarioId === pendingRun.scenarioId && !pendingRun.existingIds.has(item.id) && !['running', 'cancelling'].includes(item.status))
    if (!completed) return
    pendingCelebration.current = null
    if (completed.status === 'passed' && scenario.id === completed.scenarioId && !historyId && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) setCelebration({ runId: completed.id, sequence: ++celebrationSequence.current })
  }, [snapshot.runs, scenario.id, historyId])
  useEffect(() => {
    if (!celebration) return
    const timer = setTimeout(() => setCelebration(null), 1200)
    return () => clearTimeout(timer)
  }, [celebration])
  useEffect(() => {
    if (!currentCall || !following) return
    if (cursorAction) setActionId(cursorAction.id)
    const panel = panelsRef.current
    const call = document.getElementById(`qa-call-${currentCall}`)
    const flow = cursorAction && document.getElementById(`qa-flow-${cursorAction.id}`)
    const frame = requestAnimationFrame(() => {
      const behavior = window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth'
      if (call && panel?.querySelector('.qa-calls-panel')) call.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior })
      if (flow && panel?.querySelector('.qa-flow-panel')) flow.scrollIntoView({ block: 'nearest', inline: 'nearest', behavior })
    })
    return () => cancelAnimationFrame(frame)
  }, [currentCall, following, cursorAction?.id, view])
  useEffect(() => {
    if (!showFailure || !ownFailure || !failed || !following || run?.suiteRevision !== snapshot.revision) return
    setExpanded(failed.callId); setTab('断言')
    const action = scenario.actions.find(item => item.calls.some(call => call.id === failed.callId))
    if (action) setActionId(action.id)
    const frame = requestAnimationFrame(() => document.getElementById(`qa-call-${failed.callId}`)?.scrollIntoView({ block: 'nearest', behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' }))
    return () => cancelAnimationFrame(frame)
  }, [showFailure, ownFailure, failed?.callId, run?.id, run?.suiteRevision, snapshot.revision, scenario.id, following])
  function inspectAction(id: string | null) { setFollowing(false); setActionId(id) }
  function setFolderExpanded(id: string, open: boolean) { setCollapsed(items => open ? items.filter(item => item !== id) : items.includes(id) ? items : [...items, id]) }
  function chooseScenario(id: string) { setScenarioId(id); setActionId(null); setExpanded(null); setHistoryId(''); if (window.matchMedia('(max-width: 600px)').matches) setTreeOpen(false) }
  function selectAction(id: string) { inspectAction(id); const call = scenario.actions.find(action => action.id === id)?.calls[0]; if (call) document.getElementById(`qa-call-${call.id}`)?.scrollIntoView({ block: 'nearest', behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' }) }
  async function deleteEntry(kind: 'scenario' | 'module', id: string) {
    if (pending || activeRun) return
    const removed = new Set(qaScenarios.filter(item => kind === 'scenario' ? item.id === id : item.module === id).map(item => item.id))
    let before = -1
    while (before !== removed.size) {
      before = removed.size
      for (const item of qaScenarios) if (item.uses.some(dependency => removed.has(dependency))) removed.add(item.id)
    }
    const affected = qaScenarios.filter(item => removed.has(item.id))
    const runs = snapshot.runs.filter(item => removed.has(item.scenarioId) || item.results.some(result => removed.has(result.scenarioId)))
    const dependent = affected.filter(item => kind === 'scenario' ? item.id !== id : item.module !== id)
    const description = `永久删除${kind === 'module' ? `文件夹「${id}」及其场景` : `场景「${qaScenarios.find(item => item.id === id)?.title ?? id}」`}，共 ${affected.length} 个场景及 ${runs.length} 条相关执行记录。${dependent.length ? `\n级联删除引用它的场景：${dependent.map(item => item.title).join('、')}。` : ''}\n此操作无法撤销，已执行的远端操作不会被撤销。`
    if (!await dialogs.confirm({ title: kind === 'module' ? '删除场景文件夹' : '删除场景', description, tone: 'danger', confirmText: '永久删除' })) return
    const operation: QaOperation = kind === 'module' ? { type: 'delete_module', module: id } : { type: 'delete_scenario', scenarioId: id }
    if (await execute(operation)) { setActionId(null); setExpanded(null); setHistoryId('') }
  }
  async function clearHistory() {
    const runIds = history.map(item => item.id)
    if (!runIds.length || pending || activeRun) return
    if (!await dialogs.confirm({ title: '清除执行记录', description: `永久清除当前列表中的 ${runIds.length} 条执行记录及其请求和响应？其中的引用执行记录也会从所属场景中移除。`, tone: 'danger', confirmText: '清除记录' })) return
    if (await execute({ type: 'clear_history', runIds })) setHistoryId('')
  }
  async function start() { pendingCelebration.current = { scenarioId: scenario.id, existingIds: new Set(snapshot.runs.map(item => item.id)) }; setFollowing(true); setHistoryId(''); if (!await execute({ type: 'run', scenarioId: scenario.id, inputs: { adminPassword } })) pendingCelebration.current = null }
  function detail(call: QaCall) {
    const actor = qaActors[call.actor]
    const result = run?.results.find(item => item.callId === call.id)
    if (tab === '请求与响应') {
      const request = result?.request as { method?: string; url?: string; headers?: Record<string, string>; body?: string | null; live?: boolean } | null | undefined
      const quote = (value: string) => `'${value.replace(/'/g, `'\"'\"'`)}'`
      const curl = request?.method && request.url && request.headers
        ? [`curl -X ${quote(request.method)} ${quote(request.url)}`, ...Object.entries(request.headers).map(([key, value]) => `  -H ${quote(`${key}: ${value}`)}`), ...(request.body == null ? [] : [`  --data-raw ${quote(request.body)}`])].join(' \\\n')
        : ''
      const requestJson = typeof request?.body === 'string' ? formatQaJson(request.body) : null
      const responseText = result ? result.responseRaw ?? JSON.stringify(result.response, null, 2) : ''
      const responseJson = result ? formatQaJson(responseText) : null
      const displayedResponse = responseJson ?? responseText
      const copy = (text: string) => void copyText(text).then(() => setCopyMessage({ id: ++toastSequence.current, text: '已复制', tone: 'success' })).catch(() => setCopyMessage({ id: ++toastSequence.current, text: '复制失败', tone: 'error' }))
      return <div className="qa-exchange">
        <QaExchangeSection title="请求 · cURL" action={curl ? <button type="button" aria-label="复制请求 cURL" onClick={() => copy(curl)}>复制</button> : undefined}>
          {!result ? <p className="qa-detail-empty">执行后显示实际请求</p> : curl ? <>{!request?.live && <p className="qa-detail-empty">历史记录中的敏感字段已脱敏，原始请求不可用</p>}<QaCurl value={curl} /></> : <><p className="qa-detail-empty">此记录没有完整请求，原始请求不可用</p><QaJson value={JSON.stringify(result.request, null, 2) ?? 'null'} /></>}
        </QaExchangeSection>
        {requestJson && <QaExchangeSection title="请求体 · JSON" action={<button type="button" aria-label="复制请求体 JSON" onClick={() => copy(requestJson)}>复制</button>}><QaJson value={requestJson} /></QaExchangeSection>}
        <QaExchangeSection title="响应" action={result && <><span className={result.status === 'passed' ? 'qa-success' : 'qa-error'}>HTTP {result.httpStatus ?? '未收到响应'} · {result.elapsedMs} ms</span><button type="button" aria-label="复制响应" onClick={() => copy(displayedResponse)}>复制</button></>}>
          {result ? <>{result.responseRaw === undefined && <p className="qa-detail-empty">历史响应已脱敏，原始响应不可用</p>}{responseJson === null ? <pre>{responseText}</pre> : <QaJson value={responseJson} />}{result.error && <p className="qa-error">{result.error}</p>}</> : <p className="qa-detail-empty">执行后显示响应</p>}
        </QaExchangeSection>
      </div>
    }
    if (tab === '断言') return <div className="qa-checks">{result ? result.checks.map((check, i) => <div className={`qa-assertion ${check.passed ? '' : 'qa-error'}`} key={i}>{check.passed ? <CheckCircle2 size={15} /> : <XCircle size={15} />}<span>{check.name}</span><small>{check.passed ? '通过' : '失败'}</small></div>) : <><p className="qa-detail-empty">内置断言：HTTP 200、code = 200、success = true</p>{call.assertions.map((check, i) => <code key={i}>{check.path} {check.op} {JSON.stringify(check.value)}</code>)}</>}</div>
    return <div className="qa-variable">{actor.token && <code>授权引用：{'{{'}{actor.token}{'}}'}</code>}{Object.entries(call.extracts).map(([name, pointer]) => <code key={name}>{name} ← {pointer}{run?.variables[name] !== undefined && <small>　{String(run.variables[name])}</small>}</code>)}<small>变量按本轮执行共享，不同账号 token 独立保存；密码和 token 不写入执行记录。</small></div>
  }

  return <main className="qa-page">
    <BaseToast message={copyMessage} />
    <QaSidebar open={treeOpen} onToggle={() => setTreeOpen(value => !value)}>
      <label className="qa-search"><Search size={13} /><input aria-label="搜索场景" placeholder="搜索场景…" value={query} onChange={event => setQuery(event.target.value)} /></label>
      <div className="ws-tree-scroll qa-tree"><ul role="tree" aria-label="场景目录">
        <li role="treeitem" tabIndex={0} aria-label="场景库" aria-expanded={!collapsed.includes('system')} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); setFolderExpanded('system', collapsed.includes('system')) }} onKeyDown={event => handleTreeNavigation(event, { expanded: !collapsed.includes('system'), expandable: true, onToggle: open => setFolderExpanded('system', open), onActivate: () => setFolderExpanded('system', collapsed.includes('system')) })}>
          <FileTreeRow className="ws-tree-row" style={{ paddingLeft: 9 }}><TreeChevron expanded={!collapsed.includes('system')} /><TreeEntryIcon name="场景库" directory expanded={!collapsed.includes('system')} root /><span className="file-tree-label">场景库</span></FileTreeRow>
          {!collapsed.includes('system') && <ul role="group">{[...new Set(qaScenarios.map(item => item.module))].map(module => <li key={module} role="treeitem" tabIndex={0} aria-label={module} aria-expanded={!collapsed.includes(module)} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); setFolderExpanded(module, collapsed.includes(module)) }} onKeyDown={event => handleTreeNavigation(event, { expanded: !collapsed.includes(module), expandable: true, onToggle: open => setFolderExpanded(module, open), onActivate: () => setFolderExpanded(module, collapsed.includes(module)) })}>
            <FileTreeRow className="ws-tree-row" style={{ paddingLeft: 23 }}><TreeChevron expanded={!collapsed.includes(module)} /><TreeEntryIcon name={module} directory expanded={!collapsed.includes(module)} root={false} /><span className="file-tree-label">{module}</span><button type="button" className="qa-tree-delete" aria-label={`删除文件夹 ${module}`} title="删除文件夹" disabled={pending || !!activeRun} onClick={event => { event.stopPropagation(); void deleteEntry('module', module) }}><Trash2 size={14} /></button></FileTreeRow>
            {!collapsed.includes(module) && <ul role="group">{qaScenarios.filter(item => item.module === module && item.title.includes(query)).map(item => <li key={item.id} role="treeitem" tabIndex={0} aria-label={item.title} aria-selected={scenario.id === item.id} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); chooseScenario(item.id) }} onKeyDown={event => handleTreeNavigation(event, { expanded: false, expandable: false, onToggle: () => {}, onActivate: () => chooseScenario(item.id) })}>
              <FileTreeRow className="ws-tree-row" style={{ paddingLeft: 37 }}><TreeChevron expanded={false} expandable={false} /><GitBranch size={16} aria-hidden="true" /><span className="file-tree-label" title={item.title}>{item.title}</span><button type="button" className="qa-tree-delete" aria-label={`删除场景 ${item.title}`} title="删除场景" disabled={pending || !!activeRun} onClick={event => { event.stopPropagation(); void deleteEntry('scenario', item.id) }}><Trash2 size={14} /></button></FileTreeRow>
            </li>)}</ul>}
          </li>)}</ul>}
        </li>
      </ul>{query && !qaScenarios.some(item => item.title.includes(query)) && <p className="qa-tree-empty">没有匹配的场景</p>}</div>
    </QaSidebar>
    <section className="qa-workspace">
      <div className="qa-title-row"><div className="qa-title-copy"><div className="qa-eyebrow">USER JOURNEY / API SCENARIO</div><div className="qa-title-heading"><h1>{scenario.title}</h1><span className={`qa-demo-status ${running ? 'is-running' : run?.status === 'passed' ? 'has-results' : run?.status === 'failed' ? 'is-failed' : ''}`} role="status">{run?.status === 'failed' ? <XCircle size={14} /> : <FlaskConical size={14} />}{run ? `${({ running: '执行中', cancelling: '取消中', passed: '已通过', failed: '失败', cancelled: '已取消', interrupted: '已中断' } as Record<string, string>)[run.status] ?? run.status} ${actualDone.length}/${calls.length}` : '待执行'}{celebration && celebration.runId === run?.id && run?.status === 'passed' && <span key={celebration.sequence} className="qa-confetti" aria-hidden="true">{Array.from({ length: 10 }, (_, i) => <i key={i} style={{ '--angle': `${i * 36}deg`, '--distance': `${25 + (i % 3) * 7}px`, '--piece-color': ['#e6ad42', '#6aaee3', '#d97583', '#66bd94', '#b38ad2'][i % 5] } as CSSProperties} />)}</span>}</span></div><p>{scenario.purpose}</p></div><div className="qa-title-actions"><div className="qa-view-switch" role="group" aria-label="切换场景视图">{([{ id: 'both', label: '联动视图', icon: PanelsTopLeft }, { id: 'calls', label: 'API 调用', icon: ListTree }, { id: 'flow', label: '行为流程', icon: GitBranch }] as const).map(({ id, label, icon: Icon }) => <button key={id} title={label} aria-label={label} aria-pressed={view === id} className={view === id ? 'active' : ''} onClick={() => setView(id)}><Icon size={15} /></button>)}</div><div className="qa-history-select"><BaseSelect aria-label="选择执行记录" value={run?.id ?? ''} onValueChange={setHistoryId} disabled={!history.length} placeholder="尚无执行记录" options={history.map(item => ({ value: item.id, label: `${new Date(item.createdAt * 1000).toLocaleString()} · ${item.status}${item.scenarioId !== scenario.id ? ' · 引用执行' : ''} · ${item.accountName}` }))} /><button type="button" className="qa-history-clear" aria-label="清除当前执行记录" title={activeRun ? '执行期间无法清除记录' : '清除当前执行记录'} disabled={!history.length || !!activeRun || pending} onClick={() => void clearHistory()}><Trash2 size={15} /></button></div></div></div>
      <div className="qa-toolbar"><div className="qa-toolbar-left"><span className="qa-environment" title={suite.baseUrl}><span className="qa-tree-dot" />{suite.name}</span><span className="qa-count">{ownCalls.length} 个接口{prerequisiteCalls.length > 0 && ` + ${prerequisiteCalls.length} 个前置`}</span></div><div className="qa-run-actions"><label className="qa-password">管理员密码<input type="password" autoComplete="off" value={adminPassword} onChange={event => setAdminPassword(event.target.value)} aria-label="管理员密码（仅本轮使用）" /></label>{activeRun ? <button className="qa-run" disabled={pending || activeRun.status === 'cancelling'} onClick={() => void execute({ type: 'cancel', runId: activeRun.id })}><Square size={13} />{activeRun.status === 'cancelling' ? '取消中' : '停止执行'}</button> : <button className="qa-run" disabled={pending || !adminPassword} onClick={() => void start()}><Play size={13} />执行测试</button>}</div></div>
      {error && <div className="qa-error qa-notice" role="alert">{error}</div>}
      {(run?.error || run?.results.find(item => item.status === 'failed')?.error) && <div className="qa-error qa-notice" role="alert">{run?.error ?? run?.results.find(item => item.status === 'failed')?.error}</div>}
      {run && run.suiteRevision !== snapshot.revision && <div className="qa-notice">该记录使用配置版本 {run.suiteRevision}，当前为 {snapshot.revision}；请求和响应以历史记录为准。</div>}
      {prerequisites.length > 0 && <div className="qa-references"><span>引用前置</span>{prerequisites.map(item => { const items = item.actions.flatMap(action => action.calls); const passed = items.filter(call => done.includes(call.id)).length; return <button key={item.id} className={`${item.actions.some(action => action.calls.some(call => call.id === currentCall)) ? 'is-running' : ''} ${showFailure && item.actions.some(action => action.calls.some(call => call.id === failed?.callId)) ? 'is-failed' : ''}`} onClick={() => chooseScenario(item.id)} title={`查看公共场景：${item.title}`} ><GitBranch size={13} />{item.title}<small className={passed === items.length ? 'qa-success' : ''}>{passed}/{items.length}</small><ArrowRight size={12} /></button> })}</div>}
      {showFailure && !ownFailure && failed && <details className="qa-prerequisite-failure" open><summary>公共前置失败：{failed.name}</summary><p>{failed.error ?? '请求未满足预期，后续步骤未执行'}</p>{failed.checks.map((check, index) => <div key={index} className={check.passed ? 'qa-success' : 'qa-error'}>{check.passed ? '✓' : '✕'} {check.name}</div>)}<small>点击上方对应公共场景可查看完整请求和响应。</small></details>}
      <div ref={panelsRef} onWheelCapture={() => setFollowing(false)} onTouchMoveCapture={() => setFollowing(false)} onPointerDownCapture={() => setFollowing(false)} onKeyDownCapture={event => { if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown', 'Home', 'End', ' '].includes(event.key)) setFollowing(false) }} className={`qa-panels qa-view-${view}`} style={{ '--qa-flow-width': `${flowPercent}%`, '--qa-flow-height': `${flowHeight}px` } as CSSProperties}>
        {view !== 'flow' && <section className="qa-calls-panel" aria-label="API 调用链"><div className="qa-panel-heading"><span><ListTree size={15} />API 调用链</span><small>按用户操作顺序执行</small></div><div className="qa-calls-scroll">
          {ownCalls.some(call => Object.keys(call.extracts).length) && <div className="qa-outputs"><span>场景输出</span>{ownCalls.filter(call => Object.keys(call.extracts).length).map(call => <code key={call.id} className={done.includes(call.id) ? 'qa-success' : ''} title={Object.entries(call.extracts).map(([name, pointer]) => `${name} ← ${pointer}`).join(', ')}>{done.includes(call.id) && <Check size={12} />}{Object.keys(call.extracts).join(', ')}</code>)}</div>}
          {scenario.actions.map((action, index) => <div className={`qa-call-group ${selectedActionId === action.id ? 'is-selected' : ''}`} key={action.id}>
            <button className="qa-group-heading" onClick={() => inspectAction(selectedActionId === action.id ? null : action.id)}><span className="qa-action-number">{String(index + 1).padStart(2, '0')}</span><strong>{action.title}</strong><span className={`qa-actor qa-actor-${action.calls[0].actor}`}>{qaActors[action.calls[0].actor].label}</span><small>{action.calls.length} API</small></button>
            {action.calls.map(call => <article id={`qa-call-${call.id}`} key={call.id} className={`qa-call ${expanded === call.id ? 'is-open' : ''} ${currentCall === call.id ? 'is-executing' : ''} ${visibleResults.some(item => item.callId === call.id && item.status === 'failed') ? 'is-failed' : ''}`} aria-current={currentCall === call.id ? 'step' : undefined}><button className="qa-call-summary" aria-expanded={expanded === call.id} onClick={() => { setExpanded(expanded === call.id ? null : call.id); inspectAction(action.id) }}>{expanded === call.id ? <ChevronDown size={13} /> : <ChevronRight size={13} />}<span className={`qa-method qa-method-${call.method.toLowerCase()}`}>{call.method}</span><span className="qa-call-name">{call.name}<code title={call.path}>{call.path}</code></span>{running && currentCall === call.id ? <LoaderCircle size={14} className="qa-execution-spinner" aria-label="正在请求" /> : done.includes(call.id) ? <span className="qa-call-result"><Check size={13} />{run?.results.find(item => item.callId === call.id)?.elapsedMs} ms</span> : visibleResults.some(item => item.callId === call.id && item.status === 'failed') ? <XCircle size={14} className="qa-error" /> : <Circle size={12} className="qa-pending" />}</button>{expanded === call.id && <div className="qa-call-detail"><div className="qa-detail-tabs">{['请求与响应', '断言', '变量'].map(label => <button className={tab === label ? 'active' : ''} onClick={() => setTab(label)} key={label}>{label}{label === '断言' ? ` · ${3 + call.assertions.length}` : ''}</button>)}</div>{detail(call)}</div>}</article>)}
          </div>)}
          <div className="qa-chain-end"><CheckCircle2 size={14} />{run?.status === 'passed' ? '场景执行完成，全部断言通过' : '调用链结束 · 验证预期结果'}</div>
        </div></section>}
        {view === 'both' && <QaPanelDivider flowPercent={flowPercent} flowHeight={flowHeight} onChange={(value, stacked) => stacked ? setFlowHeight(value) : setFlowPercent(value)} />}
        {view !== 'calls' && <section className="qa-flow-panel" aria-label="用户行为流程"><div className="qa-panel-heading"><span><GitBranch size={15} />用户行为流程</span><button onClick={() => inspectAction(null)}>取消选择</button></div><div className="qa-flow-scroll"><div className="qa-flow-start"><span />{prerequisites.length > 0 ? '引用通用授权 · 准备身份' : '开始 · 无需登录'}</div>{scenario.actions.map((action, index) => <div id={`qa-flow-${action.id}`} className="qa-flow-step" key={action.id}><div className={`qa-flow-edge ${selectedActionId === action.id ? 'active' : ''}`}><ArrowDown size={13} /></div><button className={`qa-flow-node ${selectedActionId === action.id ? 'selected' : ''} ${cursorAction?.id === action.id ? 'is-executing' : ''} ${showFailure && action.calls.some(call => call.id === failed?.callId) ? 'is-failed' : ''} ${action.calls.every(call => done.includes(call.id)) ? 'complete' : ''}`} aria-pressed={selectedActionId === action.id} onClick={() => selectAction(action.id)}><div className="qa-flow-node-title"><span>{String(index + 1).padStart(2, '0')}</span><strong>{action.title}</strong>{action.calls.every(call => done.includes(call.id)) && <CheckCircle2 size={14} />}</div><p>{action.description}</p><div className="qa-flow-node-footer"><span className={`qa-actor qa-actor-${action.calls[0].actor}`}>{qaActors[action.calls[0].actor].label}</span><span>{action.calls.length} API<ArrowRight size={12} /></span></div></button></div>)}<div className="qa-flow-edge"><ArrowDown size={13} /></div><div className="qa-flow-end"><Check size={12} />场景结束</div></div><div className="qa-flow-selection"><span className="qa-selection-dot" /><span>{selected ? `已选择「${selected.title}」· 关联 ${selected.calls.length} 个接口` : '选择一个行为，查看它触发的接口组'}</span></div></section>}
      </div>
    </section>
  </main>
}

function QaExchangeSection({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(true)
  const id = useId()
  return <section><div className="qa-exchange-heading"><button type="button" className="qa-exchange-toggle" aria-expanded={open} aria-controls={id} onClick={() => setOpen(value => !value)}>{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<strong>{title}</strong></button><div className="qa-exchange-actions">{action}</div></div><div id={id} hidden={!open}>{children}</div></section>
}
