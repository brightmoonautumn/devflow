import { useEffect, useId, useRef, useState, type CSSProperties, type FormEvent, type ReactNode } from 'react'
import { Check, ChevronLeft, ChevronRight, History, Flag, Kanban, LockKeyhole, Plus, RefreshCw, Trash2, X } from 'lucide-react'
import { useDialogs } from '../../components/form/BaseDialog'
import { BoardSortMenu } from './BoardSortMenu'
import { BoardFeatureMenu } from './BoardFeatureMenu'
import { BoardActivityDrawer } from './BoardActivityDrawer'
import { TaskDetailEditor } from './TaskDetailEditor'
import { executeBoard, getBoard } from './api'
import { activityText, readBoardDraft, sortBoardTasks, statuses, type BoardSort, type BoardOperation, type BoardSnapshot, type BoardTask, type TaskStatus } from './boardModel'
import { BoardSyncButton, BoardSyncPanel } from './BoardSyncPanel'
import { copyText } from '../../lib/clipboard'
import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseTextarea } from '../../components/form/BaseTextarea'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseButton } from '../../components/form/BaseButton'
import { featureChoices, priorityChoices } from './boardForm'
import './board.css'

type Editor = { kind: 'task'; task?: BoardTask } | { kind: 'note'; task: BoardTask; action: 'submit' | 'progress' | 'reject' | 'block' }
type Props = { workspaceName: string; workspaceKey: string; workspaceRevision: number; query: string; onQueryChange: (query: string) => void; onSearchFocus: () => void }
export function BoardPage({ workspaceName, workspaceKey, workspaceRevision, query, onQueryChange, onSearchFocus }: Props) {
  const detailFormId = useId()
  const [quickTitle, setQuickTitle] = useState('')
  const [taskMenu, setTaskMenu] = useState<{ taskId: string; x: number; y: number } | null>(null)
  const [syncOpen, setSyncOpen] = useState(false)
  const [historyOpen, setHistoryOpen] = useState(false)
  const [board, setBoard] = useState<BoardSnapshot | null>(null)
  const [feature, setFeature] = useState('all')
  const [showActivity, setShowActivity] = useState(false)
  const [detailExpanded, setDetailExpanded] = useState(false)
  const [detailWidth, setDetailWidth] = useState(430)
  const [detailResizing, setDetailResizing] = useState(false)
  const detailDrag = useRef<{ pointerId: number; x: number; width: number } | null>(null)
  const activityRef = useRef<HTMLDivElement>(null)
  const [detailDirty, setDetailDirty] = useState(false)
  const [selected, setSelected] = useState<string | null>(null)
  const [editor, setEditor] = useState<Editor | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const [mobileStatus, setMobileStatus] = useState<TaskStatus>('todo')
  const [dragged, setDragged] = useState<string | null>(null)
  const [settledTask, setSettledTask] = useState<string | null>(null)
  const [dropStatus, setDropStatus] = useState<TaskStatus | null>(null)
  const [dropPosition, setDropPosition] = useState<{ id: string; after: boolean } | null>(null)
  const [sortPreferences, setSortPreferences] = useState<Record<string, Partial<Record<TaskStatus, BoardSort>>>>({})
  const columnSort = (status: TaskStatus): BoardSort => sortPreferences[workspaceKey]?.[status] ?? 'manual'
  useEffect(() => {
    try {
      const saved = JSON.parse(localStorage.getItem(`devflow.board.sort.v1:${workspaceKey}`) ?? '{}')
      const values: Partial<Record<TaskStatus, BoardSort>> = {}
      for (const { id } of statuses) if (['manual', 'time_desc', 'time_asc'].includes(saved?.[id])) values[id] = saved[id]
      setSortPreferences(current => ({ ...current, [workspaceKey]: values }))
    } catch { /* Use manual order if browser preferences are unavailable. */ }
  }, [workspaceKey])
  function changeSort(status: TaskStatus, value: string) {
    if (value !== 'manual' && value !== 'time_desc' && value !== 'time_asc') return
    const values: Partial<Record<TaskStatus, BoardSort>> = { ...sortPreferences[workspaceKey], [status]: value }
    setSortPreferences(current => ({ ...current, [workspaceKey]: values }))
    try { localStorage.setItem(`devflow.board.sort.v1:${workspaceKey}`, JSON.stringify(values)) } catch { /* Sorting still works for this session. */ }
  }
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
      if (!mounted.current || seq !== sequence.current || writing.current || (background && paused.current)) return
      latest.current = value; setBoard(value)
      if (!background) setError('')
    } catch (e) { if (!controller.signal.aborted && mounted.current && seq === sequence.current) setError(e instanceof Error ? e.message : '读取看板失败') }
    finally { if (readAbort.current === controller) readAbort.current = null }
  }
  useEffect(() => {
    setHistoryOpen(false)
    void refresh()
    const timer = setInterval(() => { if (!paused.current && !writing.current && document.visibilityState === 'visible') void refresh(true) }, 5000)
    try { setHasDraft(localStorage.getItem(`devflow.board.prototype.v1:${workspaceKey}`) !== null) } catch { setHasDraft(false) }
    return () => { clearInterval(timer); sequence.current++; readAbort.current?.abort(); readAbort.current = null }
    // Workspace revision changes invalidate requests as well as workspace identity.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [workspaceKey, workspaceRevision])
  useEffect(() => {
    if (!taskMenu) return
    const close = () => setTaskMenu(null)
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') { event.stopPropagation(); close() } }
    window.addEventListener('pointerdown', close); window.addEventListener('keydown', key, true); window.addEventListener('resize', close); window.addEventListener('scroll', close, true)
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('keydown', key, true); window.removeEventListener('resize', close); window.removeEventListener('scroll', close, true) }
  }, [taskMenu])
  useEffect(() => { if (!settledTask) return; const timer = setTimeout(() => setSettledTask(null), 500); return () => clearTimeout(timer) }, [settledTask])
  useEffect(() => { if (!notice) return; const timer = setTimeout(() => { setNotice('') }, 6000); return () => clearTimeout(timer) }, [notice])
  useEffect(() => { setShowActivity(false); if (selected) detailRef.current?.focus() }, [selected])
  useEffect(() => { if (showActivity) activityRef.current?.scrollIntoView({ block: 'nearest' }) }, [showActivity])
  async function mutate(operation: BoardOperation, expectedBoardRevision?: number): Promise<boolean> {
    const current = latest.current
    if (!current || writing.current) return false
    writing.current = true; sequence.current++; readAbort.current?.abort(); setBusy(true); setError('')
    try {
      const value = await executeBoard(workspaceKey, workspaceRevision, expectedBoardRevision ?? current.revision, operation)
      if (!mounted.current) return false
      latest.current = value; setBoard(value); setNotice('已保存')
      if (operation.type === 'task_create') {
        onQueryChange(''); setFeature('all'); setMobileStatus('todo')
        setSelected(value.tasks.find(t => !current.tasks.some(old => old.id === t.id))?.id ?? null)
      }
      if (operation.type === 'feature_delete') setFeature(current => current === operation.id ? 'all' : current)
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
  const activeTask = tasks.find(t => t.id === selected)
  const featureColors = new Map([...features].sort((a, b) => a.id.localeCompare(b.id)).map((item, index) => [item.id, index % 8]))
  const visible = tasks.filter(t => (feature === 'all' || t.feature === feature) && `${t.id} ${t.title} ${t.description} ${t.owner ?? ''}`.toLocaleLowerCase().includes(query.trim().toLocaleLowerCase()))
  const action = (task: BoardTask, action: Extract<BoardOperation, { type: 'action' }>['action']) => mutate({ type: 'action', id: task.id, action })
  function moveTask(id: string, target: TaskStatus, beforeId?: string) {
    setDragged(null); setDropStatus(null); setDropPosition(null)
    const task = tasks.find(t => t.id === id)
    if (!task || writing.current || id === beforeId) return
    if (columnSort(target) === 'manual') {
      void mutate({ type: 'task_reorder', id, status: target, beforeId }).then(ok => { if (ok) setSettledTask(id) })
    } else if (task.status !== target) void mutate({ type: 'action', id, action: 'move', text: target }).then(ok => { if (ok) setSettledTask(id) })
  }
  function copyTaskInstructions(task: BoardTask) {
    void copyText(`请读取 DevFlow 当前工作区看板，核对工作区 ${workspaceKey}，优化任务 ${task.id}：${task.title}。结合所属功能与已有任务，提出描述、归类、验收条件和必要的拆分建议，信息不足时列出问题。先给我方案，确认后再通过 board_execute 更新任务；不要认领或执行，不要编译或测试。`).then(() => setNotice('已复制，请粘贴到 Codex 对话')).catch(() => setError('复制失败，请手动让 Codex 整理该任务'))
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
      <div className="kb-board-tools"><BoardFeatureMenu features={features} featureColors={featureColors} tasks={tasks} value={feature} busy={busy} onChange={setFeature} onSave={mutate} revision={() => latest.current?.revision} /><form className="kb-quick-create" onSubmit={event => { event.preventDefault(); const title = quickTitle.trim(); if (title && !busy) void mutate({ type: 'task_create', title, feature: features.some(f => f.id === feature) ? feature : '', description: '', acceptance: '', priority: '普通' }).then(ok => { if (ok) setQuickTitle('') }) }}><BaseInput aria-label="快速记录任务" placeholder="先记一句待办，回车记录…" maxLength={120} value={quickTitle} onChange={event => setQuickTitle(event.target.value)} disabled={busy} /></form><BaseButton disabled={busy} title="看板活动记录" aria-label="看板活动记录" onClick={() => setHistoryOpen(true)}><History size={16} /></BaseButton><BoardSyncButton disabled={busy} workspaceKey={workspaceKey} workspaceRevision={workspaceRevision} boardRevision={board.revision} onClick={() => setSyncOpen(true)} /><BaseButton disabled={busy} title="刷新看板" aria-label="刷新看板" onClick={() => void refresh()}><RefreshCw size={16} /></BaseButton>{hasDraft && board.revision === 0 && <BaseButton disabled={busy} onClick={() => void importDraft()}>导入草稿</BaseButton>}</div>
      <div className="kb-body"><div className="kb-content">
          <div className="kb-mobile-status" role="group" aria-label="任务阶段">{statuses.map(s => <button key={s.id} className={mobileStatus === s.id ? 'is-active' : ''} aria-pressed={mobileStatus === s.id} onClick={() => setMobileStatus(s.id)} onDragOver={event => { if (dragged) { event.preventDefault(); setMobileStatus(s.id); setDropStatus(s.id) } }} onDrop={event => { event.preventDefault(); moveTask(event.dataTransfer.getData('application/devflow-task'), s.id) }}>{s.label}<small>{visible.filter(t => t.status === s.id).length}</small></button>)}</div>
          <div className="kb-columns">{statuses.map(s => <section key={s.id} className={`kb-column kb-column-${s.id} ${mobileStatus === s.id ? 'is-mobile-active' : ''} ${dropStatus === s.id ? 'is-drop-target' : ''}`} aria-label={s.label} onDragOver={event => { if (dragged && !busy) { event.preventDefault(); setDropStatus(s.id); setDropPosition(null) } }} onDragLeave={event => { if (!event.currentTarget.contains(event.relatedTarget as Node | null)) setDropStatus(null) }} onDrop={event => { event.preventDefault(); moveTask(event.dataTransfer.getData('application/devflow-task'), s.id) }}>
            <header><span className={`kb-dot status-${s.id}`}>{s.id === 'done' && <Check size={8} />}</span><h2>{s.label}</h2><span className="kb-count">{visible.filter(t => t.status === s.id).length}</span><div className="kb-column-actions"><BoardSortMenu key={`${workspaceKey}:${s.id}`} status={s.id} label={s.label} value={columnSort(s.id)} disabled={busy} onChange={value => changeSort(s.id, value)} />{s.id === 'todo' && <button disabled={busy} aria-label="新建任务" onClick={() => setEditor({ kind: 'task' })}><Plus size={14} /></button>}</div></header>
            <div className="kb-column-tasks">{sortBoardTasks(visible, s.id, columnSort(s.id)).map(task => <button key={task.id} className={`kb-task ${selected === task.id ? 'is-selected' : ''} ${dragged === task.id ? 'is-dragging' : ''} ${settledTask === task.id ? 'is-settling' : ''} ${dropPosition?.id === task.id ? dropPosition.after ? 'is-drop-after' : 'is-drop-before' : ''}`} draggable={!busy} onDragOver={event => {
              if (!dragged || busy || columnSort(s.id) !== 'manual') return
              event.preventDefault(); event.stopPropagation(); setDropStatus(s.id)
              const rect = event.currentTarget.getBoundingClientRect()
              setDropPosition({ id: task.id, after: event.clientY > rect.top + rect.height / 2 })
            }} onDrop={event => {
              event.preventDefault(); event.stopPropagation()
              const id = event.dataTransfer.getData('application/devflow-task')
              if (id === task.id) { setDragged(null); setDropStatus(null); setDropPosition(null); return }
              const rect = event.currentTarget.getBoundingClientRect()
              const column = tasks.filter(t => t.status === s.id && t.id !== id)
              const beforeId = event.clientY > rect.top + rect.height / 2 ? column[column.findIndex(t => t.id === task.id) + 1]?.id : task.id
              moveTask(id, s.id, beforeId)
            }} onContextMenu={event => { event.preventDefault(); if (!busy) setTaskMenu({ taskId: task.id, x: Math.min(event.clientX, window.innerWidth - 140), y: Math.min(event.clientY, window.innerHeight - 55) }) }} onDragStart={event => { setSettledTask(null); event.dataTransfer.setData('application/devflow-task', task.id); event.dataTransfer.effectAllowed = 'move'; setDragged(task.id) }} onDragEnd={() => { setDragged(null); setDropStatus(null); setDropPosition(null) }} onClick={() => { if (!dragged) setSelected(task.id) }}><span className="kb-task-top"><span className={`kb-task-feature kb-feature-color-${featureColors.get(task.feature) ?? 'none'}`}>{features.find(f => f.id === task.feature)?.name ?? '未归类'}</span><span className="kb-task-id">{task.id}</span></span><strong>{task.title}</strong><p>{task.description}</p>{task.blocked && <span className="kb-blocked"><LockKeyhole size={11} />{task.blocked}</span>}<span className="kb-task-bottom"><span className={task.priority === '高' ? 'kb-priority-high' : 'kb-muted'}><Flag size={11} />{task.priority}</span><span className="kb-owner">{task.status === 'done' ? '已验收' : task.owner ?? '未认领'}</span></span></button>)}{!visible.some(t => t.status === s.id) && <div className="kb-column-empty"><span className="kb-empty-art" aria-hidden="true"><i /><i /></span><strong>{query || feature !== 'all' ? '没有匹配的任务' : '暂无任务'}</strong><span>{query || feature !== 'all' ? '试试调整筛选条件' : s.id === 'review' ? '完成开发后移至此处验收' : '任务会显示在这里'}</span></div>}</div>
            {s.id === 'todo' && <button className="kb-add-task" disabled={busy} onClick={() => setEditor({ kind: 'task' })}><Plus size={14} />添加任务</button>}
          </section>)}</div>

      </div></div>
    </>}
    {taskMenu && <div className="kb-task-menu" role="menu" aria-label="任务操作" style={{ left: taskMenu.x, top: taskMenu.y }} onPointerDown={event => event.stopPropagation()}><button role="menuitem" autoFocus onClick={() => { const id = taskMenu.taskId; setTaskMenu(null); if (detailDirty) { setError('请先保存或撤销任务修改，再删除任务。'); return } void mutate({ type: 'task_delete', id }).then(ok => { if (ok) setSelected(current => current === id ? null : current) }) }}><Trash2 size={14} />删除任务</button></div>}
    {notice && <div className="kb-toast" role="status"><Check size={14} />{notice}</div>}
    {activeTask && <><button className="kb-detail-backdrop" aria-label="关闭任务详情" onClick={closeDetail} /><aside ref={detailRef} tabIndex={-1} className={`kb-detail ${detailExpanded ? 'is-expanded' : ''} ${detailResizing ? 'is-resizing' : ''}`} style={{ '--kb-detail-width': `${detailWidth}px` } as CSSProperties} aria-label={`${activeTask.title}任务详情`}><button className="kb-detail-toggle" aria-label={detailExpanded ? '向右收起任务详情' : '向左展开任务详情'} title={detailExpanded ? '向右收起任务详情' : '向左展开任务详情'} aria-expanded={detailExpanded} onClick={() => setDetailExpanded(value => !value)}>{detailExpanded ? <ChevronRight size={14} /> : <ChevronLeft size={14} />}</button><div className="kb-detail-resizer" role="separator" aria-label="调整任务详情宽度" aria-orientation="vertical" aria-valuemin={280} aria-valuemax={detailRef.current?.parentElement?.clientWidth ?? 430} aria-valuenow={detailExpanded ? detailRef.current?.clientWidth : detailWidth} tabIndex={0} title="拖拽调整宽度 · 双击恢复默认" onPointerDown={event => { if (event.button !== 0) return; detailDrag.current = { pointerId: event.pointerId, x: event.clientX, width: event.currentTarget.parentElement?.clientWidth ?? detailWidth }; event.currentTarget.setPointerCapture(event.pointerId); setDetailResizing(true) }} onPointerMove={event => { const drag = detailDrag.current; if (!drag || drag.pointerId !== event.pointerId) return; const max = detailRef.current?.parentElement?.clientWidth ?? window.innerWidth; setDetailWidth(Math.max(Math.min(280, max), Math.min(max, drag.width + drag.x - event.clientX))); setDetailExpanded(false) }} onPointerUp={event => { if (detailDrag.current?.pointerId !== event.pointerId) return; detailDrag.current = null; setDetailResizing(false); if (event.currentTarget.hasPointerCapture(event.pointerId)) event.currentTarget.releasePointerCapture(event.pointerId) }} onPointerCancel={() => { detailDrag.current = null; setDetailResizing(false) }} onLostPointerCapture={() => { detailDrag.current = null; setDetailResizing(false) }} onDoubleClick={() => { setDetailExpanded(false); setDetailWidth(430) }} onKeyDown={event => { if (!['ArrowLeft', 'ArrowRight', 'Home', 'End'].includes(event.key)) return; event.preventDefault(); const max = detailRef.current?.parentElement?.clientWidth ?? window.innerWidth; const current = detailRef.current?.clientWidth ?? detailWidth; setDetailExpanded(false); setDetailWidth(Math.max(Math.min(280, max), Math.min(max, event.key === 'Home' ? 280 : event.key === 'End' ? max : current + (event.key === 'ArrowLeft' ? 1 : -1) * (event.shiftKey ? 40 : 10)))) }}><span aria-hidden="true" /></div><header><span className="kb-detail-heading"><Kanban size={14} />{activeTask.id}<span className={`kb-dot kb-detail-indicator status-${activeTask.status}`} role="img" tabIndex={0} aria-label={`状态：${statuses.find(status => status.id === activeTask.status)?.label}；执行者：${activeTask.owner ?? '未认领'}`} title={`状态：${statuses.find(status => status.id === activeTask.status)?.label} · 执行者：${activeTask.owner ?? '未认领'}`} /></span><div className="kb-detail-header-actions"><BaseButton aria-label={showActivity ? '隐藏活动记录' : '显示活动记录'} title={showActivity ? '隐藏活动记录' : '显示活动记录'} aria-expanded={showActivity} aria-controls={`${detailFormId}-activity`} onClick={() => setShowActivity(value => !value)}><History size={16} /></BaseButton><BaseButton aria-label="关闭任务详情" onClick={closeDetail}><X size={17} /></BaseButton></div></header><div className="kb-detail-content">
      {errorView}

      <TaskDetailEditor formId={detailFormId} key={activeTask.id} task={activeTask} features={features} revision={board!.revision} busy={busy} onDirty={setDetailDirty} onSave={mutate} onCopyInstructions={() => copyTaskInstructions(activeTask)} onCopyDescription={description => void copyText(description).then(() => setNotice('已复制任务描述')).catch(() => setError('复制失败，请手动复制任务描述'))} />
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
    {historyOpen && board && <BoardActivityDrawer activity={board.activity} onClose={() => setHistoryOpen(false)} />}
    {syncOpen && board && <BoardSyncPanel workspaceKey={workspaceKey} workspaceRevision={workspaceRevision} boardRevision={board.revision} onClose={() => { setSyncOpen(false); void refresh() }} />}
    {editor && board && <BoardEditor key={editor.kind === 'task' ? `task:${editor.task?.id ?? 'new'}` : `note:${editor.task.id}:${editor.action}`} editor={editor} board={board} defaultFeature={feature} busy={busy} error={errorView} onRefresh={() => void refresh()} onClose={() => setEditor(null)} onSave={mutate} />}
  </section>
}

function Modal({ title, children, onClose, busy }: { title: string; children: ReactNode; onClose: () => void; busy: boolean }) {
  const id = useId()
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => { const el = ref.current; const previous = document.activeElement as HTMLElement | null; el?.showModal(); return () => { el?.close(); previous?.focus() } }, [])
  return <dialog ref={ref} className="kb-create-dialog" aria-labelledby={`${id}-title`} onKeyDown={e => e.stopPropagation()} onCancel={e => { e.preventDefault(); if (!busy) onClose() }}><header><h2 id={`${id}-title`}>{title}</h2><BaseButton variant="ghost" className="kb-dialog-close" disabled={busy} aria-label="关闭" onClick={onClose}><X size={18} /></BaseButton></header>{children}</dialog>
}
function BoardEditor({ editor, board, defaultFeature, busy, error, onRefresh, onClose, onSave }: { editor: Editor; board: BoardSnapshot; defaultFeature: string; busy: boolean; error: ReactNode; onRefresh: () => void; onClose: () => void; onSave: (o: BoardOperation) => Promise<boolean> }) {
  const id = useId()
  const choices = featureChoices(board.features)
  const [taskFeature, setTaskFeature] = useState(editor.kind === 'task' ? editor.task?.feature ?? (board.features.some(f => f.id === defaultFeature) ? defaultFeature : '') : '')
  const [priority, setPriority] = useState(editor.kind === 'task' ? editor.task?.priority ?? '普通' : '普通')
  const [localError, setLocalError] = useState('')
  const labels = { submit: '提交验收', progress: '记录进展 / 交接', reject: '退回修改', block: '标记阻塞' }
  const title = editor.kind === 'task' ? editor.task ? '编辑任务' : '新建任务' : labels[editor.action]
  async function save(e: FormEvent<HTMLFormElement>) {
    e.preventDefault(); setLocalError('')
    const data = new FormData(e.currentTarget), value = (key: string) => String(data.get(key) ?? '').trim()
    let operation: BoardOperation
    if (editor.kind === 'task') {
      if (!value('title')) { setLocalError('请输入任务标题'); return }
      const task = { title: value('title'), description: value('description'), acceptance: value('acceptance'), feature: taskFeature, priority }
      operation = editor.task ? { type: 'task_update', id: editor.task.id, ...task } : { type: 'task_create', ...task }
    } else if (editor.kind === 'note') {
      if (!value('text')) { setLocalError('请填写说明'); return }
      operation = { type: 'action', id: editor.task.id, action: editor.action, text: value('text') }
    } else return
    if (await onSave(operation)) onClose()
  }
  return <Modal title={title} onClose={onClose} busy={busy}><form key={editor.kind === 'task' ? editor.task?.id ?? 'new-task' : title} onSubmit={e => void save(e)}>
    {error}{localError && <p className="kb-error" role="alert">{localError}</p>}
    <fieldset disabled={busy} className="kb-editor-fields">
      {editor.kind === 'task' && <><BaseField htmlFor={`${id}-field-1`} label="任务标题"><BaseInput id={`${id}-field-1`} autoFocus name="title" required maxLength={120} defaultValue={editor.task?.title} /></BaseField><div className="kb-form-row"><BaseField htmlFor={`${id}-field-2`} label="所属功能"><BaseSelect id={`${id}-field-2`} disabled={busy} value={taskFeature || choices.unclassified} onValueChange={value => setTaskFeature(value === choices.unclassified ? '' : value)} options={choices.withUnclassified} /></BaseField><BaseField htmlFor={`${id}-field-3`} label="优先级"><BaseSelect id={`${id}-field-3`} disabled={busy} value={priority} onValueChange={value => { if (value === '高' || value === '普通') setPriority(value) }} options={priorityChoices} /></BaseField></div><BaseField htmlFor={`${id}-field-4`} label="核心描述"><BaseTextarea id={`${id}-field-4`} name="description" rows={4} maxLength={4000} defaultValue={editor.task?.description} /></BaseField><BaseField htmlFor={`${id}-field-5`} label="目标与验收"><BaseTextarea id={`${id}-field-5`} name="acceptance" rows={4} maxLength={4000} defaultValue={editor.task?.acceptance} /></BaseField></>}
      {editor.kind === 'note' && <><p className="kb-muted">{editor.task.id} · {editor.task.title}</p><BaseField htmlFor={`${id}-field-11`} label={editor.action === 'submit' ? '交付内容、验证情况与已知限制' : editor.action === 'progress' ? '已完成、剩余工作和接手提示' : editor.action === 'reject' ? '需要继续修改的原因' : '阻塞原因'}><BaseTextarea id={`${id}-field-11`} autoFocus name="text" required rows={7} maxLength={4000} /></BaseField></>}

    </fieldset>
    <footer><span>{editor.kind === 'task' && !editor.task ? '创建后进入待开始' : '保存后生效'}</span>{error && <BaseButton type="button" disabled={busy} onClick={onRefresh}>刷新数据</BaseButton>}<BaseButton type="button" disabled={busy} onClick={onClose}>取消</BaseButton><BaseButton type="submit" variant="primary" disabled={busy}>{busy ? '保存中…' : '保存'}</BaseButton></footer>
  </form></Modal>
}
