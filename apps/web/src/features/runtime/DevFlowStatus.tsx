import { useEffect, useState } from 'react'
import { Cpu, MemoryStick } from 'lucide-react'
import { getDevFlowMetrics, type DevFlowMetrics } from '../../lib/api'

function resourceLevel(value: number | null | undefined, warning: number, high: number) {
  if (value == null || !Number.isFinite(value)) return 'unknown'
  return value >= high ? 'high' : value >= warning ? 'warning' : 'normal'
}

const levelLabels = { unknown: '等待采样', normal: '正常', warning: '偏高', high: '较高' }

export function DevFlowStatus() {
  const [metrics, setMetrics] = useState<DevFlowMetrics | null>(null)
  const [error, setError] = useState('')
  useEffect(() => {
    let disposed = false
    let pending: AbortController | null = null
    let timer: ReturnType<typeof setTimeout> | undefined
    async function sample() {
      if (disposed || document.hidden || pending) return
      const controller = new AbortController()
      pending = controller
      const timeout = setTimeout(() => controller.abort(), 10000)
      try {
        const result = await getDevFlowMetrics(controller.signal)
        if (!disposed && !controller.signal.aborted) { setMetrics(result); setError('') }
      } catch (err) {
        if (!disposed) { setMetrics(null); setError(err instanceof Error ? err.message : String(err)) }
      } finally {
        clearTimeout(timeout)
        if (pending === controller) pending = null
        if (!disposed && !document.hidden) timer = setTimeout(() => void sample(), 3000)
      }
    }
    function visible() { clearTimeout(timer); if (!document.hidden) void sample() }
    document.addEventListener('visibilitychange', visible)
    void sample()
    return () => { disposed = true; clearTimeout(timer); pending?.abort(); document.removeEventListener('visibilitychange', visible) }
  }, [])
  const cpu = metrics?.cpuPercent == null ? error ? '—' : '采样中' : `${metrics.cpuPercent.toFixed(1)}%`
  const memory = metrics?.memoryBytes == null ? '—' : `${(metrics.memoryBytes / 1024 ** 2).toFixed(1)} MiB`
  const cpuLevel = resourceLevel(metrics?.cpuPercent, 30, 70)
  const memoryLevel = resourceLevel(metrics?.memoryBytes, 256 * 1024 ** 2, 512 * 1024 ** 2)
  const title = error ? `DevFlow 指标暂不可用：${error}` : `DevFlow 后端进程${metrics ? ` · PID ${metrics.pid}` : ''}。CPU 按整机逻辑核心归一；内存为物理内存。不含 Codex/浏览器或托管项目。每 3 秒刷新。`
  return <div className={`ws-self-metrics ${error ? 'is-unavailable' : ''}`} role="group" aria-label="DevFlow 自身资源占用" title={title}>
    <span className={`ws-metric-${cpuLevel}`} title={`${title}\nCPU：${levelLabels[cpuLevel]}；低于 30% 绿色，30%–70% 黄色，达到 70% 红色。`} aria-label={`CPU ${cpu}，${levelLabels[cpuLevel]}`}><Cpu size={12} aria-hidden="true" /><span className="ws-self-caption">CPU</span><strong>{cpu}</strong></span>
    <span className={`ws-metric-${memoryLevel}`} title={`${title}\n内存：${levelLabels[memoryLevel]}；低于 256 MiB 绿色，256–512 MiB 黄色，达到 512 MiB 红色。`} aria-label={`内存 ${memory}，${levelLabels[memoryLevel]}`}><MemoryStick size={12} aria-hidden="true" /><span className="ws-self-caption">内存</span><strong>{memory}</strong></span>
  </div>
}
