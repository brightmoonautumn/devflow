import { useCallback, useEffect, useMemo, useRef, useState, type CSSProperties } from 'react'
import { ChevronRight, Download, GitBranch, RefreshCw, Search } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import { TreeEntryIcon } from '../../components/files/TreeEntryIcon'
import { readOnlyShare } from '../../lib/access'
import type { WorkspaceResponse } from '../../lib/api'
import { gitRequest, type GitDiff, type GitHistoryOverview, type GitHistoryCommit, type GitHistoryTarget, type GitRepository, type GitStatus, type GitSync, type GitSyncInfo, type HistoryFile, type OverviewCommit } from './api'
import { HistoryRepositoryFilter, rootColor } from './HistoryRepositoryFilter'
import { historyGraph } from './historyGraph'
import './git-history.css'

type FileTree = { folders: Map<string, FileTree>; files: HistoryFile[] }
type Selection = { repository: string; commit: string }
function fileTree(files: HistoryFile[]) {
  const root: FileTree = { folders: new Map(), files: [] }
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
const messageOf = (error: unknown) => error instanceof Error ? error.message : String(error)
const date = (timestamp: number) => new Date(timestamp * 1000).toLocaleString([], { year: 'numeric', month: '2-digit', day: '2-digit', hour: '2-digit', minute: '2-digit' })
const commitKey = (repository: string, commit: string) => JSON.stringify([repository, commit])
const rootName = (root: string) => root.split(/[\\/]/).at(-1) ?? root
function readScope(key: string): string[] | null {
  try {
    const value: unknown = JSON.parse(sessionStorage.getItem(key) ?? 'null')
    return Array.isArray(value) && value.length <= 64 && value.every(item => typeof item === 'string') ? value : null
  } catch { return null }
}
function syncLabel(info: GitSyncInfo) {
  if (info.ahead !== null && info.behind !== null) {
    if (info.ahead && info.behind) return `↑ ${info.ahead} ↓ ${info.behind} 已分叉`
    if (info.ahead) return `↑ ${info.ahead} 待推送`
    if (info.behind) return `↓ ${info.behind} 待更新`
    return '与已知上游一致'
  }
  return info.reason ?? '上游状态未知'
}
function referenceClass(reference: string) {
  if (reference.startsWith('HEAD')) return 'is-head'
  if (reference.startsWith('refs/remotes/')) return 'is-remote'
  if (reference.startsWith('tag:')) return 'is-tag'
  return 'is-local'
}
function referenceLabel(reference: string) { return reference.replace(/refs\/(heads|remotes|tags)\//g, '') }
function References({ references }: { references: string[] }) {
  return <span className="history-refs">{references.map(reference => <small className={referenceClass(reference)} key={reference} title={`${reference.startsWith('refs/remotes/') ? '本机记录的远程分支：' : ''}${reference}`}>{referenceLabel(reference)}</small>)}</span>
}
function HistoryFiles({ tree, openFile, openSubmodule, prefix = '' }: { tree: FileTree; openFile: (file: HistoryFile) => void; openSubmodule: (file: HistoryFile) => void; prefix?: string }) {
  return <>{[...tree.folders].map(([name, node]) => <details key={name} className="history-folder" open={prefix === '' ? true : undefined}><summary><ChevronRight size={12} /><TreeEntryIcon name={name} directory expanded={false} root={false} /><span>{name}</span></summary><div className="history-indent"><HistoryFiles tree={node} openFile={openFile} openSubmodule={openSubmodule} prefix={`${prefix}/${name}`} /></div></details>)}{tree.files.map(file => <div className="history-file-entry" key={file.path}>
    <button className={`history-file ${file.status === 'D' ? 'is-deleted' : ''}`} disabled={!file.available} title={file.reason ?? file.path} onClick={() => openFile(file)}><TreeEntryIcon name={file.path.split('/').at(-1) ?? file.path} directory={file.submodule} expanded={false} root={file.submodule} /><span>{file.path.split('/').at(-1)}</span><small>{file.status}</small></button>
    {file.linkedRepository && file.linkedCommit && <button className="history-submodule-link" title={`查看子仓库提交 ${file.linkedCommit}`} onClick={() => openSubmodule(file)}>查看子仓库提交 <ChevronRight size={12} /></button>}
  </div>)}</>
}

export default function GitHistoryPage({ workspace, onOpenDiff, target }: { target?: GitHistoryTarget | null; workspace: WorkspaceResponse | null; onOpenDiff: (diff: GitDiff) => void }) {
  const preferenceKey = `devflow-history-roots:${workspace?.workspaceFile ?? workspace?.folders.map(folder => folder.path).sort().join('|') ?? ''}`
  const [repositories, setRepositories] = useState<GitRepository[]>([])
  const [scope, setScope] = useState<string[] | null>(() => target ? [target.repository] : readScope(preferenceKey))
  const [reference, setReference] = useState('all'), [query, setQuery] = useState(''), [search, setSearch] = useState('')
  const [history, setHistory] = useState<GitHistoryOverview | null>(null), [detail, setDetail] = useState<GitHistoryCommit | null>(null)
  const [selected, setSelected] = useState<Selection | null>(null), [parent, setParent] = useState<string | undefined>()
  const [discovering, setDiscovering] = useState(false), [loading, setLoading] = useState(false), [detailLoading, setDetailLoading] = useState(false), [notice, setNotice] = useState('')
  const [fetchNotice, setFetchNotice] = useState('')
  const [sync, setSync] = useState<GitSyncInfo[]>([]), [syncLoading, setSyncLoading] = useState(false), [syncNotice, setSyncNotice] = useState('')
  const [fetching, setFetching] = useState(''), [diffLoading, setDiffLoading] = useState(''), [generation, setGeneration] = useState(0)
  const listRequest = useRef<AbortController | null>(null), diffRequest = useRef<AbortController | null>(null), fetchRequest = useRef<AbortController | null>(null)
  const latestHistory = useRef(history); latestHistory.current = history
  const latestSelected = useRef(selected); latestSelected.current = selected
  const navigation = useRef(target), previousPreference = useRef(preferenceKey)
  const log = useRef<HTMLDivElement>(null)
  const revision = workspace?.revision
  const rootsKey = JSON.stringify(repositories.filter(repo => scope === null || scope.includes(repo.root)).map(repo => repo.root).sort())
  const roots = useMemo(() => JSON.parse(rootsKey) as string[], [rootsKey])
  const syncByRoot = useMemo(() => new Map(sync.map(info => [info.repository, info])), [sync])
  useEffect(() => {
    if (previousPreference.current === preferenceKey) return
    previousPreference.current = preferenceKey; navigation.current = null
    setScope(readScope(preferenceKey)); setReference('all'); setQuery(''); setSearch(''); setSelected(null); setRepositories([])
  }, [preferenceKey])
  useEffect(() => {
    const changed = (event: Event) => {
      if (fetchRequest.current && (event as CustomEvent<{ source?: string }>).detail?.source === 'fetch') return
      setGeneration(value => value + 1)
    }
    window.addEventListener('devflow-git-updated', changed)
    return () => window.removeEventListener('devflow-git-updated', changed)
  }, [])
  useEffect(() => {
    navigation.current = target
    if (!target) return
    diffRequest.current?.abort()
    setScope([target.repository]); setReference('all'); setQuery(''); setSearch('')
    setSelected({ repository: target.repository, commit: target.commit }); setParent(undefined); setNotice('')
  }, [target])
  useEffect(() => {
    if (revision === undefined) return
    const controller = new AbortController()
    setDiscovering(true); setNotice('')
    void gitRequest<GitStatus>('repositories', { expectedRevision: revision, force: generation > 0 }, controller.signal).then(value => {
      if (controller.signal.aborted) return
      setRepositories(value.repositories)
      setScope(old => old === null ? null : old.filter(root => value.repositories.some(repo => repo.root === root)))
      if (value.warnings.length) setNotice(value.warnings.join('\n'))
    }).catch(error => { if (!controller.signal.aborted) setNotice(messageOf(error)) }).finally(() => { if (!controller.signal.aborted) setDiscovering(false) })
    return () => controller.abort()
  }, [revision, generation])
  const load = useCallback(async (more = false) => {
    listRequest.current?.abort()
    if (!roots.length || revision === undefined) { setLoading(false); return }
    const previous = latestHistory.current
    const previousSelected = latestSelected.current
    if (more && !previous?.nextCursor) return
    const controller = new AbortController(); listRequest.current = controller
    setLoading(true); setNotice('')
    try {
      const value = await gitRequest<GitHistoryOverview>('history/overview', { expectedRevision: revision, repositories: roots, reference, query: search, limit: 50,
        cursor: more ? previous!.nextCursor : undefined }, AbortSignal.any([controller.signal, AbortSignal.timeout(180000)]))
      if (controller.signal.aborted) return
      const next = more && previous ? { ...value, commits: [...previous.commits, ...value.commits] } : value
      latestHistory.current = next; setHistory(next)
      if (value.warnings.length) setNotice(value.warnings.join('\n'))
      if (!more) {
        const destination = navigation.current
        setSelected(destination && roots.includes(destination.repository) ? { repository: destination.repository, commit: destination.commit }
          : previousSelected && value.commits.some(commit => commit.repository === previousSelected.repository && commit.id === previousSelected.commit) ? previousSelected
          : value.commits[0] ? { repository: value.commits[0].repository, commit: value.commits[0].id } : null)
        setParent(undefined)
      }
    } catch (error) { if (!controller.signal.aborted) setNotice(messageOf(error)) }
    finally { if (!controller.signal.aborted) setLoading(false) }
  }, [roots, revision, reference, search])
  useEffect(() => {
    setHistory(null); latestHistory.current = null; setDetail(null); setSelected(null); setParent(undefined)
    void load()
    return () => { listRequest.current?.abort(); diffRequest.current?.abort() }
  }, [load, generation])
  useEffect(() => {
    setSync([]); setSyncNotice('')
    if (!roots.length || revision === undefined) { setSyncLoading(false); return }
    const controller = new AbortController(); setSyncLoading(true)
    void gitRequest<GitSync>('sync', { expectedRevision: revision, repositories: roots }, AbortSignal.any([controller.signal, AbortSignal.timeout(180000)])).then(value => {
      if (!controller.signal.aborted) { setSync(value.repositories); if (value.warnings.length) setSyncNotice(value.warnings.join('\n')) }
    }).catch(error => { if (!controller.signal.aborted) setSyncNotice(messageOf(error)) }).finally(() => { if (!controller.signal.aborted) setSyncLoading(false) })
    return () => controller.abort()
  }, [roots, revision, generation])
  const selectedRepository = selected?.repository, selectedCommit = selected?.commit
  useEffect(() => {
    setDetail(null); diffRequest.current?.abort(); setDiffLoading('')
    if (!selectedCommit || !selectedRepository || revision === undefined) { setDetailLoading(false); return }
    const controller = new AbortController(); setDetailLoading(true)
    void gitRequest<GitHistoryCommit>('history/commit', { expectedRevision: revision, repository: selectedRepository, commit: selectedCommit, parent }, controller.signal).then(value => {
      if (!controller.signal.aborted) setDetail(value)
    }).catch(error => { if (!controller.signal.aborted) setNotice(messageOf(error)) }).finally(() => { if (!controller.signal.aborted) setDetailLoading(false) })
    return () => controller.abort()
  }, [selectedCommit, selectedRepository, parent, revision])
  useEffect(() => () => { listRequest.current?.abort(); diffRequest.current?.abort(); fetchRequest.current?.abort() }, [])
  const selectedKey = selected ? commitKey(selected.repository, selected.commit) : ''
  const pinnedCommit = detail && !history?.commits.some(commit => commit.repository === detail.repository && commit.id === detail.commit.id) ? { ...detail.commit, repository: detail.repository } : null
  useEffect(() => {
    const row = Array.from(log.current?.querySelectorAll<HTMLButtonElement>('[data-commit]') ?? []).find(button => button.dataset.commit === selectedKey)
    row?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [selectedKey, detail, history])
  const graph = useMemo(() => historyGraph((history?.commits ?? []).map(commit => ({ ...commit, id: commitKey(commit.repository, commit.id), parents: commit.parents.map(parent => commitKey(commit.repository, parent)) }))), [history])
  const showGraph = graph.every(row => row.width <= 32)
  const graphWidth = Math.max(1, ...graph.map(row => row.width)) * 12 + 8
  const tree = useMemo(() => fileTree(detail?.files ?? []), [detail])
  const fullMessage = detail?.message.trim() ?? ''
  const body = detail && fullMessage.startsWith(detail.commit.subject) ? fullMessage.slice(detail.commit.subject.length).trimStart() : fullMessage
  function chooseRoots(value: string[] | null) {
    navigation.current = null; setScope(value); setReference('all')
    try { sessionStorage.setItem(preferenceKey, JSON.stringify(value)) } catch { /* Optional session preference. */ }
  }
  function jump(repository: string, commit: string) {
    navigation.current = { repository, commit, sequence: Date.now() }
    setScope([repository]); setReference('all'); setQuery(''); setSearch(''); setSelected({ repository, commit }); setParent(undefined)
  }
  async function fetchRemotes() {
    if (fetchRequest.current || revision === undefined || readOnlyShare) return
    const items = sync.filter(info => info.canFetch && roots.includes(info.repository))
    if (!items.length) return
    const controller = new AbortController(); fetchRequest.current = controller
    let completed = 0; const failures: string[] = []
    setFetchNotice(''); setSyncNotice('')
    try {
      for (const [index, item] of items.entries()) {
        if (controller.signal.aborted) return
        setFetching(`${index + 1}/${items.length} · ${rootName(item.repository)}`)
        try {
          const info = await gitRequest<GitSyncInfo>('fetch', { expectedRevision: revision, repository: item.repository, version: item.version }, AbortSignal.any([controller.signal, AbortSignal.timeout(135000)]))
          if (controller.signal.aborted) return
          setSync(old => old.map(value => value.repository === info.repository ? info : value)); completed++
          window.dispatchEvent(new CustomEvent('devflow-git-updated', { detail: { repository: info.repository, source: 'fetch' } }))
        } catch (error) { if (controller.signal.aborted) return; failures.push(`${rootName(item.repository)}：${messageOf(error)}`) }
      }
    } finally {
      fetchRequest.current = null
      if (!controller.signal.aborted) {
        setFetching(''); setFetchNotice(`已获取 ${completed}/${items.length} 个仓库的上游状态，工作区代码未合并。${failures.length ? `\n${failures.join('\n')}` : ''}`)
        setGeneration(value => value + 1)
      }
    }
  }
  async function openFile(file: HistoryFile) {
    if (!detail || revision === undefined) return
    diffRequest.current?.abort(); const controller = new AbortController(); diffRequest.current = controller
    setDiffLoading(file.path); setNotice('')
    try {
      const diff = await gitRequest<GitDiff>('history/diff', { expectedRevision: revision, repository: detail.repository, commit: detail.commit.id, parent: detail.parent, path: file.path }, controller.signal)
      if (!controller.signal.aborted) onOpenDiff(diff)
    } catch (error) { if (!controller.signal.aborted) setNotice(messageOf(error)) }
    finally { if (!controller.signal.aborted) setDiffLoading('') }
  }
  function renderCommit(commit: OverviewCommit, index?: number) {
    const row = index === undefined ? undefined : graph[index], x = (column: number) => 8 + column * 12
    const info = syncByRoot.get(commit.repository), key = commitKey(commit.repository, commit.id)
    const head = commit.refs.some(ref => ref.startsWith('HEAD'))
    return <button className={`history-row ${selectedKey === key ? 'is-selected' : ''} ${head ? 'is-current-head' : ''}`} key={key} data-commit={key} aria-pressed={selectedKey === key}
      style={{ '--history-root-color': rootColor(commit.repository) } as CSSProperties}
      onClick={() => { navigation.current = null; setSelected({ repository: commit.repository, commit: commit.id }); setParent(undefined) }}
      title={`${commit.repository}\n${commit.id}\n${commit.author} <${commit.email}>\n${date(commit.committedTimestamp ?? commit.timestamp)}\n${commit.subject}`}>
      <span className="history-root-name" title={commit.repository}>{rootName(commit.repository)}</span>
      {row && showGraph ? <svg aria-hidden="true" width={graphWidth} height={36} viewBox={`0 0 ${graphWidth} 36`}>
        {row.edges.map((edge, i) => <path key={i} className={`history-graph-color-${edge.color}`} d={`M ${x(edge.from)} ${edge.from === row.column ? 18 : 0} C ${x(edge.from)} 26 ${x(edge.to)} 26 ${x(edge.to)} 36`} />)}
        <path className={`history-graph-color-${row.color}`} d={`M ${x(row.column)} 0 V 18`} /><circle className={`history-graph-color-${row.color}`} cx={x(row.column)} cy={18} r={3} />
      </svg> : <span className="history-root-dot" aria-hidden="true" />}
      <span className="history-subject">{index === undefined && <small>定位提交 · 当前页外 </small>}{commit.subject || '(无提交说明)'}</span>
      <References references={commit.refs} />{head && info && <small className="history-row-sync" title="相对本机已知的上游引用">{syncLabel(info)}</small>}
      <span className="history-author">{commit.author}</span><time>{date(commit.committedTimestamp ?? commit.timestamp)}</time>
    </button>
  }
  const fetchable = sync.filter(info => info.canFetch).length
  return <section className="git-history-page" aria-label="Git History">
    <div className="history-toolbar">
      <GitBranch size={15} aria-hidden="true" />
      <HistoryRepositoryFilter repositories={repositories} scope={scope} disabled={!!fetching} onChange={chooseRoots} />
      <BaseSelect aria-label="历史分支或标签" disabled={!!fetching} value={reference} options={[{ value: 'all', label: '全部分支' }, { value: 'HEAD', label: '当前 HEAD' }, ...(roots.length === 1 ? history?.references ?? [] : []).map(ref => ({ value: ref, label: referenceLabel(ref) }))]} onValueChange={value => { navigation.current = null; setReference(value) }} />
      <form onSubmit={event => { event.preventDefault(); navigation.current = null; setSearch(query.trim()) }}><BaseInput value={query} disabled={!!fetching} onChange={event => setQuery(event.target.value)} placeholder="搜索所选仓库的提交信息…" aria-label="搜索提交信息" /><BaseButton disabled={!!fetching} type="submit" aria-label="搜索历史"><Search size={14} /></BaseButton></form>
      <button className="ws-icon-button" title="刷新本地历史和已知上游状态，不联网" aria-label="刷新本地历史" disabled={discovering || loading || !!fetching} onClick={() => setGeneration(value => value + 1)}><RefreshCw size={14} className={discovering || loading ? 'spinning' : ''} /></button>
      {!readOnlyShare && <BaseButton disabled={!!fetching || syncLoading || !fetchable} title="获取所选仓库当前分支的远端状态（Fetch），不合并工作区代码" onClick={() => void fetchRemotes()}><Download size={14} />{fetching ? '获取中…' : `Fetch${fetchable ? ` (${fetchable})` : ''}`}</BaseButton>}
    </div>
    <details className="history-sync-panel"><summary>上游状态 · {syncLoading ? '读取本地记录中…' : `${sync.length} 个仓库 · ${sync.filter(info => (info.ahead ?? 0) > 0).length} 个待推送 · ${sync.filter(info => (info.behind ?? 0) > 0).length} 个待更新`}<small>基于本机已知引用</small></summary>
      <p className="git-notice">刷新历史不会联网。Fetch 更新远程跟踪引用，更新项目才会合并到本地分支。获取时间仅记录本次会话的成功操作。</p>
      <div className="history-sync-list">{sync.map(info => <div className="history-sync-item" key={info.repository}>
        <button disabled={!!fetching} title={info.repository} onClick={() => chooseRoots([info.repository])}><i style={{ background: rootColor(info.repository) }} />{rootName(info.repository)}</button>
        <span title={`${info.branch} → ${info.upstream ?? '未设置上游'}${info.reason ? `\n${info.reason}` : ''}`}>{syncLabel(info)}</span>
        <small>{info.lastFetchedAt ? `获取于 ${date(info.lastFetchedAt)}` : '获取时间未知'}</small>
      </div>)}</div>
    </details>
    {fetching && <p className="git-notice" role="status">正在获取上游 · {fetching}，不合并工作区代码…</p>}
    {fetchNotice && <p className="git-notice" role="status">{fetchNotice}</p>}
    {syncNotice && <p className="git-notice" role="status">{syncNotice}</p>}
    {notice && <p className="file-notice" role="status">{notice}</p>}
    <div className="history-layout">
      <div ref={log} className="history-log" aria-label="提交记录" aria-busy={loading}>
        <div className="history-log-heading"><span>提交记录 · {roots.length} 个仓库</span><small>{history?.commits.length ?? 0} 条{!showGraph && roots.length > 0 ? ' · 聚焦仓库查看提交图' : ''}</small></div>
        {pinnedCommit && renderCommit(pinnedCommit)}
        {(history?.commits ?? []).map((commit, index) => renderCommit(commit, index))}
        {!history?.commits.length && <p className="git-notice">{discovering || loading ? '正在读取历史…' : !repositories.length ? '没有发现 Git 仓库。' : !roots.length ? '请选择至少一个仓库。' : '没有匹配的提交。'}</p>}
        {history?.nextCursor && <BaseButton className="history-more" disabled={loading || !!fetching} onClick={() => void load(true)}>{loading ? '读取中…' : '加载更多'}</BaseButton>}
      </div>
      <aside className="history-inspector" aria-label="提交详情" aria-busy={detailLoading}>
        {detail ? <>
          <div className="history-files"><div className="history-files-heading"><strong>变更文件 · {detail.files.length}</strong>{diffLoading && <small role="status">正在打开 Diff…</small>}</div>
            <div className="history-detail-root" title={detail.repository}><span>{rootName(detail.repository)}</span><BaseButton disabled={!!fetching} onClick={() => chooseRoots([detail.repository])}>仅看此仓库</BaseButton></div>
            {detail.commit.parents.length > 1 && <BaseSelect aria-label="合并提交对比父节点" value={detail.parent ?? ''} options={detail.commit.parents.map((oid, i) => ({ value: oid, label: `父提交 ${i + 1} · ${oid.slice(0, 8)}` }))} onValueChange={setParent} />}
            <HistoryFiles key={`${detail.repository}:${detail.commit.id}:${detail.parent ?? 'root'}`} tree={tree} openFile={file => void openFile(file)} openSubmodule={file => { if (file.linkedRepository && file.linkedCommit) jump(file.linkedRepository, file.linkedCommit) }} />
            {detail.truncated && <p className="git-notice">文件超过 2000 项，仅展示前 2000 项。</p>}
            {!detail.files.length && <p className="git-notice">相对此父提交没有文件变化。</p>}
          </div>
          <div className="history-message"><h2>{detail.commit.subject || '(无提交说明)'}</h2>{body && <pre>{body}</pre>}<p><code title={detail.commit.id}>{detail.commit.id.slice(0, 12)}</code> · {detail.commit.author} &lt;{detail.commit.email}&gt;</p><p>{date(detail.commit.committedTimestamp ?? detail.commit.timestamp)}</p><div className="history-detail-refs"><References references={detail.commit.refs} /></div><p className="history-parent">{detail.parent ? `对比父提交 ${detail.parent.slice(0, 12)}` : '根提交 · 对比空树'}</p></div>
        </> : <p className="git-notice">{detailLoading ? '正在读取提交详情…' : '选择提交以查看变更文件和说明。'}</p>}
      </aside>
    </div>
  </section>
}
