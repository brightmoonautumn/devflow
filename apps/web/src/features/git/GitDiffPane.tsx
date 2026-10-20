import { lazy, Suspense } from 'react'
import type { GitDiff } from './api'
import './git.css'
const Editor = lazy(() => import('../../components/editor/GitDiffEditor'))
export default function GitDiffPane({ diff }: { diff: GitDiff }) {
  return <div className="git-diff-pane"><header className="file-toolbar"><span className="file-path" title={diff.repository}>{diff.originalPath ? `${diff.originalPath} → ` : ''}{diff.path}</span><span>只读 Diff · 快照</span></header><div className="git-diff-labels"><span>{diff.submodule ? '父仓库原指针' : 'HEAD'}</span><span>{diff.submodule ? '子仓库当前 HEAD' : '工作区'}</span></div>{diff.message && <p className="file-notice">{diff.message}</p>}{(diff.before || diff.after || !diff.message) && <Suspense fallback={<p>正在加载 Diff…</p>}><Editor path={diff.path} before={diff.before} after={diff.after} /></Suspense>}</div>
}
