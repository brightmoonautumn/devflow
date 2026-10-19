import * as Select from '@radix-ui/react-select'
import { Check, ChevronDown, ChevronUp } from 'lucide-react'
import { useState } from 'react'
import './form.css'

type Props = {
  id?: string
  name?: string
  value: string
  onValueChange: (value: string) => void
  options: { value: string; label: string }[]
  placeholder?: string
  required?: boolean
  disabled?: boolean
  mono?: boolean
  className?: string
  'aria-label'?: string
  'aria-describedby'?: string
}

/** Keep vendor primitives, popup placement and theme inheritance behind our API. */
export function BaseSelect({ id, name, value, onValueChange, options, placeholder = '请选择', required, disabled, mono, className = '', ...aria }: Props) {
  const [trigger, setTrigger] = useState<HTMLButtonElement | null>(null)
  const [open, setOpen] = useState(false)
  // Native modal dialogs are in the top layer; a body portal would be inert behind them.
  const host = trigger?.closest<HTMLElement>('dialog, .workspace-shell') ?? trigger?.ownerDocument.body
  const selected = options.find(option => option.value === value)
  return <Select.Root name={name} value={selected ? value : ''} onValueChange={onValueChange} required={required} disabled={disabled} open={open} onOpenChange={setOpen}>
    <Select.Trigger {...aria} ref={setTrigger} id={id} className={`df-control df-select ${mono ? 'is-mono' : ''} ${className}`} title={selected?.label}>
      <Select.Value placeholder={placeholder}>{selected?.label}</Select.Value>
      <Select.Icon className="df-select-icon"><ChevronDown size={15} /></Select.Icon>
    </Select.Trigger>
    {host && <Select.Portal container={host}>
      <Select.Content className={`df-select-content ${mono ? 'is-mono' : ''}`} position="popper" sideOffset={5} collisionPadding={12} onEscapeKeyDown={event => {
        // Close the list first, leaving its parent dialog and output panel open.
        event.preventDefault()
        event.stopPropagation()
        setOpen(false)
      }}>
        <Select.ScrollUpButton className="df-select-scroll"><ChevronUp size={14} /></Select.ScrollUpButton>
        <Select.Viewport className="df-select-viewport">
          {options.map(option => <Select.Item className="df-select-item" key={option.value} value={option.value}>
            <Select.ItemText>{option.label}</Select.ItemText>
            <Select.ItemIndicator className="df-select-indicator"><Check size={14} /></Select.ItemIndicator>
          </Select.Item>)}
        </Select.Viewport>
        <Select.ScrollDownButton className="df-select-scroll"><ChevronDown size={14} /></Select.ScrollDownButton>
      </Select.Content>
    </Select.Portal>}
  </Select.Root>
}
