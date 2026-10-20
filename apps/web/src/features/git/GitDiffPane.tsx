import { lazy, Suspense, useEffect, useRef, useState } from 'react'
import { readOnlyShare } from '../../lib/access'
import { readWorkspaceFile, writeWorkspaceFile } from '../../lib/api'
import type { DiffSnapshot } from '../../components/editor/GitDiffEditor'
import type { GitDiff } from './api'
import './git.css'
const Editor = lazy(() => import('../../components/editor/GitDiffEditor'))
export default function GitDiffPane({ diff, revision, onDiffChange }: { diff: GitDiff; revision?: number; onDiffChange?: (diff: GitDiff) => void }) {
  const [reverting, setReverting] = useState(false), [notice, setNotice] = useState('')
  const pending = useRef(false), mounted = useRef(true), latest = useRef(diff)
  latest.current = diff
  useEffect(() => { mounted.current = true; return () => { mounted.current = false } }, [])
  async function revert(content: string, expected: DiffSnapshot) {
    if (pending.current || readOnlyShare || revision === undefined || diff.submodule || diff.historical) return
    if (expected.path !== diff.path || expected.before !== diff.before || expected.after !== diff.after) {
      setNotice('Diff 已更新，请在当前视图重新选择要还原的差异块。'); return
    }
    const snapshot = diff
    const path = `${snapshot.repository.replace(/[\\/]$/, '')}/${snapshot.path}`
    const guard = () => {
      if (!window.dispatchEvent(new CustomEvent('devflow-before-replace', { detail: [path], cancelable: true }))) throw new Error('文件存在未保存内容或正在保存，请完成保存后再还原。')
    }
    pending.current = true; setReverting(true); setNotice('')
    try {
      guard()
      const file = await readWorkspaceFile(path, revision)
      if (!mounted.current || latest.current !== snapshot) return
      if (file.readonly) throw new Error(file.readonlyReason ?? '文件为只读，无法还原。')
      if (file.content.replace(/\r\n?/g, '\n') !== snapshot.after.replace(/\r\n?/g, '\n')) throw new Error('磁盘文件已变化，请从 Git 列表重新打开最新 Diff 后还原。')
      guard()
      const saved = await writeWorkspaceFile(path, revision, file.content, content)
      if (mounted.current && latest.current === snapshot) {
        onDiffChange?.({ ...snapshot, after: saved.content.replace(/\r\n/g, '\n') })
        setNotice('已还原此处改动并保存。')
      }
      window.dispatchEvent(new CustomEvent('devflow-files-replaced', { detail: [path] }))
    } catch (error) { if (mounted.current) setNotice(error instanceof Error ? error.message : String(error)) }
    finally { pending.current = false; if (mounted.current) setReverting(false) }
  }
  return <div className="git-diff-pane"><header className="git-diff-heading"><span title={`${diff.repository}/${diff.path}`}>{diff.originalPath ? `${diff.originalPath} → ` : ''}{diff.path}</span><small>{diff.historical ? `历史 · ${diff.commit?.slice(0, 8)}` : 'Diff · 工作区快照'}</small></header>
    {(notice || reverting) && <p className="file-notice" role="status">{reverting ? '正在还原并保存…' : notice}</p>}
    {diff.message && <p className="file-notice">{diff.message}</p>}
    {diff.submodule ? <div className="git-pointer-diff" aria-label="子模块提交引用对比">
      <div><h3>{diff.historical ? `父提交 ${diff.parent?.slice(0, 8) ?? '空树'}` : '父仓库原引用'}</h3><pre className="is-before"><span aria-hidden="true">1</span><code>{diff.before.replace(/^Subproject commit /, '').trimEnd()}</code></pre></div>
      <div><h3>{diff.historical ? `所选提交 ${diff.commit?.slice(0, 8)}` : '子仓库当前 HEAD'}</h3><pre className="is-after"><span aria-hidden="true">1</span><code>{diff.after.replace(/^Subproject commit /, '').trimEnd()}</code></pre></div>
    </div> : (diff.before || diff.after || !diff.message) && <Suspense fallback={<div className="file-loading">正在加载 Diff…</div>}><Editor path={diff.path} before={diff.before} after={diff.after} beforeLabel={diff.historical ? `父提交 ${diff.parent?.slice(0, 8) ?? '空树'}` : undefined} afterLabel={diff.historical ? `所选提交 ${diff.commit?.slice(0, 8)}` : undefined} reverting={reverting} onRevert={!diff.historical && !readOnlyShare && revision !== undefined && onDiffChange && !diff.message ? revert : undefined} /></Suspense>}
  </div>
}
