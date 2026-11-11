export type TaskStatus = 'todo' | 'doing' | 'review' | 'done'
export type BoardTask = {
  id: string; title: string; description: string; acceptance: string; feature: string
  status: TaskStatus; priority: '高' | '普通'; owner?: string; ownerId?: string; blocked?: string; result?: string
  codexThreadId?: string
  createdAt?: number; statusChangedAt?: number
  activity: string[]
}
export type BoardFeature = { id: string; name: string; description: string; goal: string; x: number; y: number }
export type BoardSnapshot = { workspaceKey: string; revision: number; nextTask: number; features: BoardFeature[]; tasks: BoardTask[]; edges: [string, string][]; activity: string[] }
export type BoardOperation =
  | { type: 'idea_branch'; feature: BoardFeature; tasks: { title: string; description: string }[] }
  | { type: 'feature_save'; feature: BoardFeature }
  | { type: 'feature_delete'; id: string; moveTo?: string }
  | { type: 'edges'; edges: [string, string][] }
  | ({ type: 'task_create' } & Pick<BoardTask, 'title' | 'description' | 'acceptance' | 'feature' | 'priority'>)
  | ({ type: 'task_update' } & Pick<BoardTask, 'id' | 'title' | 'description' | 'acceptance' | 'feature' | 'priority'>)
  | { type: 'task_session'; id: string; threadId: string | null }
  | { type: 'task_delete'; id: string }
  | { type: 'task_reorder'; id: string; status: TaskStatus; beforeId?: string }
  | { type: 'action'; id: string; action: 'move' | 'complete' | 'claim' | 'release' | 'submit' | 'progress' | 'approve' | 'reject' | 'reopen' | 'block' | 'unblock'; text?: string }
  | { type: 'import'; features: BoardFeature[]; tasks: BoardTask[]; edges: [string, string][] }

export function activityText(entry: string) {
  return entry.replace(/^(\d+) · /, (_, seconds: string) => `${new Date(Number(seconds) * 1000).toLocaleString()} · `)
}
export type BoardSort = 'manual' | 'time_desc' | 'time_asc'
export function sortBoardTasks(tasks: BoardTask[], status: TaskStatus, sort: BoardSort) {
  const column = tasks.filter(task => task.status === status)
  if (sort === 'manual') return column
  const time = (task: BoardTask) => (status === 'todo' ? task.createdAt : task.statusChangedAt) ?? 0
  return column.sort((a, b) => (time(a) - time(b)) * (sort === 'time_desc' ? -1 : 1))
}
export const statuses: { id: TaskStatus; label: string }[] = [
  { id: 'todo', label: '待开始' },
  { id: 'doing', label: '进行中' },
  { id: 'review', label: '待验收' },
  { id: 'done', label: '已完成' },
]
// Legacy browser drafts are read only for an explicit one-time import.
export type BoardDraft = { tasks: BoardTask[]; features: BoardFeature[] }
export function readBoardDraft(key: string): BoardDraft | null {
  try {
    const value = JSON.parse(localStorage.getItem(`devflow.board.prototype.v1:${key}`) ?? 'null')
    const text = (v: unknown) => typeof v === 'string'
    if (value && Array.isArray(value.features) && value.features.length && value.features.every((f: BoardFeature) => f && [f.id, f.name, f.description, f.goal].every(text) && Number.isFinite(f.x) && Number.isFinite(f.y)) && Array.isArray(value.tasks) && value.tasks.every((t: BoardTask) => t && [t.id, t.title, t.description, t.acceptance, t.feature].every(text) && statuses.some(s => s.id === t.status) && ['高', '普通'].includes(t.priority) && [t.owner, t.blocked, t.result].every(v => v === undefined || text(v)) && Array.isArray(t.activity) && t.activity.every(text) && value.features.some((f: BoardFeature) => f.id === t.feature))) return { features: value.features, tasks: value.tasks }
  } catch { /* Preserve unavailable or invalid legacy data; never substitute demos. */ }
  return null
}
