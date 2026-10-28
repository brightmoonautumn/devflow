import { readWorkspaceImage } from '../../lib/api'

export type MarkdownImageContext = { imageRevision?: number; imageRoots?: string[] }

const pathKey = (path: string) => /^[a-z]:\//i.test(path) || path.startsWith('//') ? path.toLowerCase() : path

/** URL-rooted images belong to the deepest workspace folder containing the document. */
export function resolveMarkdownImage(path: string, source: string, roots: string[]): string | null {
  const value = source.trim()
  if (!value || /^(?:[a-z][\w+.-]*:|[\\/]{2}|#)/i.test(value)) return null
  let relative: string
  try { relative = decodeURIComponent(value.split(/[?#]/)[0]).replace(/\\/g, '/') } catch { return null }
  if (!relative || relative.includes('\0') || relative.includes(':') || relative.startsWith('//')) return null
  const documentPath = path.replace(/\\/g, '/')
  const root = roots.map(item => item.replace(/\\/g, '/').replace(/\/+$/, ''))
    .filter(item => pathKey(documentPath).startsWith(`${pathKey(item)}/`))
    .sort((a, b) => b.length - a.length)[0]
  if (!root) return null
  const parts = (relative.startsWith('/') ? root : documentPath.slice(0, documentPath.lastIndexOf('/'))).split('/')
  const minimum = root.split('/').length
  for (const part of relative.split('/')) {
    if (!part || part === '.') continue
    if (part === '..') { if (parts.length <= minimum) return null; parts.pop() }
    else parts.push(part)
  }
  return parts.join('/')
}

/** Only mutate the lossless preview DOM; resolved data URLs must never be saved. */
export function loadMarkdownImages(container: HTMLElement, path: string, context: MarkdownImageContext, signal: AbortSignal): Promise<void> {
  const pending = new Map<string, HTMLImageElement[]>()
  for (const image of container.querySelectorAll<HTMLImageElement>('img')) {
    const source = image.getAttribute('src') ?? ''
    image.removeAttribute('srcset')
    if (/^(?:https?:|data:|blob:|\/\/)/i.test(source.trim())) continue
    image.removeAttribute('src')
    const resolved = resolveMarkdownImage(path, source, context.imageRoots ?? [])
    if (!resolved || context.imageRevision === undefined) {
      image.title = `无法加载工作区图片：${source}`
      image.alt ||= source || '图片地址为空'
      continue
    }
    const images = pending.get(resolved) ?? []
    images.push(image)
    pending.set(resolved, images)
  }
  const entries = Array.from(pending)
  // Bound concurrent reads and deduplicate repeated images for this preview only.
  return Promise.all(Array.from({ length: Math.min(4, entries.length) }, async () => {
    while (entries.length && !signal.aborted) {
      const [resolved, images] = entries.shift()!
      try {
        const result = await readWorkspaceImage(resolved, context.imageRevision!, signal)
        if (signal.aborted) return
        for (const image of images) {
          image.onerror = () => { image.title = `图片无法解码：${resolved}`; image.alt ||= resolved }
          image.src = result.dataUrl
        }
      } catch (error) {
        if (signal.aborted) return
        for (const image of images) {
          image.title = `图片加载失败：${error instanceof Error ? error.message : String(error)}`
          image.alt ||= resolved
        }
      }
    }
  })).then(() => {})
}
