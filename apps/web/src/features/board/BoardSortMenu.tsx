import { useEffect, useId, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { ArrowDownWideNarrow, ArrowUpNarrowWide, Check, GripVertical } from 'lucide-react'
import type { BoardSort, TaskStatus } from './boardModel'

export function BoardSortMenu({ status, label, value, disabled, onChange }: {
  status: TaskStatus; label: string; value: BoardSort; disabled: boolean; onChange: (value: BoardSort) => void
}) {
  const id = useId()
  const trigger = useRef<HTMLButtonElement>(null)
  const menu = useRef<HTMLDivElement>(null)
  const [open, setOpen] = useState(false)
  const [position, setPosition] = useState({ left: 0, top: 0 })
  const timeLabel = status === 'todo' ? '创建时间' : status === 'done' ? '完成时间' : '进入阶段时间'
  const options = [
    { value: 'manual' as const, label: '手动顺序', Icon: GripVertical },
    { value: 'time_desc' as const, label: '最新优先', Icon: ArrowDownWideNarrow },
    { value: 'time_asc' as const, label: '最早优先', Icon: ArrowUpNarrowWide },
  ]
  const selected = options.find(option => option.value === value)!
  const Icon = selected.Icon
  useLayoutEffect(() => {
    if (!open) return
    const anchor = trigger.current!.getBoundingClientRect()
    const popup = menu.current!.getBoundingClientRect()
    setPosition({ left: Math.max(8, Math.min(anchor.right - popup.width, window.innerWidth - popup.width - 8)), top: Math.max(8, Math.min(anchor.bottom + 6, window.innerHeight - popup.height - 8)) })
    menu.current?.querySelector<HTMLButtonElement>('[aria-checked="true"]')?.focus()
  }, [open])
  useEffect(() => {
    if (!open) return
    const outside = (event: PointerEvent) => {
      if (!menu.current?.contains(event.target as Node) && !trigger.current?.contains(event.target as Node)) setOpen(false)
    }
    const dismiss = () => setOpen(false)
    const scroll = (event: Event) => { if (!menu.current?.contains(event.target as Node)) dismiss() }
    document.addEventListener('pointerdown', outside)
    window.addEventListener('resize', dismiss); window.addEventListener('scroll', scroll, true); window.addEventListener('blur', dismiss)
    return () => {
      document.removeEventListener('pointerdown', outside)
      window.removeEventListener('resize', dismiss); window.removeEventListener('scroll', scroll, true); window.removeEventListener('blur', dismiss)
    }
  }, [open])
  useEffect(() => { if (disabled) setOpen(false) }, [disabled])
  function close() { setOpen(false); trigger.current?.focus() }
  const host = trigger.current?.closest('.workspace-shell') ?? document.body
  return <>
    <button ref={trigger} type="button" className={`kb-sort-trigger ${value !== 'manual' ? 'is-active' : ''}`} disabled={disabled}
      aria-label={`${label}排序：${selected.label}`} title={`排序：${selected.label} · ${value === 'manual' ? '拖动卡片调整顺序' : timeLabel}`}
      aria-haspopup="menu" aria-expanded={open} aria-controls={open ? id : undefined} onClick={() => setOpen(current => !current)}
      onKeyDown={event => { if (event.key === 'ArrowDown' || event.key === 'ArrowUp') { event.preventDefault(); setOpen(true) } }}><Icon size={15} /></button>
    {open && createPortal(<div ref={menu} id={id} role="menu" aria-label={`${label}排序`} className="kb-sort-menu" style={position} onKeyDown={event => {
      if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); close(); return }
      if (event.key === 'Tab') { close(); return }
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
      event.preventDefault(); event.stopPropagation()
      const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button'))
      const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
      buttons[next]?.focus()
    }}>
      <div className="kb-sort-menu-label">排序方式 · {timeLabel}</div>
      {options.map(option => <button key={option.value} type="button" role="menuitemradio" aria-checked={value === option.value}
        onClick={() => { onChange(option.value); close() }}><option.Icon size={14} /><span>{option.label}</span>{value === option.value && <Check size={14} />}</button>)}
    </div>, host)}
  </>
}
