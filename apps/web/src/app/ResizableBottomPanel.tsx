import { useEffect, useRef, useState, type ReactNode } from 'react'
import { Terminal, X } from 'lucide-react'

const storageKey = 'devflow.bottom-panel.height'
const defaultHeight = 220

type Props = { title: string, project?: string, description: string, children?: ReactNode, headerActions?: ReactNode, hidden?: boolean, onClose: () => void }

export function ResizableBottomPanel({ title, project, description, children, headerActions, hidden = false, onClose }: Props) {
  const [height, setHeight] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(storageKey) ?? defaultHeight)
      return Number.isFinite(saved) && saved > 0 ? saved : defaultHeight
    } catch { return defaultHeight }
  })
  const [resizing, setResizing] = useState(false)
  const panel = useRef<HTMLElement>(null)
  const liveHeight = useRef(height)
  const preferredHeight = useRef(height)
  const [bounds, setBounds] = useState({ min: 120, max: defaultHeight })
  const limits = useRef(bounds)
  const cleanup = useRef<(() => void) | null>(null)
  const dragging = useRef(false)

  function resize(next: number, persist = false) {
    const value = Math.round(Math.min(limits.current.max, Math.max(limits.current.min, next)))
    liveHeight.current = value
    setHeight(value)
    if (persist) {
      preferredHeight.current = value
      try { localStorage.setItem(storageKey, String(value)) } catch { /* Keep the in-memory preference. */ }
    }
  }

  function finishResize() {
    if (!dragging.current) return
    dragging.current = false
    cleanup.current?.()
    cleanup.current = null
    setResizing(false)
    resize(liveHeight.current, true)
  }

  useEffect(() => {
    const container = panel.current?.parentElement
    if (!container) return
    function updateBounds() {
      // Leave room for the service list, including in short embedded browser views.
      const max = Math.max(0, Math.floor(container!.clientHeight - Math.min(120, container!.clientHeight / 2)))
      const next = { min: Math.min(120, max), max }
      limits.current = next
      setBounds(next)
      resize(preferredHeight.current)
    }
    updateBounds()
    const observer = new ResizeObserver(updateBounds)
    observer.observe(container)
    return () => {
      observer.disconnect()
      dragging.current = false
      cleanup.current?.()
      cleanup.current = null
    }
  }, [])

  return <section ref={panel} className={`bottom-panel ws-bottom-panel ${resizing ? 'is-resizing' : ''}`} style={{ height, display: hidden ? 'none' : undefined }} aria-label={title}>
    <div className="ws-panel-resizer" role="separator" aria-label="调整底部面板高度" aria-orientation="horizontal" aria-valuemin={bounds.min} aria-valuemax={bounds.max} aria-valuenow={height} aria-valuetext={`${height} 像素`} tabIndex={0} title="拖拽调整高度；上下方向键微调；双击复位"
      onPointerDown={event => {
        if (event.button !== 0) return
        event.preventDefault()
        event.stopPropagation()
        event.currentTarget.focus()
        cleanup.current?.()
        const handle = event.currentTarget
        const pointerId = event.pointerId
        const startY = event.clientY
        const startHeight = liveHeight.current
        dragging.current = true
        setResizing(true)
        try { handle.setPointerCapture(pointerId) } catch { /* Window events also handle embedded browsers. */ }
        const move = (next: PointerEvent) => {
          if (next.pointerId === pointerId && dragging.current) resize(startHeight + startY - next.clientY)
        }
        const end = (next: PointerEvent) => { if (next.pointerId === pointerId) finishResize() }
        const blur = () => finishResize()
        window.addEventListener('pointermove', move)
        window.addEventListener('pointerup', end)
        window.addEventListener('pointercancel', end)
        window.addEventListener('blur', blur)
        cleanup.current = () => {
          window.removeEventListener('pointermove', move)
          window.removeEventListener('pointerup', end)
          window.removeEventListener('pointercancel', end)
          window.removeEventListener('blur', blur)
          try { if (handle.hasPointerCapture(pointerId)) handle.releasePointerCapture(pointerId) } catch { /* The handle may already be detached. */ }
        }
      }}
      onLostPointerCapture={finishResize}
      onDoubleClick={() => resize(defaultHeight, true)}
      onKeyDown={event => {
        if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return
        event.preventDefault()
        resize(event.key === 'Home' ? limits.current.min : event.key === 'End' ? limits.current.max : liveHeight.current + (event.key === 'ArrowUp' ? 16 : -16), true)
      }}><span aria-hidden="true" /></div>
    <div className="panel-heading"><span className="panel-heading-title" title={project ? `${title} / ${project}` : title}><Terminal size={14} /><span>{title}</span>{project && <small>/ {project}</small>}</span><div className="panel-heading-actions">{headerActions}<button className="icon-button" aria-label="收起底部面板" title="收起面板" onClick={onClose}><X size={15} /></button></div></div>
    <div className={`panel-content ${children ? 'has-console' : ''}`}>{children ? <>{description && <p className="java-console-message" role="status">{description}</p>}{children}</> : <><span className="panel-caption">{project ?? 'DEVFLOW'}</span><p>{description}</p></>}</div>
  </section>
}
