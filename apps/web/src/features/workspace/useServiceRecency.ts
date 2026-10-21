import { useCallback, useEffect, useState } from 'react'
import { readOnlyShare } from '../../lib/access'
import { metricsKey, type RuntimeSnapshot } from '../../lib/api'

type ServiceRecency = Record<string, number>
const storageKey = 'devflow.service-recency.v1'
function storage() { return readOnlyShare ? sessionStorage : localStorage }

function readRecency(): ServiceRecency {
  try {
    const value: unknown = JSON.parse(storage().getItem(storageKey) ?? '{}')
    if (!value || typeof value !== 'object' || Array.isArray(value)) return {}
    return Object.fromEntries(Object.entries(value).filter(([, time]) => typeof time === 'number' && Number.isSafeInteger(time) && time > 0 && time <= Date.now()))
  } catch { return {} }
}

/** Presentation history only; runtime state still comes from the shared core engine. */
export function useServiceRecency(snapshot: RuntimeSnapshot | null) {
  const [recency, setRecency] = useState(readRecency)

  const remember = useCallback((keys: string[], at: number) => {
    if (!keys.length) return
    setRecency(current => {
      const next = { ...current }
      for (const [key, time] of Object.entries(readRecency())) next[key] = Math.max(next[key] ?? 0, time)
      for (const key of keys) next[key] = Math.max(next[key] ?? 0, at)
      const bounded = Object.fromEntries(Object.entries(next).sort((a, b) => b[1] - a[1]).slice(0, 1000))
      try { storage().setItem(storageKey, JSON.stringify(bounded)) } catch { /* Keep sorting in memory if storage is unavailable. */ }
      return bounded
    })
  }, [])

  useEffect(() => {
    if (!snapshot) return
    const running = snapshot.projects.filter(project => project.pids.length || project.managedState === 'starting' || project.managedState === 'running')
    remember(running.map(metricsKey), Math.min(Date.now(), snapshot.sampledAt * 1000))
  }, [snapshot, remember])

  return {
    lastUsed: (key: string) => recency[key] ?? 0,
    rememberService: (key: string) => remember([key], Date.now()),
  }
}
