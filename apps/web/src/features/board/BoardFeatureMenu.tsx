import { useEffect, useRef, useState } from 'react'
import { Check, ChevronDown, Pencil, Plus, Trash2, X } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseInput } from '../../components/form/BaseInput'
import { useDialogs } from '../../components/form/BaseDialog'
import type { BoardFeature, BoardOperation, BoardTask } from './boardModel'

type Props = { features: BoardFeature[]; featureColors: Map<string, number>; tasks: BoardTask[]; value: string; busy: boolean; onChange: (value: string) => void; onSave: (operation: BoardOperation, revision?: number) => Promise<boolean>; revision: () => number | undefined }

export function BoardFeatureMenu({ features, featureColors, tasks, value, busy, onChange, onSave, revision }: Props) {
  const [open, setOpen] = useState(false)
  const [editing, setEditing] = useState<string | null>(null)
  const [name, setName] = useState('')
  const [editRevision, setEditRevision] = useState<number | undefined>()
  const [deleting, setDeleting] = useState<string | null>(null)
  const [target, setTarget] = useState('')
  const root = useRef<HTMLDivElement>(null)
  const dialogs = useDialogs()
  const label = value === 'all' ? '全部分类' : features.find(f => f.id === value)?.name ?? '未归类'
  useEffect(() => {
    if (!open) return
    const close = (event: PointerEvent) => { if (!root.current?.contains(event.target as Node)) setOpen(false) }
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.preventDefault(); event.stopPropagation(); if (editing || deleting) { setEditing(null); setDeleting(null) } else { setOpen(false); root.current?.querySelector<HTMLButtonElement>('.kb-feature-trigger')?.focus() } } }
    document.addEventListener('pointerdown', close); document.addEventListener('keydown', key, true)
    return () => { document.removeEventListener('pointerdown', close); document.removeEventListener('keydown', key, true) }
  }, [open, editing, deleting])
  function edit(id: string, current: string) { setEditing(id); setName(current); setEditRevision(revision()); setDeleting(null) }
  async function save() {
    const trimmed = name.trim()
    if (!trimmed || !editing) return
    const existing = features.find(f => f.id === editing)
    const feature = existing ? { ...existing, name: trimmed } : { id: crypto.randomUUID(), name: trimmed, description: '', goal: '', x: 100 + features.length % 3 * 350, y: 100 + Math.floor(features.length / 3) * 320 }
    if (await onSave({ type: 'feature_save', feature }, editRevision)) { setEditing(null); if (!existing) onChange(feature.id) }
  }
  async function remove(feature: BoardFeature) {
    const count = tasks.filter(t => t.feature === feature.id).length
    if (count && deleting !== feature.id) { setDeleting(feature.id); setTarget(''); setEditing(null); return }
    if (count && !target) return
    const expected = revision()
    setOpen(false)
    if (!await dialogs.confirm({ title: '删除功能', description: `删除「${feature.name}」及其关联线${count ? `，并迁移 ${count} 个任务到「${features.find(f => f.id === target)?.name}」` : ''}？`, tone: 'danger', confirmText: '删除' })) return
    if (await onSave({ type: 'feature_delete', id: feature.id, moveTo: count ? target : undefined }, expected)) { setDeleting(null); if (value === feature.id) onChange('all') }
  }
  return <div ref={root} className="kb-feature-menu">
    <button type="button" className="df-control kb-feature-trigger" aria-label="筛选分类" aria-expanded={open} aria-haspopup="true" disabled={busy} onClick={() => setOpen(v => !v)}><span className="kb-feature-label">{value !== 'all' && <i className={`kb-feature-color-dot kb-feature-color-${featureColors.get(value) ?? 'none'}`} aria-hidden="true" />}<span>{label}</span></span><ChevronDown size={15} /></button>
    {open && <div className="kb-feature-popover" role="group" aria-label="分类筛选和管理">
      {[{ id: 'all', name: '全部分类' }, { id: '', name: '未归类' }].map(item => <button type="button" className="kb-feature-option" key={item.id} onClick={() => { onChange(item.id); setOpen(false) }}><span className="kb-feature-label">{item.id === '' && <i className="kb-feature-color-dot" aria-hidden="true" />}<span>{item.name}</span></span>{value === item.id && <Check size={14} />}</button>)}
      <div className="kb-feature-options">{features.map(feature => <div className="kb-feature-option-group" key={feature.id}>
        {editing === feature.id ? <div className="kb-feature-inline"><BaseInput autoFocus aria-label={`编辑${feature.name}名称`} maxLength={60} value={name} onChange={e => setName(e.target.value)} onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); void save() } }} /><BaseButton variant="ghost" disabled={busy || !name.trim()} aria-label="保存名称" onClick={() => void save()}><Check size={15} /></BaseButton><BaseButton variant="ghost" aria-label="取消编辑" onClick={() => setEditing(null)}><X size={15} /></BaseButton></div> : <div className="kb-feature-option-row"><button type="button" className="kb-feature-option" onClick={() => { onChange(feature.id); setOpen(false) }}><span className="kb-feature-label"><i className={`kb-feature-color-dot kb-feature-color-${featureColors.get(feature.id) ?? 'none'}`} aria-hidden="true" /><span>{feature.name}</span></span>{value === feature.id && <Check size={14} />}</button><BaseButton variant="ghost" disabled={busy} aria-label={`重命名${feature.name}`} title="重命名" onClick={() => edit(feature.id, feature.name)}><Pencil size={14} /></BaseButton><BaseButton variant="ghost" disabled={busy} aria-label={`删除${feature.name}`} title="删除" onClick={() => void remove(feature)}><Trash2 size={14} /></BaseButton></div>}
        {deleting === feature.id && <div className="kb-feature-migrate"><label htmlFor={`move-${feature.id}`}>删除前迁移 {tasks.filter(t => t.feature === feature.id).length} 个任务</label><select className="df-control" id={`move-${feature.id}`} value={target} onChange={e => setTarget(e.target.value)}><option value="">{features.length > 1 ? '选择目标功能' : '没有可用的目标功能'}</option>{features.filter(f => f.id !== feature.id).map(f => <option key={f.id} value={f.id}>{f.name}</option>)}</select><div><BaseButton variant="ghost" onClick={() => setDeleting(null)}>取消</BaseButton><BaseButton variant="danger" disabled={busy || !target} onClick={() => void remove(feature)}>删除</BaseButton></div></div>}
      </div>)}</div>
      {editing === '__new__' ? <div className="kb-feature-inline"><BaseInput autoFocus aria-label="新功能名称" maxLength={60} value={name} onChange={e => setName(e.target.value)} onKeyDown={e => { if (e.key === 'Enter') { e.preventDefault(); void save() } }} /><BaseButton variant="ghost" disabled={busy || !name.trim()} aria-label="创建功能" onClick={() => void save()}><Check size={15} /></BaseButton><BaseButton variant="ghost" aria-label="取消新建" onClick={() => setEditing(null)}><X size={15} /></BaseButton></div> : <button type="button" className="kb-feature-add" disabled={busy} onClick={() => edit('__new__', '')}><Plus size={14} />新建功能</button>}
    </div>}
  </div>
}
