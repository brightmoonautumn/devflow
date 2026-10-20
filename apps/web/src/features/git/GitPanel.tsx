import { readOnlyShare } from '../../lib/access'
import { useDialogs } from '../../components/form/BaseDialog'
import { useCallback, useEffect, useRef, useState, type ReactNode } from 'react'
import { ChevronRight, FileDiff, Folder, GitBranch, Plus, RefreshCw, Undo2, X } from 'lucide-react'
import type { WorkspaceResponse } from '../../lib/api'
import { gitRequest, type GitChange, type GitDiff, type GitOutcome, type GitPreview, type GitRepository, type GitSelection, type GitStatus } from './api'
import './git.css'

const key = (repo: GitRepository, path: string) => `${repo.root}\0${path}`
const absolute = (root: string, path: string) => `${root.replace(/[\\/]$/, '')}/${path}`
const messageOf = (error: unknown) => error instanceof Error ? error.message : String(error)
function Checkbox({ checked, mixed = false, disabled = false, label, onChange }: { checked: boolean; mixed?: boolean; disabled?: boolean; label: string; onChange: () => void }) {
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => { if (ref.current) ref.current.indeterminate = mixed }, [mixed])
  if (readOnlyShare) return null
  return <input ref={ref} type="checkbox" checked={checked} disabled={disabled} aria-label={label} onChange={onChange} />
}
type Tree = { folders: Map<string, Tree>; files: GitChange[] }
function treeOf(files: GitChange[]) {
  const root: Tree = { folders: new Map(), files: [] }
  for (const file of files) {
    let node = root
    for (const part of file.path.split('/').slice(0, -1)) {
      if (!node.folders.has(part)) node.folders.set(part, { folders: new Map(), files: [] })
      node = node.folders.get(part)!
    }
    node.files.push(file)
  }
  return root
}
function FolderRow({ name, children }: { name: string; children: ReactNode }) {
  const [open, setOpen] = useState(true)
  return <div><button className="git-tree-folder" onClick={() => setOpen(!open)} aria-expanded={open}><ChevronRight size={12} className={open ? 'is-expanded' : ''} /><Folder size={13} /><span>{name}</span></button>{open && <div className="git-indent">{children}</div>}</div>
}
export function GitPanel({ workspace, active, onOpenDiff, onClose }: { workspace: WorkspaceResponse | null; active: boolean; onOpenDiff: (diff: GitDiff) => void; onClose: () => void }) {
  const dialogs = useDialogs()
  const [status, setStatus] = useState<GitStatus | null>(null)
  const [selected, setSelected] = useState(new Set<string>())
  const [message, setMessage] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const [loading, setLoading] = useState(false)
  const [preview, setPreview] = useState<GitPreview | null>(null)
  const [menu, setMenu] = useState<{ repo: GitRepository; file: GitChange; x: number; y: number } | null>(null)
  const current = useRef(status), running = useRef(false), mounted = useRef(true), refreshing = useRef(false)
  const diffController = useRef<AbortController | null>(null)
  const revision = workspace?.revision
  current.current = status
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; diffController.current?.abort() } }, [])
  const refresh = useCallback(async (signal?: AbortSignal) => {
    if (revision === undefined || refreshing.current) return
    refreshing.current = true; setLoading(true)
    try {
      const next = await gitRequest<GitStatus>('status', { expectedRevision: revision }, signal ?? AbortSignal.timeout(60000))
      if (!mounted.current || signal?.aborted) return
      const previous = current.current
      setSelected(values => new Set([...values].filter(value => next.repositories.some(repo => previous?.repositories.find(old => old.root === repo.root)?.version === repo.version && repo.changes.some(file => key(repo, file.path) === value)))))
      if (previous && JSON.stringify(previous) !== JSON.stringify(next)) setPreview(null)
      current.current = next; setStatus(next)
    } catch (error) { if (mounted.current && !signal?.aborted) setNotice(messageOf(error)) }
    finally { refreshing.current = false; if (mounted.current) setLoading(false) }
  }, [revision])
  useEffect(() => {
    if (!active || revision === undefined) return
    const controller = new AbortController()
    void refresh(controller.signal)
    const timer = setInterval(() => { if (!running.current && !document.hidden) void refresh(controller.signal) }, 5000)
    return () => { controller.abort(); clearInterval(timer); diffController.current?.abort() }
  }, [active, revision, refresh])
  useEffect(() => {
    if (!menu) return
    const close = () => setMenu(null)
    const escape = (event: KeyboardEvent) => { if (event.key === 'Escape') close() }
    window.addEventListener('click', close); window.addEventListener('keydown', escape)
    return () => { window.removeEventListener('click', close); window.removeEventListener('keydown', escape) }
  }, [menu])
  function toggle(repo: GitRepository, files: GitChange[]) {
    setPreview(null)
    const keys = files.filter(file => file.selectable).map(file => key(repo, file.path))
    setSelected(old => { const next = new Set(old), remove = keys.every(value => next.has(value)); keys.forEach(value => remove ? next.delete(value) : next.add(value)); return next })
  }
  function selections(filter: (file: GitChange) => boolean = () => true): GitSelection[] {
    return (status?.repositories ?? []).map(repo => ({ repository: repo.root, version: repo.version, paths: repo.changes.filter(file => selected.has(key(repo, file.path)) && filter(file)).map(file => file.path) })).filter(value => value.paths.length)
  }
  function guard(items: GitSelection[]) {
    const paths = items.flatMap(item => item.paths.flatMap(path => {
      const original = status?.repositories.find(repo => repo.root === item.repository)?.changes.find(file => file.path === path)?.originalPath
      return [absolute(item.repository, path), ...(original ? [absolute(item.repository, original)] : [])]
    }))
    if (!window.dispatchEvent(new CustomEvent('devflow-before-replace', { detail: paths, cancelable: true }))) throw new Error('所选文件正在编辑或保存，请等待自动保存完成后重试。')
  }
  async function execute(action: () => Promise<void>) {
    if (running.current) return
    running.current = true; setBusy(true); setNotice(''); setMenu(null)
    try { await action() } catch (error) { if (mounted.current) { setNotice(messageOf(error)); setPreview(null) } }
    finally { running.current = false; if (mounted.current) { setBusy(false); await refresh() } }
  }
  function outcome(result: GitOutcome, rollback = false) {
    setPreview(null)
    setNotice(result.results.map(item => `${item.repository.split(/[\\/]/).at(-1)}：${item.error ? item.error.message : item.commit ? `已提交 ${item.commit.slice(0, 8)}` : '完成'}`).join('\n'))
    setSelected(old => { const next = new Set(old); result.results.forEach(item => item.paths.forEach(path => next.delete(`${item.repository}\0${path}`))); return next })
    if (rollback || result.results.some(item => item.commit)) {
      const paths = result.results.flatMap(item => item.paths.map(path => absolute(item.repository, path)))
      window.dispatchEvent(new CustomEvent('devflow-files-replaced', { detail: paths }))
      if (rollback) paths.forEach(path => window.dispatchEvent(new CustomEvent('devflow-tree-file-changed', { detail: path })))
    }
  }
  async function mutate(operation: 'add' | 'rollback', items = selections(file => operation === 'add' ? file.untracked : !file.untracked && !file.submodule)) {
    if (!items.length) { setNotice(operation === 'add' ? '请先勾选未管理的文件。' : '请先勾选可回滚的普通文件。子模块指针需在子仓库处理。'); return }
    if (operation === 'rollback' && !await dialogs.confirm({ title: '回滚所选文件？', description: `以下文件的暂存区与工作区修改将回滚，无法撤销。新增文件仅取消管理并保留磁盘文件。\n${items.flatMap(item => item.paths.map(path => absolute(item.repository, path))).join('\n')}`, confirmText: '确认回滚', tone: 'danger' })) return
    if (!mounted.current) return
    void execute(async () => { guard(items); outcome(await gitRequest<GitOutcome>('mutate', { expectedRevision: revision, selections: items, operation }), operation === 'rollback') })
  }
  function prepare() {
    const items = selections()
    if (!items.length) return
    void execute(async () => {
      guard(items)
      const plan = await gitRequest<GitPreview>('commit/preview', { expectedRevision: revision, selections: items, message })
      if (!mounted.current) return
      setPreview(plan)
      const first = plan.repositories[0]?.files[0]; if (first) onOpenDiff(first)
    })
  }
  async function showDiff(repo: GitRepository, file: GitChange) {
    diffController.current?.abort(); const controller = new AbortController(); diffController.current = controller
    try {
      const diff = await gitRequest<GitDiff>('diff', { expectedRevision: revision, repository: repo.root, version: repo.version, path: file.path }, controller.signal)
      if (mounted.current && !controller.signal.aborted) onOpenDiff(diff)
    } catch (error) { if (!controller.signal.aborted && mounted.current) setNotice(messageOf(error)) }
  }
  function renderTree(repo: GitRepository, tree: Tree): ReactNode {
    return <>{[...tree.folders].map(([name, child]) => <FolderRow key={name} name={name}>{renderTree(repo, child)}</FolderRow>)}{tree.files.map(file => <div className={`git-file ${file.selectable ? '' : 'is-disabled'}`} key={file.path} title={`${file.originalPath ? `${file.originalPath} → ` : ''}${file.path}${file.reason ? `\n${file.reason}` : ''}`} onContextMenu={event => { event.preventDefault(); setMenu({ repo, file, x: Math.max(0, Math.min(event.clientX, window.innerWidth - 210)), y: Math.max(0, Math.min(event.clientY, window.innerHeight - 145)) }) }}>
      <Checkbox checked={selected.has(key(repo, file.path))} disabled={busy || !file.selectable || !!repo.blocked || repo.truncated} label={`选择 ${repo.name}/${file.path}`} onChange={() => toggle(repo, [file])} />
      <button onClick={() => void showDiff(repo, file)}><FileDiff size={13} /><span>{file.path.split('/').at(-1)}</span><small>{file.submodule ? file.pointerChanged ? '指针' : '子仓库有改动' : file.status}</small></button>
    </div>)}</>
  }
  function section(untracked: boolean) {
    const repositories = status?.repositories.filter(repo => repo.changes.some(file => file.untracked === untracked)) ?? []
    const count = repositories.reduce((total, repo) => total + repo.changes.filter(file => file.untracked === untracked).length, 0)
    return <section className="git-section"><h3>{untracked ? 'Unversioned Files' : 'Changes'} <small>{count} files</small></h3>{repositories.map(repo => {
      const files = repo.changes.filter(file => file.untracked === untracked), available = files.filter(file => file.selectable)
      const checked = available.filter(file => selected.has(key(repo, file.path))).length
      return <div key={repo.root} className="git-repository"><div className="git-repo-title" title={`${repo.root}${repo.parent ? `\n子仓库，父仓库：${repo.parent}` : ''}`}><Checkbox checked={available.length > 0 && checked === available.length} mixed={checked > 0 && checked < available.length} disabled={busy || !available.length || !!repo.blocked || repo.truncated} label={`选择仓库 ${repo.name} 的${untracked ? '未管理文件' : '变化'}`} onChange={() => toggle(repo, files)} /><GitBranch size={13} /><strong>{repo.name}</strong><small>{files.length}</small><span className="git-branch">{repo.branch}</span>{repo.parent && <small>子仓库</small>}</div>{repo.blocked && <p className="git-notice">{repo.blocked}</p>}{repo.truncated && <p className="git-notice">列表不完整，已禁用修改操作</p>}<div className="git-indent">{renderTree(repo, treeOf(files))}</div></div>
    })}</section>
  }
  return <div className="git-panel"><header className="ws-explorer-header"><strong>Commit</strong><div><button className="ws-icon-button" disabled={busy || loading || !workspace} title="刷新 Git 状态" aria-label="刷新 Git 状态" onClick={() => void refresh()}><RefreshCw size={14} className={loading ? 'spinning' : ''} /></button>{!readOnlyShare && <><button className="ws-icon-button" disabled={busy} title="Add to VCS" aria-label="添加所选未管理文件" onClick={() => mutate('add')}><Plus size={15} /></button><button className="ws-icon-button" disabled={busy} title="Rollback" aria-label="回滚所选文件" onClick={() => mutate('rollback')}><Undo2 size={15} /></button></>}<button className="ws-icon-button" onClick={onClose} aria-label="关闭 Git 面板"><X size={14} /></button></div></header>
    <div className="git-scroll">{!workspace ? <p className="git-notice">请先打开工作区。</p> : <>{section(false)}{section(true)}{status && !status.repositories.length && <p className="git-notice">没有发现 Git 仓库。请将完整仓库根目录加入工作区。</p>}{status?.warnings.map((warning, index) => <p key={index} className="git-notice">{warning}</p>)}</>}</div>
    {notice && <p className="git-notice" role="status">{notice}</p>}
    {!readOnlyShare && <div className="git-commit"><textarea aria-label="提交信息" placeholder="Commit Message" value={message} disabled={busy} onChange={event => { setMessage(event.target.value); setPreview(null) }} />
      {preview ? <div className="git-preview"><strong>确认提交 · {preview.repositories.length} 个仓库</strong><div className="git-preview-files">{preview.repositories.map(repo => <div key={repo.repository}><small title={repo.repository}>{repo.repository.split(/[\\/]/).at(-1)} · {repo.branch}</small>{repo.files.map(diff => <button key={diff.path} title={diff.path} onClick={() => onOpenDiff(diff)}><FileDiff size={12} />{diff.path}</button>)}</div>)}</div><p>点击文件查看 Diff。各仓库分别生成提交。</p><button className="ws-primary-button" disabled={busy} onClick={() => void execute(async () => { guard(preview.repositories.map(repo => ({ repository: repo.repository, version: '', paths: repo.files.map(file => file.path) }))); outcome(await gitRequest<GitOutcome>('commit/apply', { expectedRevision: revision, planId: preview.planId })) })}>{busy ? '提交中…' : '确认 Commit'}</button><button className="ws-outline-button" disabled={busy} onClick={() => setPreview(null)}>取消</button></div>
        : <button className="ws-primary-button" disabled={busy || !message.trim() || !selected.size} onClick={prepare}>{busy ? '处理中…' : `Commit${selected.size ? ` (${selected.size})` : ''}`}</button>}
    </div>}

    {menu && <div className="git-context" role="menu" style={{ left: menu.x, top: menu.y }} onClick={event => event.stopPropagation()}><button role="menuitem" onClick={() => { void showDiff(menu.repo, menu.file); setMenu(null) }}>Show Diff</button>{!readOnlyShare && <><button role="menuitem" disabled={busy || !menu.file.selectable || !menu.file.untracked} onClick={() => mutate('add', [{ repository: menu.repo.root, version: menu.repo.version, paths: [menu.file.path] }])}>Add to VCS</button><button role="menuitem" disabled={busy || !menu.file.selectable || menu.file.untracked || menu.file.submodule} onClick={() => mutate('rollback', [{ repository: menu.repo.root, version: menu.repo.version, paths: [menu.file.path] }])}>Rollback…</button></>}</div>}
  </div>
}
