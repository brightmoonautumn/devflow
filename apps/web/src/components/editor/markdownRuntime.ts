import DOMPurify from 'dompurify'
import type { MermaidConfig } from 'mermaid'

export const markdownAssetRoot = `${import.meta.env.BASE_URL}vendor/vditor`
let ready: Promise<void> | undefined
let diagrams: Promise<typeof import('mermaid')> | undefined
let renderQueue: Promise<unknown> = Promise.resolve()
const diagramError = () => ({ svg: '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="72" viewBox="0 0 480 72"><text x="16" y="40" fill="#d97706" font-family="sans-serif" font-size="14">图表暂时无法渲染，请检查源码或重新打开文档。</text></svg>' })

/** Complex HTML and diagrams must never round-trip through a contenteditable DOM. */
export function needsSourcePreview(markdown: string): boolean {
  let fence: { character: string; length: number } | null = null
  for (const line of markdown.split(/\r?\n/)) {
    const match = /^ {0,3}(`{3,}|~{3,})(.*)$/.exec(line)
    if (fence) {
      if (match && match[1][0] === fence.character && match[1].length >= fence.length && !match[2].trim()) fence = null
      continue
    }
    if (match) {
      if (/^\s*mermaid\b/i.test(match[2])) return true
      fence = { character: match[1][0], length: match[1].length }
    } else if (/!\[[^\]]*\]|<img\b|^\s*(?:<(?:table|thead|tbody|tr|td|th|div|details|summary)\b|<a\s+[^>]*(?:id|name)\s*=)/i.test(line)) return true
  }
  return false
}

function script(path: string, id: string): Promise<void> {
  if (document.getElementById(id)) return Promise.resolve()
  return new Promise((resolve, reject) => {
    const element = document.createElement('script')
    element.src = `${markdownAssetRoot}/dist/${path}`
    element.onload = () => { element.id = id; resolve() }
    element.onerror = () => { element.remove(); reject(new Error('Markdown 本地资源加载失败，请更新页面后重试。')) }
    document.head.append(element)
  })
}

export function prepareMarkdownRuntime(): Promise<void> {
  if (!ready) ready = (async () => {
    await Promise.all([
      script('js/lute/lute.min.js', 'vditorLuteScript'),
      script('js/i18n/zh_CN.js', 'vditorI18nScriptzh_CN'), script('js/icons/ant.js', 'vditorIconScript'),
    ])
    let config: MermaidConfig = {}
    // Adapt the global API expected by Vditor; serialize and sanitize diagram output.
    Object.assign(window, { mermaid: {
      initialize(options: typeof config) { config = options },
      render(id: string, source: string) {
        const options = config
        const job = renderQueue.catch(() => {}).then(async () => {
          // Let pending clicks and paints run between expensive diagram layouts.
          await new Promise<void>(resolve => setTimeout(resolve, 0))
          const { default: mermaid } = await (diagrams ??= import('mermaid').catch(error => {
            diagrams = undefined
            throw error
          }))
          mermaid.initialize({ ...options, startOnLoad: false, securityLevel: 'strict', suppressErrorRendering: true,
            maxTextSize: 80000, maxEdges: 1000, htmlLabels: false, flowchart: { ...options?.flowchart, htmlLabels: false },
            secure: ['secure', 'securityLevel', 'startOnLoad', 'maxTextSize', 'maxEdges', 'htmlLabels', 'flowchart', 'suppressErrorRendering'],
          })
          // Mermaid otherwise measures temporary SVGs directly in body, briefly
          // expanding the page while large diagrams are being laid out.
          // Keep layout measurable (not display:none) without a scrollable box.
          const container = document.createElement('div')
          container.setAttribute('aria-hidden', 'true')
          Object.assign(container.style, {
            position: 'fixed', top: '0', left: '0', width: '880px', height: '0',
            overflow: 'hidden', visibility: 'hidden', pointerEvents: 'none', contain: 'layout paint',
          })
          document.body.append(container)
          try {
            const result = await mermaid.render(id, source, container)
            return { svg: DOMPurify.sanitize(result.svg, { USE_PROFILES: { svg: true, svgFilters: true } }) }
          } catch {
            // Vditor's error path assumes a DOM error node; return a safe diagnostic instead.
            return diagramError()
          } finally {
            container.remove()
          }
        }).catch(diagramError)
        renderQueue = job
        return job
      },
    } })
    if (!document.getElementById('vditorMermaidScript')) {
      const marker = document.createElement('script')
      marker.id = 'vditorMermaidScript'
      document.head.append(marker)
    }
  })().catch(error => { ready = undefined; throw error })
  return ready
}

/** Vditor removes the shared script on destroy, then uses synchronous XHR to reload it.
 * The SVG sprite outlives an editor; retain its executed script marker for the next tab.
 */
export function releaseMarkdownEditor(editor: { destroy(): void }) {
  const icons = document.getElementById('vditorIconScript')
  try { editor.destroy() } finally {
    if (icons && !document.getElementById('vditorIconScript')) document.head.append(icons)
  }
}

export function resolveMarkdownLink(path: string, href: string): string | null {
  let relative: string
  try { relative = decodeURIComponent(href.split(/[?#]/)[0]) } catch { return null }
  if (!relative || /^(?:[a-z][\w+.-]*:|[\\/])/i.test(relative) || relative.includes('\0')) return null
  const parts = path.replace(/\\/g, '/').split('/').slice(0, -1)
  for (const part of relative.replace(/\\/g, '/').split('/')) {
    if (!part || part === '.') continue
    if (part === '..') { if (parts.length <= 1) return null; parts.pop() }
    else parts.push(part)
  }
  return parts.join('/')
}
