import { lazy, Suspense, useEffect, useId, useRef, useState, type MouseEvent } from 'react'
import { createPortal } from 'react-dom'
import { BookOpen, Columns2, FileCode2, Minus, Plus, RotateCcw, X } from 'lucide-react'
import Vditor from 'vditor'
import DOMPurify from 'dompurify'
import { readOnlyShare } from '../../lib/access'
import type { SourceEditorProps } from './SourceEditor'
import { markdownAssetRoot, needsSourcePreview, prepareMarkdownRuntime, releaseMarkdownEditor, resolveMarkdownLink } from './markdownRuntime'
import { MarkdownSourcePreview } from './MarkdownSourcePreview'
import type { MarkdownImageContext } from './markdownImages'
import 'vditor/dist/index.css'
import './markdown-editor.css'

const SourceEditor = lazy(() => import('./SourceEditor'))
type Mode = 'document' | 'source' | 'split'

/** Vditor is an implementation detail; persistence and authorization stay in the workbench. */
export default function MarkdownEditor(props: SourceEditorProps & MarkdownImageContext & { theme: 'dark' | 'light'; active?: boolean; onOpenFile?: (path: string) => void; toolbarContainer?: HTMLElement | null; viewStateKey?: string }) {
  const [mode, setMode] = useState<Mode>(() => {
    if (props.location && props.location.sequence !== 0) return 'source'
    try {
      const saved = props.viewStateKey ? (readOnlyShare ? sessionStorage : localStorage).getItem(props.viewStateKey) : null
      if (saved === 'document' || saved === 'source' || saved === 'split') return saved
    } catch { /* Keep the default mode if storage is disabled. */ }
    return 'document'
  })
  const [sourceOpened, setSourceOpened] = useState(mode !== 'document')
  const [ready, setReady] = useState(false)
  const [error, setError] = useState('')
  const [diagram, setDiagram] = useState('')
  const [zoom, setZoom] = useState(1)
  const dark = props.theme === 'dark'
  const host = useRef<HTMLDivElement>(null)
  const editor = useRef<Vditor | null>(null)
  const modal = useRef<HTMLDialogElement>(null)
  const current = useRef(props)
  const currentMode = useRef(mode)
  const baseline = useRef('')
  const appliedValue = useRef(props.value)
  const initialized = useRef(false)
  const appliedTheme = useRef<boolean | null>(null)
  // Stay in the lossless renderer for this tab after complex content is introduced.
  const sourcePreview = useRef(false)
  sourcePreview.current ||= needsSourcePreview(props.value)
  const preserveSource = sourcePreview.current
  // Keep initialized tabs alive, but never parse a hidden or source-only document just to open it.
  const shouldInitialize = ready || (mode !== 'source' && props.active !== false)
  const id = useId()
  current.current = props
  currentMode.current = mode

  function acceptInput(value: string) {
    if (sourcePreview.current || !initialized.current || current.current.readonly || currentMode.current !== 'document' || value === baseline.current) return
    baseline.current = value
    appliedValue.current = value
    current.current.onChange(value)
  }
  useEffect(() => {
    if (!shouldInitialize || preserveSource) return
    let disposed = false
    let instance: Vditor | undefined
    let timer: ReturnType<typeof setTimeout> | undefined
    const element = host.current
    if (!element) return
    // Paint the new selection/loading state before parsing Markdown. Rapid switches cancel this work.
    const initialize = () => {
      void prepareMarkdownRuntime().then(() => {
      if (disposed) return
      const isDark = current.current.theme === 'dark'
      appliedTheme.current = isDark
      instance = new Vditor(element, {
        value: current.current.value, cdn: markdownAssetRoot, lang: 'zh_CN', height: '100%', minHeight: 180,
        mode: 'wysiwyg', theme: isDark ? 'dark' : 'classic', cache: { enable: false },
        outline: { enable: false, position: 'left' }, counter: { enable: false },
        resize: { enable: false }, toolbarConfig: { pin: false },
        toolbar: ['headings', 'bold', 'italic', 'strike', '|', 'list', 'ordered-list', 'check', 'quote', '|', 'code', 'inline-code', 'link', 'table', '|', 'undo', 'redo', 'outline'],
        preview: {
          maxWidth: 880, delay: 250, actions: [], render: { media: { enable: false } },
          hljs: { enable: true, style: isDark ? 'github-dark' : 'github' },
          math: { engine: 'KaTeX' },
          markdown: { sanitize: true, autoSpace: false, fixTermTypo: false, footnotes: true, toc: true, codeBlockPreview: true },
          theme: { current: isDark ? 'dark' : 'light', path: `${markdownAssetRoot}/dist/css/content-theme` },
        },
        link: { isOpen: false }, image: { isPreview: false },
        input: acceptInput,
        blur: acceptInput,
        after() {
          if (!instance) return
          if (disposed) { releaseMarkdownEditor(instance); return }
          editor.current = instance
          if (appliedValue.current !== current.current.value) instance.setValue(current.current.value)
          appliedValue.current = current.current.value
          baseline.current = instance.getValue()
          initialized.current = true
          if (current.current.readonly || currentMode.current !== 'document') instance.disabled()
          // Remove the dependency's inline padding so responsive application styles can govern it.
          element.querySelectorAll<HTMLElement>('.vditor-reset').forEach(node => { node.style.paddingLeft = ''; node.style.paddingRight = '' })
          setReady(true)
        },
      })
      }).catch(err => { if (!disposed) setError(err instanceof Error ? err.message : String(err)) })
    }
    const frame = requestAnimationFrame(() => { timer = setTimeout(initialize, 0) })
    return () => {
      disposed = true
      cancelAnimationFrame(frame)
      clearTimeout(timer)
      if (initialized.current && instance) releaseMarkdownEditor(instance)
      initialized.current = false
      editor.current = null
    }
  }, [props.path, shouldInitialize, preserveSource])
  useEffect(() => {
    if (!ready || !editor.current || props.active === false || mode === 'source' || appliedValue.current === props.value) return
    const timer = setTimeout(() => {
      const instance = editor.current
      if (!instance) return
      const scroller = host.current?.querySelector<HTMLElement>('.vditor-wysiwyg > pre.vditor-reset')
      const top = scroller?.scrollTop ?? 0, left = scroller?.scrollLeft ?? 0
      instance.setValue(current.current.value)
      appliedValue.current = current.current.value
      baseline.current = instance.getValue()
      if (scroller) { scroller.scrollTop = top; scroller.scrollLeft = left }
    }, 180)
    return () => clearTimeout(timer)
  }, [props.value, ready, props.active, mode])
  useEffect(() => {
    if (!ready || !editor.current) return
    if (props.readonly || mode !== 'document') editor.current.disabled()
    else editor.current.enable()
  }, [props.readonly, mode, ready])
  useEffect(() => {
    if (!ready || !editor.current || !host.current || props.active === false || mode === 'source' || appliedTheme.current === dark) return
    appliedTheme.current = dark
    editor.current.setTheme(dark ? 'dark' : 'classic', dark ? 'dark' : 'light', dark ? 'github-dark' : 'github', `${markdownAssetRoot}/dist/css/content-theme`)
    // Repaint only diagram previews; changing themes must not reset the document or undo history.
    for (const node of host.current.querySelectorAll<HTMLElement>('.language-mermaid[data-processed="true"]')) {
      const source = node.closest('.vditor-wysiwyg__preview')?.previousElementSibling?.querySelector('code')?.textContent
      if (source) { node.textContent = source; node.removeAttribute('data-processed') }
    }
    Vditor.mermaidRender(host.current, markdownAssetRoot, dark ? 'dark' : 'classic')
  }, [dark, ready, props.active, mode])
  useEffect(() => {
    if (props.location && props.location.sequence !== 0) { setSourceOpened(true); setMode('source') }
  }, [props.location?.sequence])
  useEffect(() => {
    if (!props.viewStateKey) return
    try { (readOnlyShare ? sessionStorage : localStorage).setItem(props.viewStateKey, mode) } catch { /* Keep the live mode working. */ }
  }, [props.viewStateKey, mode])
  useEffect(() => {
    if (diagram) modal.current?.showModal()
  }, [diagram])
  useEffect(() => { const dialog = modal.current; return () => dialog?.close() }, [])

  function switchMode(next: Mode) {
    const instance = editor.current
    if (mode === 'document' && instance) acceptInput(instance.getValue())
    if (next === 'document' && instance && appliedValue.current !== current.current.value) {
      instance.setValue(current.current.value)
      appliedValue.current = current.current.value
      baseline.current = instance.getValue()
    }
    if (next !== 'document') setSourceOpened(true)
    setMode(next)
  }
  function linkClick(event: MouseEvent<HTMLDivElement>) {
    const anchor = (event.target as Element).closest<HTMLAnchorElement>('a[href]')
    if (!anchor) return
    const href = anchor.getAttribute('href') ?? ''
    event.preventDefault(); event.stopPropagation()
    if (!preserveSource && mode === 'document' && event.type !== 'dblclick' && !event.ctrlKey && !event.metaKey) return
    if (href.startsWith('#')) {
      let fragment: string
      try { fragment = decodeURIComponent(href.slice(1)) } catch { return }
      const target = Array.from(host.current?.querySelectorAll<HTMLElement>('[id]') ?? []).find(node => node.id === fragment || node.id === `vditorAnchor-${fragment}`)
      target?.scrollIntoView({ block: 'start' })
    } else if (/^(?:https?:|mailto:)/i.test(href)) window.open(href, '_blank', 'noopener,noreferrer')
    else {
      const path = resolveMarkdownLink(current.current.path, href)
      if (path) current.current.onOpenFile?.(path)
    }
  }
  function expandDiagram(event: MouseEvent<HTMLDivElement>) {
    const svg = (event.target as Element).closest('.language-mermaid')?.querySelector('svg')
    if (!svg) return
    event.preventDefault(); event.stopPropagation()
    const clean = DOMPurify.sanitize(svg.outerHTML, { USE_PROFILES: { svg: true, svgFilters: true } })
    setZoom(1)
    setDiagram(`data:image/svg+xml;charset=utf-8,${encodeURIComponent(clean)}`)
  }

  const modeSwitch = <div className="md-mode-switch" role="group" aria-label="Markdown 查看与编辑模式">
    <button type="button" title="文档 · 直接编辑正文" aria-label="文档模式" aria-pressed={mode === 'document'} onClick={() => switchMode('document')}><BookOpen size={14} /></button>
    <button type="button" title="Markdown 源码" aria-label="源码模式" aria-pressed={mode === 'source'} onClick={() => switchMode('source')}><FileCode2 size={14} /></button>
    <button type="button" title="源码与文档对照" aria-label="对照模式" aria-pressed={mode === 'split'} onClick={() => switchMode('split')}><Columns2 size={14} /></button>
  </div>
  return <div className="df-markdown-editor" data-mode={mode} onInput={event => {
    // Publish native edits immediately, before a tab close or browser leave can miss Vditor's debounce.
    if (mode === 'document' && !(event.nativeEvent as InputEvent).isComposing && editor.current) acceptInput(editor.current.getValue())
  }} onCompositionEnd={() => {
    queueMicrotask(() => { if (currentMode.current === 'document' && editor.current) acceptInput(editor.current.getValue()) })
  }} onKeyDownCapture={event => {
    if ((event.ctrlKey || event.metaKey) && !event.altKey && event.key.toLowerCase() === 's') {
      event.preventDefault(); event.stopPropagation()
      if (editor.current && mode === 'document') acceptInput(editor.current.getValue())
      current.current.onSave()
    }
  }}>
    {props.toolbarContainer ? createPortal(modeSwitch, props.toolbarContainer) : props.toolbarContainer === undefined ? <div className="md-mode-floating">{modeSwitch}</div> : null}
    {error && <p className="md-load-error" role="alert">{error} 可切换源码继续编辑。</p>}
    <div className="md-editor-panes">
      <div className="md-source-pane" hidden={mode === 'document'}>{sourceOpened && <Suspense fallback={<p className="md-loading">正在加载源码编辑器…</p>}><SourceEditor {...props} /></Suspense>}</div>
      <div className="md-document-pane" hidden={mode === 'source'} onClickCapture={linkClick} onDoubleClickCapture={event => {
        if ((event.target as Element).closest('a[href]')) linkClick(event)
        else expandDiagram(event)
      }}>
        {preserveSource ? <div ref={host} className="md-source-preview-host"><MarkdownSourcePreview path={props.path} imageRevision={props.imageRevision} imageRoots={props.imageRoots} value={props.value} active={props.active !== false && mode !== 'source'} dark={dark} readonly={props.readonly} onEdit={() => switchMode('split')} /></div> : <>
          {!ready && !error && <p className="md-loading" role="status">正在打开 Markdown 文档…</p>}
          <div className={`md-vditor-frame ${ready ? 'is-ready' : ''}`}><div ref={host} className="md-vditor-host" /></div>
        </>}
      </div>
    </div>
    <dialog ref={modal} className="md-diagram-dialog" aria-labelledby={`${id}-title`} onClose={() => setDiagram('')} onKeyDown={event => event.stopPropagation()}>
      <header><strong id={`${id}-title`}>Mermaid 图表</strong><div>
        <button aria-label="缩小图表" disabled={zoom <= .5} onClick={() => setZoom(value => Math.max(.5, value - .25))}><Minus size={15} /></button>
        <button aria-label="重置图表缩放" onClick={() => setZoom(1)}><RotateCcw size={14} />{Math.round(zoom * 100)}%</button>
        <button aria-label="放大图表" disabled={zoom >= 3} onClick={() => setZoom(value => Math.min(3, value + .25))}><Plus size={15} /></button>
        <button autoFocus aria-label="关闭图表" onClick={() => modal.current?.close()}><X size={17} /></button>
      </div></header>
      <div className="md-diagram-canvas">{diagram && <img src={diagram} style={{ width: `${zoom * 100}%` }} alt="Mermaid 图表放大视图" />}</div>
    </dialog>
  </div>
}
