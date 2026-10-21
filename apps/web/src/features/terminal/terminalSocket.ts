import { getToken } from '../../lib/api'

type Callbacks = {
  write: (data: Uint8Array) => Promise<void>
  reset: () => Promise<void>
  connected: (value: boolean) => void
  error: (message: string) => void
  exit: (code: number | null) => void
}

/** Cursor advances only after xterm consumes a frame, so reconnect can replay
 * unconsumed output. Input is never replayed across a disconnected socket. */
export class TerminalSocket {
  private socket: WebSocket | null = null
  private active = false
  private disposed = false
  private stopped = false
  private ended = false
  private ready = false
  private cursor = 0
  private generation = 0
  private retries = 0
  private refreshToken = false
  private timer: ReturnType<typeof setTimeout> | undefined
  private drain = Promise.resolve()
  private inputs: string[] = []
  private pendingBytes = 0
  private inputInFlight = false
  private dimensions = { cols: 80, rows: 24 }
  private readonly encoder = new TextEncoder()

  constructor(private id: string, private revision: number, private callbacks: Callbacks) {}

  setActive(active: boolean) {
    if (this.active === active) return
    this.active = active
    if (active) void this.connect()
    else this.disconnect()
  }

  resize(cols: number, rows: number) {
    this.dimensions = { cols: Math.max(2, Math.min(500, cols)), rows: Math.max(1, Math.min(300, rows)) }
    if (this.ready) this.send({ type: 'resize', ...this.dimensions })
  }

  input(data: string) {
    if (this.ended) return
    if (!this.ready) { this.callbacks.error('终端正在连接，请连接后重新输入。'); return }
    const bytes = this.encoder.encode(data).length
    if (this.pendingBytes + bytes > 256 * 1024) { this.callbacks.error('输入过快，请等待终端处理后重试。'); return }
    // Unicode-aware chunks stay below both the core and WebSocket byte limits.
    this.inputs.push(...(data.match(/[\s\S]{1,4096}/gu) ?? []))
    this.pendingBytes += bytes
    this.flushInput()
  }

  dispose() { this.disposed = true; this.disconnect() }

  private send(message: object) {
    if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify(message))
  }

  private flushInput() {
    if (!this.ready || this.inputInFlight || !this.inputs.length) return
    this.inputInFlight = true
    this.send({ type: 'input', data: this.inputs[0] })
  }

  private disconnect() {
    this.generation += 1
    clearTimeout(this.timer)
    this.ready = false
    this.callbacks.connected(false)
    if (this.inputs.length) this.callbacks.error('连接中断，未确认的输入不会自动重发，请核对终端后重新输入。')
    this.inputs = []; this.pendingBytes = 0; this.inputInFlight = false
    this.socket?.close()
    this.socket = null
  }

  private async connect() {
    if (this.disposed || this.stopped || !this.active) return
    const generation = ++this.generation
    const current = () => !this.disposed && this.active && generation === this.generation
    try {
      // Finish writes already accepted by xterm before choosing the replay cursor.
      await this.drain
      const token = await getToken(this.refreshToken)
      this.refreshToken = false
      if (!current()) return
      const url = new URL('/api/workspace/terminal/socket', window.location.href)
      url.protocol = url.protocol === 'https:' ? 'wss:' : 'ws:'
      const socket = new WebSocket(url)
      this.socket = socket
      socket.binaryType = 'arraybuffer'
      const handshake = setTimeout(() => socket.close(), 8000)
      socket.onopen = () => {
        if (!current()) { socket.close(); return }
        socket.send(JSON.stringify({ token, expectedRevision: this.revision, id: this.id, after: this.cursor }))
      }
      socket.onmessage = event => {
        if (!current()) return
        if (event.data instanceof ArrayBuffer) {
          if (event.data.byteLength < 8 || event.data.byteLength > 65544) { socket.close(); return }
          const nextCursor = Number(new DataView(event.data).getBigUint64(0))
          const bytes = new Uint8Array(event.data, 8)
          this.drain = this.drain.then(async () => {
            if (this.disposed) return
            await this.callbacks.write(bytes)
            this.cursor = nextCursor
            if (current() && socket.readyState === WebSocket.OPEN) socket.send(JSON.stringify({ type: 'ack', cursor: nextCursor }))
          }).catch(() => { if (current()) socket.close() })
          return
        }
        try {
          const message = JSON.parse(event.data)
          if (message.type === 'ready') {
            clearTimeout(handshake)
            this.ready = true; this.retries = 0
            this.callbacks.error(''); this.callbacks.connected(!this.ended)
            this.resize(this.dimensions.cols, this.dimensions.rows)
          } else if (message.type === 'reset') {
            this.drain = this.drain.then(() => this.disposed ? undefined : this.callbacks.reset())
          } else if (message.type === 'input_ack' || message.type === 'input_error') {
            const input = this.inputs.shift()
            if (input !== undefined) this.pendingBytes -= this.encoder.encode(input).length
            this.inputInFlight = false
            if (message.type === 'input_error') {
              this.inputs = []; this.pendingBytes = 0
              this.callbacks.error(`${message.message} 剩余粘贴已取消，请核对后重试。`)
            } else this.flushInput()
          } else if (message.type === 'exit') {
            this.ended = true; this.ready = false
            this.callbacks.connected(false); this.callbacks.exit(message.exitCode)
          } else if (message.type === 'error') {
            this.callbacks.error(message.message)
            this.refreshToken = message.code === 'unauthorized'
            this.stopped = ['not_found', 'conflict', 'outside_workspace', 'outside_roots', 'invalid_query'].includes(message.code)
          }
        } catch { this.callbacks.error('终端连接收到无效消息。'); socket.close() }
      }
      socket.onerror = () => { if (current()) this.callbacks.error('终端连接失败，正在尝试重连。') }
      socket.onclose = () => {
        clearTimeout(handshake)
        if (!current()) return
        this.disconnect()
        if (!this.stopped && !this.ended && this.active) this.scheduleReconnect()
      }
    } catch (error) {
      if (!current()) return
      this.callbacks.error(error instanceof Error ? error.message : String(error))
      this.scheduleReconnect()
    }
  }

  private scheduleReconnect() {
    if (this.disposed || this.stopped || !this.active) return
    this.timer = setTimeout(() => void this.connect(), Math.min(5000, 500 * 2 ** Math.min(this.retries++, 4)))
  }
}
