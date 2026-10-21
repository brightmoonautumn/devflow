export type TaskStatus = 'todo' | 'doing' | 'review' | 'done'
export type BoardTask = {
  id: string; title: string; description: string; acceptance: string; feature: string
  status: TaskStatus; priority: '高' | '普通'; owner?: string; blocked?: string; result?: string
  activity: string[]
}
export type BoardFeature = { id: string; name: string; description: string; goal: string; x: number; y: number }
export const statuses: { id: TaskStatus; label: string; caption: string }[] = [
  { id: 'todo', label: '待开始', caption: '准备就绪，等待认领' },
  { id: 'doing', label: '进行中', caption: '正在推进的任务' },
  { id: 'review', label: '待验收', caption: '交付结果，等你确认' },
  { id: 'done', label: '已完成', caption: '已经验收通过' },
]
export const initialFeatures: BoardFeature[] = [
  { id: 'workspace', name: '工作区管理', description: '把多个本地项目组织成一个开发空间。', goal: '工作区可在 DevFlow 与 VS Code 之间自然切换。', x: 100, y: 130 },
  { id: 'runtime', name: '项目运行', description: '统一启动 Java 和 Node.js 服务，查看运行状态。', goal: '在一个地方完成启动、停止和运行诊断。', x: 100, y: 430 },
  { id: 'board', name: '任务看板', description: '将讨论沉淀为功能与任务，让 AI 认领并交付。', goal: '每个工作区有自己的任务空间，流程与架构共用一份数据。', x: 500, y: 230 },
  { id: 'agent', name: 'AI 协作', description: 'Codex 主动获取任务、认领工作并提交执行结果。', goal: '通过 MCP / CLI 提供简洁的任务上下文，支持中断后交接。', x: 900, y: 230 },
]
export const featureEdges = [['workspace', 'runtime'], ['workspace', 'board'], ['board', 'agent']] as const
export function createDemoTasks(): BoardTask[] {
  return [
    { id: 'DF-021', feature: 'board', title: '支持创建与编辑任务', description: '讨论形成共识后，可以将核心功能描述、任务和目标写入工作区看板。', acceptance: '能够填写标题、所属功能、任务描述和验收条件；两个视图展示同一份任务。', status: 'todo', priority: '高', activity: ['你创建了任务'] },
    { id: 'DF-022', feature: 'agent', title: '提供任务认领能力', description: 'Codex 可以主动认领待开始任务，明确当前执行者。', acceptance: '认领后进入进行中；同一任务不能被两个会话同时认领。', status: 'todo', priority: '高', activity: ['你创建了任务'] },
    { id: 'DF-023', feature: 'agent', title: '支持中断后的任务交接', description: '会话中断后保留简短进展，允许释放任务并由新会话接手。', acceptance: '新会话能够了解已完成内容、剩余工作和阻塞原因。', status: 'todo', priority: '普通', blocked: '等待任务认领能力', activity: ['你创建了任务', '标记依赖：任务认领能力'] },
    { id: 'DF-019', feature: 'board', title: '实现流程与架构双视图', description: '流程视图看进度，架构画布看功能归属。切换视图时保持任务状态一致。', acceptance: '支持四阶段流程看板；画布按功能组织任务，可查看完成进度。', status: 'doing', priority: '高', owner: 'Codex', activity: ['你创建了任务', 'Codex 认领了任务', '开始整理两种视图的交互'] },
    { id: 'DF-020', feature: 'runtime', title: '补充启动失败的可读提示', description: '服务启动失败时展示明确原因，并保留输出入口方便诊断。', acceptance: '启动失败后可以看到原因并打开对应输出。', status: 'doing', priority: '普通', owner: 'Codex', activity: ['你创建了任务', 'Codex 认领了任务'] },
    { id: 'DF-018', feature: 'workspace', title: '优化最近工作区切换', description: '在工作区菜单展示最近打开记录，失效位置给出清晰提示。', acceptance: '可以切换到最近工作区；位置不存在时不能误操作。', status: 'review', priority: '普通', owner: 'Codex', result: '已补充最近打开列表和失效路径提示。切换前保留未保存配置的确认步骤。\n验证：本原型中的交付说明为演示内容，未执行真实验证。', activity: ['你创建了任务', 'Codex 认领了任务', 'Codex 提交了交付结果，等待验收'] },
    { id: 'DF-016', feature: 'workspace', title: '支持多文件夹工作区', description: '将多个本地项目加入同一工作区，并兼容 VS Code 工作区文件。', acceptance: '添加、移除文件夹后，可以保存并重新打开。', status: 'done', priority: '普通', result: '多文件夹工作区流程已完成。此记录为看板演示数据。', activity: ['Codex 提交了交付结果', '你通过了验收'] },
    { id: 'DF-017', feature: 'runtime', title: '独立挂载 Arthas', description: '服务运行后可以独立附加 Arthas，无需调整原有启动方式。', acceptance: '可以选择运行中的 Java 服务并打开诊断入口。', status: 'done', priority: '普通', activity: ['Codex 提交了交付结果', '你通过了验收'] },
  ]
}
// Frontend-only prototype: browser-local drafts, isolated by workspace identity.
export type BoardDraft = { tasks: BoardTask[]; features: BoardFeature[] }
export function readBoardDraft(key: string): BoardDraft {
  try {
    const value = JSON.parse(localStorage.getItem(`devflow.board.prototype.v1:${key}`) ?? 'null')
    const text = (v: unknown) => typeof v === 'string'
    if (value && Array.isArray(value.features) && value.features.length && value.features.every((f: BoardFeature) => f && [f.id, f.name, f.description, f.goal].every(text) && Number.isFinite(f.x) && Number.isFinite(f.y)) && Array.isArray(value.tasks) && value.tasks.every((t: BoardTask) => t && [t.id, t.title, t.description, t.acceptance, t.feature].every(text) && statuses.some(s => s.id === t.status) && ['高', '普通'].includes(t.priority) && [t.owner, t.blocked, t.result].every(v => v === undefined || text(v)) && Array.isArray(t.activity) && t.activity.every(text) && value.features.some((f: BoardFeature) => f.id === t.feature))) return value
  } catch { /* A stale or unavailable draft must not prevent opening the prototype. */ }
  return { tasks: createDemoTasks(), features: initialFeatures.map(f => ({ ...f })) }
}
