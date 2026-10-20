import { useEffect, useMemo, useRef, useState } from 'react'
import FileEditor from '../../components/editor/FileEditor'
import type { ComponentProps } from 'react'
import type { LineAnnotation } from '../../components/editor/SourceEditor'
import { gitRequest, type GitBlame, type GitHistoryTarget } from './api'

function contentVersion(value: string) {
  let hash = 0xcbf29ce484222325n
  for (const byte of new TextEncoder().encode(value)) hash = BigInt.asUintN(64, (hash ^ BigInt(byte)) * 0x100000001b3n)
  return hash.toString(16).padStart(16, '0')
}
function authorDate(timestamp: number, timezone: string) {
  // Keep the author's calendar date, independent of the viewer's timezone.
  const sign = timezone[0] === '-' ? -1 : 1
  const minutes = sign * (Number(timezone.slice(1, 3)) * 60 + Number(timezone.slice(3, 5)))
  const date = new Date((timestamp + minutes * 60) * 1000)
  return Number.isNaN(date.getTime()) ? '' : date.toISOString().slice(0, 10).replaceAll('-', '/')
}
export default function GitFileEditor({ revision, active, onOpenHistory, ...props }: ComponentProps<typeof FileEditor> & {
  revision?: number; active: boolean; onOpenHistory: (target: GitHistoryTarget) => void
}) {
  const [result, setResult] = useState<{ value: string; data: GitBlame } | null>(null)
  const [loading, setLoading] = useState(false), [notice, setNotice] = useState('')
  const pending = useRef<AbortController | null>(null)
  const latest = useRef({ value: props.value, revision, path: props.path, active })
  latest.current = { value: props.value, revision, path: props.path, active }
  // No background refresh or cross-file cache. Leaving the tab releases attribution.
  useEffect(() => {
    pending.current?.abort(); pending.current = null
    setResult(null); setLoading(false); setNotice('')
    return () => { pending.current?.abort() }
  }, [props.value, props.path, revision, active])
  async function toggle() {
    pending.current?.abort(); pending.current = null
    if (result || loading) { setResult(null); setLoading(false); setNotice(''); return }
    if (revision === undefined || !active) return
    const snapshot = latest.current
    const controller = new AbortController(); pending.current = controller
    setLoading(true); setNotice('')
    try {
      const data = await gitRequest<GitBlame>('blame', { path: snapshot.path, content: snapshot.value, expectedRevision: revision }, controller.signal)
      if (controller.signal.aborted) return
      if (data.contentVersion !== contentVersion(snapshot.value)) throw new Error('磁盘文件已改变，请重新读取文件后开启行提交信息。')
      if (!controller.signal.aborted && latest.current.value === snapshot.value && latest.current.revision === revision && latest.current.active) setResult({ value: snapshot.value, data })
    } catch (error) {
      if (!controller.signal.aborted) setNotice(error instanceof Error ? error.message : String(error))
    } finally { if (!controller.signal.aborted) { pending.current = null; setLoading(false) } }
  }
  const annotations = useMemo<LineAnnotation[] | undefined>(() => {
    if (!result || result.value !== props.value || !active) return undefined
    const commits = new Map(result.data.commits.map(commit => [commit.id, commit]))
    return result.data.ranges.map(range => {
      const commit = range.commit ? commits.get(range.commit) : undefined
      return { from: range.from, to: range.to, id: commit?.id, label: commit ? `${authorDate(commit.timestamp, commit.timezone)} ${commit.author}` : '未提交' }
    })
  }, [result, props.value, active])
  return <>
    {(loading || notice) && <p className="file-notice" role="status">{loading ? '正在读取行提交信息…' : notice}</p>}
    <FileEditor {...props} annotations={annotations} annotationLoading={loading} onToggleAnnotations={active ? () => void toggle() : undefined}
      onSelectAnnotation={commit => { if (result) onOpenHistory({ repository: result.data.repository, commit, sequence: Date.now() }) }} />
  </>
}
