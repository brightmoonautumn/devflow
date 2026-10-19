import type { ComponentProps } from 'react'
import './form.css'

type Props = ComponentProps<'textarea'> & { mono?: boolean }

export function BaseTextarea({ className = '', mono, ...props }: Props) {
  return <textarea {...props} className={`df-control df-textarea ${mono ? 'is-mono' : ''} ${className}`} />
}
