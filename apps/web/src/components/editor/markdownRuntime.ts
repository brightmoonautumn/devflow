import DOMPurify from 'dompurify'

export const markdownAssetRoot = `${import.meta.env.BASE_URL}vendor/vditor`
let ready: Promise<void> | undefined
let renderQueue: Promise<unknown> = Promise.resolve()

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
    const [{ default: mermaid }] = await Promise.all([
      import('mermaid'), script('js/lute/lute.min.js', 'vditorLuteScript'),
      script('js/i18n/zh_CN.js', 'vditorI18nScriptzh_CN'), script('js/icons/ant.js', 'vditorIconScript'),
    ])
    let config: Parameters<typeof mermaid.initialize>[0] = {}
    // Adapt the global API expected by Vditor; serialize and sanitize diagram output.
    Object.assign(window, { mermaid: {
      initialize(options: typeof config) { config = options },
      render(id: string, source: string) {
        const options = config
        const job = renderQueue.catch(() => {}).then(async () => {
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
            return { svg: '<svg xmlns="http://www.w3.org/2000/svg" width="480" height="72" viewBox="0 0 480 72"><text x="16" y="40" fill="#d97706" font-family="sans-serif" font-size="14">流程图语法暂不完整，请点击图表编辑源码。</text></svg>' }
          } finally {
            container.remove()
          }
        })
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
