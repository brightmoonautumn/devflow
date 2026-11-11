import { getWorkspaceChildren, type WorkspaceChildren } from '../../lib/api'
import { pathWithin, viewPathKey } from './workspaceViewState'

type ReadJob = {
  key: string
  path: string
  foreground: boolean
  controller: AbortController
  promise: Promise<WorkspaceChildren>
  resolve: (value: WorkspaceChildren) => void
  reject: (reason: unknown) => void
  readers: number
}

const MAX_CONCURRENT_READS = 4
const MAX_CACHED_DIRECTORIES = 512
const CACHE_LIFETIME_MS = 30000
const abortError = () => new DOMException('目录读取已取消', 'AbortError')

/** One model per workspace/revision, independent of mounted tree rows. */
export class WorkspaceTreeData {
  private cache = new Map<string, { value: WorkspaceChildren; expires: number }>()
  private pending = new Map<string, ReadJob>()
  private queue: ReadJob[] = []
  private active = 0

  constructor(private revision: number) {}

  read(path: string, signal: AbortSignal, foreground: boolean, refresh = false): Promise<WorkspaceChildren> {
    if (signal.aborted) return Promise.reject(abortError())
    const key = viewPathKey(path)
    if (refresh) this.cache.delete(key)
    const cached = this.cache.get(key)
    if (cached && cached.expires > Date.now()) {
      this.cache.delete(key)
      this.cache.set(key, cached)
      return Promise.resolve(cached.value)
    }
    this.cache.delete(key)
    let job = this.pending.get(key)
    if (!job) {
      let resolve!: ReadJob['resolve'], reject!: ReadJob['reject']
      const promise = new Promise<WorkspaceChildren>((yes, no) => { resolve = yes; reject = no })
      job = { key, path, foreground, controller: new AbortController(), promise, resolve, reject, readers: 0 }
      this.pending.set(key, job)
      this.queue.push(job)
    }
    if (foreground) job.foreground = true
    const current = job
    current.readers += 1
    const result = new Promise<WorkspaceChildren>((resolve, reject) => {
      let finished = false
      const finish = () => {
        if (finished) return false
        finished = true
        signal.removeEventListener('abort', cancel)
        current.readers -= 1
        return true
      }
      const cancel = () => {
        if (!finish()) return
        reject(abortError())
        if (current.readers === 0) this.cancel(current)
      }
      signal.addEventListener('abort', cancel, { once: true })
      current.promise.then(value => { if (finish()) resolve(value) }, error => { if (finish()) reject(error) })
    })
    this.pump()
    return result
  }

  invalidate(path: string, descendants = false) {
    const matches = (key: string) => descendants ? pathWithin(key, path) : key === viewPathKey(path)
    for (const key of this.cache.keys()) if (matches(key)) this.cache.delete(key)
    for (const job of this.pending.values()) if (matches(job.key)) this.cancel(job)
  }

  prioritize(path: string) {
    const job = this.pending.get(viewPathKey(path))
    if (job) job.foreground = true
  }

  clear() {
    this.cache.clear()
    for (const job of this.pending.values()) this.cancel(job)
  }

  private cancel(job: ReadJob) {
    job.controller.abort()
    job.reject(abortError())
    if (this.pending.get(job.key) === job) this.pending.delete(job.key)
    this.queue = this.queue.filter(queued => queued !== job)
  }

  private pump() {
    while (this.active < MAX_CONCURRENT_READS && this.queue.length) {
      const index = this.queue.findIndex(job => job.foreground)
      const [job] = this.queue.splice(index < 0 ? 0 : index, 1)
      this.active += 1
      // Caller cancellation and the timeout both cover local and shared reads.
      const signal = AbortSignal.any([job.controller.signal, AbortSignal.timeout(15000)])
      getWorkspaceChildren(job.path, this.revision, signal).then(value => {
        if (!signal.aborted && this.pending.get(job.key) === job) {
          this.cache.set(job.key, { value, expires: Date.now() + CACHE_LIFETIME_MS })
          if (this.cache.size > MAX_CACHED_DIRECTORIES) this.cache.delete(this.cache.keys().next().value!)
        }
        job.resolve(value)
      }, job.reject).finally(() => {
        if (this.pending.get(job.key) === job) this.pending.delete(job.key)
        this.active -= 1
        this.pump()
      })
    }
  }
}
