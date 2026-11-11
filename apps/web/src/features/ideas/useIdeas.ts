import { useCallback, useEffect, useRef, useState } from 'react'
import { getIdeas, saveIdeas } from './api'
import { ApiError } from '../../lib/api'
import type { IdeaCanvas, IdeasOperation, IdeasSnapshot } from './model'

export function useIdeas(workspaceKey: string, revision: number) {
  const [data, setData] = useState<IdeasSnapshot | null>(null)
  const [draft, setDraft] = useState<IdeaCanvas | null>(null)
  const [active, setActive] = useState(() => new URLSearchParams(window.location.hash.split('?')[1] ?? '').get('canvas') ?? '')
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [conflict, setConflict] = useState(false)
  const [changed, setChanged] = useState<string[]>([])
  const [history, setHistory] = useState<{ past: IdeaCanvas[]; future: IdeaCanvas[] }>({ past: [], future: [] })
  const dataRef = useRef(data), draftRef = useRef(draft), busyRef = useRef(false), blocked = useRef(false)
  const activeRef = useRef(active), interaction = useRef(false), controller = useRef<AbortController | null>(null), alive = useRef(true)
  dataRef.current = data; draftRef.current = draft; activeRef.current = active
  const canvas = draft ?? data?.canvases.find(c => c.id === active) ?? null
  const canvasRef = useRef(canvas); canvasRef.current = canvas

  const applyRemote = useCallback((next: IdeasSnapshot, external: boolean) => {
    const before = dataRef.current?.canvases.find(c => c.id === activeRef.current)
    const after = next.canvases.find(c => c.id === activeRef.current)
    if (external && before && after && JSON.stringify(before) !== JSON.stringify(after)) {
      setChanged(after.nodes.filter(n => JSON.stringify(n) !== JSON.stringify(before.nodes.find(v => v.id === n.id))).map(n => n.id))
      setHistory({ past: [], future: [] })
    }
    dataRef.current = next; setData(next)
    if (!next.canvases.some(c => c.id === activeRef.current)) { activeRef.current = next.canvases[0]?.id ?? ''; setActive(activeRef.current) }
  }, [])

  useEffect(() => {
    alive.current = true
    const poll = async () => {
      if (document.hidden || draftRef.current || busyRef.current || blocked.current || interaction.current) return
      controller.current?.abort()
      const abort = new AbortController(); controller.current = abort
      try {
        const next = await getIdeas(workspaceKey, revision, abort.signal, dataRef.current?.revision)
        if (alive.current && !abort.signal.aborted && !draftRef.current && !busyRef.current && !interaction.current && next) { applyRemote(next, true); setError('') }
      } catch (e) { if (alive.current && !abort.signal.aborted) setError(String(e)) }
    }
    void poll()
    const timer = setInterval(() => void poll(), 4000)
    return () => { alive.current = false; clearInterval(timer); controller.current?.abort() }
  }, [workspaceKey, revision, applyRemote])

  const persist = useCallback(async (operation: IdeasOperation, saving: IdeaCanvas | null = null, expectedIdeasRevision?: number) => {
    if (!dataRef.current || busyRef.current || blocked.current) return false
    controller.current?.abort(); busyRef.current = true; setBusy(true); setError('')
    try {
      const next = await saveIdeas(workspaceKey, revision, expectedIdeasRevision ?? dataRef.current.revision, operation)
      if (!alive.current) return false
      applyRemote(next, false)
      if (saving && draftRef.current === saving) { draftRef.current = null; setDraft(null) }
      return true
    } catch (e) {
      if (alive.current) {
        setError(String(e)); blocked.current = true
        setConflict(e instanceof ApiError && ['conflict', 'outside_workspace', 'outside_roots'].includes(e.code))
      }
      return false
    } finally { busyRef.current = false; if (alive.current) setBusy(false) }
  }, [workspaceKey, revision, applyRemote])

  useEffect(() => {
    if (!draft || busy || conflict || !draft.name.trim() || draft.nodes.some(n => !n.title.trim())) return
    const timer = setTimeout(() => void persist({ type: 'canvas_save', canvas: draft }, draft), 850)
    return () => clearTimeout(timer)
  }, [draft, busy, conflict, persist])

  function edit(next: IdeaCanvas, record = true) {
    if (conflict) return
    blocked.current = false
    setError('')
    controller.current?.abort()
    const current = canvasRef.current
    if (record && current) setHistory(h => ({ past: [...h.past.slice(-19), current], future: [] }))
    canvasRef.current = next; draftRef.current = next; setDraft(next)
  }
  function select(id: string, nodeId?: string) {
    if (draftRef.current || busyRef.current || blocked.current) return
    activeRef.current = id; setActive(id); setChanged([]); setHistory({ past: [], future: [] })
    const url = new URL(window.location.href); url.hash = `ideas?canvas=${encodeURIComponent(id)}${nodeId ? `&node=${encodeURIComponent(nodeId)}` : ''}`
    window.history.replaceState(window.history.state, '', url)
  }
  function undo(redo = false) {
    if (busyRef.current || blocked.current || !canvasRef.current) return
    const list = redo ? history.future : history.past
    const next = list.at(-1)
    if (!next) return
    const current = canvasRef.current
    setHistory(h => redo ? { past: [...h.past, current], future: h.future.slice(0, -1) } : { past: h.past.slice(0, -1), future: [...h.future, current] })
    edit(next, false)
  }
  async function reload() {
    controller.current?.abort()
    const abort = new AbortController(); controller.current = abort
    try {
      const next = await getIdeas(workspaceKey, revision, abort.signal)
      if (next && alive.current) {
        draftRef.current = null; setDraft(null); blocked.current = false; setConflict(false); setError(''); setChanged([])
        setHistory({ past: [], future: [] }); applyRemote(next, false)
      }
    } catch (e) { if (alive.current && !abort.signal.aborted) setError(String(e)) }
  }
  return { data, canvas, active, error, busy, conflict, dirty: !!draft, changed, history, interaction, edit, select, undo, reload, persist, setChanged }
}
