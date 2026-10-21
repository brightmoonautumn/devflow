import { useEffect, useRef, useState } from 'react'
import { Eraser, FolderOpen, Plus, Terminal, Trash2 } from 'lucide-react'
import { ResizableBottomPanel } from '../../app/ResizableBottomPanel'
import type { WorkspaceResponse } from '../../lib/api'
import { terminalRequest, type TerminalSession } from './api'
import { TerminalView } from './TerminalView'
import './terminal.css'

type Props = { workspace: WorkspaceResponse | null; visible: boolean; theme: 'dark' | 'light'; onClose: () => void }
export function TerminalPanel({ workspace, visible, theme, onClose }: Props) {
  const revision = workspace?.revision
  const [sessions, setSessions] = useState<TerminalSession[]>([])
  const [active, setActive] = useState('')
  const [choosing, setChoosing] = useState(false)
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const generation = useRef(0)
  const mutations = useRef(0)
  const creating = useRef(false)
  const menu = useRef<HTMLDivElement>(null)
  const addButton = useRef<HTMLButtonElement>(null)
  const folders = workspace?.folders.filter(folder => folder.status === 'available') ?? []

  useEffect(() => {
    const current = ++generation.current
    setSessions([]); setActive(''); setChoosing(false); setError('')
    if (revision === undefined) return
    const controller = new AbortController()
    let timer: ReturnType<typeof setTimeout> | undefined
    async function poll() {
      const mutation = mutations.current
      try {
        const response = await terminalRequest(revision!, { type: 'list' }, controller.signal)
        if (current !== generation.current || controller.signal.aborted) return
        if (mutation === mutations.current) {
          setSessions(response.sessions)
          setActive(id => response.sessions.some(session => session.id === id) ? id : response.sessions.at(-1)?.id ?? '')
        }
      } catch (err) { if (!controller.signal.aborted && current === generation.current) setError(err instanceof Error ? err.message : String(err)) }
      if (!controller.signal.aborted) timer = setTimeout(() => void poll(), 3000)
    }
    void poll()
    return () => { controller.abort(); clearTimeout(timer) }
  }, [revision])

  useEffect(() => {
    if (!choosing) return
    const dismiss = (event: PointerEvent) => { if (!menu.current?.contains(event.target as Node)) setChoosing(false) }
    document.addEventListener('pointerdown', dismiss)
    return () => document.removeEventListener('pointerdown', dismiss)
  }, [choosing])
  async function create(folder: string) {
    if (revision === undefined || creating.current) return
    creating.current = true; setBusy(true); setChoosing(false); setError('')
    mutations.current += 1
    const current = generation.current
    try {
      const response = await terminalRequest(revision, { type: 'create', folder, cols: 80, rows: 24 })
      if (current !== generation.current) return
      setSessions(response.sessions); setActive(response.id ?? '')
    } catch (err) { if (current === generation.current) setError(err instanceof Error ? err.message : String(err)) }
    finally { mutations.current += 1; creating.current = false; setBusy(false) }
  }
  async function close(id: string) {
    if (revision === undefined || busy) return
    const current = generation.current
    setBusy(true); setError('')
    mutations.current += 1
    try {
      const response = await terminalRequest(revision, { type: 'close', id })
      if (current !== generation.current) return
      setSessions(response.sessions)
      setActive(previous => previous === id ? response.sessions.at(-1)?.id ?? '' : previous)
    } catch (err) { if (current === generation.current) setError(err instanceof Error ? err.message : String(err)) }
    finally { mutations.current += 1; setBusy(false) }
  }
  const selected = sessions.find(session => session.id === active)
  return <ResizableBottomPanel title="终端" project={selected?.folder} description="" hidden={!visible} onClose={onClose} headerActions={<>
    <div className="terminal-add" ref={menu} onKeyDown={event => { if (event.key === 'Escape' && choosing) { event.preventDefault(); event.stopPropagation(); setChoosing(false); addButton.current?.focus() } }}>
      <button ref={addButton} className="icon-button" title="新建终端" aria-label="新建终端" aria-expanded={choosing} disabled={busy || !folders.length || sessions.length >= 12} onClick={() => { if (folders.length === 1) void create(folders[0].path); else setChoosing(value => !value) }}><Plus size={15} /></button>
      {choosing && <div className="terminal-folder-menu" role="group" aria-label="选择终端工作目录">
        <span>选择终端工作目录</span>
        {folders.map((folder, index) => <button key={folder.path} autoFocus={index === 0} title={folder.path} onClick={() => void create(folder.path)}><FolderOpen size={14} /><span>{folder.name}<small>{folder.path}</small></span></button>)}
      </div>}
    </div>
    <button className="icon-button" title="清除终端滚动历史" aria-label="清除终端滚动历史" disabled={!selected} onClick={() => { if (selected) window.dispatchEvent(new CustomEvent('devflow-terminal-clear', { detail: selected.id })) }}><Eraser size={14} /></button>
    <button className="icon-button" title="终止并关闭当前终端" aria-label="终止并关闭当前终端" disabled={!selected || busy} onClick={() => { if (selected) void close(selected.id) }}><Trash2 size={14} /></button>
  </>}>
    {error && <div className="terminal-notice is-error" role="alert">{error}</div>}
    <div className="terminal-body">
      <div className="terminal-views">
        {sessions.map(session => <TerminalView key={session.id} session={session} revision={revision!} active={visible && session.id === active} theme={theme} />)}
        {!sessions.length && <div className="terminal-empty"><Terminal size={24} /><span>{busy ? '正在启动终端…' : folders.length ? '点击 +，在工作空间文件夹中打开终端' : '请先打开工作空间文件夹'}</span><small>支持 ANSI 颜色、中文、交互输入与 Ctrl+C</small></div>}
      </div>
      {sessions.length > 0 && <div className="terminal-sessions" aria-label="终端列表">{sessions.map(session => <div key={session.id} className={`terminal-session ${session.id === active ? 'is-active' : ''}`}>
        <button title={`${session.folder}${session.exited ? ` · 已退出 (${session.exitCode})` : ` · PID ${session.pid}`}`} aria-pressed={session.id === active} onClick={() => setActive(session.id)}><Terminal size={13} /><span>{session.shell}<small>{session.folder.split(/[\\/]/).filter(Boolean).at(-1)}</small>{session.exited && <small>已退出</small>}</span></button>
        <button className="terminal-session-close" title="终止并关闭终端" aria-label={`关闭 ${session.shell} ${session.folder}`} disabled={busy} onClick={() => void close(session.id)}><Trash2 size={12} /></button>
      </div>)}</div>}
    </div>
  </ResizableBottomPanel>
}
