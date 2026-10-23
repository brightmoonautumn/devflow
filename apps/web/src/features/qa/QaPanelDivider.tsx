import { useEffect, useRef, useState } from 'react'

export function QaPanelDivider({ flowPercent, flowHeight, onChange }: {
  flowPercent: number; flowHeight: number; onChange: (value: number, stacked: boolean) => void
}) {
  const [stacked, setStacked] = useState(() => window.matchMedia('(max-width: 850px)').matches)
  const [dragging, setDragging] = useState(false)
  const drag = useRef<{ pointerId: number; position: number; value: number; width: number } | null>(null)
  const handle = useRef<HTMLDivElement>(null)
  function finish() {
    const pointerId = drag.current?.pointerId
    drag.current = null; setDragging(false)
    if (pointerId !== undefined && handle.current?.hasPointerCapture(pointerId)) handle.current.releasePointerCapture(pointerId)
  }
  useEffect(() => {
    const media = window.matchMedia('(max-width: 850px)')
    const change = () => { finish(); setStacked(media.matches) }
    media.addEventListener('change', change)
    window.addEventListener('blur', finish)
    window.addEventListener('resize', finish)
    return () => { media.removeEventListener('change', change); window.removeEventListener('blur', finish); window.removeEventListener('resize', finish) }
  }, [])
  const min = stacked ? 180 : 25
  const max = stacked ? 500 : 65
  const value = stacked ? flowHeight : flowPercent
  function resize(next: number) { onChange(Math.round(Math.max(min, Math.min(max, next))), stacked) }
  return <div ref={handle} className={`qa-panel-divider ${dragging ? 'is-dragging' : ''}`} role="separator" tabIndex={0}
    aria-label="调整 API 调用链与用户行为流程大小" aria-orientation={stacked ? 'horizontal' : 'vertical'}
    aria-valuemin={min} aria-valuemax={max} aria-valuenow={value} aria-valuetext={stacked ? `流程高度 ${value} 像素` : `流程宽度 ${value}%`}
    title="拖拽调整面板 · 双击恢复默认 · 方向键微调"
    onPointerDown={event => {
      if (event.button !== 0) return
      event.preventDefault(); event.currentTarget.focus()
      const width = event.currentTarget.parentElement!.getBoundingClientRect().width
      if (width <= 0) return
      event.currentTarget.setPointerCapture(event.pointerId)
      drag.current = { pointerId: event.pointerId, position: stacked ? event.clientY : event.clientX, value, width }
      setDragging(true)
    }} onPointerMove={event => {
      const start = drag.current
      if (!start || start.pointerId !== event.pointerId) return
      resize(stacked ? start.value + event.clientY - start.position : start.value - (event.clientX - start.position) / start.width * 100)
    }} onPointerUp={finish} onPointerCancel={finish} onLostPointerCapture={finish}
    onDoubleClick={() => resize(stacked ? 245 : 35)} onKeyDown={event => {
      const keys = stacked ? ['ArrowUp', 'ArrowDown'] : ['ArrowLeft', 'ArrowRight']
      if (![...keys, 'Home', 'End'].includes(event.key)) return
      event.preventDefault()
      const direction = stacked ? (event.key === 'ArrowDown' ? 1 : -1) : (event.key === 'ArrowLeft' ? 1 : -1)
      resize(event.key === 'Home' ? min : event.key === 'End' ? max : value + direction * (stacked ? 10 : 1) * (event.shiftKey ? 5 : 1))
    }}><span /></div>
}
