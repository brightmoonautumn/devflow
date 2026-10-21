// Presentation only: callers supply loaded directory entries.
export const MAX_COMPACT_FOLDER_SEGMENTS = 32

export function compactFolderChild<T>(segments: readonly T[], entries: readonly T[], options: {
  enabled: boolean
  truncated?: boolean
  key: (entry: T) => string
  isDirectory: (entry: T) => boolean
}): T | null {
  if (!options.enabled || options.truncated || segments.length >= MAX_COMPACT_FOLDER_SEGMENTS || entries.length !== 1) return null
  const child = entries[0]
  if (!options.isDirectory(child) || segments.some(segment => options.key(segment) === options.key(child))) return null
  return child
}
