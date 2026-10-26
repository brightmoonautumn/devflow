import type { ReactNode } from 'react'
import './form.css'

export function BaseField({ htmlFor, label, hint, action, children }: { htmlFor: string; label: string; hint?: string; action?: ReactNode; children: ReactNode }) {
  return <div className="df-field">
    {action ? <div className="df-field-heading"><label className="df-field-label" htmlFor={htmlFor}>{label}</label>{action}</div> : <label className="df-field-label" htmlFor={htmlFor}>{label}</label>}
    {children}
    {hint && <small className="df-field-hint" id={`${htmlFor}-hint`}>{hint}</small>}
  </div>
}
