import { useEffect, useRef, useState } from 'react'
import { Check, ChevronDown, Pencil, Plus, Trash2, X } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseInput } from '../../components/form/BaseInput'
import { useDialogs } from '../../components/form/BaseDialog'
import '../../components/form/managed-choice-menu.css'
import type { IdeaCanvas, IdeasOperation } from './model'

type Props = {
  canvases: IdeaCanvas[]; value: string; busy: boolean; revision: number
  interaction: { current: boolean }
  onChange: (id: string) => void
  onSave: (operation: IdeasOperation, revision: number) => Promise<boolean>
}

export function IdeasCanvasMenu({ canvases, value, busy, revision, interaction, onChange, onSave }: Props) {
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<{ id?: string; revision: number } | null>(null)
  const [name, setName] = useState('')
  const root = useRef<HTMLDivElement>(null)
  const dialogs = useDialogs()
  useEffect(() => {
    if (!open) return
    interaction.current = true
    const close = () => { setOpen(false); setEditing(null) }
    const outside = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) close() }
    const escape = (event: KeyboardEvent) => {
      if (event.key !== 'Escape') return
      event.preventDefault(); event.stopPropagation()
      if (editing) setEditing(null)
      else { close(); root.current?.querySelector<HTMLButtonElement>('.kb-feature-trigger')?.focus() }
    }
    document.addEventListener('pointerdown', outside); document.addEventListener('keydown', escape, true)
    return () => { interaction.current = false; document.removeEventListener('pointerdown', outside); document.removeEventListener('keydown', escape, true) }
  }, [open, editing, interaction])

  function edit(canvas?: IdeaCanvas) { setEditing({ id: canvas?.id, revision }); setName(canvas?.name ?? '') }
  async function save() {
    const trimmed = name.trim()
    if (!editing || !trimmed || busy) return
    const existing = editing.id ? canvases.find(c => c.id === editing.id) : undefined
    if (editing.id && !existing) return
    const canvas: IdeaCanvas = existing ? { ...existing, name: trimmed } : { id: crypto.randomUUID(), name: trimmed, nodes: [], edges: [], updatedAt: 0 }
    if (await onSave({ type: 'canvas_save', canvas }, editing.revision)) {
      setEditing(null)
      if (!existing) { onChange(canvas.id); setOpen(false) }
    }
  }
  async function remove(canvas: IdeaCanvas) {
    const expected = revision
    if (!await dialogs.confirm({ title: '删除画布', description: `永久删除「${canvas.name}」及全部节点、图片和原型？`, confirmText: '删除画布', tone: 'danger' })) return
    if (await onSave({ type: 'canvas_delete', id: canvas.id }, expected)) { setEditing(null); setOpen(false) }
  }
  function editor(creating: boolean) {
    return <div className="kb-feature-inline"><BaseInput autoFocus aria-label={creating ? '新画布名称' : '编辑画布名称'} maxLength={80} value={name} onChange={e => setName(e.target.value)} onKeyDown={e => { if (e.key === 'Enter' && !e.nativeEvent.isComposing) { e.preventDefault(); void save() } }} /><BaseButton variant="ghost" disabled={busy || !name.trim()} aria-label={creating ? '创建画布' : '保存名称'} onClick={() => void save()}><Check size={15} /></BaseButton><BaseButton variant="ghost" aria-label="取消编辑" onClick={() => setEditing(null)}><X size={15} /></BaseButton></div>
  }
  return <div ref={root} className="kb-feature-menu ideas-canvas-menu">
    <button type="button" className="df-control kb-feature-trigger" aria-label="选择和管理画布" aria-expanded={open} aria-haspopup="true" disabled={busy} onClick={() => { setOpen(v => !v); setEditing(null) }}><span className="kb-feature-label"><span>{canvases.find(c => c.id === value)?.name ?? '构思'}</span></span><ChevronDown size={15} /></button>
    {open && <div className="kb-feature-popover" role="group" aria-label="画布选择和管理">
      <div className="kb-feature-options">{canvases.map(canvas => <div className="kb-feature-option-group" key={canvas.id}>
        {editing?.id === canvas.id ? editor(false) : <div className="kb-feature-option-row"><button type="button" className="kb-feature-option" disabled={busy} onClick={() => { onChange(canvas.id); setOpen(false); setEditing(null) }}><span className="kb-feature-label"><span>{canvas.name}</span></span>{value === canvas.id && <Check size={14} />}</button><BaseButton variant="ghost" disabled={busy} aria-label={`重命名${canvas.name}`} title="重命名" onClick={() => edit(canvas)}><Pencil size={14} /></BaseButton><BaseButton variant="ghost" disabled={busy} aria-label={`删除${canvas.name}`} title="删除" onClick={() => void remove(canvas)}><Trash2 size={14} /></BaseButton></div>}
      </div>)}</div>
      {editing && !editing.id ? editor(true) : <button type="button" className="kb-feature-add" disabled={busy} onClick={() => edit()}><Plus size={14} />新建画布</button>}
    </div>}
  </div>
}
