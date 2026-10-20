import { useCallback, useEffect, useMemo, useRef, useState } from 'react'
import { ChevronRight, GitBranch, RefreshCw, Search } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import { TreeEntryIcon } from '../../components/files/TreeEntryIcon'
import type { WorkspaceResponse } from '../../lib/api'
import { gitRequest, type GitDiff, type GitHistory, type GitHistoryCommit, type GitHistoryTarget, type GitRepository, type GitStatus, type HistoryFile } from './api'
import { historyGraph } from './historyGraph'
import './git-history.css'

type FileTree = { folders: Map<string, FileTree>; files: HistoryFile[] }
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
function HistoryFiles({ tree, openFile, prefix = '' }: { tree: FileTree; openFile: (file: HistoryFile) => void; prefix?: string }) {
  return <>{[...tree.folders].map(([name, node]) => <details key={name} className="history-folder" open={prefix === '' ? true : undefined}><summary><ChevronRight size={12} /><TreeEntryIcon name={name} directory expanded={false} root={false} /><span>{name}</span></summary><div className="history-indent"><HistoryFiles tree={node} openFile={openFile} prefix={`${prefix}/${name}`} /></div></details>)}{tree.files.map(file => <button className="history-file" key={file.path} disabled={!file.available} title={file.reason ?? file.path} onClick={() => openFile(file)}><TreeEntryIcon name={file.path.split('/').at(-1) ?? file.path} directory={file.submodule} expanded={false} root={file.submodule} /><span>{file.path.split('/').at(-1)}</span><small>{file.status}</small></button>)}</>
}

