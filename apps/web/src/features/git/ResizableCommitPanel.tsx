import { useEffect, useRef, useState, type ReactNode } from 'react'

const storageKey = 'devflow.git.commit.height'
const defaultHeight = 190

export function ResizableCommitPanel({ children }: { children: ReactNode }) {
  const [height, setHeight] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(storageKey) ?? defaultHeight)
      return Number.isFinite(saved) && saved > 0 ? saved : defaultHeight
    } catch { return defaultHeight }
  })
  const [resizing, setResizing] = useState(false)
  const panel = useRef<HTMLDivElement>(null)
  const liveHeight = useRef(height)
  const preferredHeight = useRef(height)
  const [bounds, setBounds] = useState({ min: 160, max: defaultHeight })
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
      // Hidden sidebar views have no layout; preserve the saved height until visible.
      if (!container!.clientHeight) return
      const reserved = Array.from(container!.children).reduce((total, child) =>
        child === panel.current || child.classList.contains('git-scroll') ? total : total + child.getBoundingClientRect().height, 0)
      // Keep the change list accessible even in a short split view.
      const max = Math.max(0, Math.floor(container!.clientHeight - reserved - 88))
      const next = { min: Math.min(160, max), max }
      limits.current = next
      setBounds(next)
      resize(preferredHeight.current)
    }
    updateBounds()
    const observer = new ResizeObserver(updateBounds)
    observer.observe(container)
    for (const child of container.children) {
      if (child !== panel.current && !child.classList.contains('git-scroll')) observer.observe(child)
    }
    const mutations = new MutationObserver(() => {
      for (const child of container.children) {
        if (child !== panel.current && !child.classList.contains('git-scroll')) observer.observe(child)
      }
      updateBounds()
    })
    mutations.observe(container, { childList: true })
    return () => {
      observer.disconnect()
      mutations.disconnect()
      dragging.current = false
      cleanup.current?.()
      cleanup.current = null
    }
  }, [])

  return <div ref={panel} className={`git-commit ${resizing ? 'is-resizing' : ''}`} style={{ height }}>
    <div className="ws-panel-resizer" role="separator" aria-label="调整提交信息区域高度" aria-orientation="horizontal" aria-valuemin={bounds.min} aria-valuemax={bounds.max} aria-valuenow={height} aria-valuetext={`${height} 像素`} tabIndex={0} title="拖拽调整高度；上下方向键微调；双击复位"
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
    <div className="git-commit-content">{children}</div>
  </div>
}
