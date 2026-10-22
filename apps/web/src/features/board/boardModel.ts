export type TaskStatus = 'todo' | 'doing' | 'review' | 'done'
export type BoardTask = {
  id: string; title: string; description: string; acceptance: string; feature: string
  status: TaskStatus; priority: '高' | '普通'; owner?: string; ownerId?: string; blocked?: string; result?: string
  activity: string[]
}
export type BoardFeature = { id: string; name: string; description: string; goal: string; x: number; y: number }
export type BoardSnapshot = { workspaceKey: string; revision: number; nextTask: number; features: BoardFeature[]; tasks: BoardTask[]; edges: [string, string][]; activity: string[] }
export type BoardOperation =
  | { type: 'feature_save'; feature: BoardFeature }
  | { type: 'feature_delete'; id: string; moveTo?: string }
  | { type: 'edges'; edges: [string, string][] }
  | ({ type: 'task_create' } & Pick<BoardTask, 'title' | 'description' | 'acceptance' | 'feature' | 'priority'>)
  | ({ type: 'task_update' } & Pick<BoardTask, 'id' | 'title' | 'description' | 'acceptance' | 'feature' | 'priority'>)
  | { type: 'task_delete'; id: string }
  | { type: 'action'; id: string; action: 'claim' | 'release' | 'submit' | 'progress' | 'approve' | 'reject' | 'reopen' | 'block' | 'unblock'; text?: string }
  | { type: 'import'; features: BoardFeature[]; tasks: BoardTask[]; edges: [string, string][] }

export function activityText(entry: string) {
  return entry.replace(/^(\d+) · /, (_, seconds: string) => `${new Date(Number(seconds) * 1000).toLocaleString()} · `)
}
export const statuses: { id: TaskStatus; label: string; caption: string }[] = [
  { id: 'todo', label: '待开始', caption: '准备就绪，等待认领' },
  { id: 'doing', label: '进行中', caption: '正在推进的任务' },
  { id: 'review', label: '待验收', caption: '交付结果，等你确认' },
  { id: 'done', label: '已完成', caption: '已经验收通过' },
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
