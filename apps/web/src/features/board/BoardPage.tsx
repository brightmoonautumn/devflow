import { useEffect, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { Check, ChevronRight, Circle, Flag, Kanban, LockKeyhole, Plus, RefreshCw, Workflow, X } from 'lucide-react'
import { useDialogs } from '../../components/form/BaseDialog'
import { BoardCanvas } from './BoardCanvas'
import { executeBoard, getBoard } from './api'
import { activityText, readBoardDraft, statuses, type BoardFeature, type BoardOperation, type BoardSnapshot, type BoardTask, type TaskStatus } from './boardModel'
import './board.css'

type Editor = { kind: 'task'; task?: BoardTask } | { kind: 'feature'; feature?: BoardFeature } | { kind: 'features' } | { kind: 'edges' } | { kind: 'history' } | { kind: 'note'; task: BoardTask; action: 'submit' | 'progress' | 'reject' | 'block' }
type Props = { workspaceName: string; workspaceKey: string; workspaceRevision: number; query: string; onQueryChange: (query: string) => void; onSearchFocus: () => void }
export function BoardPage({ workspaceName, workspaceKey, workspaceRevision, query, onQueryChange, onSearchFocus }: Props) {
  const [board, setBoard] = useState<BoardSnapshot | null>(null)
  const [view, setView] = useState<'flow' | 'architecture'>('flow')
  const [feature, setFeature] = useState('all')
  const [selected, setSelected] = useState<string | null>(null)
  const [editor, setEditor] = useState<Editor | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const [mobileStatus, setMobileStatus] = useState<TaskStatus>('todo')
  const [dragged, setDragged] = useState<string | null>(null)
  const [dropStatus, setDropStatus] = useState<TaskStatus | null>(null)
  const [hasDraft, setHasDraft] = useState(false)
  const latest = useRef(board)
  const writing = useRef(false)
  const sequence = useRef(0)
  const readAbort = useRef<AbortController | null>(null)
  const mounted = useRef(false)
  const paused = useRef(false)
  const detailRef = useRef<HTMLElement>(null)
  const dialogs = useDialogs()
  latest.current = board
  paused.current = !!editor || !!dragged
  useEffect(() => {
    mounted.current = true
    return () => { mounted.current = false; sequence.current++; readAbort.current?.abort(); readAbort.current = null }
  }, [])
  async function refresh(background = false) {
    if (workspaceKey === 'demo') return
    if (background && readAbort.current) return
    readAbort.current?.abort()
    const controller = new AbortController()
    readAbort.current = controller
    const seq = ++sequence.current
    try {
      const value = await getBoard(workspaceKey, workspaceRevision, controller.signal)
      if (!mounted.current || seq !== sequence.current || writing.current || (background && paused.current)) return
      latest.current = value; setBoard(value)
      if (!background) setError('')
    } catch (e) { if (!controller.signal.aborted && mounted.current && seq === sequence.current) setError(e instanceof Error ? e.message : '读取看板失败') }
    finally { if (readAbort.current === controller) readAbort.current = null }
  }
  useEffect(() => {
    void refresh()
    const timer = setInterval(() => { if (!paused.current && !writing.current && document.visibilityState === 'visible') void refresh(true) }, 5000)
    try { setHasDraft(localStorage.getItem(`devflow.board.prototype.v1:${workspaceKey}`) !== null) } catch { setHasDraft(false) }
    return () => { clearInterval(timer); sequence.current++; readAbort.current?.abort(); readAbort.current = null }
    // Workspace revision changes invalidate requests as well as workspace identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceKey, workspaceRevision])
  useEffect(() => { if (!notice) return; const timer = setTimeout(() => setNotice(''), 3500); return () => clearTimeout(timer) }, [notice])
  useEffect(() => { if (selected) detailRef.current?.focus() }, [selected])
  async function mutate(operation: BoardOperation): Promise<boolean> {
    const current = latest.current
    if (!current || writing.current) return false
    writing.current = true; sequence.current++; readAbort.current?.abort(); setBusy(true); setError('')
    try {
      const value = await executeBoard(workspaceKey, workspaceRevision, current.revision, operation)
      if (!mounted.current) return false
      latest.current = value; setBoard(value); setNotice('已保存')
      if (operation.type === 'task_create') {
        onQueryChange(''); setFeature('all'); setMobileStatus('todo')
        setSelected(value.tasks.find(t => !current.tasks.some(old => old.id === t.id))?.id ?? null)
      }
      if (operation.type === 'feature_delete') setFeature('all')
      return true
    } catch (e) { if (mounted.current) setError(e instanceof Error ? e.message : '保存失败，修改尚未保存'); return false }
    finally { writing.current = false; if (mounted.current) setBusy(false) }
  }
  const tasks = board?.tasks ?? [], features = board?.features ?? []
  const activeTask = tasks.find(t => t.id === selected)
  const activeFeature = features.find(f => f.id === feature)
  const visible = tasks.filter(t => (feature === 'all' || t.feature === feature) && `${t.id} ${t.title} ${t.description} ${t.owner ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  const action = (task: BoardTask, action: Extract<BoardOperation, { type: 'action' }>['action']) => mutate({ type: 'action', id: task.id, action })
  function moveTask(id: string, target: TaskStatus) {
    setDragged(null); setDropStatus(null)
    const task = tasks.find(t => t.id === id)
    if (!task || task.status === target || writing.current) return
    if (task.status === 'todo' && target === 'doing') void action(task, 'claim')
    else if (task.status === 'doing' && target === 'todo') void action(task, 'release')
    else if (task.status === 'doing' && target === 'review') setEditor({ kind: 'note', task, action: 'submit' })
    else if (task.status === 'review' && target === 'doing') setEditor({ kind: 'note', task, action: 'reject' })
    else if (task.status === 'review' && target === 'done') void approve(task)
    else if (task.status === 'done' && target === 'todo') void action(task, 'reopen')
    else setError('请按任务流程操作：认领 → 提交验收 → 通过；详情中可释放、退回或重新打开。')
  }
  async function approve(task: BoardTask) {
    const revision = latest.current?.revision
    if (await dialogs.confirm({ title: '通过验收', description: `确认「${task.title}」已达到验收条件？`, confirmText: '通过验收' })) {
      if (revision !== latest.current?.revision) { setError('确认期间看板已变化，请重新查看交付后验收'); return }
      await action(task, 'approve')
    }
  }
  async function deleteTask(task: BoardTask) {
    const revision = latest.current?.revision
    if (await dialogs.confirm({ title: '删除任务', description: `永久删除「${task.title}」及其活动记录？`, tone: 'danger', confirmText: '删除' })) {
      if (revision !== latest.current?.revision) { setError('确认期间看板已变化，请重新检查后删除'); return }
      if (await mutate({ type: 'task_delete', id: task.id })) setSelected(null)
    }
  }
  async function importDraft() {
    if (!await dialogs.confirm({ title: '导入浏览器草稿', description: '导入功能、布局和任务正文；演示认领、状态和交付会清除，任务统一进入待开始。原草稿继续保留。', confirmText: '导入' })) return
    try {
      const draft = readBoardDraft(workspaceKey)
      if (!draft) throw new Error('草稿格式无效，原数据仍保留在浏览器中')
      await mutate({ type: 'import', ...draft, edges: [] })
    } catch (e) { setError(e instanceof Error ? e.message : '无法读取草稿') }
  }
  const errorView = error && <div className="kb-error" role="alert"><span>{error}</span><button disabled={busy} onClick={() => void refresh()}>刷新数据</button></div>
  return <section className="kb-page" aria-label={`${workspaceName}工作区看板`} aria-busy={busy} onKeyDown={event => {
    if (editor) return
    if (event.key === 'Escape' && selected) { setSelected(null); onSearchFocus(); event.stopPropagation() }
    if (event.key === '/' && !(event.target as Element).closest('input, textarea, select')) { event.preventDefault(); onSearchFocus() }
  }}>
    {errorView}
    {!board ? <div className="kb-empty-board">{workspaceKey === 'demo' ? '请先打开一个工作区，再使用看板。' : error ? '看板暂不可用，原有数据不会被替换。' : '正在读取看板…'}</div> : <>
      <div className="kb-board-tools"><select aria-label="筛选功能" value={feature} onChange={e => setFeature(e.target.value)}><option value="all">全部功能</option>{features.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}</select><button disabled={busy} onClick={() => setEditor({ kind: 'features' })}>管理功能</button><button disabled={busy} onClick={() => setEditor({ kind: 'history' })}>记录</button><button disabled={busy} title="刷新看板" aria-label="刷新看板" onClick={() => void refresh()}><RefreshCw size={14} /></button>{hasDraft && board.revision === 0 && <button disabled={busy} onClick={() => void importDraft()}>导入草稿</button>}</div>
      <div className="kb-body"><div className="kb-content">
        {activeFeature && <div className="kb-feature-context"><span className="kb-feature-pin" /><div><strong>{activeFeature.name}</strong><p>{activeFeature.goal}</p></div><button disabled={busy} onClick={() => setEditor({ kind: 'feature', feature: activeFeature })}>编辑</button><button aria-label="清除功能筛选" onClick={() => setFeature('all')}><X size={14} /></button></div>}
        {!features.length ? <div className="kb-empty-board"><Workflow size={28} /><p>先创建一个功能，再把讨论整理为任务。</p><button className="kb-primary" onClick={() => setEditor({ kind: 'feature' })}><Plus size={14} />新建功能</button></div> : view === 'flow' ? <>
          <div className="kb-mobile-status" role="group" aria-label="任务阶段">{statuses.map(s => <button key={s.id} className={mobileStatus === s.id ? 'is-active' : ''} aria-pressed={mobileStatus === s.id} onClick={() => setMobileStatus(s.id)} onDragOver={event => { if (dragged) { event.preventDefault(); setMobileStatus(s.id); setDropStatus(s.id) } }} onDrop={event => { event.preventDefault(); moveTask(event.dataTransfer.getData('application/devflow-task'), s.id) }}>{s.label}<small>{visible.filter(t => t.status === s.id).length}</small></button>)}</div>
          <div className="kb-columns">{statuses.map(s => <section key={s.id} className={`kb-column ${mobileStatus === s.id ? 'is-mobile-active' : ''} ${dropStatus === s.id ? 'is-drop-target' : ''}`} aria-label={s.label} onDragOver={event => { if (dragged && !busy) { event.preventDefault(); setDropStatus(s.id) } }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropStatus(null) }} onDrop={event => { event.preventDefault(); moveTask(event.dataTransfer.getData('application/devflow-task'), s.id) }}>
            <header><span className={`kb-dot status-${s.id}`}>{s.id === 'done' && <Check size={8} />}</span><h2>{s.label}</h2><span className="kb-count">{visible.filter(t => t.status === s.id).length}</span>{s.id === 'todo' && <button disabled={busy} aria-label="新建任务" onClick={() => setEditor({ kind: 'task' })}><Plus size={14} /></button>}</header><p className="kb-column-caption">{s.caption}</p>
            <div className="kb-column-tasks">{visible.filter(t => t.status === s.id).map(task => <button key={task.id} className={`kb-task ${selected === task.id ? 'is-selected' : ''} ${dragged === task.id ? 'is-dragging' : ''}`} draggable={!busy} onDragStart={event => { event.dataTransfer.setData('application/devflow-task', task.id); event.dataTransfer.effectAllowed = 'move'; setDragged(task.id) }} onDragEnd={() => { setDragged(null); setDropStatus(null) }} onClick={() => { if (!dragged) setSelected(task.id) }}><span className="kb-task-top"><span className={`kb-task-feature feature-${task.feature}`}>{features.find(f => f.id === task.feature)?.name}</span><span className="kb-task-id">{task.id}</span></span><strong>{task.title}</strong><p>{task.description}</p>{task.blocked && <span className="kb-blocked"><LockKeyhole size={11} />{task.blocked}</span>}<span className="kb-task-bottom"><span className={task.priority === '高' ? 'kb-priority-high' : 'kb-muted'}><Flag size={11} />{task.priority}</span><span className="kb-owner">{task.status === 'done' ? '已验收' : task.owner ?? '未认领'}</span></span></button>)}{!visible.some(t => t.status === s.id) && <div className="kb-column-empty"><Circle size={18} /><span>{query || feature !== 'all' ? '没有匹配的任务' : '暂无任务'}</span></div>}</div>
            {s.id === 'todo' && <button className="kb-add-task" disabled={busy} onClick={() => setEditor({ kind: 'task' })}><Plus size={14} />添加任务</button>}
          </section>)}</div>
        </> : <BoardCanvas features={features} tasks={visible} edges={board.edges} activeFeature={feature} onFeature={setFeature} onTask={task => setSelected(task.id)} disabled={busy} onMove={(id, x, y) => { const f = latest.current?.features.find(f => f.id === id); if (f) void mutate({ type: 'feature_save', feature: { ...f, x, y } }) }} />}
      </div></div>
      <div className="kb-view-switch kb-view-floating" role="group" aria-label="看板视图"><button aria-pressed={view === 'flow'} className={view === 'flow' ? 'is-active' : ''} onClick={() => setView('flow')}><Kanban size={13} />流程</button><button aria-pressed={view === 'architecture'} className={view === 'architecture' ? 'is-active' : ''} onClick={() => setView('architecture')}><Workflow size={13} />架构</button></div>
    </>}
    {notice && <div className="kb-toast" role="status"><Check size={14} />{notice}</div>}
    {activeTask && <><button className="kb-detail-backdrop" aria-label="关闭任务详情" onClick={() => setSelected(null)} /><aside ref={detailRef} tabIndex={-1} className="kb-detail" aria-label={`${activeTask.title}任务详情`}><header><span><Kanban size={14} />{activeTask.id}<ChevronRight size={12} />任务详情</span><button aria-label="关闭任务详情" onClick={() => { setSelected(null); onSearchFocus() }}><X size={17} /></button></header><div className="kb-detail-content">
      {errorView}<span className="kb-task-feature">{features.find(f => f.id === activeTask.feature)?.name}</span><h2>{activeTask.title}</h2><div className="kb-detail-properties"><span>状态</span><span>{statuses.find(s => s.id === activeTask.status)?.label}</span><span>执行者</span><span>{activeTask.owner ?? '未认领'}</span><span>优先级</span><span>{activeTask.priority}</span></div>
      <div className="kb-detail-edit-actions"><button disabled={busy} onClick={() => setEditor({ kind: 'task', task: activeTask })}>编辑任务</button><button disabled={busy} onClick={() => void deleteTask(activeTask)}>删除</button></div>
      <div className="kb-read-field"><h3>任务描述</h3><p>{activeTask.description || '暂无描述'}</p></div><div className="kb-read-field"><h3>目标与验收</h3><p>{activeTask.acceptance || '尚未填写验收条件'}</p></div>
      {activeTask.blocked && <div className="kb-detail-blocked"><LockKeyhole size={14} /><span>{activeTask.blocked}</span><button disabled={busy} onClick={() => void action(activeTask, 'unblock')}>解除</button></div>}
      {activeTask.result && <div className="kb-delivery"><h3>交付结果</h3><p>{activeTask.result}</p></div>}
      {activeTask.status !== 'done' && <div className="kb-detail-edit-actions">{activeTask.status === 'doing' && <button disabled={busy} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'progress' })}>记录进展 / 交接</button>}{!activeTask.blocked && <button disabled={busy} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'block' })}>标记阻塞</button>}</div>}
      <div className="kb-activity"><h3>活动记录</h3>{activeTask.activity.map((entry, i) => <div key={i}><i /><span>{activityText(entry)}</span></div>)}</div>
    </div><footer className="kb-detail-actions">
      {activeTask.status === 'todo' && <button className="kb-primary" disabled={busy || !!activeTask.blocked} onClick={() => void action(activeTask, 'claim')}>认领任务</button>}
      {activeTask.status === 'doing' && <><button className="kb-secondary" disabled={busy} onClick={() => void action(activeTask, 'release')}>释放任务</button><button className="kb-primary" disabled={busy || !!activeTask.blocked} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'submit' })}>提交验收</button></>}
      {activeTask.status === 'review' && <><button className="kb-secondary" disabled={busy} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'reject' })}>退回修改</button><button className="kb-primary" disabled={busy || !!activeTask.blocked} onClick={() => void approve(activeTask)}>通过验收</button></>}
      {activeTask.status === 'done' && <button className="kb-secondary" disabled={busy} onClick={() => void action(activeTask, 'reopen')}>重新打开</button>}
    </footer></aside></>}
    {editor && board && <BoardEditor editor={editor} board={board} defaultFeature={feature} busy={busy} error={errorView} onRefresh={() => void refresh()} onClose={() => setEditor(null)} onEditor={setEditor} onSave={mutate} />}
  </section>
}

function Modal({ title, children, onClose, busy }: { title: string; children: ReactNode; onClose: () => void; busy: boolean }) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => { const el = ref.current; const previous = document.activeElement as HTMLElement | null; el?.showModal(); return () => { el?.close(); previous?.focus() } }, [])
  return <dialog ref={ref} className="kb-create-dialog" aria-labelledby="kb-editor-title" onKeyDown={e => e.stopPropagation()} onCancel={e => { e.preventDefault(); if (!busy) onClose() }}><header><h2 id="kb-editor-title">{title}</h2><button type="button" disabled={busy} aria-label="关闭" onClick={onClose}><X size={18} /></button></header>{children}</dialog>
}
function BoardEditor({ editor, board, defaultFeature, busy, error, onRefresh, onClose, onEditor, onSave }: { editor: Editor; board: BoardSnapshot; defaultFeature: string; busy: boolean; error: ReactNode; onRefresh: () => void; onClose: () => void; onEditor: (e: Editor) => void; onSave: (o: BoardOperation) => Promise<boolean> }) {
  const dialogs = useDialogs()
  const [localError, setLocalError] = useState('')
  const [edges, setEdges] = useState(board.edges)
  const [source, setSource] = useState(board.features[0]?.id ?? '')
  const [target, setTarget] = useState(board.features[1]?.id ?? '')
  const [moveTo, setMoveTo] = useState<Record<string, string>>({})
  const labels = { submit: '提交验收', progress: '记录进展 / 交接', reject: '退回修改', block: '标记阻塞' }
  const title = editor.kind === 'task' ? editor.task ? '编辑任务' : '新建任务' : editor.kind === 'feature' ? editor.feature ? '编辑功能' : '新建功能' : editor.kind === 'features' ? '管理功能' : editor.kind === 'edges' ? '功能关联' : editor.kind === 'history' ? '看板活动记录' : labels[editor.action]
  async function save(e: FormEvent<HTMLFormElement>) {
    e.preventDefault(); setLocalError('')
    const data = new FormData(e.currentTarget), value = (key: string) => String(data.get(key) ?? '').trim()
    let operation: BoardOperation
    if (editor.kind === 'task') {
      if (!value('title')) { setLocalError('请输入任务标题'); return }
      const task = { title: value('title'), description: value('description'), acceptance: value('acceptance'), feature: value('feature'), priority: value('priority') as BoardTask['priority'] }
      operation = editor.task ? { type: 'task_update', id: editor.task.id, ...task } : { type: 'task_create', ...task }
    } else if (editor.kind === 'feature') {
      if (!value('name')) { setLocalError('请输入功能名称'); return }
      operation = { type: 'feature_save', feature: { id: editor.feature?.id ?? crypto.randomUUID(), name: value('name'), description: value('description'), goal: value('goal'), x: Number(value('x')), y: Number(value('y')) } }
    } else if (editor.kind === 'note') {
      if (!value('text')) { setLocalError('请填写说明'); return }
      operation = { type: 'action', id: editor.task.id, action: editor.action, text: value('text') }
    } else if (editor.kind === 'edges') operation = { type: 'edges', edges }
    else return
    if (await onSave(operation)) onClose()
  }
  async function removeFeature(f: BoardFeature) {
    const count = board.tasks.filter(t => t.feature === f.id).length
    if (count && !moveTo[f.id]) { setLocalError('该功能含有任务，请先选择迁移目标功能'); return }
    if (await dialogs.confirm({ title: '删除功能', description: `删除「${f.name}」及其关联线${count ? `，并迁移 ${count} 个任务到所选功能` : ''}？`, tone: 'danger', confirmText: '删除' })) {
      if (await onSave({ type: 'feature_delete', id: f.id, moveTo: moveTo[f.id] || undefined })) setLocalError('')
    }
  }
  return <Modal title={title} onClose={onClose} busy={busy}><form key={editor.kind === 'task' ? editor.task?.id ?? 'new-task' : editor.kind === 'feature' ? editor.feature?.id ?? 'new-feature' : title} onSubmit={e => void save(e)}>
    {error}{localError && <p className="kb-error" role="alert">{localError}</p>}
    <fieldset disabled={busy} className="kb-editor-fields">
      {editor.kind === 'task' && <><label>任务标题<input autoFocus name="title" required maxLength={120} defaultValue={editor.task?.title} /></label><div className="kb-form-row"><label>所属功能<select name="feature" defaultValue={editor.task?.feature ?? (board.features.some(f => f.id === defaultFeature) ? defaultFeature : board.features[0]?.id)} required>{board.features.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}</select></label><label>优先级<select name="priority" defaultValue={editor.task?.priority ?? '普通'}><option>普通</option><option>高</option></select></label></div><label>核心描述<textarea name="description" rows={4} maxLength={4000} defaultValue={editor.task?.description} /></label><label>目标与验收<textarea name="acceptance" rows={4} maxLength={4000} defaultValue={editor.task?.acceptance} /></label></>}
      {editor.kind === 'feature' && <><label>功能名称<input autoFocus name="name" required maxLength={60} defaultValue={editor.feature?.name} /></label><label>核心描述<textarea name="description" rows={4} maxLength={4000} defaultValue={editor.feature?.description} /></label><label>功能目标<textarea name="goal" rows={4} maxLength={4000} defaultValue={editor.feature?.goal} /></label><div className="kb-form-row"><label>画布横坐标<input name="x" type="number" step="any" min={-100000} max={100000} required defaultValue={editor.feature?.x ?? 100 + board.features.length % 3 * 350} /></label><label>画布纵坐标<input name="y" type="number" step="any" min={-100000} max={100000} required defaultValue={editor.feature?.y ?? 100 + Math.floor(board.features.length / 3) * 320} /></label></div></>}
      {editor.kind === 'note' && <><p className="kb-muted">{editor.task.id} · {editor.task.title}</p><label>{editor.action === 'submit' ? '交付内容、验证情况与已知限制' : editor.action === 'progress' ? '已完成、剩余工作和接手提示' : editor.action === 'reject' ? '需要继续修改的原因' : '阻塞原因'}<textarea autoFocus name="text" required rows={7} maxLength={4000} /></label></>}
      {editor.kind === 'features' && <><button type="button" className="kb-secondary" onClick={() => onEditor({ kind: 'feature' })}><Plus size={14} />新建功能</button> <button type="button" className="kb-secondary" onClick={() => onEditor({ kind: 'edges' })}>功能关联</button>{board.features.map(f => <div className="kb-feature-row" key={f.id}><strong>{f.name}</strong><p>{f.goal}</p><div><button type="button" onClick={() => onEditor({ kind: 'feature', feature: f })}>编辑</button><button type="button" onClick={() => void removeFeature(f)}>删除</button></div>{board.tasks.some(t => t.feature === f.id) && <label>删除时迁移任务到<select value={moveTo[f.id] ?? ''} onChange={e => setMoveTo(current => ({ ...current, [f.id]: e.target.value }))}><option value="">请选择目标功能</option>{board.features.filter(other => other.id !== f.id).map(other => <option key={other.id} value={other.id}>{other.name}</option>)}</select></label>}</div>)}</>}
      {editor.kind === 'history' && <div className="kb-activity">{board.activity.length ? [...board.activity].reverse().map((a, i) => <div key={i}><span>{activityText(a)}</span></div>) : <p>暂无活动</p>}</div>}
      {editor.kind === 'edges' && <><p className="kb-muted">关联表示功能之间的结构关系，不影响任务执行顺序。</p><div className="kb-form-row"><label>起点<select value={source} onChange={e => setSource(e.target.value)}><option value="">请选择</option>{board.features.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}</select></label><label>终点<select value={target} onChange={e => setTarget(e.target.value)}><option value="">请选择</option>{board.features.map(f => <option key={f.id} value={f.id}>{f.name}</option>)}</select></label></div><button type="button" className="kb-secondary" disabled={!source || !target || source === target || edges.some(([a, b]) => a === source && b === target)} onClick={() => setEdges(current => [...current, [source, target]])}>添加关联</button>{edges.map(([a, b]) => <div className="kb-edge-row" key={`${a}-${b}`}><span>{board.features.find(f => f.id === a)?.name} → {board.features.find(f => f.id === b)?.name}</span><button type="button" onClick={() => setEdges(current => current.filter(edge => edge[0] !== a || edge[1] !== b))}>移除</button></div>)}</>}
    </fieldset>
    <footer><span>{editor.kind === 'task' && !editor.task ? '创建后进入待开始' : '保存后生效'}</span>{error && <button type="button" disabled={busy} onClick={onRefresh}>刷新数据</button>}<button type="button" className="kb-secondary" disabled={busy} onClick={onClose}>{editor.kind === 'history' || editor.kind === 'features' ? '关闭' : '取消'}</button>{editor.kind !== 'features' && editor.kind !== 'history' && <button type="submit" className="kb-primary" disabled={busy}>{busy ? '保存中…' : '保存'}</button>}</footer>
  </form></Modal>
}
