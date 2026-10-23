import { useEffect, useLayoutEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'

export type FileTabMenuAction = 'current' | 'others' | 'right' | 'all'
export function FileTabMenu({ container, x, y, items, onAction, onClose }: {
  container: Element; x: number; y: number; items: { action: FileTabMenuAction; label: string; disabled: boolean }[]
  onAction: (action: FileTabMenuAction) => void; onClose: (restoreFocus: boolean) => void
}) {
  const ref = useRef<HTMLDivElement>(null)
  const [position, setPosition] = useState({ left: x, top: y })
  useLayoutEffect(() => {
    const rect = ref.current!.getBoundingClientRect()
    setPosition({ left: Math.max(8, Math.min(x, window.innerWidth - rect.width - 8)), top: Math.max(8, Math.min(y, window.innerHeight - rect.height - 8)) })
    const first = ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')
    if (first) first.focus(); else ref.current?.focus()
  }, [x, y])
  useEffect(() => {
    const outside = (event: PointerEvent) => { if (!ref.current?.contains(event.target as Node)) onClose(false) }
    const dismiss = () => onClose(false)
    const scroll = (event: Event) => { if (!ref.current?.contains(event.target as Node)) dismiss() }
    document.addEventListener('pointerdown', outside)
    window.addEventListener('resize', dismiss); window.addEventListener('blur', dismiss); window.addEventListener('scroll', scroll, true)
    return () => { document.removeEventListener('pointerdown', outside); window.removeEventListener('resize', dismiss); window.removeEventListener('blur', dismiss); window.removeEventListener('scroll', scroll, true) }
  }, [onClose])
  return createPortal(<div ref={ref} role="menu" tabIndex={-1} aria-label="标签操作" className="file-tab-menu" style={position} onContextMenu={event => event.preventDefault()} onKeyDown={event => {
    if (event.key === 'Escape' || event.key === 'Tab') { event.preventDefault(); event.stopPropagation(); onClose(true); return }
    if (!['ArrowUp', 'ArrowDown', 'Home', 'End'].includes(event.key)) return
    event.preventDefault()
    const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)'))
    const index = buttons.indexOf(document.activeElement as HTMLButtonElement)
    const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : (index + (event.key === 'ArrowDown' ? 1 : -1) + buttons.length) % buttons.length
    buttons[next]?.focus()
  }}>{items.map(item => <button key={item.action} role="menuitem" disabled={item.disabled} onClick={() => onAction(item.action)}>{item.label}</button>)}</div>, container)
}
