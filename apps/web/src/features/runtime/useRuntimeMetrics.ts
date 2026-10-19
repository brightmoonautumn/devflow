import { useEffect, useState } from 'react'
import { getRuntime, type RuntimeSnapshot } from '../../lib/api'

/** Sequential polling avoids overlapping samples. Stop polling when hidden/unmounted. */
export function useRuntimeMetrics(enabled: boolean) {
  const [snapshot, setSnapshot] = useState<RuntimeSnapshot | null>(null)
  const [error, setError] = useState('')
  const [history, setHistory] = useState<RuntimeSnapshot[]>([])
  useEffect(() => {
    setSnapshot(null)
    setError('')
    setHistory([])
    if (!enabled) return
    let disposed = false
    let timer: ReturnType<typeof setTimeout> | undefined
    let pending: AbortController | undefined
    async function poll() {
      if (disposed || document.hidden || pending) return
      const controller = new AbortController()
      pending = controller
      const timeout = setTimeout(() => controller.abort(), 10000)
      try {
        const result = await getRuntime(controller.signal)
        if (!disposed && !controller.signal.aborted) {
          setSnapshot(result)
          setError('')
          setHistory(current => [...current.filter(sample => sample.sampledAt !== result.sampledAt), result].slice(-30))
        }
      } catch (err) {
        if (!disposed) { setSnapshot(null); setHistory([]); setError(String(err)) }
      } finally {
        clearTimeout(timeout)
        if (pending === controller) pending = undefined
        if (!disposed && !document.hidden) timer = setTimeout(() => void poll(), 3000)
      }
    }
    function onVisibilityChange() {
      clearTimeout(timer)
      if (!document.hidden) void poll()
    }
    document.addEventListener('visibilitychange', onVisibilityChange)
    void poll()
    return () => {
      disposed = true
      clearTimeout(timer)
      pending?.abort()
      document.removeEventListener('visibilitychange', onVisibilityChange)
    }
  }, [enabled])
  return { snapshot, history, error }
}
