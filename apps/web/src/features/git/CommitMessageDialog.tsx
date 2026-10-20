import { useEffect, useId, useRef, useState } from 'react'
import { X } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseTextarea } from '../../components/form/BaseTextarea'

const changeTypes = [
  { value: 'feat', label: 'feat · 新功能' },
  { value: 'fix', label: 'fix · 修复问题' },
  { value: 'refactor', label: 'refactor · 代码重构' },
  { value: 'perf', label: 'perf · 性能优化' },
  { value: 'docs', label: 'docs · 文档更新' },
  { value: 'style', label: 'style · 代码格式' },
  { value: 'test', label: 'test · 测试' },
  { value: 'build', label: 'build · 构建与依赖' },
  { value: 'ci', label: 'ci · 持续集成' },
  { value: 'chore', label: 'chore · 日常维护' },
  { value: 'revert', label: 'revert · 撤销提交' },
]

function parseMessage(message: string) {
  const [header = '', ...lines] = message.replace(/\r\n?/g, '\n').split('\n')
  const match = /^([a-zA-Z][\w-]*)(?:\(([^()\n]+)\))?(!)?:\s+(.+)$/.exec(header)
  return { type: match?.[1] ?? 'feat', scope: match?.[2] ?? '', breaking: match?.[3] ?? '', summary: match?.[4] ?? header, body: lines.join('\n').replace(/^\n/, '') }
}

export function CommitMessageDialog({ message, onApply, onClose }: { message: string; onApply: (message: string) => void; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const id = useId()
  const [draft, setDraft] = useState(() => parseMessage(message))
  const scope = draft.scope.trim(), summary = draft.summary.trim()
  const formatted = `${draft.type}${scope ? `(${scope})` : ''}${draft.breaking}: ${summary}${draft.body.trim() ? `\n\n${draft.body.trim()}` : ''}`
  const options = changeTypes.some(option => option.value === draft.type) ? changeTypes : [...changeTypes, { value: draft.type, label: draft.type }]
  useEffect(() => { const element = dialog.current; element?.showModal(); return () => element?.close() }, [])

  return <dialog ref={dialog} className="ws-dialog git-message-dialog" aria-labelledby={`${id}-title`} onKeyDown={event => event.stopPropagation()} onCancel={event => { event.preventDefault(); onClose() }}>
    <header className="git-message-dialog-heading"><h2 id={`${id}-title`}>编写提交信息</h2><button type="button" className="ws-icon-button" aria-label="关闭提交信息表单" onClick={onClose}><X size={16} /></button></header>
    <form onSubmit={event => { event.preventDefault(); if (summary) onApply(formatted) }}>
      <div className="git-message-fields">
        <div className="git-message-header-fields">
          <BaseField label="提交类型" htmlFor={`${id}-type`}><BaseSelect id={`${id}-type`} value={draft.type} options={options} onValueChange={type => setDraft({ ...draft, type })} /></BaseField>
          <BaseField label="影响范围（可选）" htmlFor={`${id}-scope`}><BaseInput id={`${id}-scope`} value={draft.scope} pattern="[^()\r\n]*" placeholder="例如 r2dbc" onChange={event => setDraft({ ...draft, scope: event.target.value })} /></BaseField>
        </div>
        <BaseField label="简短描述" htmlFor={`${id}-summary`}><BaseInput autoFocus required id={`${id}-summary`} value={draft.summary} placeholder="例如 更新 R2dbcServiceImpl 忽略路径配置" onChange={event => setDraft({ ...draft, summary: event.target.value })} /></BaseField>
        <BaseField label="详细说明（可选）" htmlFor={`${id}-body`} hint="支持多行说明，可用 - 列出变更要点。"><BaseTextarea id={`${id}-body`} aria-describedby={`${id}-body-hint`} rows={6} value={draft.body} placeholder={'- 调整忽略字段配置\n- 简化属性过滤逻辑'} onChange={event => setDraft({ ...draft, body: event.target.value })} /></BaseField>
        <div className="git-message-preview"><span>消息预览</span><pre>{formatted}</pre></div>
      </div>
      <footer className="git-message-dialog-actions"><BaseButton onClick={onClose}>取消</BaseButton><BaseButton type="submit" variant="primary" disabled={!summary}>填入提交信息</BaseButton></footer>
    </form>
  </dialog>
}
