import { readOnlyShare } from '../../lib/access'
import { useEffect, useMemo, useRef, useState } from 'react'
import { Check, Circle, Copy, ExternalLink, ListX, PlugZap } from 'lucide-react'
import { getRuntimeDiagnostics, executeNode, getNodeOutput, metricsKey, projectKey, executeJava, getJavaOutput, type JavaLogLine, type JavaOutput, type Project } from '../../lib/api'
import { useRuntimeMetrics } from './useRuntimeMetrics'
import { ResizableBottomPanel } from '../../app/ResizableBottomPanel'
import { formatConsoleLines } from './consoleLines'
import { BaseSelect } from '../../components/form/BaseSelect'
import { copyText } from '../../lib/clipboard'
import { DiagnosisDialog } from './DiagnosisDialog'

export function RuntimeConsole({ project, revision, title, description, onClose }: { project: Project; revision: number; title: string; description: string; onClose: () => void }) {
  const node = project.kind === 'node'
  const [output, setOutput] = useState<JavaOutput | null>(null)
  const [lines, setLines] = useState<JavaLogLine[]>([])
  const formattedLines = useMemo(() => formatConsoleLines(lines), [lines])
  const [error, setError] = useState('')
  const [operationError, setOperationError] = useState('')
  const [busy, setBusy] = useState(false)
  const [diagnosisOpen, setDiagnosisOpen] = useState(false)
  const [copying, setCopying] = useState(false)
  const [copyMessage, setCopyMessage] = useState('')
  const [copied, setCopied] = useState(false)
  const copyRequest = useRef<AbortController | null>(null)
  const copyTimer = useRef<ReturnType<typeof setTimeout> | undefined>(undefined)
  const [pid, setPid] = useState('')
  const scroll = useRef<HTMLDivElement>(null)
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  const menuRef = useRef<HTMLDivElement>(null)
  const follow = useRef(true)
  const runtime = useRuntimeMetrics(true)
  const metrics = runtime.snapshot?.projects.find(value => metricsKey(value) === projectKey(project))
  const pids = (node ? metrics?.nodePids : metrics?.javaPids) ?? []
  const selectedPid = pids.includes(Number(pid)) ? Number(pid) : output?.pid && pids.includes(output.pid) ? output.pid : pids.length === 1 ? pids[0] : undefined
  useEffect(() => {
    let disposed = false
    let cursor = 0, runId = ''
    let timer: ReturnType<typeof setTimeout> | undefined
    let controller: AbortController | undefined
    async function poll() {
      controller = new AbortController()
      try {
        const value = await (node ? getNodeOutput(project, cursor, runId, controller.signal) : getJavaOutput(project.path, cursor, runId, controller.signal))
        if (disposed) return
        const reset = runId !== value.runId
        runId = value.runId; cursor = value.nextCursor
        setOutput(value); setError('')
        setLines(current => [...(reset ? [] : current), ...(value.truncated ? [{ sequence: -cursor, stream: 'system', text: '较早日志已超过保留上限。' }] : []), ...value.lines].slice(-2000))
      } catch (err) { if (!disposed) setError(err instanceof Error ? err.message : String(err)) }
      if (!disposed) timer = setTimeout(() => void poll(), document.hidden ? 3000 : 1000)
    }
    void poll()
    return () => { disposed = true; clearTimeout(timer); controller?.abort() }
  }, [project.path, project.nodeScript, node])
  useEffect(() => { if (follow.current && scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight }, [lines])
  useEffect(() => {
    if (!menu) return
    const dismiss = (event: PointerEvent) => { if (!menuRef.current?.contains(event.target as Node)) setMenu(null) }
    const close = () => setMenu(null)
    document.addEventListener('pointerdown', dismiss)
    window.addEventListener('resize', close)
    window.addEventListener('scroll', close, true)
    window.addEventListener('blur', close)
    return () => {
      document.removeEventListener('pointerdown', dismiss)
      window.removeEventListener('resize', close)
      window.removeEventListener('scroll', close, true)
      window.removeEventListener('blur', close)
    }
  }, [menu])
  const managed = output && ['starting', 'running'].includes(output.state)
  async function operate(type: 'stop' | 'attach') {
    if (busy) return
    setBusy(true); setOperationError('')
    try {
      const value = await (node ? executeNode(project, revision, { type: 'stop', ...(managed ? {} : { pid: selectedPid }) }) : executeJava(project.path, revision, type === 'attach' ? { type, pid: selectedPid! } : { type, ...(managed ? {} : { pid: selectedPid }) }))
      setOutput(value)
    } catch (err) { setOperationError(err instanceof Error ? err.message : String(err)) } finally { setBusy(false) }
  }
  useEffect(() => () => { copyRequest.current?.abort(); copyRequest.current = null; clearTimeout(copyTimer.current) }, [])
  async function copyForAgent() {
    if (copyRequest.current || !output) return
    const controller = new AbortController()
    copyRequest.current = controller
    setCopying(true); setCopyMessage(''); setCopied(false)
    const timeout = setTimeout(() => controller.abort(), 10000)
    try {
      const context = await getRuntimeDiagnostics(project, output.state === 'external' ? '' : output.runId, controller.signal)
      if (controller.signal.aborted) return
      await copyText(context.text)
      if (controller.signal.aborted) return
      setCopied(true); setCopyMessage('服务定位信息已复制，可粘贴给 Agent')
      clearTimeout(copyTimer.current)
      copyTimer.current = setTimeout(() => { setCopied(false); setCopyMessage('') }, 4000)
    } catch (err) {
      if (copyRequest.current === controller) setCopyMessage(controller.signal.aborted ? '读取排查上下文超时，请重试' : `复制失败：${err instanceof Error ? err.message : String(err)}`)
    } finally {
      clearTimeout(timeout)
      if (copyRequest.current === controller) { copyRequest.current = null; setCopying(false) }
    }
  }
  const stateNames: Record<string, string> = { starting: '构建 / 启动中', running: '运行中', stopped: '已停止', failed: '启动失败或异常退出', external: '外部进程' }
  const toolbar = <div className="java-console-toolbar" role="group" aria-label="运行控制">
    <span className={`java-console-state is-${output?.state ?? 'loading'}`} title={output ? stateNames[output.state] ?? output.state : '读取输出…'}>{output ? stateNames[output.state] ?? output.state : '读取输出…'}</span>
    {pids.length > 1 ? <BaseSelect aria-label={node ? "选择 Node.js 进程" : "选择 Java 进程"} value={selectedPid ? String(selectedPid) : ''} onValueChange={setPid} placeholder="选择应用 PID" options={pids.map(value => ({ value: String(value), label: `PID ${value}` }))} /> : selectedPid && <span className="java-console-pid">PID {selectedPid}</span>}
    {!readOnlyShare && <div className="java-console-actions">{!node && <button className="ws-outline-button" aria-label="AI 诊断" title="描述问题，让 Agent 通过 Arthas 持续排查" onClick={() => setDiagnosisOpen(true)}><PlugZap size={13} /><span className="java-action-label">AI 诊断</span></button>}<button className="ws-outline-button" aria-label="复制给 Agent" title="复制服务定位信息，让 Agent 按需读取日志" disabled={copying || !output} onClick={() => void copyForAgent()}>{copied ? <Check size={13} /> : <Copy size={13} />}<span className="java-action-label">{copying ? '复制中…' : copied ? '已复制' : '复制给 Agent'}</span></button><button className="ws-launch is-stop" aria-label="停止服务" title="停止服务" disabled={busy || (!managed && !selectedPid)} onClick={() => void operate('stop')}><Circle size={10} fill="currentColor" /><span className="java-action-label">停止</span></button>{!node && (output?.arthasUrl ? <a href={output.arthasUrl} target="_blank" rel="noreferrer" aria-label="打开 Arthas 控制台" title="打开 Arthas 控制台"><ExternalLink size={12} /><span className="java-action-label">打开 Arthas</span></a> : <button className="ws-outline-button" aria-label={output?.attaching ? 'Arthas 附加中' : '挂载 Arthas'} title={output?.attaching ? 'Arthas 附加中' : '挂载 Arthas'} disabled={busy || !selectedPid || output?.attaching || output?.state === 'starting'} onClick={() => void operate('attach')}><PlugZap size={13} /><span className="java-action-label">{output?.attaching ? '附加中…' : '挂载 Arthas'}</span></button>)}</div>}
  </div>
  return <ResizableBottomPanel title={title} project={project.name} description={description} headerActions={toolbar} onClose={onClose}><div className="java-console">
    {(error || operationError) && <p className="java-console-error" role="alert">{operationError || error}</p>}
    <div className="java-console-log" ref={scroll} role="log" aria-label={`${project.name} 运行输出`} aria-live="off" tabIndex={0}
      onContextMenu={event => { event.preventDefault(); setMenu({ x: event.clientX, y: event.clientY }) }}
      onKeyDown={event => {
        if (event.key === 'ContextMenu' || (event.shiftKey && event.key === 'F10')) {
          event.preventDefault()
          const rect = event.currentTarget.getBoundingClientRect()
          setMenu({ x: rect.left + 12, y: rect.top + 12 })
        }
      }}
      onScroll={event => { const element = event.currentTarget; follow.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40 }}>
      {formattedLines.length ? formattedLines.map(line => <div className="java-log-line" key={`${output?.runId}-${line.sequence}`}>{line.parts.map((part, index) => part.href ? <a className="java-log-link" key={index} href={part.href} target="_blank" rel="noopener noreferrer" title={`在新标签页打开 ${part.href}`}>{part.text}</a> : <span className={`java-log-part is-${part.tone}`} key={index}>{part.text}</span>)}</div>) : <p>{output?.state === 'external' ? node ? '尚无 DevFlow 运行输出。外部进程无法追溯捕获日志，可选择 PID 停止。' : '尚无 DevFlow 运行输出。外部启动的进程不能追溯捕获 stdout；可以选择 PID 停止或附加 Arthas。' : '等待进程输出…'}</p>}
    </div>
    {menu && <div ref={menuRef} className="ws-tree-context" role="menu" aria-label="运行输出操作" style={{ left: Math.max(8, Math.min(menu.x, window.innerWidth - 250)), top: Math.max(8, Math.min(menu.y, window.innerHeight - 60)) }} onKeyDown={event => {
      if (event.key === 'Escape' || event.key === 'Tab') {
        event.preventDefault(); event.stopPropagation(); setMenu(null); scroll.current?.focus()
      }
    }}>
      <button type="button" role="menuitem" autoFocus onClick={() => {
        // Keep the polling cursor so previously displayed output is not fetched again.
        setLines([]); follow.current = true; setMenu(null); scroll.current?.focus()
      }}><ListX size={15} />清空当前输出</button>
    </div>}
    {diagnosisOpen && !readOnlyShare && <DiagnosisDialog key={project.path} project={project} revision={revision} pid={selectedPid} runId={output?.state === 'external' ? undefined : output?.runId} onClose={() => setDiagnosisOpen(false)} />}
    <div className="java-console-footer"><span role="status" aria-live="polite">{copyMessage || '保留最近 2000 条输出 · UTF-8'}</span><button onClick={() => { follow.current = true; if (scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight }}>滚动到底部</button></div>
  </div></ResizableBottomPanel>
}
