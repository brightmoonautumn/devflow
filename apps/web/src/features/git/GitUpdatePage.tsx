import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ArrowDown, ArrowUp, Download, Expand, GitMerge, RefreshCw } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseTextarea } from '../../components/form/BaseTextarea'
import { useDialogs } from '../../components/form/BaseDialog'
import ThreeWayMergeEditor from '../../components/editor/ThreeWayMergeEditor'
import { gitRequest, notifyGitUpdated, type GitConflictContent, type GitMergeState, type GitRepository, type GitStatus, type GitUpdateInfo, type GitUpdateProgress } from './api'
import { hasMergeMarkers, mergeBlocks } from './mergeBlocks'
import './git-update.css'

const messageOf = (error: unknown) => error instanceof Error ? error.message : String(error)
const repositoryGuard = (repository: string) => {
  if (!window.dispatchEvent(new CustomEvent('devflow-before-git-update', { detail: repository, cancelable: true }))) throw new Error('仓库中有未保存、读取或保存中的文件，请完成后重试。')
}
const lockRepository = (repository: string, locked: boolean) => window.dispatchEvent(new CustomEvent('devflow-git-lock', { detail: { repository, locked } }))

export default function GitUpdatePage({ revision, target, active }: { revision: number; target?: { repository?: string; sequence: number }; active: boolean }) {
  const dialogs = useDialogs()
  const [repositories, setRepositories] = useState<GitRepository[]>([])
  const [info, setInfo] = useState<GitUpdateInfo[]>([])
  const [selected, setSelected] = useState(new Set<string>())
  const [repository, setRepository] = useState(target?.repository ?? '')
  const [job, setJob] = useState<GitUpdateProgress | null>(null)
  const [results, setResults] = useState<Record<string, string>>({})
  const [merge, setMerge] = useState<GitMergeState | null>(null)
  const [file, setFile] = useState<GitConflictContent | null>(null)
  const [draft, setDraft] = useState(''), [savedDraft, setSavedDraft] = useState('')
  const [commitMessage, setCommitMessage] = useState('')
  const [index, setIndex] = useState(0), [baseVisible, setBaseVisible] = useState(false)
  const [busy, setBusy] = useState(false), [loading, setLoading] = useState(false), [notice, setNotice] = useState('')
  const [expanded, setExpanded] = useState(false)
  const mounted = useRef(true), running = useRef(false)
  const latest = useRef({ file, draft, savedDraft, repository, busy }); latest.current = { file, draft, savedDraft, repository, busy }
  const loadSequence = useRef(0), fileSequence = useRef(0)
  const blocks = useMemo(() => mergeBlocks(draft), [draft])
  const currentBlock = blocks[Math.min(index, Math.max(0, blocks.length - 1))] ?? null
  const dirty = draft !== savedDraft
  const request = useCallback(<T,>(action: string, body: object) => gitRequest<T>(action, { expectedRevision: revision, ...body }, AbortSignal.timeout(action === 'merge/finish' || action === 'merge/abort' ? 135000 : 60000)), [revision])

  const mergeSequence = useRef(0)
  const loadMerge = useCallback(async (root: string) => {
    const sequence = ++mergeSequence.current
    const value = await request<GitMergeState>('merge/status', { repository: root })
    if (mounted.current && sequence === mergeSequence.current && latest.current.repository === root) {
      setMerge(value); setCommitMessage(value.message.replace(/^#.*\n?/gm, '').trim())
    }
    return value
  }, [request])
  const load = useCallback(async () => {
    if (running.current) return
    const sequence = ++loadSequence.current
    setLoading(true); setNotice('')
    try {
      const catalog = await request<GitStatus>('repositories', {})
      const values: GitUpdateInfo[] = []
      for (const repo of catalog.repositories) values.push(await request<GitUpdateInfo>('update/check', { repository: repo.root }))
      if (!mounted.current || sequence !== loadSequence.current) return
      setRepositories(catalog.repositories); setInfo(values)
      setSelected(old => old.size ? new Set([...old].filter(root => values.some(v => v.repository === root && !v.reason))) : new Set(values.filter(v => !v.reason).map(v => v.repository)))
      setRepository(old => catalog.repositories.some(repo => repo.root === old) ? old : catalog.repositories[0]?.root ?? '')
      if (catalog.warnings.length) setNotice(catalog.warnings.join('\n'))
    } catch (error) { if (mounted.current && sequence === loadSequence.current) setNotice(messageOf(error)) }
    finally { if (mounted.current && sequence === loadSequence.current) setLoading(false) }
  }, [request])

  const followJob = useCallback(async (initial: GitUpdateProgress) => {
    let value = initial
    lockRepository(value.repository, true)
    try {
      while (!value.done) {
        if (!mounted.current) return value
        setJob(value)
        await new Promise(resolve => setTimeout(resolve, 900))
        if (!mounted.current) return value
        const next = await request<GitUpdateProgress | null>('update/progress', { jobId: value.jobId })
        if (!next) throw new Error('更新任务不存在，请刷新仓库状态确认结果。')
        value = next
      }
      if (mounted.current) { setJob(value); setResults(old => ({ ...old, [value.repository]: value.message })) }
      return value
    } finally {
      // A closed/reloaded UI does not cancel Git. The core task remains queryable.
      if (value.done) { lockRepository(value.repository, false); notifyGitUpdated(value.repository) }
    }
  }, [request])

  useEffect(() => {
    mounted.current = true
    void (async () => {
      try {
        const pending = await request<GitUpdateProgress | null>('update/progress', {})
        if (!mounted.current) return
        if (pending && !pending.done) {
          running.current = true; setBusy(true); setRepository(pending.repository)
          try { await followJob(pending) } finally { running.current = false; if (mounted.current) setBusy(false) }
        }
        if (mounted.current) await load()
      } catch (error) { if (mounted.current) setNotice(messageOf(error)) }
    })()
    return () => { mounted.current = false; loadSequence.current++; fileSequence.current++ }
  }, [request, followJob, load])
  useEffect(() => {
    if (target?.repository && !latest.current.busy && !latest.current.file) setRepository(target.repository)
  }, [target])
  useEffect(() => {
    if (!repository || busy) return
    let cancelled = false
    void loadMerge(repository).catch(error => { if (!cancelled) setNotice(messageOf(error)) })
    return () => { cancelled = true }
  }, [repository, busy, loadMerge])
  useEffect(() => {
    const unload = (event: BeforeUnloadEvent) => { if (latest.current.draft !== latest.current.savedDraft || latest.current.busy) event.preventDefault() }
    const close = (event: Event) => { if (latest.current.draft !== latest.current.savedDraft || latest.current.busy) event.preventDefault() }
    window.addEventListener('beforeunload', unload); window.addEventListener('devflow-git-update-before-close', close)
    return () => { window.removeEventListener('beforeunload', unload); window.removeEventListener('devflow-git-update-before-close', close) }
  }, [])
  // Escape exits only the expanded view. It never aborts the Git merge.
  useEffect(() => {
    if (!expanded) return
    const key = (event: KeyboardEvent) => { if (event.key === 'Escape') setExpanded(false) }
    window.addEventListener('keydown', key); return () => window.removeEventListener('keydown', key)
  }, [expanded])

  async function openFile(path: string) {
    if (running.current) return
    if (dirty && !await dialogs.confirm({ title: '放弃未保存的合并编辑？', description: '当前结果还没有保存并标记已解决。', confirmText: '放弃编辑' })) return
    const sequence = ++fileSequence.current
    setLoading(true); setNotice('')
    try {
      const value = await request<GitConflictContent>('merge/file', { repository, path })
      if (!mounted.current || sequence !== fileSequence.current || latest.current.repository !== repository) return
      setFile(value); setDraft(value.result ?? ''); setSavedDraft(value.result ?? ''); setIndex(0); setBaseVisible(false)
    } catch (error) { if (mounted.current && sequence === fileSequence.current) setNotice(messageOf(error)) }
    finally { if (mounted.current && sequence === fileSequence.current) setLoading(false) }
  }
  async function update() {
    if (running.current || loading) return
    const entries = info.filter(value => selected.has(value.repository) && !value.reason)
    if (!entries.length) return
    if (!await dialogs.confirm({ title: 'Update Project', description: `${entries.map(value => `${value.repository}\n${value.branch} ← ${value.upstream}`).join('\n\n')}\n\n获取并合并上游代码；分支分叉时可能生成合并提交。遇到冲突或失败暂停剩余仓库。`, confirmText: '开始更新' })) return
    if (!mounted.current || running.current) return
    running.current = true; setBusy(true); setNotice(''); setResults({}); setFile(null)
    try {
      for (const entry of entries) {
        if (!mounted.current) return
        repositoryGuard(entry.repository)
        lockRepository(entry.repository, true)
        try {
          const started = await request<GitUpdateProgress>('update/start', { repository: entry.repository, version: entry.version })
          const completed = await followJob(started)
          if (!mounted.current) return
          if (completed.phase !== 'completed') {
            setRepository(entry.repository)
            setNotice('已暂停后续仓库。处理当前结果后，刷新并重新选择需要更新的仓库。')
            break
          }
        } catch (error) {
          // The process may still be running after a transport error: preserve the lock until a progress query succeeds.
          const pending = await request<GitUpdateProgress | null>('update/progress', {})
          if (pending && !pending.done) {
            setJob(pending)
            const completed = await followJob(pending)
            setRepository(completed.repository)
          } else { lockRepository(entry.repository, false); notifyGitUpdated(entry.repository) }
          throw error
        }
      }
    } catch (error) { if (mounted.current) setNotice(messageOf(error)) }
    finally { running.current = false; if (mounted.current) { setBusy(false); await load() } }
  }
  async function resolve(choice: 'edit' | 'local' | 'incoming' | 'delete') {
    if (!file || running.current) return
    if (choice !== 'edit' && !await dialogs.confirm({ title: choice === 'delete' ? '接受删除？' : '采用整个文件版本？', description: `${file.path}\n${choice === 'local' ? '采用本地版本' : choice === 'incoming' ? '采用远端版本' : '删除此文件'}，并标记已解决。该侧不存在文件时将接受删除。`, confirmText: '确认并标记已解决' })) return
    repositoryGuard(repository)
    running.current = true; setBusy(true); setNotice('')
    try {
      const value = await request<GitMergeState>('merge/resolve', { repository, path: file.path, version: file.version, choice, ...(choice === 'edit' ? { content: draft } : {}) })
      if (!mounted.current) return
      setMerge(value); setFile(null); setDraft(''); setSavedDraft(''); notifyGitUpdated(repository, [file.path])
      setNotice(`已解决 ${file.path}。${value.files.length ? '请选择下一文件。' : '所有冲突已解决，可以完成合并。'}`)
    } catch (error) { if (mounted.current) setNotice(`${messageOf(error)}；请重新读取冲突文件后再保存。`) }
    finally { running.current = false; if (mounted.current) setBusy(false) }
  }
  async function complete(abort: boolean) {
    if (!merge || running.current || dirty) return
    if (!await dialogs.confirm({ title: abort ? '中止合并？' : '完成合并？', description: abort ? '本次合并和已保存的冲突解决结果将被撤销。合并期间额外的未暂存修改会阻止操作。' : `将整个合并暂存区提交为合并提交。\n\n${commitMessage}`, confirmText: abort ? '中止合并' : '创建合并提交', ...(abort ? { tone: 'danger' as const } : {}) })) return
    repositoryGuard(repository)
    running.current = true; setBusy(true); setNotice(''); lockRepository(repository, true)
    try {
      const value = await request<GitMergeState>(abort ? 'merge/abort' : 'merge/finish', { repository, version: merge.version, ...(abort ? {} : { message: commitMessage }) })
      if (mounted.current) { setMerge(value); setFile(null); setDraft(''); setSavedDraft(''); setNotice(abort ? '已中止合并。' : '合并完成。') }
    } catch (error) { if (mounted.current) setNotice(messageOf(error)) }
    finally { lockRepository(repository, false); notifyGitUpdated(repository); running.current = false; if (mounted.current) { setBusy(false); await load() } }
  }
  function accept(side: 'local' | 'incoming' | 'both' | 'incoming-first') {
    if (!currentBlock) return
    const content = side === 'local' ? currentBlock.local : side === 'incoming' ? currentBlock.incoming : side === 'both' ? currentBlock.local + currentBlock.incoming : currentBlock.incoming + currentBlock.local
    setDraft(draft.slice(0, currentBlock.from) + content + draft.slice(currentBlock.to)); setIndex(Math.max(0, Math.min(index, blocks.length - 2)))
  }
  return <div className={`git-update-page ${expanded && active ? 'is-expanded' : ''}`}>
    <header className="git-update-toolbar"><strong><GitMerge size={15} />Update Project</strong><BaseSelect aria-label="当前仓库" value={repository} disabled={busy || dirty || loading} options={repositories.map(repo => ({ value: repo.root, label: `${repo.name} · ${repo.branch}` }))} onValueChange={value => { setFile(null); setDraft(''); setSavedDraft(''); setRepository(value); setMerge(null) }} /><button className="ws-icon-button" title="刷新更新状态" aria-label="刷新更新状态" disabled={busy || loading || dirty} onClick={() => { setFile(null); void load().then(async () => { if (repository) await loadMerge(repository) }).catch(error => setNotice(messageOf(error))) }}><RefreshCw size={14} className={loading ? 'spinning' : ''} /></button><button className="ws-icon-button" title={expanded ? '退出展开' : '展开合并视图'} aria-label={expanded ? '退出展开' : '展开合并视图'} aria-pressed={expanded} onClick={() => setExpanded(!expanded)}><Expand size={14} /></button></header>
    {job && <p className="git-update-status" role="status">{job.done ? '' : '● '}{job.repository.split(/[\\/]/).at(-1)} · {job.message}</p>}
    {notice && <p className="git-update-status" role="status">{notice}</p>}
    {merge?.active ? <>
      <div className="merge-session"><strong>Merge · {merge.branch}</strong><span>{merge.files.length ? `${merge.files.length} 个文件待解决` : '所有冲突已解决'}</span><BaseButton disabled={busy || loading || dirty} onClick={() => void complete(true).catch(error => setNotice(messageOf(error)))}>中止合并</BaseButton></div>
      <div className="merge-workspace"><nav className="merge-files" aria-label="冲突文件">{merge.files.map(item => <button key={item.path} disabled={busy || loading} className={file?.path === item.path ? 'is-active' : ''} onClick={() => void openFile(item.path)} title={item.path}><span>{item.path}</span><small>{!item.supported ? '外部处理' : item.localDeleted || item.incomingDeleted ? '修改/删除' : '冲突'}</small></button>)}{!merge.files.length && <p>没有未解决文件</p>}</nav>
        <main className="merge-main">{file ? <>
          <div className="merge-file-heading"><strong title={file.path}>{file.path}</strong><BaseButton disabled={busy || loading} onClick={() => void openFile(file.path)}>重新读取</BaseButton></div>
          {file.reason ? <p className="git-update-status">{file.reason}</p> : <>
            <div className="merge-block-controls"><button className="ws-icon-button" aria-label="上一处冲突" disabled={!blocks.length || busy} onClick={() => setIndex((index - 1 + blocks.length) % blocks.length)}><ArrowUp size={14} /></button><button className="ws-icon-button" aria-label="下一处冲突" disabled={!blocks.length || busy} onClick={() => setIndex((index + 1) % blocks.length)}><ArrowDown size={14} /></button><span>{blocks.length ? `${Math.min(index + 1, blocks.length)}/${blocks.length} 处冲突` : hasMergeMarkers(draft) ? '请清理剩余冲突标记' : '冲突块已处理'}</span><BaseButton disabled={!currentBlock || busy} onClick={() => accept('local')}>采用本地块</BaseButton><BaseButton disabled={!currentBlock || busy} onClick={() => accept('incoming')}>采用远端块</BaseButton><BaseButton disabled={!currentBlock || busy} onClick={() => accept('both')}>本地 → 远端</BaseButton><BaseButton disabled={!currentBlock || busy} onClick={() => accept('incoming-first')}>远端 → 本地</BaseButton><BaseButton onClick={() => setBaseVisible(!baseVisible)}>{baseVisible ? '隐藏 Base' : '共同祖先'}</BaseButton></div>
            {baseVisible && <pre className="merge-base" aria-label="共同祖先">{file.base || '（共同祖先为空）'}</pre>}
            <ThreeWayMergeEditor key={`${repository}:${file.path}:${file.version}`} path={file.path} local={file.local ?? ''} incoming={file.incoming ?? ''} value={draft} ranges={blocks} selected={currentBlock} disabled={busy} onChange={setDraft} onSave={() => { if (!hasMergeMarkers(latest.current.draft)) void resolve('edit').catch(error => setNotice(messageOf(error))) }} />
          </>}
          <footer className="merge-file-actions"><BaseButton disabled={busy || loading || !merge.files.find(f => f.path === file.path)?.supported} onClick={() => void resolve('local').catch(error => setNotice(messageOf(error)))}>{file.localDeleted ? '本地：接受删除' : '采用整个本地文件'}</BaseButton><BaseButton disabled={busy || loading || !merge.files.find(f => f.path === file.path)?.supported} onClick={() => void resolve('incoming').catch(error => setNotice(messageOf(error)))}>{file.incomingDeleted ? '远端：接受删除' : '采用整个远端文件'}</BaseButton>{!file.reason && <BaseButton variant="primary" disabled={busy || loading || hasMergeMarkers(draft)} onClick={() => void resolve('edit').catch(error => setNotice(messageOf(error)))}>保存并标记已解决{dirty ? ' *' : ''}</BaseButton>}</footer>
        </> : <p className="git-update-status">{merge.files.length ? '选择冲突文件，比较双方修改并编辑合并结果。' : '检查合并提交信息，然后完成合并。'}</p>}</main></div>
      {!merge.files.length && <footer className="merge-finish"><label htmlFor="merge-commit-message">合并提交信息</label><BaseTextarea id="merge-commit-message" rows={3} value={commitMessage} disabled={busy} onChange={event => setCommitMessage(event.target.value)} /><BaseButton variant="primary" disabled={busy || !commitMessage.trim()} onClick={() => void complete(false).catch(error => setNotice(messageOf(error)))}>完成合并</BaseButton></footer>}
    </> : <div className="git-update-list"><p>获取并合并当前分支的上游代码。工作区和暂存区需干净；遇到冲突暂停后续仓库。</p>{info.map(value => <label key={value.repository} className="git-update-repository"><input type="checkbox" checked={selected.has(value.repository)} disabled={busy || loading || !!value.reason} onChange={event => setSelected(old => { const next = new Set(old); if (event.target.checked) next.add(value.repository); else next.delete(value.repository); return next })} /><span><strong>{repositories.find(repo => repo.root === value.repository)?.name ?? value.repository}</strong><small>{value.branch} ← {value.upstream ?? '未配置上游'}</small><small title={value.repository}>{value.repository}</small>{value.reason && <em>{value.reason}</em>}{results[value.repository] && <small>{results[value.repository]}</small>}</span></label>)}{!loading && !repositories.length && <p>没有发现 Git 仓库。</p>}<BaseButton variant="primary" disabled={busy || loading || !info.some(value => selected.has(value.repository) && !value.reason)} onClick={() => void update()}><Download size={14} />{busy ? '更新中…' : 'Update Project'}</BaseButton></div>}
  </div>
}
