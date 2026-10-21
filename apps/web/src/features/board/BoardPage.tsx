import { useEffect, useRef, useState, type FormEvent } from 'react'
import { ArrowLeft, ArrowRight, Check, ChevronRight, Circle, CircleCheck, Clock3, Flag, Kanban, LockKeyhole, Plus, RotateCcw, Sparkles, Workflow, X } from 'lucide-react'
import { BoardCanvas } from './BoardCanvas'
import { readBoardDraft, statuses, type BoardDraft, type BoardTask, type TaskStatus } from './boardModel'
import './board.css'

type Props = { workspaceName: string; workspaceKey: string; query: string; onQueryChange: (query: string) => void; onSearchFocus: () => void }
export function BoardPage({ workspaceName, workspaceKey, query, onQueryChange: setQuery, onSearchFocus }: Props) {
  const [draft, setDraft] = useState<BoardDraft>(() => readBoardDraft(workspaceKey))
  const { tasks, features } = draft
  const [view, setView] = useState<'flow' | 'architecture'>('flow')
  const [feature, setFeature] = useState('all')
  const [selected, setSelected] = useState<string | null>(null)
  const [creating, setCreating] = useState(false)
  const [notice, setNotice] = useState('')
  const [storageFailed, setStorageFailed] = useState(false)
  const [mobileStatus, setMobileStatus] = useState<TaskStatus>('todo')
  const newTaskTrigger = useRef<HTMLButtonElement | null>(null)
  const [creationStatus, setCreationStatus] = useState<TaskStatus>('todo')
  const [draggedTask, setDraggedTask] = useState<string | null>(null)
  const [dropStatus, setDropStatus] = useState<TaskStatus | null>(null)
  const detailRef = useRef<HTMLElement>(null)
  useEffect(() => {
    try { localStorage.setItem(`devflow.board.prototype.v1:${workspaceKey}`, JSON.stringify(draft)); setStorageFailed(false) }
    catch { setStorageFailed(true) }
  }, [draft, workspaceKey])
  useEffect(() => { if (!notice) return; const timer = setTimeout(() => setNotice(''), 3500); return () => clearTimeout(timer) }, [notice])
  useEffect(() => { if (selected) detailRef.current?.focus() }, [selected])
  const activeTask = tasks.find(t => t.id === selected)
  const activeFeature = features.find(f => f.id === feature)
  const visible = tasks.filter(t => (feature === 'all' || t.feature === feature) && `${t.id} ${t.title} ${t.description}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  function updateTask(id: string, update: Partial<BoardTask>, activity?: string) {
    setDraft(current => ({ ...current, tasks: current.tasks.map(t => t.id === id ? { ...t, ...update, activity: activity ? [...t.activity, activity] : t.activity } : t) }))
  }
  function moveTask(id: string, status: TaskStatus) {
    const task = tasks.find(t => t.id === id)
    if (!task || task.status === status) return
    if (task.blocked && status === 'doing') { setNotice('请先在任务详情中解除阻塞'); return }
    updateTask(id, { status, owner: status === 'doing' ? task.owner ?? 'Codex（演示）' : task.owner }, `演示操作：移至${statuses.find(s => s.id === status)?.label}`)
    setNotice(`任务已移至${statuses.find(s => s.id === status)?.label}`)
  }
  function chooseFeature(id: string) { setFeature(id) }
  function openCreate(status: TaskStatus, trigger: HTMLButtonElement) {
    newTaskTrigger.current = trigger
    setCreationStatus(status)
    setCreating(true)
  }
  function finishDrop(id: string, status: TaskStatus) {
    moveTask(id, status)
    setDraggedTask(null)
    setDropStatus(null)
  }
  return <section className="kb-page" aria-label={`${workspaceName}工作区看板`} onKeyDown={event => {
    if (event.key === 'Escape' && selected) { setSelected(null); onSearchFocus(); event.stopPropagation() }
    if (event.key === '/' && !(event.target as Element).closest('input, textarea, select')) { event.preventDefault(); onSearchFocus() }
  }}>
    <div className="kb-body">
      <div className="kb-content">{activeFeature && <div className="kb-feature-context"><span className={`kb-feature-pin feature-${activeFeature.id}`} /><div><strong>{activeFeature.name}</strong><p>{activeFeature.goal}</p></div><button aria-label="清除功能筛选" onClick={() => chooseFeature('all')}><X size={14} /></button></div>}
        {view === 'flow' ? <><div className="kb-mobile-status" role="group" aria-label="任务阶段">{statuses.map(s => <button key={s.id} className={mobileStatus === s.id ? 'is-active' : ''} aria-pressed={mobileStatus === s.id} onClick={() => setMobileStatus(s.id)} onDragOver={event => { if (draggedTask) { event.preventDefault(); setMobileStatus(s.id); setDropStatus(s.id) } }} onDrop={event => { event.preventDefault(); finishDrop(event.dataTransfer.getData('application/devflow-task'), s.id) }}>{s.label}<small>{visible.filter(t => t.status === s.id).length}</small></button>)}</div><div className="kb-columns">{statuses.map(s => <section key={s.id} className={`kb-column ${mobileStatus === s.id ? 'is-mobile-active' : ''} ${dropStatus === s.id ? 'is-drop-target' : ''}`} aria-label={s.label} onDragOver={event => { if (event.dataTransfer.types.includes('application/devflow-task')) { event.preventDefault(); event.dataTransfer.dropEffect = 'move'; setDropStatus(s.id) } }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropStatus(null) }} onDrop={event => { event.preventDefault(); finishDrop(event.dataTransfer.getData('application/devflow-task'), s.id) }}><header><span className={`kb-dot status-${s.id}`}>{s.id === 'done' && <Check size={8} />}</span><h2>{s.label}</h2><span className="kb-count">{visible.filter(t => t.status === s.id).length}</span><button aria-label={`在${s.label}列新建任务`} title={`新建${s.label}任务`} onClick={event => openCreate(s.id, event.currentTarget)}><Plus size={14} /></button></header><p className="kb-column-caption">{s.caption}</p><div className="kb-column-tasks">{visible.filter(t => t.status === s.id).map(task => <button key={task.id} className={`kb-task ${selected === task.id ? 'is-selected' : ''} ${draggedTask === task.id ? 'is-dragging' : ''}`} draggable onDragStart={event => { event.dataTransfer.setData('application/devflow-task', task.id); event.dataTransfer.effectAllowed = 'move'; setDraggedTask(task.id) }} onDragEnd={() => { setDraggedTask(null); setDropStatus(null) }} onClick={() => { if (!draggedTask) setSelected(task.id) }}><span className="kb-task-top"><span className={`kb-task-feature feature-${task.feature}`}>{features.find(f => f.id === task.feature)?.name}</span><span className="kb-task-id">{task.id}</span></span><strong>{task.title}</strong><p>{task.description}</p>{task.blocked && <span className="kb-blocked"><LockKeyhole size={11} />{task.blocked}</span>}<span className="kb-task-bottom"><span className={task.priority === '高' ? 'kb-priority-high' : 'kb-muted'}><Flag size={11} />{task.priority === '高' ? '高优先级' : '普通'}</span>{task.owner ? <span className="kb-owner"><span>C</span>Codex</span> : task.status === 'done' ? <span className="kb-done-label"><CircleCheck size={12} />已验收</span> : <span className="kb-muted">未认领</span>}</span></button>)}{!visible.some(t => t.status === s.id) && <div className="kb-column-empty"><Circle size={18} /><span>{query || feature !== 'all' ? '没有匹配的任务' : '暂无任务'}</span></div>}</div><button className="kb-add-task" onClick={event => openCreate(s.id, event.currentTarget)}><Plus size={14} />添加任务</button></section>)}</div></> : <BoardCanvas features={features} tasks={visible} activeFeature={feature} onFeature={chooseFeature} onTask={task => setSelected(task.id)} onMove={(id, x, y) => setDraft(current => ({ ...current, features: current.features.map(f => f.id === id ? { ...f, x, y } : f) }))} />}
      </div>
    </div>
    <footer className="kb-footer">
      <div className="kb-view-switch" role="group" aria-label="看板视图">
        <button aria-pressed={view === 'flow'} className={view === 'flow' ? 'is-active' : ''} onClick={() => { setView('flow'); setFeature('all') }}><Kanban size={13} />流程</button>
        <button aria-pressed={view === 'architecture'} className={view === 'architecture' ? 'is-active' : ''} onClick={() => setView('architecture')}><Workflow size={13} />架构</button>
      </div>
      <span><span className="kb-local-dot" />{storageFailed ? '浏览器存储不可用，修改仅本次有效' : '演示数据 · 浏览器自动保存'}</span>
      <span>{view === 'flow' ? '拖动卡片到其他列以改变状态' : '功能归属与任务进度同步'}</span>
    </footer>
    {notice && <div className="kb-toast" role="status"><Check size={14} />{notice}</div>}
    {activeTask && <><button className="kb-detail-backdrop" aria-label="关闭任务详情" onClick={() => setSelected(null)} /><aside ref={detailRef} tabIndex={-1} className="kb-detail" aria-label={`${activeTask.title}任务详情`}><header><span><Kanban size={14} />{activeTask.id}<ChevronRight size={12} />任务详情</span><button aria-label="关闭任务详情" onClick={() => { setSelected(null); onSearchFocus() }}><X size={17} /></button></header><div className="kb-detail-content"><span className={`kb-task-feature feature-${activeTask.feature}`}>{features.find(f => f.id === activeTask.feature)?.name}</span><h2>{activeTask.title}</h2><div className="kb-detail-properties"><span>状态</span><span className="kb-detail-status"><i className={`kb-dot status-${activeTask.status}`} />{statuses.find(s => s.id === activeTask.status)?.label}</span><span>执行者</span><span>{activeTask.owner ?? '未认领'}</span><span>优先级</span><button onClick={() => updateTask(activeTask.id, { priority: activeTask.priority === '高' ? '普通' : '高' })}><Flag size={12} />{activeTask.priority}</button></div><label className="kb-edit-field">任务描述<textarea key={`${activeTask.id}-description`} defaultValue={activeTask.description} rows={3} onBlur={e => { if (e.target.value !== activeTask.description) updateTask(activeTask.id, { description: e.target.value }) }} /></label><label className="kb-edit-field">目标与验收<textarea key={`${activeTask.id}-acceptance`} defaultValue={activeTask.acceptance} rows={3} onBlur={e => { if (e.target.value !== activeTask.acceptance) updateTask(activeTask.id, { acceptance: e.target.value }) }} /></label>{activeTask.blocked && <div className="kb-detail-blocked"><LockKeyhole size={14} /><span>{activeTask.blocked}</span><button onClick={() => updateTask(activeTask.id, { blocked: undefined }, '已解除阻塞')}>解除</button></div>}{activeTask.result && <div className="kb-delivery"><h3><CircleCheck size={15} />交付结果</h3><p>{activeTask.result}</p></div>}<div className="kb-activity"><h3><Clock3 size={14} />活动记录</h3>{activeTask.activity.map((entry, i) => <div key={i}><i /><span>{entry}</span></div>)}</div></div><footer className="kb-detail-actions">{activeTask.status === 'todo' && <button className="kb-primary" disabled={!!activeTask.blocked} onClick={() => moveTask(activeTask.id, 'doing')}><Sparkles size={14} />模拟 Codex 认领</button>}{activeTask.status === 'doing' && <><button className="kb-secondary" onClick={() => { updateTask(activeTask.id, { status: 'todo', owner: undefined }, '执行者释放了任务'); setNotice('已释放任务') }}><RotateCcw size={13} />释放任务</button><button className="kb-primary" onClick={() => updateTask(activeTask.id, { status: 'review', result: '已按任务目标完成本次交付，请检查功能效果和验收条件。\n这是前端演示结果，未执行真实代码修改或验证。' }, 'Codex 提交了交付结果，等待验收')}>模拟提交验收<ArrowRight size={13} /></button></>}{activeTask.status === 'review' && <><button className="kb-secondary" onClick={() => { updateTask(activeTask.id, { status: 'doing' }, '你退回了任务，需要继续修改'); setNotice('已退回进行中') }}><ArrowLeft size={13} />退回修改</button><button className="kb-primary" onClick={() => { updateTask(activeTask.id, { status: 'done' }, '你通过了验收'); setNotice('已通过验收') }}><Check size={14} />通过验收</button></>}{activeTask.status === 'done' && <button className="kb-secondary" onClick={() => { updateTask(activeTask.id, { status: 'todo', owner: undefined }, '你重新打开了任务'); setNotice('已重新打开任务') }}><RotateCcw size={13} />重新打开</button>}</footer></aside></>}
    {creating && <NewTaskDialog features={features} initialStatus={creationStatus} defaultFeature={feature === 'all' ? features[0].id : feature} onClose={() => { setCreating(false); newTaskTrigger.current?.focus() }} onCreate={task => {
      const id = `DF-${String(Math.max(0, ...tasks.map(t => Number(t.id.replace('DF-', '')) || 0)) + 1).padStart(3, '0')}`
      setDraft(current => ({ ...current, tasks: [...current.tasks, { ...task, id, status: creationStatus, activity: ['你创建了任务'] }] }))
      setFeature('all'); setQuery(''); setMobileStatus(creationStatus); setNotice(`任务已添加到${statuses.find(s => s.id === creationStatus)?.label}`); setCreating(false); setSelected(id)
    }} />}
  </section>
}

function NewTaskDialog({ features, defaultFeature, initialStatus, onClose, onCreate }: { initialStatus: TaskStatus; features: BoardDraft['features']; defaultFeature: string; onClose: () => void; onCreate: (task: Pick<BoardTask, 'title' | 'description' | 'acceptance' | 'feature' | 'priority'>) => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  useEffect(() => { const el = dialog.current; el?.showModal(); return () => el?.close() }, [])
  function submit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault()
    const form = event.currentTarget, data = new FormData(form)
    const title = String(data.get('title') ?? '').trim()
    if (!title) { const input = form.elements.namedItem('title') as HTMLInputElement; input.setCustomValidity('请输入任务标题'); input.reportValidity(); return }
    onCreate({ title, description: String(data.get('description') ?? '').trim(), acceptance: String(data.get('acceptance') ?? '').trim(), feature: String(data.get('feature')), priority: data.get('priority') === '高' ? '高' : '普通' })
  }
  return <dialog
    ref={dialog}
    className="kb-create-dialog"
    aria-labelledby="kb-create-title"
    onCancel={event => { event.preventDefault(); onClose() }}
    onClick={event => { if (event.target === event.currentTarget) onClose() }}
  >
    <form onSubmit={submit}>
      <header>
        <div><span className="kb-eyebrow">从一个明确的目标开始</span><h2 id="kb-create-title">新建任务</h2></div>
        <button type="button" aria-label="关闭新建任务" onClick={onClose}><X size={18} /></button>
      </header>
      <label>任务标题
        <input autoFocus name="title" required maxLength={120} placeholder="例如：支持 Codex 认领任务" onInput={e => e.currentTarget.setCustomValidity('')} />
      </label>
      <div className="kb-form-row">
        <label>所属功能
          <select name="feature" defaultValue={defaultFeature}>
            {features.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}
          </select>
        </label>
        <label>优先级
          <select name="priority" defaultValue="普通"><option>普通</option><option>高</option></select>
        </label>
      </div>
      <label>核心描述
        <textarea name="description" rows={3} placeholder="需要提供什么功能？只写核心意图。" maxLength={4000} />
      </label>
      <label>目标与验收
        <textarea name="acceptance" rows={3} placeholder="做到什么程度，就可以交给你验收？" maxLength={4000} />
      </label>
      <footer>
        <span>创建后进入「{statuses.find(s => s.id === initialStatus)?.label}」</span>
        <button type="button" className="kb-secondary" onClick={onClose}>取消</button>
        <button type="submit" className="kb-primary"><Plus size={14} />创建任务</button>
      </footer>
    </form>
  </dialog>
}
