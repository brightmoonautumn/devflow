import type { ComponentProps } from 'react'
import './form.css'

type Props = ComponentProps<'input'> & { mono?: boolean }

export function BaseInput({ className = '', mono, ...props }: Props) {
  return <input {...props} className={`df-control ${mono ? 'is-mono' : ''} ${className}`} />
}
