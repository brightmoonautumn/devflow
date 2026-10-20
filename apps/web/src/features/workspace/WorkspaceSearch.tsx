import { readOnlyShare } from '../../lib/access'
import { useEffect, useMemo, useRef, useState, type ReactNode } from 'react'
import { ChevronDown, ChevronRight, ChevronsDownUp, FileText, ListX, LoaderCircle, RefreshCw, Replace, ReplaceAll, Search, SlidersHorizontal, X } from 'lucide-react'
import { applyWorkspaceReplace, previewWorkspaceReplace, searchWorkspace, type ReplacePreview, type SearchMatch, type WorkspaceResponse, type WorkspaceSearch as SearchResponse } from '../../lib/api'
import './workspace-search.css'

const matchKey = (match: SearchMatch) => `${match.path}:${match.line ?? 0}`

function MatchPreview({ match }: { match: SearchMatch }) {
  const text = match.preview ?? ''
  const parts: ReactNode[] = []
  let cursor = 0
  for (const { start, end } of match.previewMatches ?? []) {
    if (start < cursor || end <= start || end > text.length) continue
    parts.push(text.slice(cursor, start), <mark key={`${start}:${end}`}>{text.slice(start, end)}</mark>)
    cursor = end
  }
  parts.push(text.slice(cursor))
  return <code>{parts}</code>
}

