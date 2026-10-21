import { useEffect, useState } from 'react'

// Keep the existing Explorer display preference.
const storageKey = 'devflow.explorer.compactFolders'
const changeEvent = 'devflow-compact-folders-changed'
function readPreference() {
  try { return localStorage.getItem(storageKey) !== 'false' } catch { return true }
}

export function useCompactFolders(): [boolean, (value: boolean) => void] {
  const [compact, setCompact] = useState(readPreference)
  useEffect(() => {
    const changed = (event: Event) => setCompact((event as CustomEvent<boolean>).detail)
    const stored = (event: StorageEvent) => { if (event.key === storageKey || event.key === null) setCompact(readPreference()) }
    window.addEventListener(changeEvent, changed)
    window.addEventListener('storage', stored)
    return () => { window.removeEventListener(changeEvent, changed); window.removeEventListener('storage', stored) }
  }, [])
  return [compact, value => {
    setCompact(value)
    try { localStorage.setItem(storageKey, String(value)) } catch { /* Preference storage is optional. */ }
    window.dispatchEvent(new CustomEvent(changeEvent, { detail: value }))
  }]
}
