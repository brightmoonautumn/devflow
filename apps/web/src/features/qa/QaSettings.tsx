import { useEffect, useId, useRef, useState, type ReactNode } from 'react'
import { Menu, Plus, Trash2 } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import type { QaEnvironment, QaSseOptions } from './qaApi'

function SettingsDialog({ title, busy, onClose, onSave, children }: {
  title: string
  busy: boolean
  onClose: () => void
  onSave: () => Promise<void>
  children: ReactNode
}) {
  const dialog = useRef<HTMLDialogElement>(null)
  const id = useId()
  useEffect(() => {
    const element = dialog.current
    element?.showModal()
    return () => element?.close()
  }, [])
  return (
    <dialog ref={dialog} className="df-dialog qa-settings-dialog" aria-labelledby={id}
      onCancel={event => { event.preventDefault(); if (!busy) onClose() }}
      onKeyDown={event => event.stopPropagation()}>
      <form onSubmit={event => { event.preventDefault(); if (!busy) void onSave() }}>
        <div className="df-dialog-heading"><h2 id={id}>{title}</h2></div>
        <div className="qa-settings-fields">{children}</div>
        <footer className="df-dialog-actions">
          <BaseButton disabled={busy} onClick={onClose}>关闭</BaseButton>
          <BaseButton type="submit" variant="primary" disabled={busy}>{busy ? '保存中' : '保存'}</BaseButton>
        </footer>
      </form>
    </dialog>
  )
}

type VariableRow = { id: string; name: string; value: string }
type EnvironmentVariables = Record<string, Record<string, string>>
const storageKey = (workspaceKey: string) => `devflow.qa.environmentVariables:${workspaceKey}`
export function readQaEnvironmentVariables(workspaceKey: string): EnvironmentVariables {
  try { return JSON.parse(localStorage.getItem(storageKey(workspaceKey)) || '{}') as EnvironmentVariables } catch { return {} }
}

