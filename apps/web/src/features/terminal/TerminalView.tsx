import { useEffect, useRef, useState } from 'react'
import { Terminal } from '@xterm/xterm'
import { FitAddon } from '@xterm/addon-fit'
import type { TerminalSession } from './api'
import { TerminalSocket } from './terminalSocket'
import '@xterm/xterm/css/xterm.css'

type Props = { session: TerminalSession; revision: number; active: boolean; theme: 'dark' | 'light' }
export function TerminalView({ session, revision, active, theme }: Props) {
  const host = useRef<HTMLDivElement>(null)
  const instance = useRef<Terminal | null>(null)
  const fit = useRef<FitAddon | null>(null)
  const connection = useRef<TerminalSocket | null>(null)
  const visible = useRef(active)
  visible.current = active
  const [error, setError] = useState('')
  const [readError, setReadError] = useState('')
  const [ended, setEnded] = useState(false)
  const [exitCode, setExitCode] = useState<number | null>(null)

  useEffect(() => {
    if (!host.current) return
    let disposed = false
    let resizeTimer: ReturnType<typeof setTimeout> | undefined
    let fitFrame = 0
    setEnded(false); setExitCode(null); setReadError(''); setError('')
    const terminal = new Terminal({ cursorBlink: true, fontSize: 12, fontFamily: 'Cascadia Code, Consolas, "Microsoft YaHei", monospace', scrollback: 5000, allowProposedApi: false, allowTransparency: false, convertEol: false, disableStdin: true })
    const addon = new FitAddon()
    terminal.loadAddon(addon)
    terminal.open(host.current)
    instance.current = terminal
    fit.current = addon
    const transport = new TerminalSocket(session.id, revision, {
      write: data => new Promise<void>(resolve => { if (disposed) resolve(); else terminal.write(data, resolve) }),
      reset: () => new Promise<void>(resolve => {
        if (disposed) { resolve(); return }
        terminal.reset()
        terminal.write('\x1b[33m较早的输出已超出缓冲区，以下为保留内容。\x1b[0m\r\n', resolve)
      }),
      connected: connected => { if (!disposed) terminal.options.disableStdin = !connected },
      error: message => { if (!disposed) setReadError(message) },
      exit: code => { if (!disposed) { setEnded(true); setExitCode(code) } },
    })
    connection.current = transport
    const input = terminal.onData(data => transport.input(data))
    const resize = terminal.onResize(({ cols, rows }) => {
      clearTimeout(resizeTimer)
      resizeTimer = setTimeout(() => transport.resize(cols, rows), 32)
    })
    const observer = new ResizeObserver(() => {
      cancelAnimationFrame(fitFrame)
      fitFrame = requestAnimationFrame(() => { if (visible.current && host.current?.clientWidth) addon.fit() })
    })
    observer.observe(host.current)
    const clear = (event: Event) => { if ((event as CustomEvent<string>).detail === session.id) terminal.clear() }
    window.addEventListener('devflow-terminal-clear', clear)
    // Escape and Ctrl+C belong to the shell; don't let workbench shortcuts close it.
    terminal.attachCustomKeyEventHandler(event => {
      if (event.key === 'Escape') event.stopPropagation()
      if ((event.ctrlKey || event.metaKey) && event.shiftKey && event.code === 'KeyC' && terminal.hasSelection()) {
        if (event.type === 'keydown') void navigator.clipboard.writeText(terminal.getSelection()).catch(() => setError('无法复制，请使用浏览器复制操作。'))
        return false
      }
      return true
    })
    const visibility = () => transport.setActive(visible.current && !document.hidden)
    document.addEventListener('visibilitychange', visibility)
    visibility()
    return () => {
      disposed = true; transport.dispose(); clearTimeout(resizeTimer); cancelAnimationFrame(fitFrame)
      observer.disconnect(); input.dispose(); resize.dispose(); terminal.dispose()
      document.removeEventListener('visibilitychange', visibility)
      window.removeEventListener('devflow-terminal-clear', clear)
      instance.current = null; fit.current = null; connection.current = null
    }
  }, [session.id, revision])

  useEffect(() => {
    const terminal = instance.current
    if (!terminal) return
    terminal.options.theme = theme === 'dark' ? {
      background: '#11151b', foreground: '#d4d4d4', cursor: '#e6edf3', selectionBackground: '#3a4d68',
      black: '#1e1e1e', red: '#f14c4c', green: '#23d18b', yellow: '#f5e06c', blue: '#6daaf7', magenta: '#d670d6', cyan: '#29b8db', white: '#e5e5e5',
      brightBlack: '#808080', brightRed: '#ff8080', brightGreen: '#6fe7b3', brightYellow: '#fff4a0', brightBlue: '#a0c8ff', brightMagenta: '#f4a6ef', brightCyan: '#80e3ff', brightWhite: '#ffffff',
    } : { background: '#ffffff', foreground: '#24292f', cursor: '#24292f', selectionBackground: '#b7d7ff', black: '#24292f', red: '#b42318', green: '#116329', yellow: '#855500', blue: '#0550ae', magenta: '#8250df', cyan: '#096e79', white: '#6e7781', brightBlack: '#57606a', brightRed: '#cf222e', brightGreen: '#1a7f37', brightYellow: '#9a6700', brightBlue: '#0969da', brightMagenta: '#a475f9', brightCyan: '#1b7c83', brightWhite: '#8c959f' }
  }, [theme, session.id, revision])
  useEffect(() => {
    connection.current?.setActive(active && !document.hidden)
    if (!active) return
    const frame = requestAnimationFrame(() => { fit.current?.fit(); instance.current?.focus() })
    return () => cancelAnimationFrame(frame)
  }, [active, session.id, revision])

  return <div className="terminal-view" hidden={!active}>
    <div className="terminal-screen" ref={host} aria-label={`${session.shell} · ${session.folder}`} />
    {(error || readError) && <div className="terminal-notice is-error" role="alert">{error || readError}<button onClick={() => { setError(''); setReadError('') }}>关闭提示</button></div>}
    {ended && <div className="terminal-notice" role="status">终端已退出 · 退出码 {exitCode ?? session.exitCode ?? '—'}</div>}
  </div>
}
