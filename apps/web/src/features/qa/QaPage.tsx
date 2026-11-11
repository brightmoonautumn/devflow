import { useEffect, useId, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { ArrowDown, ArrowRight, Check, CheckCircle2, ChevronDown, ChevronRight, ChevronsDownUp, ChevronsUpDown, Circle, FlaskConical, LoaderCircle, GitBranch, ListTree, PanelsTopLeft, Play, Search, Square, Trash2, Eraser, XCircle } from 'lucide-react'
import { FileTreeRow, TreeChevron, handleTreeNavigation } from '../../components/files/FileTreeRow'
import { TreeEntryIcon } from '../../components/files/TreeEntryIcon'
import { executeQa, getQa, qaEnvironments, qaExecutionPlan, type QaCall, type QaOperation, type QaSnapshot, type QaSseOptions } from './qaApi'
import { QaEnvironmentControl, QaResponseSettings, readQaEnvironmentVariables } from './QaSettings'
import { getToken, type WorkspaceResponse } from '../../lib/api'
import { QaPanelDivider } from './QaPanelDivider'
import { QaHeaderDivider } from './QaHeaderDivider'
import { clearQaAuthToken, getQaAuthToken, qaAuthCacheKey, saveQaAuthToken } from './authCache'
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
  const [emptyEnvironmentId, setEmptyEnvironmentId] = useState('')
  const workspaceKey = workspace?.workspaceFile ?? workspace?.workspaceRoot
  const revision = workspace?.revision
  const generation = useRef(0)
  const snapshotSequence = useRef(0)
  const refreshQa = useRef<() => void>(() => {})
  useEffect(() => {
    const current = ++generation.current
    setSnapshot(null); setError(''); setLoadError(''); setEmptyEnvironmentId('')
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
  if (!snapshot.suite.scenarios.length) {
    const suite = snapshot.suite
    const environments = qaEnvironments(suite)
    return <div className="qa-empty"><h2>质量保证</h2><p>暂无场景</p>
      <QaEnvironmentControl environments={environments} value={environments.find(item => item.id === emptyEnvironmentId)?.id ?? environments[0].id}
        disabled={pending} workspaceKey={snapshot.workspaceKey} onChange={setEmptyEnvironmentId}
        onSave={environments => execute({ type: 'save', suite: { ...suite, environments } })} />
      {(error || loadError) && <p role="alert">{error || loadError}</p>}</div>
  }
  return <QaWorkspace key={workspaceKey} snapshot={snapshot} error={error || loadError} pending={pending} execute={execute} />
}

function QaWorkspace({ snapshot, error, pending, execute }: { snapshot: QaSnapshot; error: string; pending: boolean; execute: (operation: QaOperation) => Promise<boolean> }) {
  const dialogs = useDialogs()
  const suite = snapshot.suite!
  const environments = qaEnvironments(suite)
  const [environmentId, setEnvironmentId] = useState('')
  const environment = environments.find(item => item.id === environmentId) ?? environments[0]
  const [responseCall, setResponseCall] = useState<QaCall | null>(null)
  const qaScenarios = suite.scenarios
  const qaActors = suite.actors
  const [scenarioId, setScenarioId] = useState('admin-login')
  const scenario = qaScenarios.find(item => item.id === scenarioId) ?? qaScenarios[0]
  const actions = [...scenario.actions.filter(action => !action.cleanup), ...scenario.actions.filter(action => action.cleanup)]
  const [actionId, setActionId] = useState<string | null>('admin-signin')
  const [expanded, setExpanded] = useState<string[]>(['admin-login-call'])
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
  const [headerHeight, setHeaderHeight] = useState<number | null>(null)
  const [environmentVariables, setEnvironmentVariables] = useState(() => readQaEnvironmentVariables(snapshot.workspaceKey))
  const inputsForEnvironment = environmentVariables[environment.id] ?? {}
  const adminPassword = inputsForEnvironment.adminPassword || 'admin'
  const [historyId, setHistoryId] = useState('')
  const history = snapshot.runs.filter(item => item.scenarioId === scenario.id || item.results.some(result => result.scenarioId === scenario.id))
  const run = history.find(item => item.id === historyId) ?? history[0]
  const activeRun = snapshot.runs.find(item => ['running', 'cancelling'].includes(item.status))
  const running = !!run && ['running', 'cancelling'].includes(run.status)
  const plan = qaExecutionPlan(scenario, qaScenarios)
  const prerequisites = plan.filter(item => item.id !== scenario.id)
  const calls = plan.flatMap(item => item.actions.flatMap(action => action.calls))
  const login = calls.find(call => call.id === 'admin-login-call')
  const needsAdminPassword = !!login || calls.some(call => JSON.stringify(call.body)?.includes('{{adminPassword}}'))
  const extracted = new Set(calls.flatMap(call => Object.keys(call.extracts)))
  const externalTokens = [...new Set(calls.map(call => qaActors[call.actor]?.token).filter((token): token is string =>
    Boolean(token && !extracted.has(token) && suite.variables[token] === undefined)))]
  const missingVariables = [...(needsAdminPassword && !adminPassword ? ['adminPassword'] : []), ...externalTokens.filter(token => !inputsForEnvironment[token]?.trim())]
  const actualDone = run?.results.filter(item => item.status === 'passed' && calls.some(call => call.id === item.callId)).map(item => item.callId) ?? []
  const ownCalls = scenario.actions.flatMap(action => action.calls)
  const visibleResults = run?.results ?? []
  const done = actualDone
  const [following, setFollowing] = useState(true)
  const [celebration, setCelebration] = useState<{ runId: string } | null>(null)
  const preparingRun = useRef(false)
  const pendingAuth = useRef<{ key: string; existingIds: Set<string> } | null>(null)
  const pendingCelebration = useRef<{ scenarioId: string; existingIds: Set<string> } | null>(null)
  const currentCall = running ? run?.currentCall : null
  const cursorAction = scenario.actions.find(action => action.calls.some(call => call.id === currentCall))
  const selectedActionId = following && running ? cursorAction?.id ?? null : actionId
  const failed = run?.results.find(result => result.status === 'failed' && calls.some(call => call.id === result.callId))
  const ownFailure = failed && ownCalls.some(call => call.id === failed.callId)
  const showFailure = !!failed && !running
  const panelsRef = useRef<HTMLDivElement>(null)
  useEffect(() => {
    const pending = pendingAuth.current
    if (!pending) return
    const current = snapshot.runs.find(item => !pending.existingIds.has(item.id))
    if (!current) return
    const login = current.results.find(item => item.callId === 'admin-login-call' && item.status === 'passed')
    if (login?.responseRaw) {
      try {
        const response = JSON.parse(login.responseRaw) as { data?: unknown }
        if (typeof response.data === 'string' && response.data && response.data !== '[REDACTED]') saveQaAuthToken(pending.key, response.data)
      } catch { /* Raw login response may be unavailable after restarting the backend. */ }
    }
    if (current.results.some(item => (item.callId === 'admin-login-call' || qaActors[item.actor]?.token === 'adminToken')
      && (item.httpStatus === 401 || String((item.response as { code?: unknown } | null)?.code) === '401'))) clearQaAuthToken(pending.key)
    if (!['running', 'cancelling'].includes(current.status)) pendingAuth.current = null
  }, [snapshot.runs, qaActors])

  useEffect(() => {
    const pendingRun = pendingCelebration.current
    if (!pendingRun) return
    const completed = snapshot.runs.find(item => item.scenarioId === pendingRun.scenarioId && !pendingRun.existingIds.has(item.id) && !['running', 'cancelling'].includes(item.status))
    if (!completed) return
    pendingCelebration.current = null
    if (completed.status === 'passed' && scenario.id === completed.scenarioId && !historyId && !window.matchMedia('(prefers-reduced-motion: reduce)').matches) setCelebration({ runId: completed.id })
  }, [snapshot.runs, scenario.id, historyId])
  useEffect(() => {
    if (!celebration) return
    const timer = setTimeout(() => setCelebration(null), 1200)
    return () => clearTimeout(timer)
  }, [celebration])
  useEffect(() => {
    if (!currentCall || !following) return
    if (cursorAction) setActionId(cursorAction.id)
    if (cursorAction?.calls.some(call => call.id === currentCall && call.sse)) {
      setExpanded(items => items.includes(currentCall) ? items : [...items, currentCall])
      setTab('请求与响应')
    }
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
    setExpanded(items => items.includes(failed.callId) ? items : [...items, failed.callId]); setTab('断言')
    const action = scenario.actions.find(item => item.calls.some(call => call.id === failed.callId))
    if (action) setActionId(action.id)
    const frame = requestAnimationFrame(() => document.getElementById(`qa-call-${failed.callId}`)?.scrollIntoView({ block: 'nearest', behavior: window.matchMedia('(prefers-reduced-motion: reduce)').matches ? 'auto' : 'smooth' }))
    return () => cancelAnimationFrame(frame)
  }, [showFailure, ownFailure, failed?.callId, run?.id, run?.suiteRevision, snapshot.revision, scenario.id, following])
  function inspectAction(id: string | null) { setFollowing(false); setActionId(id) }
  function setFolderExpanded(id: string, open: boolean) { setCollapsed(items => open ? items.filter(item => item !== id) : items.includes(id) ? items : [...items, id]) }
  function chooseScenario(id: string) { setCelebration(null); setScenarioId(id); setActionId(null); setExpanded([]); setHistoryId(''); if (window.matchMedia('(max-width: 600px)').matches) setTreeOpen(false) }
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
    if (await execute(operation)) { setActionId(null); setExpanded([]); setHistoryId('') }
  }
  async function clearHistory() {
    const runIds = history.map(item => item.id)
    if (!runIds.length || pending || activeRun) return
    if (!await dialogs.confirm({ title: '清除执行记录', description: `永久清除当前列表中的 ${runIds.length} 条执行记录及其请求和响应？其中的引用执行记录也会从所属场景中移除。`, tone: 'danger', confirmText: '清除记录' })) return
    if (await execute({ type: 'clear_history', runIds })) setHistoryId('')
  }
  async function clearScopeHistory(module?: string) {
    const scenarioIds = new Set(qaScenarios.filter(item => item.module === module).map(item => item.id))
    const runIds = snapshot.runs.filter(item => !module || scenarioIds.has(item.scenarioId)).map(item => item.id)
    if (!runIds.length || pending || activeRun) return
    const scope = module ? `文件夹「${module}」` : '所有场景'
    if (!await dialogs.confirm({ title: '清空执行记录', description: `永久清空${scope}的 ${runIds.length} 条执行记录及其请求和响应？引用这些记录的场景也将无法查看它们。此操作无法撤销。`, tone: 'danger', confirmText: '清空记录' })) return
    if (await execute({ type: 'clear_history', runIds })) setHistoryId('')
  }
  async function start() {
    if (pending || activeRun || preparingRun.current) return
    preparingRun.current = true
    try {
      setCelebration(null)
      const existingIds = new Set(snapshot.runs.map(item => item.id))
      pendingCelebration.current = { scenarioId: scenario.id, existingIds }
      setFollowing(true); setHistoryId('')
      const inputs: Record<string, string> = { ...inputsForEnvironment }
      if (needsAdminPassword) inputs.adminPassword = adminPassword
      if (login) {
        const key = await qaAuthCacheKey([snapshot.workspaceKey, environment.baseUrl, suite.publicKey, login, qaActors, adminPassword]).catch(() => null)
        if (key) {
          pendingAuth.current = { key, existingIds }
          const token = getQaAuthToken(key)
          if (token) inputs.adminToken = token
        }
      }
      if (!await execute({ type: 'run', scenarioId: scenario.id, environmentId: environment.id, inputs })) {
        pendingCelebration.current = null; pendingAuth.current = null
      }
    } finally { preparingRun.current = false }
  }

  function saveResponse(sse?: QaSseOptions) {
    return execute({
      type: 'save',
      suite: {
        ...suite,
        scenarios: suite.scenarios.map(item => item.id !== scenario.id ? item : {
          ...item,
          actions: item.actions.map(action => ({
            ...action,
            calls: action.calls.map(call => call.id === responseCall?.id ? { ...call, sse } : call),
          })),
        }),
      },
    })
  }

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
        <div className="qa-response-settings">
          <span>{call.sse ? `SSE · ${call.sse.endEvent}` : '普通 JSON'}</span>
          <button type="button" disabled={pending || !!activeRun} onClick={() => setResponseCall(call)}>响应设置</button>
        </div>
        <QaExchangeSection title="请求 · cURL" action={curl ? <button type="button" aria-label="复制请求 cURL" onClick={() => copy(curl)}>复制</button> : undefined}>
          {!result ? <p className="qa-detail-empty">执行后显示实际请求</p> : curl ? <>{!request?.live && <p className="qa-detail-empty">历史记录中的敏感字段已脱敏，原始请求不可用</p>}<QaCurl value={curl} /></> : <><p className="qa-detail-empty">此记录没有完整请求，原始请求不可用</p><QaJson value={JSON.stringify(result.request, null, 2) ?? 'null'} /></>}
        </QaExchangeSection>
        {requestJson && <QaExchangeSection title="请求体 · JSON" action={<button type="button" aria-label="复制请求体 JSON" onClick={() => copy(requestJson)}>复制</button>}><QaJson value={requestJson} /></QaExchangeSection>}
        {!!result?.streamEvents?.length && <QaExchangeSection title="SSE 事件">
          <QaJson value={JSON.stringify(result.streamEvents, null, 2)} />
        </QaExchangeSection>}
        <QaExchangeSection title={call.sse ? '结束事件数据' : '响应'} action={result && result.status !== 'streaming' && <><span className={result.status === 'passed' ? 'qa-success' : result.status === 'skipped' ? 'qa-pending' : 'qa-error'}>{result.status === 'skipped' ? '已跳过' : `HTTP ${result.httpStatus ?? '未收到响应'} · ${result.elapsedMs} ms`}</span><button type="button" aria-label="复制响应" onClick={() => copy(displayedResponse)}>复制</button></>}>
          {result?.status === 'streaming' ? <p className="qa-detail-empty" role="status">等待结束事件</p> :
            result ? <>{result.responseRaw === undefined && !call.sse && <p className="qa-detail-empty">历史响应已脱敏，原始响应不可用</p>}{responseJson === null ? <pre>{responseText}</pre> : <QaJson value={responseJson} />}{result.error && <p className="qa-error">{result.error}</p>}</> :
            <p className="qa-detail-empty">执行后显示响应</p>}
        </QaExchangeSection>
      </div>
    }
    if (tab === '断言') return <div className="qa-checks">{result ? result.checks.map((check, i) => <div className={`qa-assertion ${check.passed ? '' : 'qa-error'}`} key={i}>{check.passed ? <CheckCircle2 size={15} /> : <XCircle size={15} />}<span>{check.name}</span><small>{check.passed ? '通过' : '失败'}</small></div>) : <><p className="qa-detail-empty">内置断言：HTTP 200、code = 200、success = true</p>{call.assertions.map((check, i) => <code key={i}>{check.path} {check.op} {JSON.stringify(check.value)}</code>)}</>}</div>
    return <div className="qa-variable">{actor.token && <code>授权引用：{'{{'}{actor.token}{'}}'}</code>}{Object.entries(call.extracts).map(([name, pointer]) => <code key={name}>{name} ← {pointer}{run?.variables[name] !== undefined && <small>　{String(run.variables[name])}</small>}</code>)}<small>变量按本轮执行共享，不同账号 token 独立保存；密码和 token 不写入执行记录。</small></div>
  }

  const viewSwitch = <div className="qa-view-switch" role="group" aria-label="切换场景视图">{([{ id: 'both', label: '联动视图', icon: PanelsTopLeft }, { id: 'calls', label: 'API 调用', icon: ListTree }, { id: 'flow', label: '行为流程', icon: GitBranch }] as const).map(({ id, label, icon: Icon }) => <button key={id} title={label} aria-label={label} aria-pressed={view === id} className={view === id ? 'active' : ''} onClick={() => setView(id)}><Icon size={15} /></button>)}</div>

  return <main className="qa-page">
    <BaseToast message={copyMessage} />
    {responseCall && <QaResponseSettings initial={responseCall.sse} onSave={saveResponse} onClose={() => setResponseCall(null)} />}
    <QaSidebar open={treeOpen} onToggle={() => setTreeOpen(value => !value)}>
      <label className="qa-search"><Search size={13} /><input aria-label="搜索场景" placeholder="搜索场景…" value={query} onChange={event => setQuery(event.target.value)} /></label>
      <div className="ws-tree-scroll qa-tree"><ul role="tree" aria-label="场景目录">
        <li role="treeitem" tabIndex={0} aria-label="场景库" aria-expanded={!collapsed.includes('system')} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); setFolderExpanded('system', collapsed.includes('system')) }} onKeyDown={event => handleTreeNavigation(event, { expanded: !collapsed.includes('system'), expandable: true, onToggle: open => setFolderExpanded('system', open), onActivate: () => setFolderExpanded('system', collapsed.includes('system')) })}>
          <FileTreeRow className="ws-tree-row" style={{ paddingLeft: 9 }}><TreeChevron expanded={!collapsed.includes('system')} /><TreeEntryIcon name="场景库" directory expanded={!collapsed.includes('system')} root /><span className="file-tree-label">场景库</span><button type="button" className="qa-tree-clear" aria-label="清空所有执行记录" title="清空所有执行记录" disabled={pending || !!activeRun || !snapshot.runs.length} onClick={event => { event.stopPropagation(); void clearScopeHistory() }}><Eraser size={14} /></button></FileTreeRow>
          {!collapsed.includes('system') && <ul role="group">{[...new Set(qaScenarios.map(item => item.module))].map(module => <li key={module} role="treeitem" tabIndex={0} aria-label={module} aria-expanded={!collapsed.includes(module)} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); setFolderExpanded(module, collapsed.includes(module)) }} onKeyDown={event => handleTreeNavigation(event, { expanded: !collapsed.includes(module), expandable: true, onToggle: open => setFolderExpanded(module, open), onActivate: () => setFolderExpanded(module, collapsed.includes(module)) })}>
            <FileTreeRow className="ws-tree-row" style={{ paddingLeft: 23 }}><TreeChevron expanded={!collapsed.includes(module)} /><TreeEntryIcon name={module} directory expanded={!collapsed.includes(module)} root={false} /><span className="file-tree-label">{module}</span><button type="button" className="qa-tree-clear" aria-label={`清空文件夹 ${module} 的执行记录`} title="清空文件夹执行记录" disabled={pending || !!activeRun || !snapshot.runs.some(run => qaScenarios.some(item => item.module === module && item.id === run.scenarioId))} onClick={event => { event.stopPropagation(); void clearScopeHistory(module) }}><Eraser size={14} /></button><button type="button" className="qa-tree-delete" aria-label={`删除文件夹 ${module}`} title="删除文件夹" disabled={pending || !!activeRun} onClick={event => { event.stopPropagation(); void deleteEntry('module', module) }}><Trash2 size={14} /></button></FileTreeRow>
            {!collapsed.includes(module) && <ul role="group">{qaScenarios.filter(item => item.module === module && item.title.includes(query)).map(item => <li key={item.id} role="treeitem" tabIndex={0} aria-label={item.title} aria-selected={scenario.id === item.id} onClick={event => { event.stopPropagation(); event.currentTarget.focus(); chooseScenario(item.id) }} onKeyDown={event => handleTreeNavigation(event, { expanded: false, expandable: false, onToggle: () => {}, onActivate: () => chooseScenario(item.id) })}>
              <FileTreeRow className="ws-tree-row" style={{ paddingLeft: 37 }}><TreeChevron expanded={false} expandable={false} /><GitBranch size={16} aria-hidden="true" /><span className="file-tree-label" title={item.title}>{item.title}</span><button type="button" className="qa-tree-delete" aria-label={`删除场景 ${item.title}`} title="删除场景" disabled={pending || !!activeRun} onClick={event => { event.stopPropagation(); void deleteEntry('scenario', item.id) }}><Trash2 size={14} /></button></FileTreeRow>
            </li>)}</ul>}
          </li>)}</ul>}
        </li>
      </ul>{query && !qaScenarios.some(item => item.title.includes(query)) && <p className="qa-tree-empty">没有匹配的场景</p>}</div>
    </QaSidebar>
    <section className="qa-workspace">
      <div className="qa-header" style={headerHeight === null ? undefined : { height: headerHeight }}>
      <div className="qa-title-row"><div className={`qa-title-copy ${run?.status === 'passed' ? 'has-stamp' : ''}`}><div className="qa-eyebrow">USER JOURNEY / API SCENARIO</div><div className="qa-title-heading"><h1>{scenario.title}</h1><span className={`qa-demo-status ${running ? 'is-running' : run?.status === 'passed' ? 'has-results' : run?.status === 'failed' ? 'is-failed' : ''}`} role="status">{run?.status === 'failed' ? <XCircle size={14} /> : <FlaskConical size={14} />}{run ? `${({ running: '执行中', cancelling: '取消中', passed: '已通过', failed: '失败', cancelled: '已取消', interrupted: '已中断' } as Record<string, string>)[run.status] ?? run.status} ${actualDone.length}/${calls.length}` : '待执行'}</span></div><p>{scenario.purpose}</p>{run?.status === 'passed' && <QaSuccessStamp key={run.id} animate={celebration?.runId === run.id} />}</div><div className="qa-title-actions"><QaEnvironmentControl environments={environments} value={environment.id} disabled={pending || !!activeRun} workspaceKey={snapshot.workspaceKey} requiredVariables={[...(needsAdminPassword ? ['adminPassword'] : []), ...externalTokens]} onChange={setEnvironmentId} onVariablesChange={setEnvironmentVariables} onSave={items => execute({ type: 'save', suite: { ...suite, environments: items } })} /><div className="qa-history-select"><BaseSelect aria-label="选择执行记录" value={run?.id ?? ''} onValueChange={id => { setCelebration(null); setHistoryId(id) }} disabled={!history.length} placeholder="尚无执行记录" options={history.map(item => ({ value: item.id, label: `${new Date(item.createdAt * 1000).toLocaleString()} · ${item.status}${item.scenarioId !== scenario.id ? ' · 引用执行' : ''} · ${item.accountName}` }))} /><button type="button" className="qa-history-clear" aria-label="清除当前执行记录" title={activeRun ? '执行期间无法清除记录' : '清除当前执行记录'} disabled={!history.length || !!activeRun || pending} onClick={() => void clearHistory()}><Trash2 size={15} /></button></div></div></div>
      <div className="qa-toolbar">
        <div className="qa-run-actions">
          {!activeRun && missingVariables.length > 0 && <span className="qa-missing-variables" role="status" title={`请在环境管理中配置：${missingVariables.join('、')}`}>缺少环境变量：{missingVariables.join('、')}</span>}
          {activeRun ? <button className="qa-run" disabled={pending || activeRun.status === 'cancelling'} onClick={() => void execute({ type: 'cancel', runId: activeRun.id })}><Square size={13} />{activeRun.status === 'cancelling' ? '取消中' : '停止执行'}</button> :
            <button className="qa-run" title={missingVariables.length ? `请在环境管理中配置：${missingVariables.join('、')}` : undefined} disabled={pending || missingVariables.length > 0} onClick={() => void start()}><Play size={13} />执行测试</button>}
        </div>
      </div>
      {error && <div className="qa-error qa-notice" role="alert">{error}</div>}
      {(run?.error || run?.results.find(item => item.status === 'failed')?.error) && <div className="qa-error qa-notice" role="alert">{run?.error ?? run?.results.find(item => item.status === 'failed')?.error}</div>}
      {run && run.suiteRevision !== snapshot.revision && <div className="qa-notice">该记录使用配置版本 {run.suiteRevision}，当前为 {snapshot.revision}；请求和响应以历史记录为准。</div>}
      {prerequisites.length > 0 && <div className="qa-references"><span>引用前置</span>{prerequisites.map(item => { const items = item.actions.flatMap(action => action.calls); const passed = items.filter(call => done.includes(call.id)).length; return <button key={item.id} className={`${item.actions.some(action => action.calls.some(call => call.id === currentCall)) ? 'is-running' : ''} ${showFailure && item.actions.some(action => action.calls.some(call => call.id === failed?.callId)) ? 'is-failed' : ''}`} onClick={() => chooseScenario(item.id)} title={`查看公共场景：${item.title}`} ><GitBranch size={13} />{item.title}<small className={passed === items.length ? 'qa-success' : ''}>{passed}/{items.length}</small><ArrowRight size={12} /></button> })}</div>}
      {showFailure && !ownFailure && failed && <details className="qa-prerequisite-failure" open><summary>公共前置失败：{failed.name}</summary><p>{failed.error ?? '请求未满足预期，后续步骤未执行'}</p>{failed.checks.map((check, index) => <div key={index} className={check.passed ? 'qa-success' : 'qa-error'}>{check.passed ? '✓' : '✕'} {check.name}</div>)}<small>点击上方对应公共场景可查看完整请求和响应。</small></details>}
      </div>
      <QaHeaderDivider height={headerHeight} onChange={setHeaderHeight} />
      <div ref={panelsRef} onWheelCapture={() => setFollowing(false)} onTouchMoveCapture={() => setFollowing(false)} onPointerDownCapture={() => setFollowing(false)} onKeyDownCapture={event => { if (['ArrowUp', 'ArrowDown', 'ArrowLeft', 'ArrowRight', 'PageUp', 'PageDown', 'Home', 'End', ' '].includes(event.key)) setFollowing(false) }} className={`qa-panels qa-view-${view}`} style={{ '--qa-flow-width': `${flowPercent}%`, '--qa-flow-height': `${flowHeight}px` } as CSSProperties}>
        {view !== 'flow' && <section className="qa-calls-panel" aria-label="API 调用链"><div className="qa-panel-heading"><span><ListTree size={15} />API 调用链</span><div className="qa-panel-actions">{viewSwitch}<div className="qa-view-switch" role="group" aria-label="展开或收起接口"><button type="button" title="展开全部" aria-label="展开全部" disabled={ownCalls.every(call => expanded.includes(call.id))} onClick={() => { setFollowing(false); setExpanded(ownCalls.map(call => call.id)) }}><ChevronsUpDown size={15} /></button><button type="button" title="收起全部" aria-label="收起全部" disabled={!ownCalls.some(call => expanded.includes(call.id))} onClick={() => { setFollowing(false); setExpanded([]) }}><ChevronsDownUp size={15} /></button></div></div></div><div className="qa-calls-scroll">
          {actions.map((action, index) => <div className={`qa-call-group ${selectedActionId === action.id ? 'is-selected' : ''}`} key={action.id}>
            <button className="qa-group-heading" onClick={() => inspectAction(selectedActionId === action.id ? null : action.id)}><span className="qa-action-number">{String(index + 1).padStart(2, '0')}</span><strong title={action.cleanup ? "成功、失败或取消后执行清理" : undefined}>{action.title}{action.cleanup && " · 清理"}</strong><span className={`qa-actor qa-actor-${action.calls[0].actor}`}>{qaActors[action.calls[0].actor].label}</span><small>{action.calls.length} API</small></button>
            {action.calls.map(call => <article id={`qa-call-${call.id}`} key={call.id} className={`qa-call ${expanded.includes(call.id) ? 'is-open' : ''} ${currentCall === call.id ? 'is-executing' : ''} ${visibleResults.some(item => item.callId === call.id && item.status === 'failed') ? 'is-failed' : ''}`} aria-current={currentCall === call.id ? 'step' : undefined}><button className="qa-call-summary" aria-expanded={expanded.includes(call.id)} onClick={() => { setExpanded(items => items.includes(call.id) ? items.filter(id => id !== call.id) : [...items, call.id]); inspectAction(action.id) }}>{expanded.includes(call.id) ? <ChevronDown size={13} /> : <ChevronRight size={13} />}<span className={`qa-method qa-method-${call.method.toLowerCase()}`}>{call.method}</span><span className="qa-call-name">{call.name}<code title={call.path}>{call.path}</code></span>{running && currentCall === call.id ? <LoaderCircle size={14} className="qa-execution-spinner" aria-label="正在请求" /> : done.includes(call.id) ? <span className="qa-call-result"><Check size={13} />{run?.results.find(item => item.callId === call.id)?.elapsedMs} ms</span> : visibleResults.some(item => item.callId === call.id && item.status === 'failed') ? <XCircle size={14} className="qa-error" /> : visibleResults.some(item => item.callId === call.id && item.status === 'skipped') ? <span className="qa-pending">已跳过</span> : <Circle size={12} className="qa-pending" />}</button>{expanded.includes(call.id) && <div className="qa-call-detail"><div className="qa-detail-tabs">{['请求与响应', '断言', '变量'].map(label => <button className={tab === label ? 'active' : ''} onClick={() => setTab(label)} key={label}>{label}{label === '断言' ? ` · ${3 + call.assertions.length}` : ''}</button>)}</div>{detail(call)}</div>}</article>)}
          </div>)}
        </div></section>}
        {view === 'both' && <QaPanelDivider flowPercent={flowPercent} flowHeight={flowHeight} onChange={(value, stacked) => stacked ? setFlowHeight(value) : setFlowPercent(value)} />}
        {view !== 'calls' && <section className="qa-flow-panel" aria-label="用户行为流程"><div className="qa-panel-heading"><span><GitBranch size={15} />用户行为流程</span><div className="qa-panel-actions">{view === 'flow' && viewSwitch}<button onClick={() => inspectAction(null)}>取消选择</button></div></div><div className="qa-flow-scroll"><div className="qa-flow-start"><span />{prerequisites.length > 0 ? '引用通用授权 · 准备身份' : '开始 · 无需登录'}</div>{actions.map((action, index) => <div id={`qa-flow-${action.id}`} className="qa-flow-step" key={action.id}><div className={`qa-flow-edge ${selectedActionId === action.id ? 'active' : ''}`}><ArrowDown size={13} /></div><button className={`qa-flow-node ${selectedActionId === action.id ? 'selected' : ''} ${cursorAction?.id === action.id ? 'is-executing' : ''} ${showFailure && action.calls.some(call => call.id === failed?.callId) ? 'is-failed' : ''} ${action.calls.every(call => done.includes(call.id)) ? 'complete' : ''}`} aria-pressed={selectedActionId === action.id} onClick={() => selectAction(action.id)}><div className="qa-flow-node-title"><span>{String(index + 1).padStart(2, '0')}</span><strong title={action.cleanup ? "成功、失败或取消后执行清理" : undefined}>{action.title}{action.cleanup && " · 清理"}</strong>{action.calls.every(call => done.includes(call.id)) && <CheckCircle2 size={14} />}</div><p>{action.description}</p><div className="qa-flow-node-footer"><span className={`qa-actor qa-actor-${action.calls[0].actor}`}>{qaActors[action.calls[0].actor].label}</span><span>{action.calls.length} API<ArrowRight size={12} /></span></div></button></div>)}<div className="qa-flow-edge"><ArrowDown size={13} /></div><div className="qa-flow-end"><Check size={12} />场景结束</div></div></section>}
      </div>
    </section>
  </main>
}


