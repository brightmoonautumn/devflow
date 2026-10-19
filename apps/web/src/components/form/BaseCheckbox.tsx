import * as Checkbox from '@radix-ui/react-checkbox'
import { Check } from 'lucide-react'
import { useId } from 'react'
import './form.css'

type Props = { label: string; checked: boolean; onCheckedChange: (checked: boolean) => void; disabled?: boolean }

export function BaseCheckbox({ label, checked, onCheckedChange, disabled }: Props) {
  const id = useId()
  return <div className="df-checkbox-field" data-disabled={disabled || undefined}>
    <Checkbox.Root id={id} className="df-checkbox" checked={checked} onCheckedChange={value => onCheckedChange(value === true)} disabled={disabled}>
      <Checkbox.Indicator className="df-checkbox-indicator"><Check size={12} strokeWidth={2.5} /></Checkbox.Indicator>
    </Checkbox.Root>
    <label htmlFor={id}>{label}</label>
  </div>
}
