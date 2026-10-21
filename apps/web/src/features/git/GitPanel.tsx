import { readOnlyShare } from '../../lib/access'
import { useDialogs } from '../../components/form/BaseDialog'
import { useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { Download, GitMerge, History, ChevronRight, ChevronsDownUp, ChevronsUpDown, Plus, Settings2, RefreshCw, Undo2, X } from 'lucide-react'
import { TreeEntryIcon } from '../../components/files/TreeEntryIcon'
import { FileTreeRow, TreeChevron, TreePathLabel, handleTreeNavigation } from '../../components/files/FileTreeRow'
import { compactFolderChild } from '../../components/files/compactFolders'
import { useCompactFolders } from '../../components/files/useCompactFolders'
import { BaseButton } from '../../components/form/BaseButton'
import { CommitMessageDialog } from './CommitMessageDialog'
import { ResizableCommitPanel } from './ResizableCommitPanel'
import type { WorkspaceResponse } from '../../lib/api'
import { gitRequest, openGitUpdate, type GitChange, type GitDiff, type GitOutcome, type GitPreview, type GitRepository, type GitSelection, type GitStatus } from './api'
import './git.css'

const key = (repo: GitRepository, path: string) => `${repo.root}\0${path}`
const absolute = (root: string, path: string) => `${root.replace(/[\\/]$/, '')}/${path}`
const visibleChange = (file: GitChange) => !file.submodule || file.pointerChanged
const messageOf = (error: unknown) => error instanceof Error ? error.message : String(error)
function Checkbox({ checked, mixed = false, disabled = false, label, onChange }: { checked: boolean; mixed?: boolean; disabled?: boolean; label: string; onChange: () => void }) {
  const ref = useRef<HTMLInputElement>(null)
  useEffect(() => { if (ref.current) ref.current.indeterminate = mixed }, [mixed])
  if (readOnlyShare) return null
  return <input ref={ref} type="checkbox" checked={checked} disabled={disabled} aria-label={label} onChange={onChange} />
}
type Tree = { folders: Map<string, Tree>; files: GitChange[]; descendants: GitChange[] }
const emptyTree = (): Tree => ({ folders: new Map(), files: [], descendants: [] })
function treeOf(files: GitChange[]) {
  const root = emptyTree()
  for (const file of files) {
    let node = root
    node.descendants.push(file)
    for (const part of file.path.split('/').slice(0, -1)) {
      if (!node.folders.has(part)) node.folders.set(part, emptyTree())
      node = node.folders.get(part)!
      node.descendants.push(file)
    }
    node.files.push(file)
  }
  return root
}
type FolderSegment = { name: string; path: string; tree: Tree | null }
function folderSegments(name: string, tree: Tree, path: string, compact: boolean, truncated: boolean) {
  const segments: FolderSegment[] = [{ name, path, tree }]
  let current = segments[0]
  while (compact && !truncated && current.tree) {
    const entries: FolderSegment[] = [
      ...Array.from(current.tree.folders, ([name, tree]) => ({ name, path: `${current.path}/${name}`, tree })),
      ...current.tree.files.map(file => ({ name: file.path.split('/').at(-1)!, path: file.path, tree: null })),
    ]
    const child = compactFolderChild(segments, entries, { enabled: compact, truncated, key: entry => entry.path, isDirectory: entry => entry.tree !== null })
    if (!child) break
    segments.push(child); current = child
  }
  return segments
}
export function GitPanel({ workspace, active, onOpenDiff, onOpenHistory, onClose }: { workspace: WorkspaceResponse | null; active: boolean; onOpenHistory: () => void; onOpenDiff: (diff: GitDiff) => void; onClose: () => void }) {
  const dialogs = useDialogs()
  const [compact, setCompact] = useCompactFolders()
  const [status, setStatus] = useState<GitStatus | null>(null)
  // Reuse the change topology when only selection, expansion or presentation changes.
  const trees = useMemo(() => new Map((status?.repositories ?? []).map(repo => [repo.root, {
    changes: treeOf(repo.changes.filter(file => !file.untracked && visibleChange(file))),
    untracked: treeOf(repo.changes.filter(file => file.untracked && visibleChange(file))),
  }])), [status])
  const [selected, setSelected] = useState(new Set<string>())
  const [message, setMessage] = useState('')
  const [messageDialogOpen, setMessageDialogOpen] = useState(false)
  const [notice, setNotice] = useState('')
  const [operationBusy, setBusy] = useState(false)
  const [gitLocked, setGitLocked] = useState(false)
  const busy = operationBusy || gitLocked
  const [loading, setLoading] = useState(false)
  const [preview, setPreview] = useState<GitPreview | null>(null)
  const [pushAfterCommit, setPushAfterCommit] = useState(false)
  const [pendingPush, setPendingPush] = useState<{ repository: string; head: string }[]>([])
  const [menu, setMenu] = useState<{ repo: GitRepository; file: GitChange; x: number; y: number } | null>(null)
  // Show the two change groups initially; repositories and directories open
  // one visible level at a time (a compact chain is one visible row).
  const [collapsed, setCollapsed] = useState(new Set<string>(['changes', 'untracked']))
  const [allCollapsed, setAllCollapsed] = useState(true)
  const [activeFile, setActiveFile] = useState<string | null>(null)
  const current = useRef(status), running = useRef(false), mounted = useRef(true)
  const refreshing = useRef<AbortController | null>(null), refreshedAt = useRef(0)
  const activeRef = useRef(active)
  const diffController = useRef<AbortController | null>(null)
  const revision = workspace?.revision
  useEffect(() => {
    const roots = new Set<string>()
    const lock = (event: Event) => { const { repository, locked } = (event as CustomEvent<{ repository: string; locked: boolean }>).detail; if (locked) roots.add(repository); else roots.delete(repository); setGitLocked(roots.size > 0) }
    window.addEventListener('devflow-git-lock', lock)
    return () => window.removeEventListener('devflow-git-lock', lock)
  }, [])
  activeRef.current = active
  current.current = status
  useEffect(() => { mounted.current = true; return () => { mounted.current = false; diffController.current?.abort(); refreshing.current?.abort(); refreshing.current = null } }, [])
  const refresh = useCallback(async (foreground = false, force = false) => {
    if (revision === undefined) return
    if (refreshing.current && !force) { if (foreground) setLoading(true); return }
    refreshing.current?.abort()
    const controller = new AbortController()
    refreshing.current = controller; setLoading(foreground || !current.current)
    try {
      const next = await gitRequest<GitStatus>('status', { expectedRevision: revision, force: foreground }, AbortSignal.any([controller.signal, AbortSignal.timeout(60000)]))
      if (!mounted.current || controller.signal.aborted) return
      const previous = current.current
      const changed = !previous || JSON.stringify(previous) !== JSON.stringify(next)
      if (changed) {
        setSelected(values => new Set([...values].filter(value => next.repositories.some(repo => previous?.repositories.find(old => old.root === repo.root)?.version === repo.version && repo.changes.some(file => key(repo, file.path) === value)))))
        if (previous && JSON.stringify(previous.repositories.map(repo => [repo.root, repo.version])) !== JSON.stringify(next.repositories.map(repo => [repo.root, repo.version]))) setPreview(null)
        setStatus(next)
      }
      refreshedAt.current = Date.now()
      current.current = next
    } catch (error) { if (mounted.current && !controller.signal.aborted) setNotice(messageOf(error)) }
    finally { if (refreshing.current === controller) { refreshing.current = null; if (mounted.current) setLoading(false) } }
  }, [revision])
  useEffect(() => {
    if (!active || revision === undefined) return
    // Opening a cached panel is a view switch, not a refresh request.
    if (!current.current) void refresh()
    return () => { diffController.current?.abort() }
  }, [active, revision, refresh])
  useEffect(() => {
    if (revision === undefined) return
    // Keep the polling clock independent of sidebar switches and window focus.
    const timer = setInterval(() => {
      if (activeRef.current && !running.current && !document.hidden && Date.now() - refreshedAt.current >= (current.current?.refreshing || current.current?.stale ? 1000 : 5000)) void refresh()
    }, 1000)
    return () => clearInterval(timer)
  }, [revision, refresh])
  useEffect(() => {
    const changed = () => { if (!running.current) void refresh(false, true) }
    window.addEventListener('devflow-files-replaced', changed)
    window.addEventListener('devflow-git-updated', changed)
    return () => { window.removeEventListener('devflow-files-replaced', changed); window.removeEventListener('devflow-git-updated', changed) }
  }, [refresh])
  function toggleCollapsed(id: string) {
    setCollapsed(old => { const next = new Set(old); if (next.has(id)) next.delete(id); else next.add(id); return next })
  }
  // Entries store exceptions to the default, so bulk collapse also applies to
  // directories discovered by later status updates without walking the DOM.
  function isCollapsed(id: string) { return collapsed.has(id) ? !allCollapsed : allCollapsed }
  function setAllCollapsedTo(value: boolean) { setAllCollapsed(value); setCollapsed(new Set()) }
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
    if (running.current || gitLocked) return
    refreshing.current?.abort(); refreshing.current = null
    running.current = true; setBusy(true); setNotice(''); setMenu(null)
    try { await action() } catch (error) { if (mounted.current) { setNotice(messageOf(error)); setPreview(null) } }
    finally { running.current = false; if (mounted.current) { setBusy(false); await refresh(false, true) } }
  }
  function outcome(result: GitOutcome, rollback = false) {
    setPreview(null)
    setNotice(result.results.map(item => `${item.repository.split(/[\\/]/).at(-1)}：${item.error ? item.error.message : item.commit ? `已提交 ${item.commit.slice(0, 8)}${item.pushError ? `；推送失败，本地提交已保留：${item.pushError.message}` : item.pushed ? '，已推送' : ''}` : '完成'}`).join('\n'))
    setPendingPush(old => {
      const next = old.filter(value => !result.results.some(item => item.repository === value.repository && (item.pushed || item.commit)))
      return [...next, ...result.results.filter(item => item.commit && item.pushError).map(item => ({ repository: item.repository, head: item.commit! }))]
    })
    setSelected(old => { const next = new Set(old); result.results.forEach(item => item.paths.forEach(path => next.delete(`${item.repository}\0${path}`))); return next })
    if (result.results.some(item => item.commit)) window.dispatchEvent(new CustomEvent('devflow-git-updated', { detail: { source: 'commit' } }))
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
  function prepare(push: boolean) {
    const items = selections()
    if (!items.length) return
    void execute(async () => {
      guard(items)
      const plan = await gitRequest<GitPreview>('commit/preview', { expectedRevision: revision, selections: items, message })
      if (!mounted.current) return
      setPushAfterCommit(push)
      setPreview(plan)
      const first = plan.repositories[0]?.files[0]; if (first) onOpenDiff(first)
    })
  }
  function applyCommit() {
    if (!preview) return
    void execute(async () => {
      guard(preview.repositories.map(repo => ({ repository: repo.repository, version: '', paths: repo.files.map(file => file.path) })))
      outcome(await gitRequest<GitOutcome>('commit/apply', { expectedRevision: revision, planId: preview.planId, push: pushAfterCommit }))
    })
  }
  function retryPush() {
    const items = [...pendingPush]
    void execute(async () => {
      const results: GitOutcome['results'] = []
      for (const item of items) {
        try {
          const result = await gitRequest<GitOutcome>('push', { expectedRevision: revision, repository: item.repository, expectedHead: item.head })
          results.push(...result.results)
        } catch (error) {
          results.push({ repository: item.repository, paths: [], commit: item.head, error: null, pushError: { code: 'push_failed', message: messageOf(error) } })
        }
      }
      if (mounted.current) outcome({ results })
    })
  }
  async function showDiff(repo: GitRepository, file: GitChange) {
    if (file.kind === 'conflict' && !readOnlyShare) { openGitUpdate(repo.root); return }
    setActiveFile(key(repo, file.path))
    diffController.current?.abort(); const controller = new AbortController(); diffController.current = controller
    try {
      const diff = await gitRequest<GitDiff>('diff', { expectedRevision: revision, repository: repo.root, version: repo.version, path: file.path }, controller.signal)
      if (mounted.current && !controller.signal.aborted) onOpenDiff(diff)
    } catch (error) { if (!controller.signal.aborted && mounted.current) setNotice(messageOf(error)) }
  }
  function renderTree(repo: GitRepository, tree: Tree, group: 'changes' | 'untracked', prefix = '', level = 1): ReactNode {
    return <>{[...tree.folders].map(([name, child]) => {
      const path = prefix ? `${prefix}/${name}` : name
      const segments = folderSegments(name, child, path, compact, repo.truncated)
      const tail = segments[segments.length - 1]
      const ids = segments.map(segment => `${group}:${repo.root}:folder:${segment.path}`)
      const open = !isCollapsed(ids[0])
      // Store every original directory so changing compact mode or splitting a
      // chain on refresh preserves expansion without changing file identities.
      const expand = (nextOpen: boolean) => setCollapsed(old => {
        const next = new Set(old)
        ids.forEach(id => { if ((!nextOpen) === allCollapsed) next.delete(id); else next.add(id) })
        return next
      })
      const available = child.descendants.filter(file => file.selectable)
      const checked = available.filter(file => selected.has(key(repo, file.path))).length
      return <div key={name} role="treeitem" tabIndex={0} aria-level={level} aria-label={segments.map(segment => segment.name).join('/')} aria-expanded={open}
        onKeyDown={event => handleTreeNavigation(event, { expanded: open, expandable: true, onToggle: expand, onActivate: () => expand(!open) })}>
        <FileTreeRow className="git-folder-row" title={tail.path}
          leading={<><TreeChevron expanded={open} label={`${open ? '收起' : '展开'}目录 ${tail.path}`} onToggle={() => expand(!open)} />
            <Checkbox checked={available.length > 0 && checked === available.length} mixed={checked > 0 && checked < available.length} disabled={busy || !available.length || !!repo.blocked || repo.truncated} label={`选择 ${repo.name}/${tail.path} 下所有可选变更`} onChange={() => toggle(repo, child.descendants)} /></>}>
          <button className="git-tree-folder" aria-expanded={open} onClick={() => expand(!open)}><TreeEntryIcon name={tail.name} directory expanded={open} root={false} /><TreePathLabel segments={segments} /><small>{child.descendants.length} files</small></button>
        </FileTreeRow><div className="git-indent" role="group" hidden={!open}>{renderTree(repo, tail.tree!, group, tail.path, level + 1)}</div>
      </div>
    })}{tree.files.map(file => <FileTreeRow className={`git-file ${file.kind === 'deleted' ? 'is-deleted' : ''} ${file.selectable ? '' : 'is-disabled'} ${activeFile === key(repo, file.path) ? 'is-active' : ''}`} key={file.path}
      role="treeitem" tabIndex={0} aria-level={level} aria-selected={activeFile === key(repo, file.path)} aria-label={`${file.path.split('/').at(-1)}${file.kind === 'deleted' ? '（已删除）' : ''}`}
      onKeyDown={event => handleTreeNavigation(event, { expanded: false, expandable: false, onToggle: () => {}, onActivate: () => { void showDiff(repo, file) } })}
      title={`${file.originalPath ? `${file.originalPath} → ` : ''}${file.path}${file.reason ? `\n${file.reason}` : ''}`}
      onContextMenu={event => { event.preventDefault(); setMenu({ repo, file, x: Math.max(0, Math.min(event.clientX, window.innerWidth - 210)), y: Math.max(0, Math.min(event.clientY, window.innerHeight - 145)) }) }}>
      <Checkbox checked={selected.has(key(repo, file.path))} disabled={busy || !file.selectable || !!repo.blocked || repo.truncated} label={`选择 ${repo.name}/${file.path}`} onChange={() => { toggle(repo, [file]); void showDiff(repo, file) }} />
      <button aria-pressed={activeFile === key(repo, file.path)} onClick={() => void showDiff(repo, file)}><TreeEntryIcon name={file.path.split('/').at(-1) ?? file.path} directory={file.submodule} expanded={false} root={file.submodule} /><span>{file.path.split('/').at(-1)}</span><small>{file.submodule ? file.pointerChanged ? '引用更新' : '先提交子仓库' : file.status}</small></button>
    </FileTreeRow>)}</>
  }

  function section(untracked: boolean) {
    const repositories = status?.repositories.filter(repo => repo.changes.some(file => file.untracked === untracked && visibleChange(file))) ?? []
    const count = repositories.reduce((total, repo) => total + repo.changes.filter(file => file.untracked === untracked && visibleChange(file)).length, 0)
    const group = untracked ? 'untracked' : 'changes', open = !isCollapsed(group)
    return <section className="git-section"><h3><button className="git-section-toggle" aria-expanded={open} onClick={() => toggleCollapsed(group)}><ChevronRight size={12} className={open ? 'is-expanded' : ''} /><span>{untracked ? 'Unversioned Files' : 'Changes'}</span><small>{count} files</small></button></h3><div hidden={!open}>{repositories.map(repo => {
      const files = repo.changes.filter(file => file.untracked === untracked && visibleChange(file)), available = files.filter(file => file.selectable)
      const checked = available.filter(file => selected.has(key(repo, file.path))).length
      const repoGroup = `${group}:${repo.root}`, repoOpen = !isCollapsed(repoGroup)
      return <div key={repo.root} className="git-repository"><div className="git-repo-title" title={`${repo.root}${repo.parent ? `\n子仓库，父仓库：${repo.parent}\n先提交本仓库，再到父仓库提交引用更新` : ''}`}>
        <button className="git-repo-chevron" aria-label={`${repoOpen ? '收起' : '展开'} ${repo.name}`} aria-expanded={repoOpen} onClick={() => toggleCollapsed(repoGroup)}><ChevronRight size={12} className={repoOpen ? 'is-expanded' : ''} /></button>
        <Checkbox checked={available.length > 0 && checked === available.length} mixed={checked > 0 && checked < available.length} disabled={busy || !available.length || !!repo.blocked || repo.truncated} label={`选择仓库 ${repo.name} 的${untracked ? '未管理文件' : '变化'}`} onChange={() => toggle(repo, files)} />
        <button className="git-repo-toggle" aria-expanded={repoOpen} onClick={() => toggleCollapsed(repoGroup)}><TreeEntryIcon name={repo.name} directory expanded={repoOpen} root /><strong>{repo.name}</strong><small>{files.length} files</small><span className="git-branch">{repo.branch}</span>{repo.parent && <small className="git-parent-label">↳ {repo.parent.split(/[\\/]/).at(-1)}</small>}</button>{!readOnlyShare && <button className="ws-icon-button" disabled={busy} title="Update this repository" aria-label={`更新仓库 ${repo.name}`} onClick={() => openGitUpdate(repo.root)}><Download size={13} /></button>}
      </div><div hidden={!repoOpen}>{repo.blocked && <p className="git-notice">{repo.blocked}</p>}{repo.truncated && <p className="git-notice">列表不完整，已禁用修改操作</p>}<div className="git-indent" role="tree" aria-label={`${repo.name} ${untracked ? '未管理文件' : '变更文件'}`}>{renderTree(repo, trees.get(repo.root)![group], group)}</div></div></div>
    })}</div></section>
  }
  return <div className="git-panel"><header className="ws-explorer-header"><strong>Commit</strong><div>{!readOnlyShare && <button className="ws-icon-button" title="Update Project" aria-label="Update Project：拉取并合并上游代码" disabled={operationBusy || !workspace} onClick={() => openGitUpdate()}><Download size={14} /></button>}<button className="ws-icon-button" title="紧凑文件夹：合并连续单子目录" aria-label="紧凑文件夹" aria-pressed={compact} onClick={() => setCompact(!compact)}><Settings2 size={14} /></button><button className="ws-icon-button" title="Git History" aria-label="打开 Git 历史" disabled={!workspace} onClick={onOpenHistory}><History size={14} /></button><button className="ws-icon-button" title="全部展开" aria-label="全部展开 Git 分组、仓库和目录" onClick={() => setAllCollapsedTo(false)}><ChevronsUpDown size={14} /></button><button className="ws-icon-button" title="全部折叠" aria-label="全部折叠 Git 分组、仓库和目录" onClick={() => setAllCollapsedTo(true)}><ChevronsDownUp size={14} /></button><button className="ws-icon-button" disabled={busy || loading || !workspace} title="刷新 Git 状态" aria-label="刷新 Git 状态" onClick={() => void refresh(true)}><RefreshCw size={14} className={loading ? 'spinning' : ''} /></button>{!readOnlyShare && <><button className="ws-icon-button" disabled={busy} title="Add to VCS" aria-label="添加所选未管理文件" onClick={() => mutate('add')}><Plus size={15} /></button><button className="ws-icon-button" disabled={busy} title="Rollback" aria-label="回滚所选文件" onClick={() => mutate('rollback')}><Undo2 size={15} /></button></>}<button className="ws-icon-button" onClick={onClose} aria-label="关闭 Git 面板"><X size={14} /></button></div></header>
    <div className="git-scroll" aria-busy={status?.refreshing}>{!workspace ? <p className="git-notice">请先打开工作区。</p> : <>{!readOnlyShare && status?.repositories.filter(repo => repo.changes.some(file => file.kind === 'conflict') || repo.blocked).map(repo => <section className="git-section git-merge-conflicts" key={repo.root}><h3><GitMerge size={13} /> Merge Conflicts · {repo.name}</h3><button className="ws-outline-button" disabled={busy} onClick={() => openGitUpdate(repo.root)}>打开 Merge / 查看进行中的操作</button>{repo.changes.filter(file => file.kind === 'conflict').map(file => <button key={file.path} className="git-conflict-file" disabled={busy} title={file.path} onClick={() => openGitUpdate(repo.root)}>{file.path}</button>)}</section>)}{section(false)}{section(true)}{status && !status.refreshing && status.repositories.length > 0 && !status.repositories.some(repo => repo.changes.some(visibleChange)) && <p className="git-notice">没有文件变化或子模块引用更新。</p>}{status && !status.repositories.length && <p className="git-notice">没有发现 Git 仓库。请将完整仓库根目录加入工作区。</p>}{status?.warnings.map((warning, index) => <p key={index} className="git-notice">{warning}</p>)}</>}</div>
    {status?.refreshing && <p className="git-notice" role="status">正在后台更新变化…</p>}
    {notice && <p className="git-notice" role="status">{notice}</p>}
    {!readOnlyShare && pendingPush.length > 0 && <div className="git-push-retry"><button className="ws-outline-button" disabled={busy} onClick={retryPush}>{busy ? '处理中…' : '重试 Push'}</button><small>仅推送已生成的本地提交</small></div>}
    {!readOnlyShare && <ResizableCommitPanel><div className="git-commit-heading"><span>Commit Message</span><BaseButton disabled={busy} onClick={() => setMessageDialogOpen(true)}>规范填写</BaseButton></div><textarea aria-label="提交信息" placeholder="Commit Message" value={message} disabled={busy} onChange={event => { setMessage(event.target.value); setPreview(null) }} />
      {preview ? <div className="git-preview"><strong>{pushAfterCommit ? '确认提交并推送' : '确认提交'} · {preview.repositories.length} 个仓库</strong><div className="git-preview-files">{preview.repositories.map(repo => <div key={repo.repository}><small title={repo.repository}>{repo.repository.split(/[\\/]/).at(-1)} · {repo.branch}</small>{repo.files.map(diff => <button key={diff.path} title={diff.path} onClick={() => onOpenDiff(diff)}><TreeEntryIcon name={diff.path.split('/').at(-1) ?? diff.path} directory={diff.submodule} expanded={false} root={diff.submodule} />{diff.path}</button>)}</div>)}</div><p>点击文件查看 Diff。各仓库分别生成提交。{pushAfterCommit && '提交后推送到当前分支的上游；将推送该分支上全部尚未推送的历史提交。'}</p><button className="ws-primary-button" disabled={busy} onClick={applyCommit}>{busy ? '处理中…' : pushAfterCommit ? '确认 Commit and Push' : '确认 Commit'}</button><button className="ws-outline-button" disabled={busy} onClick={() => setPreview(null)}>取消</button></div>
        : <div className="git-commit-actions"><button className="ws-primary-button" disabled={busy || !message.trim() || !selected.size} onClick={() => prepare(false)}>{busy ? '处理中…' : `Commit${selected.size ? ` (${selected.size})` : ''}`}</button><button className="ws-outline-button" disabled={busy || !message.trim() || !selected.size} onClick={() => prepare(true)}>Commit and Push…</button></div>}
    </ResizableCommitPanel>}
    {!readOnlyShare && messageDialogOpen && <CommitMessageDialog message={message} onClose={() => setMessageDialogOpen(false)} onApply={value => { setMessage(value); setPreview(null); setMessageDialogOpen(false) }} />}

    {menu && <div className="git-context" role="menu" style={{ left: menu.x, top: menu.y }} onClick={event => event.stopPropagation()}><button role="menuitem" onClick={() => { void showDiff(menu.repo, menu.file); setMenu(null) }}>Show Diff</button>{!readOnlyShare && <><button role="menuitem" disabled={busy || !menu.file.selectable || !menu.file.untracked} onClick={() => mutate('add', [{ repository: menu.repo.root, version: menu.repo.version, paths: [menu.file.path] }])}>Add to VCS</button><button role="menuitem" disabled={busy || !menu.file.selectable || menu.file.untracked || menu.file.submodule} onClick={() => mutate('rollback', [{ repository: menu.repo.root, version: menu.repo.version, paths: [menu.file.path] }])}>Rollback…</button></>}</div>}
  </div>
}