function QaSuccessStamp({ animate }: { animate: boolean }) {
  return <div className={`qa-success-stamp ${animate ? 'is-stamping' : ''}`} aria-hidden="true">
    <svg viewBox="0 0 160 160" fill="none">
      <circle cx="80" cy="80" r="70" stroke="currentColor" strokeWidth="5" />
      <circle cx="80" cy="80" r="60" stroke="currentColor" strokeWidth="1.8" strokeDasharray="108 80" transform="rotate(-39 80 80)" />
      <path d="m68 43 8 8 16-17" stroke="currentColor" strokeWidth="4" strokeLinecap="round" strokeLinejoin="round" />
      <text x="80" y="94" textAnchor="middle" fill="currentColor" fontSize="34" fontWeight="750">已通过</text>
      <text x="80" y="119" textAnchor="middle" fill="currentColor" fontSize="10" fontWeight="600" letterSpacing="1.2">DEVFLOW QA</text>
    </svg>
  </div>
}

function QaExchangeSection({ title, action, children }: { title: string; action?: ReactNode; children: ReactNode }) {
  const [open, setOpen] = useState(true)
  const id = useId()
  return <section><div className="qa-exchange-heading"><button type="button" className="qa-exchange-toggle" aria-expanded={open} aria-controls={id} onClick={() => setOpen(value => !value)}>{open ? <ChevronDown size={14} /> : <ChevronRight size={14} />}<strong>{title}</strong></button><div className="qa-exchange-actions">{action}</div></div><div id={id} hidden={!open}>{children}</div></section>
}
