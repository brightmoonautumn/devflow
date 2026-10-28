import { useEffect, useRef, useState } from 'react'
import DOMPurify from 'dompurify'
import Vditor from 'vditor'
import { markdownAssetRoot, prepareMarkdownRuntime } from './markdownRuntime'
import { loadMarkdownImages, type MarkdownImageContext } from './markdownImages'

/** Render only from Markdown. No callback may serialize the rendered DOM to disk. */
export function MarkdownSourcePreview({ path, imageRevision, imageRoots, value, active, dark, readonly, onEdit }: MarkdownImageContext & { path: string; value: string; active: boolean; dark: boolean; readonly: boolean; onEdit: () => void }) {
  const host = useRef<HTMLDivElement>(null)
  const [error, setError] = useState('')
  const [loading, setLoading] = useState(true)
  useEffect(() => {
    if (!active) return
    let disposed = false
    const controller = new AbortController()
    const container = document.createElement('div')
    container.className = 'vditor-reset'
    const timer = setTimeout(() => {
      void (async () => {
        await prepareMarkdownRuntime()
        // Sanitize AFTER Markdown conversion so row/column spans and custom anchors
        // survive. Unsanitized output is never attached to the live document.
        const html = await Vditor.md2html(value, {
          cdn: markdownAssetRoot, anchor: 1, mode: dark ? 'dark' : 'light',
          markdown: { sanitize: false, autoSpace: false, fixTermTypo: false, footnotes: true, toc: true },
        })
        if (disposed || !host.current) return
        container.innerHTML = DOMPurify.sanitize(html, {
          USE_PROFILES: { html: true },
          FORBID_TAGS: ['style', 'iframe', 'form', 'input', 'button', 'textarea', 'select'],
          FORBID_ATTR: ['style', 'contenteditable'],
        })
        // Strip local URLs before mounting, then read through the authenticated image API.
        void loadMarkdownImages(container, path, { imageRevision, imageRoots }, controller.signal)
        const top = host.current.scrollTop, left = host.current.scrollLeft
        host.current.replaceChildren(container)
        host.current.scrollTop = top; host.current.scrollLeft = left
        Vditor.highlightRender({ enable: true, style: dark ? 'github-dark' : 'github' }, container, markdownAssetRoot)
        Vditor.mathRender(container, { cdn: markdownAssetRoot, math: { engine: 'KaTeX' } })
        Vditor.mermaidRender(container, markdownAssetRoot, dark ? 'dark' : 'classic')
        setError(''); setLoading(false)
      })().catch(e => { if (!disposed) { setError(e instanceof Error ? e.message : String(e)); setLoading(false) } })
    }, 180)
    return () => { disposed = true; controller.abort(); clearTimeout(timer) }
  }, [value, active, dark, path, imageRevision, imageRoots])
  return <div className="md-source-preview">
    {!readonly && <div className="md-source-preview-note">含图片、图表或 HTML 的文档请在源码或对照模式编辑。<button type="button" onClick={onEdit}>对照编辑</button></div>}
    {loading && <p className="md-loading" role="status">正在渲染文档…</p>}
    {error && <p className="md-load-error" role="alert">{error}，可切换源码继续编辑。</p>}
    <div ref={host} className="md-source-preview-scroll" aria-label="Markdown 文档预览" />
  </div>
}
