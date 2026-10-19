import type { ReactNode } from 'react'
import './form.css'

export function BaseField({ htmlFor, label, hint, children }: { htmlFor: string; label: string; hint?: string; children: ReactNode }) {
  return <div className="df-field">
    <label className="df-field-label" htmlFor={htmlFor}>{label}</label>
    {children}
    {hint && <small className="df-field-hint" id={`${htmlFor}-hint`}>{hint}</small>}
  </div>
}
