import { useEffect, useMemo, useRef, useState } from 'react'
import { Circle, ExternalLink, PlugZap } from 'lucide-react'
import { executeNode, getNodeOutput, metricsKey, projectKey, executeJava, getJavaOutput, type JavaLogLine, type JavaOutput, type Project } from '../../lib/api'
import { useRuntimeMetrics } from './useRuntimeMetrics'
import { ResizableBottomPanel } from '../../app/ResizableBottomPanel'
import { formatConsoleLines } from './consoleLines'
import { BaseSelect } from '../../components/form/BaseSelect'

export function RuntimeConsole({ project, revision, title, description, onClose }: { project: Project; revision: number; title: string; description: string; onClose: () => void }) {
  const node = project.kind === 'node'
  const [output, setOutput] = useState<JavaOutput | null>(null)
  const [lines, setLines] = useState<JavaLogLine[]>([])
  const formattedLines = useMemo(() => formatConsoleLines(lines), [lines])
  const [error, setError] = useState('')
  const [operationError, setOperationError] = useState('')
  const [busy, setBusy] = useState(false)
  const [pid, setPid] = useState('')
  const scroll = useRef<HTMLDivElement>(null)
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
  const managed = output && ['starting', 'running'].includes(output.state)
  async function operate(type: 'stop' | 'attach') {
    if (busy) return
    setBusy(true); setOperationError('')
    try {
      const value = await (node ? executeNode(project, revision, { type: 'stop', ...(managed ? {} : { pid: selectedPid }) }) : executeJava(project.path, revision, type === 'attach' ? { type, pid: selectedPid! } : { type, ...(managed ? {} : { pid: selectedPid }) }))
      setOutput(value)
    } catch (err) { setOperationError(err instanceof Error ? err.message : String(err)) } finally { setBusy(false) }
  }
  const stateNames: Record<string, string> = { starting: '构建 / 启动中', running: '运行中', stopped: '已停止', failed: '启动失败或异常退出', external: '外部进程' }
  const toolbar = <div className="java-console-toolbar" role="group" aria-label="运行控制">
    <span className={`java-console-state is-${output?.state ?? 'loading'}`} title={output ? stateNames[output.state] ?? output.state : '读取输出…'}>{output ? stateNames[output.state] ?? output.state : '读取输出…'}</span>
    {pids.length > 1 ? <BaseSelect aria-label={node ? "选择 Node.js 进程" : "选择 Java 进程"} value={selectedPid ? String(selectedPid) : ''} onValueChange={setPid} placeholder="选择应用 PID" options={pids.map(value => ({ value: String(value), label: `PID ${value}` }))} /> : selectedPid && <span className="java-console-pid">PID {selectedPid}</span>}
    <div className="java-console-actions"><button className="ws-launch is-stop" aria-label="停止服务" title="停止服务" disabled={busy || (!managed && !selectedPid)} onClick={() => void operate('stop')}><Circle size={10} fill="currentColor" /><span className="java-action-label">停止</span></button>{!node && (output?.arthasUrl ? <a href={output.arthasUrl} target="_blank" rel="noreferrer" aria-label="打开 Arthas 控制台" title="打开 Arthas 控制台"><ExternalLink size={12} /><span className="java-action-label">打开 Arthas</span></a> : <button className="ws-outline-button" aria-label={output?.attaching ? 'Arthas 附加中' : '挂载 Arthas'} title={output?.attaching ? 'Arthas 附加中' : '挂载 Arthas'} disabled={busy || !selectedPid || output?.attaching || output?.state === 'starting'} onClick={() => void operate('attach')}><PlugZap size={13} /><span className="java-action-label">{output?.attaching ? '附加中…' : '挂载 Arthas'}</span></button>)}</div>
  </div>
  return <ResizableBottomPanel title={title} project={project.name} description={description} headerActions={toolbar} onClose={onClose}><div className="java-console">
    {(error || operationError) && <p className="java-console-error" role="alert">{operationError || error}</p>}
    <div className="java-console-log" ref={scroll} role="log" aria-label={`${project.name} 运行输出`} aria-live="off" tabIndex={0} onScroll={event => { const element = event.currentTarget; follow.current = element.scrollHeight - element.scrollTop - element.clientHeight < 40 }}>
      {formattedLines.length ? formattedLines.map(line => <div className="java-log-line" key={`${output?.runId}-${line.sequence}`}>{line.parts.map((part, index) => <span className={`java-log-part is-${part.tone}`} key={index}>{part.text}</span>)}</div>) : <p>{output?.state === 'external' ? node ? '尚无 DevFlow 运行输出。外部进程无法追溯捕获日志，可选择 PID 停止。' : '尚无 DevFlow 运行输出。外部启动的进程不能追溯捕获 stdout；可以选择 PID 停止或附加 Arthas。' : '等待进程输出…'}</p>}
    </div>
    <div className="java-console-footer"><span>保留最近 2000 条输出 · UTF-8</span><button onClick={() => { follow.current = true; if (scroll.current) scroll.current.scrollTop = scroll.current.scrollHeight }}>滚动到底部</button></div>
  </div></ResizableBottomPanel>
}
