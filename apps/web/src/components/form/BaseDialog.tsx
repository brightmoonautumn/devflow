import { createContext, useCallback, useContext, useEffect, useId, useMemo, useRef, useState, type ReactNode } from 'react'
import { createPortal } from 'react-dom'
import { AlertTriangle, Info } from 'lucide-react'
import { BaseButton } from './BaseButton'
import './dialog.css'

type DialogOptions = { title: string; description: string; confirmText?: string; cancelText?: string; tone?: 'default' | 'danger' }
type DialogRequest = { id: number; options: DialogOptions; kind: 'confirm' | 'alert'; resolve: (result: boolean) => void }
type DialogApi = { confirm: (options: DialogOptions) => Promise<boolean>; alert: (options: DialogOptions) => Promise<void> }
const DialogContext = createContext<DialogApi | null>(null)

/** Business views depend only on this API, not the modal implementation. */
export function useDialogs(): DialogApi {
  const value = useContext(DialogContext)
  if (!value) throw new Error('useDialogs requires BaseDialogProvider')
  return value
}

export function BaseDialogProvider({ children }: { children: ReactNode }) {
  const [requests, setRequests] = useState<DialogRequest[]>([])
  const queue = useRef<DialogRequest[]>([])
  const sequence = useRef(0)
  const mounted = useRef(true)
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false; queue.current.forEach(request => request.resolve(false)); queue.current = [] }
  }, [])
  const ask = useCallback((kind: DialogRequest['kind'], options: DialogOptions) => new Promise<boolean>(resolve => {
    if (!mounted.current) { resolve(false); return }
    queue.current = [...queue.current, { id: ++sequence.current, kind, options, resolve }]
    setRequests(queue.current)
  }), [])
  const finish = useCallback((id: number, accepted: boolean) => {
    const request = queue.current[0]
    if (!request || request.id !== id) return
    queue.current = queue.current.slice(1)
    setRequests(queue.current)
    request.resolve(accepted)
  }, [])
  const api = useMemo<DialogApi>(() => ({ confirm: options => ask('confirm', options), alert: async options => { await ask('alert', options) } }), [ask])
  const active = requests[0]
  // Keep the modal in the themed shell while native dialog provides focus containment.
  const host = active ? document.querySelector('.workspace-shell') : null
  return <DialogContext.Provider value={api}>{children}{active && host && createPortal(<DialogView key={active.id} request={active} onFinish={finish} />, host)}</DialogContext.Provider>
}

function DialogView({ request, onFinish }: { request: DialogRequest; onFinish: (id: number, accepted: boolean) => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const id = useId()
  const { options, kind } = request
  const danger = options.tone === 'danger'
  useEffect(() => {
    const element = dialog.current
    element?.showModal()
    return () => element?.close()
  }, [])
  return <dialog ref={dialog} className={`df-dialog ${danger ? 'is-danger' : ''}`} role={kind === 'confirm' ? 'alertdialog' : 'dialog'} aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`} onKeyDown={event => event.stopPropagation()} onCancel={event => { event.preventDefault(); event.stopPropagation(); onFinish(request.id, false) }}>
    <div className="df-dialog-heading"><span className="df-dialog-icon" aria-hidden="true">{danger ? <AlertTriangle size={19} /> : <Info size={19} />}</span><h2 id={`${id}-title`}>{options.title}</h2></div>
    <p className="df-dialog-description" id={`${id}-description`}>{options.description}</p>
    <footer className="df-dialog-actions">
      {kind === 'confirm' && <BaseButton autoFocus onClick={() => onFinish(request.id, false)}>{options.cancelText ?? '取消'}</BaseButton>}
      <BaseButton autoFocus={kind === 'alert'} variant={danger ? 'danger' : 'primary'} onClick={() => onFinish(request.id, true)}>{options.confirmText ?? (kind === 'alert' ? '知道了' : '确定')}</BaseButton>
    </footer>
  </dialog>
}
