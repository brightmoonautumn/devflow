import { useEffect, useId, useRef } from 'react'
import { History, X } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { activityText } from './boardModel'

export function BoardActivityDrawer({ activity, onClose }: { activity: string[]; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const titleId = useId()
  useEffect(() => {
    const element = dialog.current
    const previous = document.activeElement as HTMLElement | null
    element?.showModal()
    return () => { element?.close(); if (previous?.isConnected) previous.focus() }
  }, [])
  return <dialog ref={dialog} className="kb-activity-drawer" aria-labelledby={titleId} onCancel={event => { event.preventDefault(); onClose() }} onClick={event => {
    if (event.target !== event.currentTarget) return
    const rect = event.currentTarget.getBoundingClientRect()
    if (event.clientX < rect.left || event.clientX > rect.right || event.clientY < rect.top || event.clientY > rect.bottom) onClose()
  }}>
    <header className="kb-activity-drawer-header"><h2 id={titleId}><History size={16} aria-hidden="true" />看板活动记录</h2><BaseButton variant="ghost" className="kb-activity-drawer-close" aria-label="关闭活动记录" onClick={onClose}><X size={17} /></BaseButton></header>
    <div className="kb-activity-drawer-body">{activity.length ? <ol className="kb-activity-timeline">{[...activity].reverse().map((entry, index) => {
      const match = /^(\d+) · (.*?) · ([\s\S]*)$/.exec(entry)
      const date = match ? new Date(Number(match[1]) * 1000) : null
      const valid = date !== null && Number.isFinite(date.getTime())
      const formatted = valid ? activityText(entry) : entry
      return <li key={index}><span className="kb-activity-marker" aria-hidden="true" /><div className="kb-activity-entry">{match && valid && date ? <><div className="kb-activity-entry-meta"><span>{match[2]}</span><time dateTime={date.toISOString()}>{formatted.slice(0, formatted.indexOf(' · '))}</time></div><p>{match[3]}</p></> : <p>{formatted}</p>}</div></li>
    })}</ol> : <div className="kb-activity-empty">暂无活动记录</div>}</div>
  </dialog>
}