export function WorkspaceSearch({ workspace, active, onOpenFile, onClose }: {
  workspace: WorkspaceResponse | null; active: boolean; onClose: () => void
  onOpenFile: (path: string, pinned?: boolean, line?: number, column?: number) => void
}) {
  const [showReplace, setShowReplace] = useState(false)
  const [replacement, setReplacement] = useState('')
  const [preserveCase, setPreserveCase] = useState(false)
  const [replacePreview, setReplacePreview] = useState<ReplacePreview | null>(null)
  const [busy, setBusy] = useState(false)
  const [notice, setNotice] = useState('')
  const mounted = useRef(true)
  const [query, setQuery] = useState('')
  const [caseSensitive, setCaseSensitive] = useState(false)
  const [wholeWord, setWholeWord] = useState(false)
  const [regex, setRegex] = useState(false)
  const [showFilters, setShowFilters] = useState(true)
  const [includeGlob, setIncludeGlob] = useState('')
  const [excludeGlob, setExcludeGlob] = useState('')
  const [response, setResponse] = useState<SearchResponse | null>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(false)
  const [cancelled, setCancelled] = useState(false)
  const [attempt, setAttempt] = useState(0)
  const [collapsed, setCollapsed] = useState(new Set<string>())
  const [selected, setSelected] = useState<string | null>(null)
  const input = useRef<HTMLInputElement>(null)
  const pending = useRef<AbortController | null>(null)
  const revision = workspace?.revision
  const available = !!workspace?.folders.some(folder => folder.status === 'available')
  const groups = useMemo(() => {
    const groups = new Map<string, SearchMatch[]>()
    for (const match of response?.matches ?? []) {
      const items = groups.get(match.path) ?? []
      items.push(match); groups.set(match.path, items)
    }
    return [...groups.entries()]
  }, [response])
  const contentCount = response?.matches.filter(match => match.line !== null).length ?? 0
  const filenameCount = response?.matches.filter(match => match.line === null).length ?? 0
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])

  useEffect(() => { if (active) input.current?.focus() }, [active])
  useEffect(() => {
    const focus = () => {
      input.current?.focus(); input.current?.select()
    }
    window.addEventListener('devflow-search', focus)
    return () => window.removeEventListener('devflow-search', focus)
  }, [])
  useEffect(() => {
    setResponse(null); setError(''); setLoading(false); setCancelled(false); setSelected(null); setReplacePreview(null)
    if (!query.trim() || revision === undefined || !available) return
    const controller = new AbortController()
    pending.current = controller
    setLoading(true)
    const timer = window.setTimeout(() => {
      if (controller.signal.aborted) return
      void searchWorkspace(query, 'both', revision, { caseSensitive, wholeWord, regex, includeGlob, excludeGlob }, controller.signal)
        .then(result => { if (!controller.signal.aborted) { setResponse(result); setCollapsed(new Set()) } })
        .catch(err => { if (!controller.signal.aborted) setError(err instanceof Error ? err.message : String(err)) })
        .finally(() => { if (!controller.signal.aborted) setLoading(false) })
    }, 350)
    return () => { window.clearTimeout(timer); controller.abort(); if (pending.current === controller) pending.current = null }
  }, [query, revision, available, caseSensitive, wholeWord, regex, includeGlob, excludeGlob, attempt])
  useEffect(() => { setReplacePreview(null) }, [replacement, preserveCase])

  function clear() { pending.current?.abort(); setQuery(''); setResponse(null); setError(''); input.current?.focus() }
  function open(match: SearchMatch, pinned = false) {
    setSelected(matchKey(match))
    onOpenFile(match.path, pinned, match.line ?? undefined, match.column ?? undefined)
  }
  function toggle(path: string) { setCollapsed(value => { const next = new Set(value); if (next.has(path)) next.delete(path); else next.add(path); return next }) }
  const canSearch = available && !!query.trim() && !busy
  const canReplace = !readOnlyShare && canSearch && !loading && contentCount > 0
  function checkEditors(paths: string[]) {
    if (!window.dispatchEvent(new CustomEvent('devflow-before-replace', { detail: paths, cancelable: true }))) {
      throw new Error('目标文件有未保存修改，或正在读取/保存，请先处理编辑器中的文件再替换。')
    }
  }
  async function commit(plan: ReplacePreview) {
    checkEditors(plan.files.map(file => file.path))
    const outcome = await applyWorkspaceReplace(plan.planId, plan.revision)
    if (outcome.files.length) window.dispatchEvent(new CustomEvent('devflow-files-replaced', { detail: outcome.files }))
    if (!mounted.current) return
    setReplacePreview(null)
    setNotice(`已替换 ${outcome.replacements} 处，修改 ${outcome.files.length} 个文件`)
    setAttempt(value => value + 1)
    if (outcome.failure) setNotice(`已修改 ${outcome.files.length} 个文件，其余未执行：${outcome.failure.message}`)
  }
  async function prepareReplace(path?: string, match?: SearchMatch) {
    if (!canReplace || revision === undefined || (!path && response?.truncated)) return
    setBusy(true); setError(''); setNotice(''); setReplacePreview(null)
    try {
      checkEditors(path ? [path] : groups.filter(([, matches]) => matches.some(match => match.line !== null)).map(([path]) => path))
      const plan = await previewWorkspaceReplace({ query, expectedRevision: revision, caseSensitive, wholeWord, regex, includeGlob, excludeGlob }, replacement, preserveCase, path, match?.line ?? undefined, match?.column ?? undefined)
      if (!mounted.current) return
      if (path) await commit(plan)
      else setReplacePreview(plan)
    } catch (err) { if (mounted.current) setError(err instanceof Error ? err.message : String(err)) }
    finally { if (mounted.current) setBusy(false) }
  }
  async function confirmReplace() {
    if (!replacePreview || busy) return
    setBusy(true); setError('')
    try { await commit(replacePreview) }
    catch (err) { if (mounted.current) setError(err instanceof Error ? err.message : String(err)) }
    finally { if (mounted.current) setBusy(false) }
  }

  return <section className="ws-search" aria-label="工作区文件搜索">
    <header className="ws-search-heading"><h2>搜索</h2><div>
      <button className="ws-icon-button" title="包含与排除文件" aria-label="显示搜索范围" aria-expanded={showFilters} onClick={() => setShowFilters(value => !value)}><SlidersHorizontal size={13} /></button>
      <button className="ws-icon-button" disabled={!canSearch} title="重新搜索" aria-label="重新搜索" onClick={() => setAttempt(value => value + 1)}><RefreshCw size={14} /></button>
      <button className="ws-icon-button" disabled={!groups.length} title="折叠所有结果" aria-label="折叠所有搜索结果" onClick={() => setCollapsed(new Set(groups.map(([path]) => path)))}><ChevronsDownUp size={14} /></button>
      <button className="ws-icon-button" disabled={!query || busy} title="清空结果" aria-label="清空搜索结果" onClick={clear}><ListX size={14} /></button>
      <button className="ws-icon-button" title="收起搜索" aria-label="收起搜索" onClick={onClose}><X size={14} /></button>
    </div></header>
    <form className="ws-search-form" onSubmit={event => { event.preventDefault(); if (!busy) setAttempt(value => value + 1) }}><fieldset disabled={busy}>
      <div className="ws-search-query-row">{!readOnlyShare && <button type="button" className="ws-replace-toggle" title="展开替换" aria-label="展开替换" aria-expanded={showReplace} onClick={() => setShowReplace(value => !value)}>{showReplace ? <ChevronDown size={14} /> : <ChevronRight size={14} />}</button>}
      <div className="ws-search-input"><input ref={input} value={query} disabled={!available} aria-label="搜索文件名与内容" placeholder="搜索" onChange={event => setQuery(event.target.value)} onKeyDown={event => { if (event.key === 'Escape' && loading) { event.stopPropagation(); pending.current?.abort(); setLoading(false); setCancelled(true) } }} />
        <div className="ws-search-options" role="group" aria-label="搜索选项">
          <button type="button" aria-label="区分大小写" aria-pressed={caseSensitive} onClick={() => setCaseSensitive(value => !value)} title="区分大小写">Aa</button>
          <button className="ws-search-word" type="button" aria-label="全字匹配" title="全字匹配" aria-pressed={wholeWord} onClick={() => setWholeWord(value => !value)}>ab</button><button type="button" aria-label="使用正则表达式" title="使用正则表达式" aria-pressed={regex} onClick={() => setRegex(value => !value)}>.*</button>
        </div>
      </div></div>
      {showReplace && <div className="ws-search-replace-row"><div className="ws-search-input"><input value={replacement} disabled={!available} placeholder="替换" aria-label="替换为" onChange={event => setReplacement(event.target.value)} /><div className="ws-search-options"><button type="button" aria-label="保留大小写" title="保留大小写" aria-pressed={preserveCase} onClick={() => setPreserveCase(value => !value)}>AB</button></div></div><button type="button" className="ws-icon-button" title="预览全部替换" aria-label="预览全部替换" disabled={!canReplace || response?.truncated} onClick={() => void prepareReplace()}><ReplaceAll size={16} /></button></div>}
      {showFilters && <div className="ws-search-filters">
        <label>包含文件<input value={includeGlob} disabled={!available} placeholder="例如 **/*.java" title="一个 glob；留空搜索全部未忽略文件" onChange={event => setIncludeGlob(event.target.value)} /></label>
        <label>排除文件<input value={excludeGlob} disabled={!available} placeholder="例如 **/test/**" title="一个 glob；依赖和构建目录始终排除" onChange={event => setExcludeGlob(event.target.value)} /></label>
      </div>}
    </fieldset></form>
    <div className="ws-search-summary" role="status">{busy ? <><LoaderCircle size={12} className="spinning" />正在准备或执行替换…</> : loading ? <><LoaderCircle size={12} className="spinning" /><span>搜索中…</span><button className="ws-icon-button" aria-label="停止搜索" title="停止搜索 · Esc" onClick={() => { pending.current?.abort(); setLoading(false); setCancelled(true) }}><X size={13} /></button></> : cancelled ? '搜索已停止' : response ? `${contentCount}${response.truncated ? '+' : ''} 个匹配行，${filenameCount} 个名称匹配 · ${groups.length} 个文件` : available ? '搜索工作区文件名与内容' : '打开工作区后即可搜索'}</div>
    <div className="ws-search-results" onKeyDown={event => {
      if (!['ArrowDown', 'ArrowUp', 'Home', 'End'].includes(event.key)) return
      const buttons = Array.from(event.currentTarget.querySelectorAll<HTMLButtonElement>('button'))
      const index = buttons.indexOf(event.target as HTMLButtonElement)
      if (index < 0) return
      event.preventDefault()
      const next = event.key === 'Home' ? 0 : event.key === 'End' ? buttons.length - 1 : Math.max(0, Math.min(buttons.length - 1, index + (event.key === 'ArrowDown' ? 1 : -1)))
      buttons[next]?.focus()
    }}>
      {error && <p className="ws-search-error" role="alert">{error}</p>}
      {notice && <p className="ws-search-warning" role="status">{notice}</p>}
      {replacePreview && <section className="ws-replace-confirm" aria-label="确认全部替换">
        <p>将在 {replacePreview.files.length} 个文件中替换 {replacePreview.replacements} 处。</p>
        {replacePreview.files.map(file => <div className="ws-replace-diff" key={file.path}><small title={file.path}>{file.path} · {file.replacements} 处</small><del>{file.before || '空行'}</del><ins>{file.after || '空行'}</ins></div>)}
        <div><button className="ws-outline-button" disabled={busy} onClick={() => setReplacePreview(null)}>取消</button><button className="ws-outline-button" disabled={busy} onClick={() => void confirmReplace()}><ReplaceAll size={14} />确认全部替换</button></div>
      </section>}
      {response?.warnings.map((warning, index) => <p className="ws-search-warning" key={index}>{warning}</p>)}
      {response && !response.matches.length && <p className="ws-search-empty">没有匹配项。可以调整搜索词或文件范围。</p>}
      {!query && !error && <div className="ws-search-empty"><Search size={24} strokeWidth={1.3} /><p>在整个工作区中搜索</p><small>搜索文件名与内容<br />Ctrl+Shift+F / Ctrl+P</small></div>}
      <ul aria-label="按文件分组的搜索结果">{groups.map(([path, matches]) => {
        const first = matches[0], expanded = !collapsed.has(path)
        const content = matches.filter(match => match.line !== null)
        const nameMatched = matches.some(match => match.line === null)
        const segments = first.relativePath.split(/[\\/]/)
        const name = segments.pop()
        const parent = segments.join('/')
        return <li className="ws-search-file" key={path}>
          <div className="ws-search-result-row"><button className={`ws-search-file-heading ${!content.length && selected === matchKey(first) ? 'is-selected' : ''}`} title={`${path}${nameMatched ? '\n文件名匹配' : ''}`} aria-expanded={content.length ? expanded : undefined} onClick={() => content.length ? toggle(path) : open(first)} onDoubleClick={() => open(first, true)} onKeyDown={event => {
            if (!content.length || !['ArrowLeft', 'ArrowRight'].includes(event.key)) return
            event.preventDefault(); if ((event.key === 'ArrowRight') !== expanded) toggle(path)
          }}>
            {content.length ? expanded ? <ChevronDown size={13} /> : <ChevronRight size={13} /> : <span className="ws-search-chevron-space" />}
            <FileText size={14} className="ws-search-file-icon" /><span className="ws-search-file-name">{name}</span>
            <small className="ws-search-file-path">{workspace && workspace.folders.length > 1 ? `${first.root.split(/[\\/]/).pop()}/${parent}` : parent}</small>
            {content.length > 0 ? <span className="ws-search-count">{content.length}</span> : <small className="ws-search-name-match">名称</small>}
          </button>
          {showReplace && content.length > 0 && <button className="ws-result-replace" disabled={!canReplace} title="替换此文件内全部匹配" aria-label={`替换 ${name} 内全部匹配`} onClick={() => void prepareReplace(path)}><ReplaceAll size={13} /></button>}</div>
          {content.length > 0 && expanded && <ul>{content.map(match => <li className="ws-search-result-row" key={matchKey(match)}><button className={`ws-search-match ${selected === matchKey(match) ? 'is-selected' : ''}`} aria-pressed={selected === matchKey(match)} title={`${path}:${match.line}:${match.column}\n${match.preview}`} onClick={() => open(match)} onDoubleClick={() => open(match, true)}><span className="ws-search-line">{match.line}</span><MatchPreview match={match} /></button>{showReplace && <button className="ws-result-replace" disabled={!canReplace} title="替换此行首个匹配" aria-label={`替换第 ${match.line} 行首个匹配`} onClick={() => void prepareReplace(path, match)}><Replace size={13} /></button>}</li>)}</ul>}
        </li>
      })}</ul>
    </div>
    <footer className="ws-search-footer">基于磁盘内容 · 遵循忽略规则</footer>
  </section>
}
