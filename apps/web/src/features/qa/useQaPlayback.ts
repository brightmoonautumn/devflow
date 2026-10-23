import { useEffect, useMemo, useRef, useState } from 'react'
import type { QaResult, QaRun } from './qaApi'

type Playback = { context: string; runId: string; steps: QaResult[]; index: number; playing: boolean; follow: boolean }
const active = (status?: string) => status === 'running' || status === 'cancelling'

// Presentation only: replay never invokes the execution API or changes stored results.
export function useQaPlayback(run: QaRun | undefined, context: string, callIds: string[], enabled: boolean) {
  const [reducedMotion, setReducedMotion] = useState(() => window.matchMedia('(prefers-reduced-motion: reduce)').matches)
  const [playback, setPlayback] = useState<Playback | null>(null)
  const [liveFollow, setLiveFollow] = useState(true)
  const previous = useRef<{ context: string; id?: string; status?: string } | null>(null)
  const replayed = useRef(new Set<string>())
  const idsKey = JSON.stringify(callIds)
  const ids = useMemo(() => new Set<string>(JSON.parse(idsKey)), [idsKey])
  const current = playback?.context === context && playback.runId === run?.id ? playback : null
  const playing = !!current?.playing && !reducedMotion && enabled && !active(run?.status)
  const quickRun = !!run && run.results.reduce((total, step) => total + step.elapsedMs, 0) <= 5000
  const waitingToReplay = !!run && !active(run.status) && enabled && !reducedMotion && quickRun
    && previous.current?.context === context && !replayed.current.has(run.id)
    && (previous.current.id !== run.id || active(previous.current.status))
  useEffect(() => {
    const media = window.matchMedia('(prefers-reduced-motion: reduce)')
    const change = () => setReducedMotion(media.matches)
    media.addEventListener('change', change)
    return () => media.removeEventListener('change', change)
  }, [])
  useEffect(() => {
    const last = previous.current
    previous.current = { context, id: run?.id, status: run?.status }
    if (last?.context !== context) { setPlayback(null); setLiveFollow(true); replayed.current.clear(); return }
    if (run?.id !== last.id) { setPlayback(null); setLiveFollow(true) }
    if (!run || active(run.status) || !enabled || reducedMotion || !quickRun || replayed.current.has(run.id)) return
    if (last.id === run.id && !active(last.status)) return
    replayed.current.add(run.id)
    const steps = run.results.filter(step => ids.has(step.callId))
    if (steps.length) setPlayback({ context, runId: run.id, steps, index: 0, playing: true, follow: run.id !== last.id ? true : liveFollow })
  }, [context, run?.id, run?.status, enabled, reducedMotion, quickRun, ids, liveFollow])
  useEffect(() => {
    if (reducedMotion) setPlayback(value => value?.playing ? { ...value, playing: false } : value)
  }, [reducedMotion])
  useEffect(() => {
    if (!playing || !current) return
    const timer = setTimeout(() => setPlayback(value => {
      if (!value || value !== current) return value
      return value.index + 1 >= value.steps.length || value.steps[value.index].status === 'failed'
        ? { ...value, playing: false }
        : { ...value, index: value.index + 1 }
    }), 380)
    return () => clearTimeout(timer)
  }, [playing, current])
  const currentCall = playing ? current!.steps[current!.index]?.callId : active(run?.status) ? run?.currentCall : null
  function replay() {
    if (!run || active(run.status) || !enabled || reducedMotion) return
    const steps = run.results.filter(step => ids.has(step.callId))
    if (steps.length) { setLiveFollow(true); setPlayback({ context, runId: run.id, steps, index: 0, playing: true, follow: true }) }
  }
  function skip() { setPlayback(value => value ? { ...value, playing: false } : null) }
  function stopFollowing() {
    setLiveFollow(false)
    setPlayback(value => !value || !value.follow ? value : { ...value, follow: false })
  }
  return { playing, waitingToReplay, currentCall, index: playing ? current!.index : -1, steps: current?.steps ?? [], following: playing ? current!.follow : liveFollow, reducedMotion, replay, skip, stopFollowing }
}