export default function GitHistoryPage({ workspace, onOpenDiff, target }: { target?: GitHistoryTarget | null; workspace: WorkspaceResponse | null; onOpenDiff: (diff: GitDiff) => void }) {
  const [repositories, setRepositories] = useState<GitRepository[]>([])
  const [repository, setRepository] = useState('')
  const [reference, setReference] = useState('all'), [query, setQuery] = useState(''), [search, setSearch] = useState('')
  const [history, setHistory] = useState<GitHistory | null>(null), [detail, setDetail] = useState<GitHistoryCommit | null>(null)
  const [selected, setSelected] = useState(''), [parent, setParent] = useState<string | undefined>()
  const [discovering, setDiscovering] = useState(false), [loading, setLoading] = useState(false), [detailLoading, setDetailLoading] = useState(false), [notice, setNotice] = useState('')
  const [diffLoading, setDiffLoading] = useState('')
  const [generation, setGeneration] = useState(0)
  const listRequest = useRef<AbortController | null>(null), diffRequest = useRef<AbortController | null>(null)
  const latestHistory = useRef(history); latestHistory.current = history
  const navigation = useRef(target)
  const log = useRef<HTMLDivElement>(null)
  const revision = workspace?.revision
  useEffect(() => {
    navigation.current = target
    if (!target) return
    diffRequest.current?.abort()
    setRepository(target.repository); setReference('all'); setQuery(''); setSearch('')
    setSelected(target.commit); setParent(undefined); setNotice('')
  }, [target])
  useEffect(() => {
    if (revision === undefined) return
    const controller = new AbortController()
    setDiscovering(true); setNotice('')
    void gitRequest<GitStatus>('repositories', { expectedRevision: revision, force: generation > 0 }, controller.signal).then(value => {
      if (controller.signal.aborted) return
      setRepositories(value.repositories)
      setRepository(old => value.repositories.some(repo => repo.root === old) ? old : value.repositories[0]?.root ?? '')
      if (value.warnings.length) setNotice(value.warnings.join('\n'))
    }).catch(error => { if (!controller.signal.aborted) setNotice(messageOf(error)) }).finally(() => { if (!controller.signal.aborted) setDiscovering(false) })
    return () => controller.abort()
  }, [revision, generation])
  const load = useCallback(async (more = false) => {
    if (!repository || revision === undefined) return
    listRequest.current?.abort()
    const controller = new AbortController(); listRequest.current = controller
    const previous = latestHistory.current
    if (more && (!previous || previous.nextSkip === null)) return
    setLoading(true); setNotice('')
    try {
      const value = await gitRequest<GitHistory>('history', { expectedRevision: revision, repository, reference, query: search, limit: 50,
        skip: more ? previous!.nextSkip : 0, version: more ? previous!.version : undefined }, controller.signal)
      if (controller.signal.aborted) return
      const next = more && previous ? { ...value, commits: [...previous.commits, ...value.commits] } : value
      latestHistory.current = next; setHistory(next)
      if (!more) { setSelected(navigation.current?.repository === repository ? navigation.current.commit : value.commits[0]?.id ?? ''); setParent(undefined) }
    } catch (error) { if (!controller.signal.aborted) setNotice(messageOf(error)) }
    finally { if (!controller.signal.aborted) setLoading(false) }
  }, [repository, revision, reference, search])
  useEffect(() => {
    setHistory(null); latestHistory.current = null; setDetail(null); setSelected(''); setParent(undefined)
    void load()
    return () => { listRequest.current?.abort(); diffRequest.current?.abort() }
  }, [load, generation])
  useEffect(() => {
    setDetail(null); diffRequest.current?.abort(); setDiffLoading('')
    if (!selected || !repository || revision === undefined) { setDetailLoading(false); return }
    const controller = new AbortController(); setDetailLoading(true)
    void gitRequest<GitHistoryCommit>('history/commit', { expectedRevision: revision, repository, commit: selected, parent }, controller.signal).then(value => {
      if (!controller.signal.aborted) setDetail(value)
    }).catch(error => { if (!controller.signal.aborted) setNotice(messageOf(error)) }).finally(() => { if (!controller.signal.aborted) setDetailLoading(false) })
    return () => controller.abort()
  }, [selected, parent, repository, revision])
  useEffect(() => () => { listRequest.current?.abort(); diffRequest.current?.abort() }, [])
  // A blamed commit can be older than this page. Keep its selected row separate
  // so the paginated graph retains its real topological ordering.
  const pinnedCommit = detail && !history?.commits.some(commit => commit.id === detail.commit.id) ? detail.commit : null
  useEffect(() => {
    const row = Array.from(log.current?.querySelectorAll<HTMLButtonElement>('[data-commit]') ?? []).find(button => button.dataset.commit === selected)
    row?.scrollIntoView({ block: 'nearest', inline: 'nearest' })
  }, [selected, detail, history])
  const graph = useMemo(() => historyGraph(history?.commits ?? []), [history])
  const graphWidth = Math.min(32, Math.max(1, ...graph.map(row => row.width))) * 12 + 8
  const tree = useMemo(() => fileTree(detail?.files ?? []), [detail])
  const fullMessage = detail?.message.trim() ?? ''
  const body = detail && fullMessage.startsWith(detail.commit.subject) ? fullMessage.slice(detail.commit.subject.length).trimStart() : fullMessage
  async function openFile(file: HistoryFile) {
    if (!detail || revision === undefined) return
    diffRequest.current?.abort(); const controller = new AbortController(); diffRequest.current = controller
    setDiffLoading(file.path); setNotice('')
    try {
      const diff = await gitRequest<GitDiff>('history/diff', { expectedRevision: revision, repository, commit: detail.commit.id, parent: detail.parent, path: file.path }, controller.signal)
      if (!controller.signal.aborted) onOpenDiff(diff)
    } catch (error) { if (!controller.signal.aborted) setNotice(messageOf(error)) }
    finally { if (!controller.signal.aborted) setDiffLoading('') }
  }
  return <section className="git-history-page" aria-label="Git History">
    <div className="history-toolbar">
      <GitBranch size={15} aria-hidden="true" />
      <BaseSelect aria-label="历史仓库" value={repository} options={repositories.map(repo => ({ value: repo.root, label: `${repo.name}${repo.parent ? ' · 子仓库' : ''} · ${repo.root}` }))} onValueChange={value => { navigation.current = null; setRepository(value); setReference('all') }} placeholder="选择仓库" />
      <BaseSelect aria-label="历史分支或标签" value={reference} options={[{ value: 'all', label: '全部分支' }, { value: 'HEAD', label: '当前 HEAD' }, ...(history?.references ?? []).map(ref => ({ value: ref, label: ref.replace(/^refs\/(heads|remotes|tags)\//, '') }))]} onValueChange={value => { navigation.current = null; setReference(value) }} />
      <form onSubmit={event => { event.preventDefault(); navigation.current = null; setSearch(query.trim()) }}><BaseInput value={query} onChange={event => setQuery(event.target.value)} placeholder="搜索提交信息…" aria-label="搜索提交信息" /><BaseButton type="submit" aria-label="搜索历史"><Search size={14} /></BaseButton></form>
      <button className="ws-icon-button" title="刷新历史" aria-label="刷新历史" disabled={discovering || loading || detailLoading} onClick={() => setGeneration(value => value + 1)}><RefreshCw size={14} className={discovering || loading ? 'spinning' : ''} /></button>
    </div>
    {notice && <p className="file-notice" role="status">{notice}</p>}
    <div className="history-layout">
      <div ref={log} className="history-log" aria-label="提交记录" aria-busy={loading}>
        <div className="history-log-heading"><span>提交记录</span><small>{history?.commits.length ?? 0} 条</small></div>
        {pinnedCommit && <button className="history-row is-selected" data-commit={pinnedCommit.id} aria-pressed="true" title={pinnedCommit.id} onClick={() => { setSelected(pinnedCommit.id); setParent(undefined) }}>
          <span className="history-refs"><small>定位提交 · 当前页外</small></span><span className="history-subject">{pinnedCommit.subject || '(无提交说明)'}</span><span className="history-author">{pinnedCommit.author}</span><time>{date(pinnedCommit.timestamp)}</time>
        </button>}
        {(history?.commits ?? []).map((commit, index) => {
          const row = graph[index], x = (column: number) => 8 + Math.min(column, 31) * 12
          return <button className={`history-row ${selected === commit.id ? 'is-selected' : ''}`} key={commit.id} data-commit={commit.id} aria-pressed={selected === commit.id} onClick={() => { navigation.current = null; setSelected(commit.id); setParent(undefined) }} title={`${commit.id}\n${commit.author} <${commit.email}>\n${date(commit.timestamp)}\n${commit.subject}`}>
            <svg aria-hidden="true" width={graphWidth} height={36} viewBox={`0 0 ${graphWidth} 36`}>
              {row.edges.map((edge, i) => <path key={i} className={`history-graph-color-${edge.color}`} d={`M ${x(edge.from)} ${edge.from === row.column ? 18 : 0} C ${x(edge.from)} 26 ${x(edge.to)} 26 ${x(edge.to)} 36`} />)}
              <path className={`history-graph-color-${row.color}`} d={`M ${x(row.column)} 0 V 18`} /><circle className={`history-graph-color-${row.color}`} cx={x(row.column)} cy={18} r={3} />
            </svg>
            <span className="history-subject">{commit.subject || '(无提交说明)'}</span><span className="history-refs">{commit.refs.map(ref => <small key={ref}>{ref}</small>)}</span><span className="history-author">{commit.author}</span><time>{date(commit.timestamp)}</time>
          </button>
        })}
        {!history?.commits.length && <p className="git-notice">{discovering || loading ? '正在读取历史…' : !repositories.length ? '没有发现 Git 仓库。' : '没有匹配的提交。'}</p>}
        {history?.nextSkip !== null && history?.nextSkip !== undefined && <BaseButton className="history-more" disabled={loading} onClick={() => void load(true)}>{loading ? '读取中…' : '加载更多'}</BaseButton>}
      </div>
      <aside className="history-inspector" aria-label="提交详情" aria-busy={detailLoading}>
        {detail ? <>
          <div className="history-files"><div className="history-files-heading"><strong>变更文件 · {detail.files.length}</strong>{diffLoading && <small role="status">正在打开 Diff…</small>}</div>
            {detail.commit.parents.length > 1 && <BaseSelect aria-label="合并提交对比父节点" value={detail.parent ?? ''} options={detail.commit.parents.map((oid, i) => ({ value: oid, label: `父提交 ${i + 1} · ${oid.slice(0, 8)}` }))} onValueChange={setParent} />}
            <HistoryFiles key={`${detail.commit.id}:${detail.parent ?? "root"}`} tree={tree} openFile={file => void openFile(file)} />
            {detail.truncated && <p className="git-notice">文件超过 2000 项，仅展示前 2000 项。</p>}
            {!detail.files.length && <p className="git-notice">相对此父提交没有文件变化。</p>}
          </div>
          <div className="history-message"><h2>{detail.commit.subject || '(无提交说明)'}</h2>{body && <pre>{body}</pre>}<p><code title={detail.commit.id}>{detail.commit.id.slice(0, 12)}</code> · {detail.commit.author} &lt;{detail.commit.email}&gt;</p><p>{date(detail.commit.timestamp)}</p><div className="history-detail-refs">{detail.commit.refs.map(ref => <span key={ref}><GitBranch size={12} />{ref}</span>)}</div><p className="history-parent">{detail.parent ? `对比父提交 ${detail.parent.slice(0, 12)}` : '根提交 · 对比空树'}</p></div>
        </> : <p className="git-notice">{detailLoading ? '正在读取提交详情…' : '选择提交以查看变更文件和说明。'}</p>}
      </aside>
    </div>
  </section>
}
