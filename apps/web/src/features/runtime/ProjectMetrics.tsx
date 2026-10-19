import { useId } from 'react'
import type { ProjectRuntime, RuntimeSnapshot } from '../../lib/api'

export function ProjectMetrics({ metrics, history }: { metrics: ProjectRuntime; history: RuntimeSnapshot[] }) {
  const identity = metrics.pids.join(',')
  const samples = history.flatMap(snapshot => {
    const sample = snapshot.projects.find(project => project.projectPath === metrics.projectPath && project.nodeScript === metrics.nodeScript)
    return sample && sample.pids.join(',') === identity ? [sample] : []
  })
  const memory = samples.flatMap(sample => sample.memoryBytes == null ? [] : [sample.memoryBytes])
  const cpu = samples.flatMap(sample => sample.cpuPercent == null ? [] : [sample.cpuPercent])
  const seconds = metrics.uptimeSeconds
  const uptime = seconds == null ? '不可用' : seconds >= 3600 ? `${Math.floor(seconds / 3600)} 小时 ${Math.floor(seconds % 3600 / 60)} 分` : seconds >= 60 ? `${Math.floor(seconds / 60)} 分 ${seconds % 60} 秒` : `${seconds} 秒`

  return <section className="project-metrics" aria-label="项目资源占用">
    <div className="project-metric-charts">
      <MetricChart label="内存" value={formatMemory(metrics.memoryBytes)} values={memory} tone="memory" hint={`最近 ${memory.length} 次采样`} />
      <MetricChart label="CPU" value={metrics.cpuPercent == null ? '采样中' : `${metrics.cpuPercent.toFixed(1)}%`} values={cpu} tone="cpu" hint={`最近 ${cpu.length} 次采样`} />
    </div>
    <div className="project-metric-meta"><span title={metrics.pids.join(', ')}>PID {metrics.pids.join(', ')}</span><span>已运行 {uptime}</span></div>
  </section>
}

function MetricChart({ label, value, values, tone, hint }: { label: string; value: string; values: number[]; tone: 'memory' | 'cpu'; hint: string }) {
  const id = useId()
  const width = 300, height = 56, padding = 4
  const max = Math.max(...values, tone === 'cpu' ? 5 : 1) * 1.15
  const points = values.map((sample, index) => ({
    x: values.length < 2 ? width - padding : padding + index / (values.length - 1) * (width - padding * 2),
    y: height - padding - Math.max(0, sample) / max * (height - padding * 2),
  }))
  const line = points.map((point, index) => `${index ? 'L' : 'M'}${point.x},${point.y}`).join(' ')
  const area = points.length > 1 ? `${line} L${points.at(-1)!.x},${height} L${points[0].x},${height} Z` : ''
  return <figure className={`project-metric-chart ${tone}`} title="纵轴按近期采样自适应缩放，曲线高度不代表容量百分比。">
    <figcaption><span>{label}</span><strong>{value}</strong></figcaption>
    <svg viewBox={`0 0 ${width} ${height}`} preserveAspectRatio="none" role="img" aria-label={`${label}趋势，当前 ${value}，${hint}`}>
      <defs><linearGradient id={id} x1="0" y1="0" x2="0" y2="1"><stop offset="0%" stopColor="currentColor" stopOpacity=".19" /><stop offset="100%" stopColor="currentColor" stopOpacity="0" /></linearGradient></defs>
      <path d={`M0,${height - padding} H${width}`} className="metric-baseline" />
      {area && <path d={area} fill={`url(#${id})`} />}
      {points.length > 1 && <path d={line} fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinejoin="round" vectorEffect="non-scaling-stroke" />}
      {points.at(-1) && <circle cx={points.at(-1)!.x} cy={points.at(-1)!.y} r="2.4" fill="currentColor" />}
    </svg>
    <span className="project-chart-hint">{values.length < 2 ? '等待更多采样' : hint} · {tone === 'cpu' ? '整机占比' : '物理内存'}</span>
  </figure>
}

function formatMemory(bytes: number | null) {
  return bytes == null ? '不可用' : bytes >= 1024 ** 3 ? `${(bytes / 1024 ** 3).toFixed(2)} GiB` : `${(bytes / 1024 ** 2).toFixed(1)} MiB`
}
