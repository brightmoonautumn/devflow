import { useEffect, useRef, useState, type CSSProperties, type ReactNode } from 'react'
import { ChevronLeft, ChevronRight } from 'lucide-react'

function maxWidth() {
  return window.innerWidth < 720
    ? Math.max(200, Math.min(360, window.innerWidth - (window.innerWidth <= 550 ? 45 : 51) - 28))
    : Math.max(200, Math.min(520, window.innerWidth - 420))
}
function clampWidth(value: number) { return Math.round(Math.max(200, Math.min(maxWidth(), value))) }

// Match the explorer's resize and collapse behavior while keeping QA's width independent.
export function QaSidebar({ open, onToggle, children }: { open: boolean; onToggle: () => void; children: ReactNode }) {
  const [width, setWidth] = useState(() => {
    try {
      const saved = Number(localStorage.getItem('devflow.qa.sidebar-width') ?? 244)
      return clampWidth(Number.isFinite(saved) ? saved : 244)
    } catch { return clampWidth(244) }
  })
  const [resizing, setResizing] = useState(false)
  const liveWidth = useRef(width)
  const drag = useRef<{ x: number; width: number; pointerId: number } | null>(null)
  const dragCleanup = useRef<(() => void) | null>(null)
  function resize(next: number, persist = false) {
    const value = clampWidth(next)
    liveWidth.current = value; setWidth(value)
    if (persist) {
      try { localStorage.setItem('devflow.qa.sidebar-width', String(value)) } catch { /* Storage is optional. */ }
    }
  }
  function finishResize() {
    drag.current = null
    dragCleanup.current?.(); dragCleanup.current = null
    setResizing(false); resize(liveWidth.current, true)
  }
  useEffect(() => () => { dragCleanup.current?.() }, [])
  useEffect(() => {
    const fit = () => { const next = clampWidth(liveWidth.current); liveWidth.current = next; setWidth(next) }
    window.addEventListener('resize', fit)
    return () => window.removeEventListener('resize', fit)
  }, [])
  return <aside className={`workspace-sidebar qa-sidebar ${open ? '' : 'is-collapsed'} ${resizing ? 'is-resizing' : ''}`} style={{ '--explorer-width': `${width}px` } as CSSProperties} aria-label="QA 场景资源管理器">
    <button className="ws-sidebar-toggle" aria-label={open ? '向左收起场景目录' : '展开场景目录'} title={open ? '向左收起场景目录' : '展开场景目录'} aria-expanded={open} aria-controls="qa-sidebar-content" onClick={onToggle}>{open ? <ChevronLeft size={14} /> : <ChevronRight size={14} />}</button>
    <div id="qa-sidebar-content" className="ws-sidebar-content" inert={!open} aria-hidden={!open}>
      <div className="qa-sidebar-scroll">{children}</div>
      <div className="ws-sidebar-resizer" role="separator" aria-label="调整场景目录宽度" aria-orientation="vertical" aria-valuemin={200} aria-valuemax={maxWidth()} aria-valuenow={width} tabIndex={0} title="拖拽调整宽度 · 双击恢复默认" onPointerDown={event => {
        if (event.button !== 0) return
        event.preventDefault(); event.stopPropagation(); event.currentTarget.focus()
        dragCleanup.current?.()
        const handle = event.currentTarget, pointerId = event.pointerId
        try { handle.setPointerCapture(pointerId) } catch { /* Window listeners also handle the drag. */ }
        drag.current = { x: event.clientX, width: liveWidth.current, pointerId }; setResizing(true)
        const move = (next: PointerEvent) => { if (drag.current?.pointerId === next.pointerId) resize(drag.current.width + next.clientX - drag.current.x) }
        const end = (next: PointerEvent) => { if (next.pointerId === pointerId) finishResize() }
        const blur = () => finishResize()
        window.addEventListener('pointermove', move); window.addEventListener('pointerup', end); window.addEventListener('pointercancel', end); window.addEventListener('blur', blur)
        dragCleanup.current = () => {
          window.removeEventListener('pointermove', move); window.removeEventListener('pointerup', end); window.removeEventListener('pointercancel', end); window.removeEventListener('blur', blur)
          try { if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId) } catch { /* The handle may have unmounted. */ }
        }
      }} onLostPointerCapture={() => { if (drag.current) finishResize() }} onDoubleClick={() => resize(244, true)} onKeyDown={event => {
        if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return
        event.preventDefault()
        resize(event.key === 'Home' ? 200 : event.key === 'End' ? maxWidth() : width + (event.key === 'ArrowLeft' ? -1 : 1) * (event.shiftKey ? 40 : 10), true)
      }}><span className="ws-resize-grip" aria-hidden="true" /></div>
    </div>
  </aside>
}
