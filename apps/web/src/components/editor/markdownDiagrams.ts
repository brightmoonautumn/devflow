import { renderMarkdownDiagram } from './markdownRuntime'

type CachedDiagram = { url: string; width: number; height: number; cost: number }
const cache = new Map<string, CachedDiagram>()
const sources = new WeakMap<HTMLElement, string>()
const maxCacheCost = 8 * 1024 * 1024
let cacheCost = 0
let sequence = 0

function cached(key: string): CachedDiagram | undefined {
  const result = cache.get(key)
  if (result) { cache.delete(key); cache.set(key, result) }
  return result
}

function remember(key: string, result: CachedDiagram) {
  if (result.cost > maxCacheCost) return
  const previous = cache.get(key)
  if (previous) { cacheCost -= previous.cost; cache.delete(key) }
  cache.set(key, result)
  cacheCost += result.cost
  while (cache.size > 64 || cacheCost > maxCacheCost) {
    const oldest = cache.keys().next().value!
    cacheCost -= cache.get(oldest)!.cost
    cache.delete(oldest)
  }
}

function imageResult(svg: string, key: string): CachedDiagram {
  const root = new DOMParser().parseFromString(svg, 'image/svg+xml').documentElement
  const box = root.getAttribute('viewBox')?.trim().split(/[\s,]+/).map(Number)
  const dimension = (value: number | undefined, fallback: number) => value !== undefined && Number.isFinite(value) && value > 0 ? value : fallback
  const width = dimension(box?.[2] ?? parseFloat(root.getAttribute('width') ?? ''), 720)
  const height = dimension(box?.[3] ?? parseFloat(root.getAttribute('height') ?? ''), 240)
  const url = `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`
  return { url, width, height, cost: 2 * (key.length + url.length) }
}

/** Preview-only images isolate SVG ids/styles and keep large SVG trees out of the page DOM. */
function display(node: HTMLElement, result: CachedDiagram, failed = false) {
  const image = document.createElement('img')
  image.className = 'md-mermaid-image'
  image.alt = failed ? '图表暂时无法渲染，请检查源码或重新打开文档。' : 'Mermaid 图表，双击放大'
  image.width = result.width
  image.height = result.height
  image.src = result.url
  image.decoding = 'async'
  node.replaceChildren(image)
  node.style.minHeight = ''
  node.classList.remove('md-mermaid-pending')
  node.removeAttribute('aria-busy')
  node.setAttribute('data-processed', 'true')
}

/** One diagram at a time, only near this preview's viewport. No offscreen render backlog. */
export function observeMarkdownDiagrams(container: HTMLElement, scroller: HTMLElement, dark: boolean, signal: AbortSignal): () => void {
  const pending = new Set<HTMLElement>()
  let running = false
  let observer: IntersectionObserver | undefined
  const stopped = () => signal.aborted || !container.isConnected

  async function drain() {
    if (running || stopped()) return
    running = true
    try {
      while (pending.size && !stopped()) {
        const node = pending.values().next().value!
        pending.delete(node)
        const source = sources.get(node)!
        const key = `${dark ? 'dark' : 'light'}\0${source}`
        let result = cached(key)
        let failed = false
        if (!result) {
          const rendered = await renderMarkdownDiagram(`df-mermaid-${++sequence}`, source, {
            theme: dark ? 'dark' : 'default', fontFamily: 'sans-serif', altFontFamily: 'sans-serif',
            flowchart: { useMaxWidth: true },
            sequence: { useMaxWidth: true, diagramMarginX: 8, diagramMarginY: 8, boxMargin: 8, showSequenceNumbers: true },
            gantt: { leftPadding: 75, rightPadding: 20 },
          }, signal)
          if (stopped()) return
          failed = rendered.failed
          result = imageResult(rendered.svg, key)
          if (!failed) remember(key, result)
        }
        if (stopped()) return
        display(node, result, failed)
        pending.delete(node)
        observer?.unobserve(node)
        // Also yield on cache hits, so a large cached document cannot monopolize a frame.
        await new Promise<void>(resolve => setTimeout(resolve, 0))
      }
    } catch {
      // Aborted work belongs to a hidden/replaced preview; never publish its result.
    } finally {
      running = false
    }
  }

  if (typeof IntersectionObserver !== 'undefined') {
    observer = new IntersectionObserver(entries => {
      for (const entry of entries) {
        const node = entry.target as HTMLElement
        if (entry.isIntersecting && node.getAttribute('data-processed') !== 'true') pending.add(node)
        else pending.delete(node)
      }
      void drain()
    }, { root: scroller, rootMargin: '240px 0px' })
  }
  for (const node of container.querySelectorAll<HTMLElement>('.language-mermaid')) {
    if (node.getAttribute('data-processed') === 'true') continue
    const source = sources.get(node) ?? node.textContent ?? ''
    sources.set(node, source)
    if (!source.trim()) continue
    node.textContent = '图表将在滚动到附近时加载…'
    node.classList.add('md-mermaid-pending')
    // Bound the initial estimate; exact intrinsic dimensions are retained once rendered.
    const measured = cached(`${dark ? 'dark' : 'light'}\0${source}`)
    const height = measured ? measured.height * Math.max(720, scroller.clientWidth - 48) / measured.width + 32
      : Math.min(640, Math.max(240, source.split('\n').length * 16))
    node.style.minHeight = `${height}px`
    node.setAttribute('aria-busy', 'true')
    if (observer) observer.observe(node)
    else pending.add(node)
  }
  if (!observer) void drain()
  const dispose = () => { observer?.disconnect(); pending.clear() }
  signal.addEventListener('abort', dispose, { once: true })
  return () => { dispose(); signal.removeEventListener('abort', dispose) }
}
