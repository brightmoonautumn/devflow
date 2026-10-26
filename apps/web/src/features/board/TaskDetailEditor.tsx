import { useEffect, useId, useState, type FormEvent } from 'react'
import type { BoardFeature, BoardOperation, BoardTask } from './boardModel'

import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseTextarea } from '../../components/form/BaseTextarea'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseButton } from '../../components/form/BaseButton'
import { featureChoices, priorityChoices } from './boardForm'

type Draft = Pick<BoardTask, 'title' | 'feature' | 'priority' | 'description' | 'acceptance'>
const fields = (task: BoardTask): Draft => ({ title: task.title, feature: task.feature, priority: task.priority, description: task.description, acceptance: task.acceptance })
type Props = {
  task: BoardTask; features: BoardFeature[]; revision: number; busy: boolean; formId: string
  onDirty: (dirty: boolean) => void
  onCopyInstructions: () => void
  onSave: (operation: BoardOperation, revision: number) => Promise<boolean>
}

export function TaskDetailEditor({ task, features, revision, busy, formId, onDirty, onSave, onCopyInstructions }: Props) {
  const id = useId()
  const choices = featureChoices(features)
  const [draft, setDraft] = useState(() => fields(task))
  const [baseRevision, setBaseRevision] = useState(revision)
  const [dirty, setDirty] = useState(false)
  useEffect(() => {
    if (!dirty) { setDraft(fields(task)); setBaseRevision(revision) }
  }, [task, revision, dirty])
  useEffect(() => () => onDirty(false), [onDirty])
  function change(next: Partial<Draft>) {
    setDraft(current => ({ ...current, ...next })); setDirty(true); onDirty(true)
  }
  function reset() { setDraft(fields(task)); setBaseRevision(revision); setDirty(false); onDirty(false) }
  async function save(event: FormEvent) {
    event.preventDefault()
    if (!dirty || busy || !draft.title.trim()) return
    if (await onSave({ type: 'task_update', id: task.id, ...draft, title: draft.title.trim(), description: draft.description.trim(), acceptance: draft.acceptance.trim() }, baseRevision)) {
      setDirty(false); onDirty(false)
    }
  }
  return <form id={formId} className="kb-detail-editor" onSubmit={event => void save(event)} onKeyDown={event => { if ((event.ctrlKey || event.metaKey) && event.key === 'Enter' && !event.nativeEvent.isComposing) { event.preventDefault(); event.currentTarget.requestSubmit() } }}>
    <fieldset disabled={busy}>
      <BaseField htmlFor={`${id}-field-1`} label="任务标题"><BaseInput id={`${id}-field-1`} required maxLength={120} value={draft.title} onChange={e => change({ title: e.target.value })} /></BaseField>
      <div className="kb-form-row"><BaseField htmlFor={`${id}-field-2`} label="所属功能"><BaseSelect id={`${id}-field-2`} disabled={busy} value={draft.feature || choices.unclassified} onValueChange={value => change({ feature: value === choices.unclassified ? '' : value })} options={choices.withUnclassified} /></BaseField><BaseField htmlFor={`${id}-field-3`} label="优先级"><BaseSelect id={`${id}-field-3`} disabled={busy} value={draft.priority} onValueChange={value => { if (value === '高' || value === '普通') change({ priority: value }) }} options={priorityChoices} /></BaseField></div>
      <BaseField htmlFor={`${id}-field-4`} label="任务描述" action={<BaseButton disabled={busy || dirty} onClick={onCopyInstructions}>复制 AI 整理指令</BaseButton>}><BaseTextarea className="kb-task-description" id={`${id}-field-4`} rows={12} maxLength={4000} placeholder="先粗略记录，之后逐步补充…" value={draft.description} onChange={e => change({ description: e.target.value })} /></BaseField>
      <BaseField htmlFor={`${id}-field-5`} label="目标与验收"><BaseTextarea id={`${id}-field-5`} rows={3} maxLength={4000} placeholder="完成后如何判断达到目标" value={draft.acceptance} onChange={e => change({ acceptance: e.target.value })} /></BaseField>
      <div className="kb-inline-save"><span role="status">{dirty ? '有未保存修改' : '已保存'}</span>{dirty && <BaseButton type="button" onClick={reset}>撤销修改</BaseButton>}</div>
    </fieldset>
  </form>
}
