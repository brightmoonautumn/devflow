import type { ComponentProps } from 'react'
import './form.css'

type Props = ComponentProps<'button'> & { variant?: 'primary' | 'secondary' }

export function BaseButton({ variant = 'secondary', className = '', type = 'button', ...props }: Props) {
  return <button {...props} type={type} className={`df-button is-${variant} ${className}`} />
}
