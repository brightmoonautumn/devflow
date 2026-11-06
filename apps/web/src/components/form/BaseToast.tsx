import { CheckCircle2, XCircle } from 'lucide-react'
import './toast.css'

export type ToastMessage = { id: number; text: string; tone: 'success' | 'error' }

export function BaseToast({ message }: { message: ToastMessage | null }) {
  return <div className="df-toast-region" aria-live="polite" aria-atomic="true">{message && <div key={message.id} className={`df-toast is-${message.tone}`} role={message.tone === 'error' ? 'alert' : 'status'}>{message.tone === 'error' ? <XCircle size={16} aria-hidden="true" /> : <CheckCircle2 size={16} aria-hidden="true" />}<span>{message.text}</span></div>}</div>
}
