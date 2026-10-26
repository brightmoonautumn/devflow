import { useCallback, useEffect, useId, useRef, useState, type FormEvent, type ReactNode } from 'react'
import { Check, Circle, History, Flag, Kanban, LockKeyhole, Plus, RefreshCw, Workflow, X } from 'lucide-react'
import { useDialogs } from '../../components/form/BaseDialog'
import { BoardCanvas } from './BoardCanvas'
import { TaskDetailEditor } from './TaskDetailEditor'
import { executeBoard, getBoard } from './api'
import { activityText, readBoardDraft, statuses, type BoardFeature, type BoardOperation, type BoardSnapshot, type BoardTask, type TaskStatus } from './boardModel'
import { BoardSyncButton, BoardSyncPanel } from './BoardSyncPanel'
import { copyText } from '../../lib/clipboard'
import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseTextarea } from '../../components/form/BaseTextarea'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseButton } from '../../components/form/BaseButton'
import { featureChoices, priorityChoices } from './boardForm'
import './board.css'

type Editor = { kind: 'task'; task?: BoardTask } | { kind: 'feature'; feature?: BoardFeature } | { kind: 'features' } | { kind: 'edges' } | { kind: 'history' } | { kind: 'note'; task: BoardTask; action: 'submit' | 'progress' | 'reject' | 'block' }
type Props = { workspaceName: string; workspaceKey: string; workspaceRevision: number; query: string; onQueryChange: (query: string) => void; onSearchFocus: () => void }
export function BoardPage({ workspaceName, workspaceKey, workspaceRevision, query, onQueryChange, onSearchFocus }: Props) {
  const detailFormId = useId()
  const [undoCompletion, setUndoCompletion] = useState<{ id: string; revision: number } | null>(null)
  const [quickTitle, setQuickTitle] = useState('')
  const [syncOpen, setSyncOpen] = useState(false)
  const [board, setBoard] = useState<BoardSnapshot | null>(null)
  const [view, setView] = useState<'flow' | 'architecture'>('flow')
  const [feature, setFeature] = useState('all')
  const [showActivity, setShowActivity] = useState(false)
  const activityRef = useRef<HTMLDivElement>(null)
  const [detailDirty, setDetailDirty] = useState(false)
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
  const canvasInteracting = useRef(false)
  const onCanvasInteractionChange = useCallback((active: boolean) => {
    canvasInteracting.current = active
    if (active) { sequence.current++; readAbort.current?.abort() }
  }, [])
  const detailRef = useRef<HTMLElement>(null)
  const dialogs = useDialogs()
  latest.current = board
  paused.current = !!editor || !!dragged || syncOpen || detailDirty
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
      if (!mounted.current || seq !== sequence.current || writing.current || (background && (paused.current || canvasInteracting.current))) return
      latest.current = value; setBoard(value)
      if (!background) setError('')
    } catch (e) { if (!controller.signal.aborted && mounted.current && seq === sequence.current) setError(e instanceof Error ? e.message : '读取看板失败') }
    finally { if (readAbort.current === controller) readAbort.current = null }
  }
  useEffect(() => {
    void refresh()
    const timer = setInterval(() => { if (!paused.current && !canvasInteracting.current && !writing.current && document.visibilityState === 'visible') void refresh(true) }, 5000)
    try { setHasDraft(localStorage.getItem(`devflow.board.prototype.v1:${workspaceKey}`) !== null) } catch { setHasDraft(false) }
    return () => { clearInterval(timer); sequence.current++; readAbort.current?.abort(); readAbort.current = null }
    // Workspace revision changes invalidate requests as well as workspace identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceKey, workspaceRevision])
  useEffect(() => { if (!notice) return; const timer = setTimeout(() => { setNotice(''); setUndoCompletion(null) }, 6000); return () => clearTimeout(timer) }, [notice])
  useEffect(() => { setShowActivity(false); if (selected) detailRef.current?.focus() }, [selected])
  useEffect(() => { if (showActivity) activityRef.current?.scrollIntoView({ block: 'nearest' }) }, [showActivity])
  async function mutate(operation: BoardOperation, expectedBoardRevision?: number): Promise<boolean> {
    const current = latest.current
    if (!current || writing.current) return false
    writing.current = true; sequence.current++; readAbort.current?.abort(); setBusy(true); setError('')
    try {
      const value = await executeBoard(workspaceKey, workspaceRevision, expectedBoardRevision ?? current.revision, operation)
      if (!mounted.current) return false
      latest.current = value; setBoard(value); setUndoCompletion(null); setNotice('已保存')
      if (operation.type === 'task_create') {
        onQueryChange(''); setFeature('all'); setMobileStatus('todo')
        setSelected(value.tasks.find(t => !current.tasks.some(old => old.id === t.id))?.id ?? null)
      }
      if (operation.type === 'feature_delete') setFeature('all')
      return true
    } catch (e) { if (mounted.current) setError(e instanceof Error ? e.message : '保存失败，修改尚未保存'); return false }
    finally { writing.current = false; if (mounted.current) setBusy(false) }
  }
  function closeDetail() {
    if (busy) return
    if (detailDirty) { setError('请先保存或撤销抽屉中的修改，再关闭任务。'); return }
    setSelected(null); setError(''); onSearchFocus()
  }
  const tasks = board?.tasks ?? [], features = board?.features ?? []
  const choices = featureChoices(features)
  const activeTask = tasks.find(t => t.id === selected)
  const activeFeature = features.find(f => f.id === feature)
  const visible = tasks.filter(t => (feature === 'all' || t.feature === feature) && `${t.id} ${t.title} ${t.description} ${t.owner ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  const action = (task: BoardTask, action: Extract<BoardOperation, { type: 'action' }>['action']) => mutate({ type: 'action', id: task.id, action })
  function moveTask(id: string, target: TaskStatus) {
    setDragged(null); setDropStatus(null)
    const task = tasks.find(t => t.id === id)
    if (!task || task.status === target || writing.current) return
    if (target === 'done') void completeTask(task)
    else if (task.status === 'todo' && target === 'doing') void action(task, 'claim')
    else if (task.status === 'doing' && target === 'todo') void action(task, 'release')
    else if (task.status === 'doing' && target === 'review') setEditor({ kind: 'note', task, action: 'submit' })
    else if (task.status === 'review' && target === 'doing') setEditor({ kind: 'note', task, action: 'reject' })
    else if (task.status === 'done' && target === 'todo') void action(task, 'reopen')
    else setError('可以拖到已完成直接完成人工待办；其他阶段请通过认领、提交、退回或重新打开操作。')
  }
  function copyTaskInstructions(task: BoardTask) {
    void copyText(`请读取 DevFlow 当前工作区看板，核对工作区 ${workspaceKey}，优化任务 ${task.id}：${task.title}。结合所属功能与已有任务，提出描述、归类、验收条件和必要的拆分建议，信息不足时列出问题。先给我方案，确认后再通过 board_execute 更新任务；不要认领或执行，不要编译或测试。`).then(() => setNotice('已复制，请粘贴到 Codex 对话')).catch(() => setError('复制失败，请手动让 Codex 整理该任务'))
  }
  async function completeTask(task: BoardTask) {
    if (detailDirty) { setError('请先保存或撤销任务修改，再标记完成。'); return }
    const revision = latest.current?.revision
    if (revision === undefined || writing.current) return
    if ((task.ownerId && task.ownerId !== '你') || task.blocked) {
      if (!await dialogs.confirm({ title: '人工标记完成', description: `「${task.title}」${task.blocked ? '存在阻塞' : '已被 AI 或其他执行者认领'}。确认已由你完成？完成后清除当前执行者和阻塞标记，保留已有交付及历史。`, confirmText: '标记完成' })) return
    }
    if (await mutate({ type: 'action', id: task.id, action: 'complete' }, revision)) {
      setNotice('已人工标记完成'); setUndoCompletion({ id: task.id, revision: latest.current!.revision })
    }
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
  const errorView = error && <div className="kb-error" role="alert"><span>{error}</span><BaseButton disabled={busy} onClick={() => void refresh()}>刷新数据</BaseButton></div>
  return <section className="kb-page" aria-label={`${workspaceName}工作区看板`} aria-busy={busy} onKeyDown={event => {
    if (editor) return
    if (event.key === 'Escape' && selected) { closeDetail(); event.stopPropagation() }
    if (event.key === '/' && !(event.target as Element).closest('input, textarea, select')) { event.preventDefault(); onSearchFocus() }
  }}>
    {errorView}
    {!board ? <div className="kb-empty-board">{workspaceKey === 'demo' ? '请先打开一个工作区，再使用看板。' : error ? '看板暂不可用，原有数据不会被替换。' : '正在读取看板…'}</div> : <>
      <div className="kb-board-tools"><BaseSelect className="kb-feature-filter" aria-label="筛选功能" disabled={busy} value={feature === '' ? choices.unclassified : feature} onValueChange={value => setFeature(value === choices.unclassified ? '' : value)} options={[{ value: 'all', label: '全部功能' }, ...choices.withUnclassified]} /><BaseButton disabled={busy} onClick={() => setEditor({ kind: 'features' })}>管理功能</BaseButton><BaseButton disabled={busy} onClick={() => setEditor({ kind: 'history' })}>记录</BaseButton><BoardSyncButton disabled={busy} workspaceKey={workspaceKey} workspaceRevision={workspaceRevision} boardRevision={board.revision} onClick={() => setSyncOpen(true)} /><BaseButton disabled={busy} title="刷新看板" aria-label="刷新看板" onClick={() => void refresh()}><RefreshCw size={14} /></BaseButton>{hasDraft && board.revision === 0 && <BaseButton disabled={busy} onClick={() => void importDraft()}>导入草稿</BaseButton>}</div>
      <div className="kb-body"><div className="kb-content">
        {activeFeature && <div className="kb-feature-context"><span className="kb-feature-pin" /><div><strong>{activeFeature.name}</strong><p>{activeFeature.goal}</p></div><BaseButton disabled={busy} onClick={() => setEditor({ kind: 'feature', feature: activeFeature })}>编辑</BaseButton><button aria-label="清除功能筛选" onClick={() => setFeature('all')}><X size={14} /></button></div>}
        {view === 'flow' ? <>
          <form className="kb-quick-create" onSubmit={event => { event.preventDefault(); const title = quickTitle.trim(); if (title) void mutate({ type: 'task_create', title, feature: features.some(f => f.id === feature) ? feature : '', description: '', acceptance: '', priority: '普通' }).then(ok => { if (ok) setQuickTitle('') }) }}><BaseInput aria-label="快速记录任务" placeholder="先记一句待办，之后再完善…" maxLength={120} value={quickTitle} onChange={event => setQuickTitle(event.target.value)} disabled={busy} /><BaseButton variant="primary" disabled={busy || !quickTitle.trim()} type="submit">记录</BaseButton></form>
          <div className="kb-mobile-status" role="group" aria-label="任务阶段">{statuses.map(s => <button key={s.id} className={mobileStatus === s.id ? 'is-active' : ''} aria-pressed={mobileStatus === s.id} onClick={() => setMobileStatus(s.id)} onDragOver={event => { if (dragged) { event.preventDefault(); setMobileStatus(s.id); setDropStatus(s.id) } }} onDrop={event => { event.preventDefault(); moveTask(event.dataTransfer.getData('application/devflow-task'), s.id) }}>{s.label}<small>{visible.filter(t => t.status === s.id).length}</small></button>)}</div>
          <div className="kb-columns">{statuses.map(s => <section key={s.id} className={`kb-column ${mobileStatus === s.id ? 'is-mobile-active' : ''} ${dropStatus === s.id ? 'is-drop-target' : ''}`} aria-label={s.label} onDragOver={event => { if (dragged && !busy) { event.preventDefault(); setDropStatus(s.id) } }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropStatus(null) }} onDrop={event => { event.preventDefault(); moveTask(event.dataTransfer.getData('application/devflow-task'), s.id) }}>
            <header><span className={`kb-dot status-${s.id}`}>{s.id === 'done' && <Check size={8} />}</span><h2>{s.label}</h2><span className="kb-count">{visible.filter(t => t.status === s.id).length}</span>{s.id === 'todo' && <button disabled={busy} aria-label="新建任务" onClick={() => setEditor({ kind: 'task' })}><Plus size={14} /></button>}</header><p className="kb-column-caption">{s.caption}</p>
            <div className="kb-column-tasks">{visible.filter(t => t.status === s.id).map(task => <button key={task.id} className={`kb-task ${selected === task.id ? 'is-selected' : ''} ${dragged === task.id ? 'is-dragging' : ''}`} draggable={!busy} onDragStart={event => { event.dataTransfer.setData('application/devflow-task', task.id); event.dataTransfer.effectAllowed = 'move'; setDragged(task.id) }} onDragEnd={() => { setDragged(null); setDropStatus(null) }} onClick={() => { if (!dragged) setSelected(task.id) }}><span className="kb-task-top"><span className={`kb-task-feature feature-${task.feature}`}>{features.find(f => f.id === task.feature)?.name ?? '未归类'}</span><span className="kb-task-id">{task.id}</span></span><strong>{task.title}</strong><p>{task.description}</p>{task.blocked && <span className="kb-blocked"><LockKeyhole size={11} />{task.blocked}</span>}<span className="kb-task-bottom"><span className={task.priority === '高' ? 'kb-priority-high' : 'kb-muted'}><Flag size={11} />{task.priority}</span><span className="kb-owner">{task.status === 'done' ? '已验收' : task.owner ?? '未认领'}</span></span></button>)}{!visible.some(t => t.status === s.id) && <div className="kb-column-empty"><Circle size={18} /><span>{query || feature !== 'all' ? '没有匹配的任务' : '暂无任务'}</span></div>}</div>
            {s.id === 'todo' && <button className="kb-add-task" disabled={busy} onClick={() => setEditor({ kind: 'task' })}><Plus size={14} />添加任务</button>}
          </section>)}</div>
        </> : <BoardCanvas features={features} tasks={visible} edges={board.edges} activeFeature={feature} onFeature={setFeature} onTask={task => setSelected(task.id)} disabled={busy} onInteractionChange={onCanvasInteractionChange} onMove={(id, x, y) => { const f = latest.current?.features.find(f => f.id === id); return f ? mutate({ type: 'feature_save', feature: { ...f, x, y } }) : Promise.resolve(false) }} />}
      </div></div>
      <div className="kb-view-switch kb-view-floating" role="group" aria-label="看板视图"><button aria-pressed={view === 'flow'} className={view === 'flow' ? 'is-active' : ''} onClick={() => setView('flow')}><Kanban size={13} />流程</button><button aria-pressed={view === 'architecture'} className={view === 'architecture' ? 'is-active' : ''} onClick={() => setView('architecture')}><Workflow size={13} />架构</button></div>
    </>}
    {notice && <div className="kb-toast" role="status"><Check size={14} />{notice}{undoCompletion && <BaseButton disabled={busy} title="撤销完成，回到待开始" onClick={() => void mutate({ type: 'action', id: undoCompletion.id, action: 'reopen' }, undoCompletion.revision)}>撤销</BaseButton>}</div>}
    {activeTask && <><button className="kb-detail-backdrop" aria-label="关闭任务详情" onClick={closeDetail} /><aside ref={detailRef} tabIndex={-1} className="kb-detail" aria-label={`${activeTask.title}任务详情`}><header><span className="kb-detail-heading"><Kanban size={14} />{activeTask.id}<span className={`kb-dot kb-detail-indicator status-${activeTask.status}`} role="img" tabIndex={0} aria-label={`状态：${statuses.find(status => status.id === activeTask.status)?.label}；执行者：${activeTask.owner ?? '未认领'}`} title={`状态：${statuses.find(status => status.id === activeTask.status)?.label} · 执行者：${activeTask.owner ?? '未认领'}`} /></span><div className="kb-detail-header-actions"><BaseButton aria-label={showActivity ? '隐藏活动记录' : '显示活动记录'} title={showActivity ? '隐藏活动记录' : '显示活动记录'} aria-expanded={showActivity} aria-controls={`${detailFormId}-activity`} onClick={() => setShowActivity(value => !value)}><History size={16} /></BaseButton><BaseButton aria-label="关闭任务详情" onClick={closeDetail}><X size={17} /></BaseButton></div></header><div className="kb-detail-content">
      {errorView}

      <TaskDetailEditor formId={detailFormId} key={activeTask.id} task={activeTask} features={features} revision={board!.revision} busy={busy} onDirty={setDetailDirty} onSave={mutate} onCopyInstructions={() => copyTaskInstructions(activeTask)} />
      {activeTask.blocked && <div className="kb-detail-blocked"><LockKeyhole size={14} /><span>{activeTask.blocked}</span><BaseButton disabled={busy || detailDirty} onClick={() => void action(activeTask, 'unblock')}>解除</BaseButton></div>}
      {activeTask.result && <div className="kb-delivery"><h3>交付结果</h3><p>{activeTask.result}</p></div>}
      {activeTask.status === 'doing' && <div className="kb-detail-edit-actions"><BaseButton disabled={busy || detailDirty} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'progress' })}>记录进展 / 交接</BaseButton></div>}
      {showActivity && <div ref={activityRef} id={`${detailFormId}-activity`} className="kb-activity"><h3>活动记录</h3>{activeTask.activity.length ? activeTask.activity.map((entry, i) => <div key={i}><i /><span>{activityText(entry)}</span></div>) : <p>暂无活动记录</p>}</div>}
    </div><footer className="kb-detail-actions">
      <BaseButton variant="danger" disabled={busy || detailDirty} onClick={() => void deleteTask(activeTask)}>删除</BaseButton>
      <BaseButton form={detailFormId} variant="primary" type="submit" disabled={busy || !detailDirty}>{busy ? '保存中…' : '保存修改'}</BaseButton>
      {activeTask.status === 'doing' && <><BaseButton disabled={busy || detailDirty} onClick={() => void action(activeTask, 'release')}>释放任务</BaseButton><BaseButton variant="primary" disabled={busy || detailDirty || !!activeTask.blocked} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'submit' })}>提交验收</BaseButton></>}
      {activeTask.status === 'review' && <><BaseButton disabled={busy || detailDirty} onClick={() => setEditor({ kind: 'note', task: activeTask, action: 'reject' })}>退回修改</BaseButton><BaseButton variant="primary" disabled={busy || detailDirty || !!activeTask.blocked} onClick={() => void approve(activeTask)}>通过验收</BaseButton></>}
      {activeTask.status === 'done' && <BaseButton disabled={busy || detailDirty} onClick={() => void action(activeTask, 'reopen')}>重新打开</BaseButton>}
    </footer></aside></>}
    {syncOpen && board && <BoardSyncPanel workspaceKey={workspaceKey} workspaceRevision={workspaceRevision} boardRevision={board.revision} onClose={() => { setSyncOpen(false); void refresh() }} />}
    {editor && board && <BoardEditor key={editor.kind === 'task' ? `task:${editor.task?.id ?? 'new'}` : editor.kind === 'feature' ? `feature:${editor.feature?.id ?? 'new'}` : editor.kind === 'note' ? `note:${editor.task.id}:${editor.action}` : editor.kind} editor={editor} board={board} defaultFeature={feature} busy={busy} error={errorView} onRefresh={() => void refresh()} onClose={() => setEditor(null)} onEditor={setEditor} onSave={mutate} />}
  </section>
}

function Modal({ title, children, onClose, busy }: { title: string; children: ReactNode; onClose: () => void; busy: boolean }) {
  const id = useId()
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => { const el = ref.current; const previous = document.activeElement as HTMLElement | null; el?.showModal(); return () => { el?.close(); previous?.focus() } }, [])
  return <dialog ref={ref} className="kb-create-dialog" aria-labelledby={`${id}-title`} onKeyDown={e => e.stopPropagation()} onCancel={e => { e.preventDefault(); if (!busy) onClose() }}><header><h2 id={`${id}-title`}>{title}</h2><BaseButton disabled={busy} aria-label="关闭" onClick={onClose}><X size={18} /></BaseButton></header>{children}</dialog>
}
function BoardEditor({ editor, board, defaultFeature, busy, error, onRefresh, onClose, onEditor, onSave }: { editor: Editor; board: BoardSnapshot; defaultFeature: string; busy: boolean; error: ReactNode; onRefresh: () => void; onClose: () => void; onEditor: (e: Editor) => void; onSave: (o: BoardOperation) => Promise<boolean> }) {
  const dialogs = useDialogs()
  const id = useId()
  const choices = featureChoices(board.features)
  const [taskFeature, setTaskFeature] = useState(editor.kind === 'task' ? editor.task?.feature ?? (board.features.some(f => f.id === defaultFeature) ? defaultFeature : '') : '')
  const [priority, setPriority] = useState(editor.kind === 'task' ? editor.task?.priority ?? '普通' : '普通')
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
      const task = { title: value('title'), description: value('description'), acceptance: value('acceptance'), feature: taskFeature, priority }
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
      {editor.kind === 'task' && <><BaseField htmlFor={`${id}-field-1`} label="任务标题"><BaseInput id={`${id}-field-1`} autoFocus name="title" required maxLength={120} defaultValue={editor.task?.title} /></BaseField><div className="kb-form-row"><BaseField htmlFor={`${id}-field-2`} label="所属功能"><BaseSelect id={`${id}-field-2`} disabled={busy} value={taskFeature || choices.unclassified} onValueChange={value => setTaskFeature(value === choices.unclassified ? '' : value)} options={choices.withUnclassified} /></BaseField><BaseField htmlFor={`${id}-field-3`} label="优先级"><BaseSelect id={`${id}-field-3`} disabled={busy} value={priority} onValueChange={value => { if (value === '高' || value === '普通') setPriority(value) }} options={priorityChoices} /></BaseField></div><BaseField htmlFor={`${id}-field-4`} label="核心描述"><BaseTextarea id={`${id}-field-4`} name="description" rows={4} maxLength={4000} defaultValue={editor.task?.description} /></BaseField><BaseField htmlFor={`${id}-field-5`} label="目标与验收"><BaseTextarea id={`${id}-field-5`} name="acceptance" rows={4} maxLength={4000} defaultValue={editor.task?.acceptance} /></BaseField></>}
      {editor.kind === 'feature' && <><BaseField htmlFor={`${id}-field-6`} label="功能名称"><BaseInput id={`${id}-field-6`} autoFocus name="name" required maxLength={60} defaultValue={editor.feature?.name} /></BaseField><BaseField htmlFor={`${id}-field-7`} label="核心描述"><BaseTextarea id={`${id}-field-7`} name="description" rows={4} maxLength={4000} defaultValue={editor.feature?.description} /></BaseField><BaseField htmlFor={`${id}-field-8`} label="功能目标"><BaseTextarea id={`${id}-field-8`} name="goal" rows={4} maxLength={4000} defaultValue={editor.feature?.goal} /></BaseField><div className="kb-form-row"><BaseField htmlFor={`${id}-field-9`} label="画布横坐标"><BaseInput id={`${id}-field-9`} name="x" type="number" step="any" min={-100000} max={100000} required defaultValue={editor.feature?.x ?? 100 + board.features.length % 3 * 350} /></BaseField><BaseField htmlFor={`${id}-field-10`} label="画布纵坐标"><BaseInput id={`${id}-field-10`} name="y" type="number" step="any" min={-100000} max={100000} required defaultValue={editor.feature?.y ?? 100 + Math.floor(board.features.length / 3) * 320} /></BaseField></div></>}
      {editor.kind === 'note' && <><p className="kb-muted">{editor.task.id} · {editor.task.title}</p><BaseField htmlFor={`${id}-field-11`} label={editor.action === 'submit' ? '交付内容、验证情况与已知限制' : editor.action === 'progress' ? '已完成、剩余工作和接手提示' : editor.action === 'reject' ? '需要继续修改的原因' : '阻塞原因'}><BaseTextarea id={`${id}-field-11`} autoFocus name="text" required rows={7} maxLength={4000} /></BaseField></>}
      {editor.kind === 'features' && <><BaseButton type="button" onClick={() => onEditor({ kind: 'feature' })}><Plus size={14} />新建功能</BaseButton> <BaseButton type="button" onClick={() => onEditor({ kind: 'edges' })}>功能关联</BaseButton>{board.features.map(f => <div className="kb-feature-row" key={f.id}><strong>{f.name}</strong><p>{f.goal}</p><div><BaseButton type="button" onClick={() => onEditor({ kind: 'feature', feature: f })}>编辑</BaseButton><BaseButton variant="danger" type="button" onClick={() => void removeFeature(f)}>删除</BaseButton></div>{board.tasks.some(t => t.feature === f.id) && <BaseField htmlFor={`${id}-move-${f.id}`} label="删除时迁移任务到"><BaseSelect id={`${id}-move-${f.id}`} disabled={busy} value={moveTo[f.id] ?? ''} onValueChange={value => setMoveTo(current => ({ ...current, [f.id]: value }))} placeholder="请选择目标功能" options={choices.options.filter(option => option.value !== f.id)} /></BaseField>}</div>)}</>}
      {editor.kind === 'history' && <div className="kb-activity">{board.activity.length ? [...board.activity].reverse().map((a, i) => <div key={i}><span>{activityText(a)}</span></div>) : <p>暂无活动</p>}</div>}
      {editor.kind === 'edges' && <><p className="kb-muted">关联表示功能之间的结构关系，不影响任务执行顺序。</p><div className="kb-form-row"><BaseField htmlFor={`${id}-field-13`} label="起点"><BaseSelect id={`${id}-field-13`} disabled={busy} value={source} onValueChange={setSource} options={choices.options} /></BaseField><BaseField htmlFor={`${id}-field-14`} label="终点"><BaseSelect id={`${id}-field-14`} disabled={busy} value={target} onValueChange={setTarget} options={choices.options} /></BaseField></div><BaseButton type="button" disabled={!source || !target || source === target || edges.some(([a, b]) => a === source && b === target)} onClick={() => setEdges(current => [...current, [source, target]])}>添加关联</BaseButton>{edges.map(([a, b]) => <div className="kb-edge-row" key={`${a}-${b}`}><span>{board.features.find(f => f.id === a)?.name} → {board.features.find(f => f.id === b)?.name}</span><BaseButton type="button" onClick={() => setEdges(current => current.filter(edge => edge[0] !== a || edge[1] !== b))}>移除</BaseButton></div>)}</>}
    </fieldset>
    <footer><span>{editor.kind === 'task' && !editor.task ? '创建后进入待开始' : '保存后生效'}</span>{error && <BaseButton type="button" disabled={busy} onClick={onRefresh}>刷新数据</BaseButton>}<BaseButton type="button" disabled={busy} onClick={onClose}>{editor.kind === 'history' || editor.kind === 'features' ? '关闭' : '取消'}</BaseButton>{editor.kind !== 'features' && editor.kind !== 'history' && <BaseButton type="submit" variant="primary" disabled={busy}>{busy ? '保存中…' : '保存'}</BaseButton>}</footer>
  </form></Modal>
}
