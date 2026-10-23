import { useEffect, useId, useRef, useState } from 'react'
import { Copy, PlugZap, X } from 'lucide-react'
import { executeDiagnosis, getDiagnoses, type Diagnosis, type Project } from '../../lib/api'
import { copyText } from '../../lib/clipboard'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseField } from '../../components/form/BaseField'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseTextarea } from '../../components/form/BaseTextarea'
import './diagnosis.css'

const statuses: Record<string, string> = { awaiting_agent: '等待 Agent', diagnosing: '分析中', waiting_reproduction: '等待复现', observing: '采样中', cancelling: '取消中', completed: '已完成', cancelled: '已取消', failed: '已中断' }
const ended = (session: Diagnosis) => ['completed', 'cancelled', 'failed'].includes(session.status)

export function DiagnosisDialog({ project, revision, pid, runId, onClose }: { project: Project; revision: number; pid?: number; runId?: string; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const fieldId = useId()
  const version = useRef(0)
  const mounted = useRef(false)
  const [sessions, setSessions] = useState<Diagnosis[]>([])
  const [selected, setSelected] = useState('')
  const [problem, setProblem] = useState('')
  const [loaded, setLoaded] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [operationError, setOperationError] = useState('')
  const [message, setMessage] = useState('')
  const session = sessions.find(value => value.id === selected) ?? sessions.at(-1)
  const active = sessions.find(value => !ended(value))

  useEffect(() => { const current = dialog.current; mounted.current = true; current?.showModal(); return () => { mounted.current = false; current?.close() } }, [])
  useEffect(() => {
    let disposed = false
    let timer: ReturnType<typeof setTimeout> | undefined
    let controller: AbortController | undefined
    async function poll() {
      const current = version.current
      controller = new AbortController()
      const timeout = setTimeout(() => controller?.abort(), 10000)
      try {
        const values = await getDiagnoses(project.path, revision, controller.signal)
        if (!disposed && current === version.current) { setSessions(values); setError(''); setLoaded(true) }
      } catch (err) { if (!disposed) setError(err instanceof Error ? err.message : String(err)) }
      finally { clearTimeout(timeout) }
      if (!disposed) timer = setTimeout(() => void poll(), document.hidden ? 5000 : 2000)
    }
    void poll()
    return () => { disposed = true; clearTimeout(timer); controller?.abort() }
  }, [project.path, revision])

  async function mutate(create: boolean) {
    if (busy || (create && !pid) || (!create && !session)) return
    setBusy(true); setOperationError(''); setMessage(''); version.current++
    try {
      const value = await executeDiagnosis(project.path, revision, create
        ? { type: 'create', pid: pid!, problem: problem.trim(), ...(runId ? { runId } : {}) }
        : { type: 'cancel', id: session!.id, revision: session!.revision })
      if (!mounted.current) return
      version.current++
      setSessions(current => [...current.filter(item => item.id !== value.id), value].sort((a, b) => a.createdAt - b.createdAt))
      setSelected(value.id)
      if (create) setProblem('')
    } catch (err) { if (mounted.current) setOperationError(err instanceof Error ? err.message : String(err)) }
    finally { if (mounted.current) setBusy(false) }
  }
  async function copy() {
    if (!session) return
    try { await copyText(session.agentPrompt); if (mounted.current) setMessage('已复制，粘贴给 Agent 后开始排查') }
    catch (err) { if (mounted.current) setOperationError(`复制失败：${err instanceof Error ? err.message : String(err)}`) }
  }
  return <dialog ref={dialog} className="ws-dialog ws-location-dialog diagnosis-dialog" aria-labelledby={`${fieldId}-title`} onCancel={onClose}>
    <header className="ws-location-heading"><div className="ws-dialog-symbol"><PlugZap size={21} /></div><div><h2 id={`${fieldId}-title`}>AI 诊断 · {project.name}</h2><p>描述问题，复制给 Agent 接手；进展和证据会同步到这里。</p></div><button className="ws-icon-button" aria-label="关闭诊断" onClick={onClose}><X size={16} /></button></header>
    <div className="diagnosis-body">
      {!active && <form onSubmit={event => { event.preventDefault(); void mutate(true) }} className="diagnosis-create">
        <BaseField htmlFor={`${fieldId}-problem`} label="需要排查什么" hint={pid ? `目标 PID ${pid}；可补充接口、预期行为与复现步骤。` : '请在运行输出中选择一个正在运行的 Java 进程。'}>
          <BaseTextarea id={`${fieldId}-problem`} aria-describedby={`${fieldId}-problem-hint`} value={problem} onChange={event => setProblem(event.target.value)} maxLength={2000} placeholder="例如：查询用户接口偶发超过 3 秒，连续刷新可复现" required />
        </BaseField>
        <BaseButton type="submit" variant="primary" disabled={busy || !loaded || !!error || !pid || !problem.trim()}>{busy ? '创建中…' : '创建诊断'}</BaseButton>
      </form>}
      {sessions.length > 1 && <BaseField htmlFor={`${fieldId}-history`} label="诊断记录"><BaseSelect id={`${fieldId}-history`} value={session?.id ?? ''} onValueChange={setSelected} options={[...sessions].reverse().map(item => ({ value: item.id, label: `${statuses[item.status] ?? item.status} · ${item.problem.slice(0, 45)}` }))} /></BaseField>}
      {session && <section className="diagnosis-session" aria-label="诊断详情">
        <div className="diagnosis-status"><strong>{statuses[session.status] ?? session.status}</strong><span>PID {session.pid}</span></div>
        <p className="diagnosis-problem">{session.problem}</p>
        <p className="diagnosis-progress" role="status">{session.progress}</p>
        {!ended(session) && <div className="diagnosis-actions"><BaseButton onClick={() => void copy()}><Copy size={13} />复制给 Agent</BaseButton><BaseButton disabled={busy || session.status === 'cancelling'} onClick={() => void mutate(false)}>取消诊断</BaseButton></div>}
        {session.conclusion && <div className="diagnosis-conclusion"><h3>诊断结论</h3><p>{session.conclusion}</p></div>}
        {session.evidence.length > 0 && <div className="diagnosis-evidence"><h3>采样证据（{session.evidence.length}）</h3>{session.evidence.map((entry, index) => <details key={`${index}-${entry.recordedAt}`}><summary>{entry.command}{entry.timedOut ? ' · 已到采样时间上限' : ''}{entry.truncated ? ' · 输出已截断' : ''}</summary><pre>{entry.output}</pre></details>)}</div>}
      </section>}
      {(error || operationError) && <p className="java-console-error" role="alert">{operationError || error}</p>}
      {message && <p className="diagnosis-progress" role="status">{message}</p>}
    </div>
  </dialog>
}
