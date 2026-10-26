import { useEffect, useRef, useState } from 'react'
import { Check, Focus, Minus, Plus, Workflow } from 'lucide-react'
import { type BoardFeature, type BoardTask } from './boardModel'

export function BoardCanvas({ features: savedFeatures, edges, disabled, tasks, activeFeature, onFeature, onTask, onMove, onInteractionChange }: {
  features: BoardFeature[]; edges: [string, string][]; disabled: boolean; tasks: BoardTask[]; activeFeature: string
  onFeature: (id: string) => void; onTask: (task: BoardTask) => void; onMove: (id: string, x: number, y: number) => Promise<boolean>
  onInteractionChange: (active: boolean) => void
}) {
  const [camera, setCamera] = useState({ x: 30, y: 30, zoom: 1 })
  const [moving, setMoving] = useState<{ id: string; x: number; y: number } | null>(null)
  const features = savedFeatures.map(f => moving?.id === f.id ? { ...f, x: moving.x, y: moving.y } : f)
  const viewport = useRef<HTMLDivElement>(null)
  const gesture = useRef<{ id?: string; x: number; y: number; startX: number; startY: number; moved: boolean } | null>(null)
  const saving = useRef(false)
  const mounted = useRef(false)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false; gesture.current = null; onInteractionChange(false) }
  }, [onInteractionChange])
  function cancelGesture() {
    if (!gesture.current) return
    gesture.current = null
    setMoving(null)
    onInteractionChange(false)
  }
  function fit() {
    const el = viewport.current
    if (!el || !features.length) return
    const minX = Math.min(...features.map(f => f.x)) - 30, minY = Math.min(...features.map(f => f.y)) - 30
    const width = Math.max(...features.map(f => f.x)) + 320 - minX
    const height = Math.max(...features.map(f => f.y)) + 300 - minY
    const zoom = Math.min(1, (el.clientWidth - 32) / width, (el.clientHeight - 32) / height)
    setCamera({ x: (el.clientWidth - width * zoom) / 2 - minX * zoom, y: (el.clientHeight - height * zoom) / 2 - minY * zoom, zoom: Math.max(.15, zoom) })
  }
  function zoomBy(factor: number) {
    const el = viewport.current
    if (!el) return
    setCamera(c => {
      const zoom = Math.max(.15, Math.min(1.8, c.zoom * factor))
      return { zoom, x: el.clientWidth / 2 - (el.clientWidth / 2 - c.x) * zoom / c.zoom, y: el.clientHeight / 2 - (el.clientHeight / 2 - c.y) * zoom / c.zoom }
    })
  }
  return <div className="kb-canvas" ref={viewport} aria-label="功能架构画布" onWheel={event => {
    if (event.ctrlKey || event.metaKey) return
    setCamera(c => ({ ...c, x: c.x - event.deltaX, y: c.y - event.deltaY }))
  }} onPointerDown={event => {
    if (event.button !== 0 || gesture.current || saving.current || (event.target as Element).closest('button, input')) return
    const node = (event.target as Element).closest<HTMLElement>('[data-feature]')
    const feature = features.find(f => f.id === node?.dataset.feature)
    if (feature && disabled) return
    gesture.current = { id: feature?.id, x: event.clientX, y: event.clientY, startX: feature?.x ?? camera.x, startY: feature?.y ?? camera.y, moved: false }
    event.currentTarget.setPointerCapture(event.pointerId)
    onInteractionChange(true)
  }} onPointerMove={event => {
    const g = gesture.current
    if (!g) return
    const dx = event.clientX - g.x, dy = event.clientY - g.y
    if (Math.abs(dx) + Math.abs(dy) > 4) g.moved = true
    if (!g.moved) return
    if (g.id) setMoving({ id: g.id, x: g.startX + dx / camera.zoom, y: g.startY + dy / camera.zoom })
    else setCamera(c => ({ ...c, x: g.startX + dx, y: g.startY + dy }))
  }} onPointerUp={event => {
    const g = gesture.current
    gesture.current = null
    if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId)
    if (g?.id && g.moved) {
      const position = { id: g.id, x: g.startX + (event.clientX - g.x) / camera.zoom, y: g.startY + (event.clientY - g.y) / camera.zoom }
      // Keep the node AND its edges at the drop position until canonical coordinates arrive.
      setMoving(position)
      saving.current = true
      void onMove(position.id, position.x, position.y).catch(() => false).finally(() => {
        saving.current = false
        if (!mounted.current) return
        // Failed saves return to the persisted position; the parent retains the error notice.
        setMoving(null)
        onInteractionChange(false)
      })
    } else {
      setMoving(null)
      onInteractionChange(false)
      if (g?.id) onFeature(g.id)
    }
  }} onPointerCancel={cancelGesture} onLostPointerCapture={cancelGesture}>
    <div className="kb-canvas-world" style={{ transform: `translate(${camera.x}px, ${camera.y}px) scale(${camera.zoom})` }}>
      <svg className="kb-edges" aria-hidden="true"><defs><marker id="kb-arrow" markerWidth="7" markerHeight="7" refX="6" refY="3.5" orient="auto"><path d="M0,0 L7,3.5 L0,7" fill="var(--ws-dim)" /></marker></defs>{edges.map(([source, target]) => {
        const a = features.find(f => f.id === source), b = features.find(f => f.id === target)
        if (!a || !b) return null
        const vertical = Math.abs(a.x - b.x) < 150 && Math.abs(a.y - b.y) > 200
        const down = b.y >= a.y, right = b.x >= a.x
        const x1 = a.x + (vertical ? 145 : right ? 290 : 0), y1 = a.y + (vertical ? down ? 280 : 0 : 100)
        const x2 = b.x + (vertical ? 145 : right ? 0 : 290), y2 = b.y + (vertical ? down ? 0 : 280 : 100)
        const offset = vertical ? down ? 30 : -30 : right ? 65 : -65
        return <path key={`${source}-${target}`} d={vertical ? `M${x1},${y1} C${x1},${y1 + offset} ${x2},${y2 - offset} ${x2},${y2}` : `M${x1},${y1} C${x1 + offset},${y1} ${x2 - offset},${y2} ${x2},${y2}`} markerEnd="url(#kb-arrow)" />
      })}</svg>
      {features.map(feature => {
        const items = tasks.filter(t => t.feature === feature.id), done = items.filter(t => t.status === 'done').length
        return <article key={feature.id} data-feature={feature.id} className={`kb-node ${activeFeature === feature.id ? 'is-selected' : ''}`} style={{ left: feature.x, top: feature.y }}>
          <div className="kb-node-heading"><span className={`kb-feature-icon feature-${feature.id}`}><Workflow size={16} /></span><button onClick={() => onFeature(feature.id)}>{feature.name}</button><span className="kb-count">{items.length}</span></div>
          <p>{feature.description}</p><div className="kb-node-progress"><span style={{ width: `${items.length ? done / items.length * 100 : 0}%` }} /></div>
          <div className="kb-node-meta"><span>{done} / {items.length} 已完成</span><span>{items.filter(t => t.status === 'doing').length} 进行中</span></div>
          <div className="kb-node-tasks">{items.slice(0, 3).map(task => <button key={task.id} onClick={() => onTask(task)}><span className={`kb-dot status-${task.status}`}>{task.status === 'done' && <Check size={8} />}</span><span>{task.title}</span></button>)}{!items.length && <span className="kb-muted">还没有任务</span>}{items.length > 3 && <button onClick={() => onFeature(feature.id)}>查看全部 {items.length} 项任务 →</button>}</div>
        </article>
      })}
    </div>
    <div className="kb-canvas-caption"><Workflow size={13} /><span>功能结构<span className="kb-canvas-hint"> · 拖动画布平移，拖动模块调整位置</span></span></div>
    <div className="kb-canvas-controls"><button aria-label="缩小画布" onClick={() => zoomBy(1 / 1.2)}><Minus size={15} /></button><span>{Math.round(camera.zoom * 100)}%</span><button aria-label="放大画布" onClick={() => zoomBy(1.2)}><Plus size={15} /></button><i /><button aria-label="适应画布" title="适应画布" onClick={fit}><Focus size={16} /></button></div>
  </div>
}
