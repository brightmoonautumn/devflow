import { useEffect, useRef, useState } from 'react'

export function QaHeaderDivider({ height, onChange }: {
  height: number | null; onChange: (height: number | null) => void
}) {
  const handle = useRef<HTMLDivElement>(null)
  const drag = useRef<{ pointerId: number; y: number; height: number } | null>(null)
  const heightRef = useRef(height)
  heightRef.current = height
  const [dragging, setDragging] = useState(false)
  const [bounds, setBounds] = useState({ height: 96, max: 480 })
  const min = 96
  function finish() {
    const pointerId = drag.current?.pointerId
    drag.current = null; setDragging(false)
    if (pointerId !== undefined && handle.current?.hasPointerCapture(pointerId)) handle.current.releasePointerCapture(pointerId)
  }
  useEffect(() => {
    const header = handle.current?.previousElementSibling
    const workspace = handle.current?.parentElement
    if (!header || !workspace) return
    const measure = () => {
      const max = Math.max(min, Math.floor(workspace.getBoundingClientRect().height - 160))
      setBounds({ height: Math.round(header.getBoundingClientRect().height), max })
      if (heightRef.current !== null && heightRef.current > max) onChange(max)
    }
    const observer = new ResizeObserver(measure)
    observer.observe(header); observer.observe(workspace); measure()
    window.addEventListener('blur', finish)
    window.addEventListener('resize', finish)
    return () => { observer.disconnect(); window.removeEventListener('blur', finish); window.removeEventListener('resize', finish) }
  }, [onChange])
  function resize(value: number) { onChange(Math.round(Math.max(min, Math.min(bounds.max, value)))) }
  return <div ref={handle} className={`qa-panel-divider qa-header-divider ${dragging ? 'is-dragging' : ''}`}
    role="separator" tabIndex={0} aria-label="调整场景信息区与 API 调用区高度" aria-orientation="horizontal"
    aria-valuemin={min} aria-valuemax={bounds.max} aria-valuenow={bounds.height}
    aria-valuetext={`场景信息区高度 ${bounds.height} 像素`}
    title="上下拖拽调整 · 双击恢复默认 · 方向键微调"
    onPointerDown={event => {
      if (event.button !== 0) return
      const header = event.currentTarget.previousElementSibling
      if (!header) return
      event.preventDefault(); event.currentTarget.focus()
      event.currentTarget.setPointerCapture(event.pointerId)
      drag.current = { pointerId: event.pointerId, y: event.clientY, height: header.getBoundingClientRect().height }
      setDragging(true)
    }} onPointerMove={event => {
      const start = drag.current
      if (start?.pointerId === event.pointerId) resize(start.height + event.clientY - start.y)
    }} onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}
    onDoubleClick={() => onChange(null)} onKeyDown={event => {
      if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return
      event.preventDefault()
      resize(event.key === 'Home' ? min : event.key === 'End' ? bounds.max
        : bounds.height + (event.key === 'ArrowDown' ? 1 : -1) * (event.shiftKey ? 50 : 10))
    }}><span /></div>
}