export function QaEnvironmentControl({ environments, value, disabled, workspaceKey, onChange, onSave, onVariablesChange, requiredVariables = [] }: {
  environments: QaEnvironment[]; value: string; disabled: boolean; workspaceKey: string
  onChange: (id: string) => void
  onSave: (environments: QaEnvironment[]) => Promise<boolean>
  onVariablesChange?: (variables: EnvironmentVariables) => void
  requiredVariables?: string[]
}) {
  const [draft, setDraft] = useState<QaEnvironment[] | null>(null)
  const [selected, setSelected] = useState(value)
  const [variables, setVariables] = useState<Record<string, VariableRow[]>>({})
  const [saving, setSaving] = useState(false)
  const [error, setError] = useState('')
  const current = draft?.find(item => item.id === selected)
  function open() {
    setError(''); setSelected(value); setDraft(environments.map(item => ({ ...item })))
    const stored = readQaEnvironmentVariables(workspaceKey)
    setVariables(Object.fromEntries(environments.map(item => {
      const values = stored[item.id] ?? {}
      const names = new Set([...Object.keys(values), ...requiredVariables])
      return [item.id, [...names].map(name => ({ id: crypto.randomUUID(), name, value: name === 'adminPassword' ? values[name] || 'admin' : values[name] ?? '' }))]
    })))
  }
  function edit(field: 'name' | 'baseUrl', text: string) {
    setDraft(items => items?.map(item => item.id === selected ? { ...item, [field]: text } : item) ?? null)
  }
  function editRow(id: string, field: 'name' | 'value', text: string) {
    setVariables(items => ({ ...items, [selected]: (items[selected] ?? []).map(row => row.id === id ? { ...row, [field]: text } : row) }))
  }
  async function save() {
    if (!draft) return
    const normalized = draft.map(item => ({ ...item, name: item.name.trim(), baseUrl: item.baseUrl.trim().replace(/\/?$/, '/') }))
    if (normalized.some(item => !item.name || !item.baseUrl)) { setError('请填写环境名称和基础 URL'); return }
    const entries: EnvironmentVariables = {}
    for (const item of normalized) {
      const rows = (variables[item.id] ?? []).filter(row => row.name.trim() || row.value)
      if (rows.length > 32 || rows.some(row => !/^[a-zA-Z_][a-zA-Z0-9_]*$/.test(row.name.trim()) || row.value.length > 4096) || new Set(rows.map(row => row.name.trim())).size !== rows.length) {
        setSelected(item.id); setError('变量名须以字母或下划线开头，且不能重复；每个环境最多 32 项'); return
      }
      entries[item.id] = Object.fromEntries(rows.map(row => [row.name.trim(), row.value]))
    }
    setError(''); setSaving(true)
    try {
      // Check storage availability before persisting the shared environment configuration.
      localStorage.setItem(storageKey(workspaceKey) + ':probe', '1'); localStorage.removeItem(storageKey(workspaceKey) + ':probe')
      if (await onSave(normalized)) {
        localStorage.setItem(storageKey(workspaceKey), JSON.stringify(entries)); onVariablesChange?.(entries); setDraft(null)
        if (!normalized.some(item => item.id === value)) onChange(normalized[0].id)
      } else setError('保存失败，请检查地址是否在允许范围内，或关闭后重试')
    } catch { setError('无法保存本地环境变量，请检查浏览器存储设置') } finally { setSaving(false) }
  }
  return <>
    <div className="qa-environment-control"><BaseSelect className="qa-environment-select" aria-label="运行环境" value={value} onValueChange={onChange}
      disabled={disabled} options={environments.map(item => ({ value: item.id, label: item.name }))} />
      <button type="button" className="qa-environment-manage" aria-label="管理环境" title="管理环境" disabled={disabled} onClick={open}><Menu size={16} /></button></div>
    {draft && <SettingsDialog title="环境管理" busy={saving} onClose={() => setDraft(null)} onSave={save}>
      <div className="qa-environment-layout"><nav className="qa-environment-list" aria-label="环境列表">
        {draft.map(item => <button type="button" key={item.id} className={selected === item.id ? 'active' : ''} onClick={() => setSelected(item.id)}>{item.name || '新环境'}</button>)}
        <button type="button" disabled={draft.length >= 20} onClick={() => { const id = crypto.randomUUID(); setDraft([...draft, { id, name: '', baseUrl: '' }]); setSelected(id) }}><Plus size={14} />新建环境</button>
      </nav>{current && <div className="qa-environment-editor">
        <div className="qa-environment-edit-row"><label>环境名称<BaseInput maxLength={40} value={current.name} onChange={event => edit('name', event.target.value)} /></label>
          <BaseButton variant="ghost" disabled={saving || draft.length === 1} aria-label={`删除环境 ${current.name}`} title="删除环境" onClick={() => { const next = draft.filter(item => item.id !== selected); setDraft(next); setSelected(next[0].id) }}><Trash2 size={15} /></BaseButton></div>
        <label>基础 URL<BaseInput type="url" mono value={current.baseUrl} placeholder="http://localhost:8081/prod-api/" onChange={event => edit('baseUrl', event.target.value)} /></label>
        <div className="qa-variables-heading"><strong>环境变量</strong><small>值仅保存在当前浏览器</small></div>
        {(variables[selected] ?? []).map(row => <div className="qa-variable-row" key={row.id}>
          <BaseInput aria-label="变量名" placeholder="变量名" value={row.name} onChange={event => editRow(row.id, 'name', event.target.value)} />
          <BaseInput aria-label={`${row.name || '变量'}的值`} type="password" autoComplete="off" placeholder="值" value={row.value} onChange={event => editRow(row.id, 'value', event.target.value)} />
          <BaseButton variant="ghost" aria-label={`删除变量 ${row.name}`} onClick={() => setVariables(items => ({ ...items, [selected]: items[selected].filter(item => item.id !== row.id) }))}><Trash2 size={14} /></BaseButton>
        </div>)}
        <BaseButton variant="ghost" disabled={(variables[selected] ?? []).length >= 32} onClick={() => setVariables(items => ({ ...items, [selected]: [...(items[selected] ?? []), { id: crypto.randomUUID(), name: '', value: '' }] }))}><Plus size={14} />添加变量</BaseButton>
      </div>}</div>
      {error && <p className="qa-error" role="alert">{error}</p>}
    </SettingsDialog>}
  </>
}

export function QaResponseSettings({ initial, onSave, onClose }: {
  initial?: QaSseOptions
  onSave: (sse?: QaSseOptions) => Promise<boolean>
  onClose: () => void
}) {
  const [mode, setMode] = useState(initial ? 'sse' : 'json')
  const [endEvent, setEndEvent] = useState(initial?.endEvent ?? '')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const id = useId()
  async function save() {
    if (mode === 'sse' && !/^[a-zA-Z0-9._:-]{1,128}$/.test(endEvent.trim())) {
      setError('请输入有效的结束事件名')
      return
    }
    setBusy(true)
    setError('')
    try {
      if (await onSave(mode === 'sse' ? { endEvent: endEvent.trim() } : undefined)) onClose()
      else setError('响应设置保存失败，请关闭后重试')
    } finally { setBusy(false) }
  }
  return <SettingsDialog title="响应设置" busy={busy} onClose={onClose} onSave={save}>
    <label htmlFor={id}>响应类型</label>
    <BaseSelect id={id} value={mode} onValueChange={setMode} options={[{ value: 'json', label: '普通 JSON' }, { value: 'sse', label: 'SSE 流式' }]} />
    {mode === 'sse' && <label>结束事件<BaseInput required mono maxLength={128}
      placeholder="run.completed" value={endEvent} onChange={event => setEndEvent(event.target.value)} /></label>}
    {error && <p className="qa-error" role="alert">{error}</p>}
  </SettingsDialog>
}
