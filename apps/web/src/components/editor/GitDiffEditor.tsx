import { useEffect, useRef, useState } from 'react'
import { ArrowDown, ArrowUp, Columns2, Rows3, Undo2 } from 'lucide-react'
import { Compartment, EditorState } from '@codemirror/state'
import { drawSelection, EditorView, highlightSpecialChars, keymap, lineNumbers } from '@codemirror/view'
import { defaultKeymap } from '@codemirror/commands'
import { getChunks, MergeView, unifiedMergeView } from '@codemirror/merge'
import { colors } from './SourceEditor'
import { loadEditorLanguage } from './editorLanguages'
import { editorSelection } from './editorSelection'
import './git-diff-editor.css'

// Upstream falls back to a coarser comparison when this budget is exhausted.
const diffConfig = { scanLimit: 500, timeout: 100 }
const collapseUnchanged = { margin: 3, minSize: 12 }
export type DiffSnapshot = { path: string; before: string; after: string }

export default function GitDiffEditor({ path, before, after, beforeLabel = 'HEAD · 原始内容', afterLabel = '工作区 · 当前内容', onRevert, reverting = false }: { path: string; before: string; after: string; beforeLabel?: string; afterLabel?: string; onRevert?: (content: string, snapshot: DiffSnapshot) => void; reverting?: boolean }) {
  const host = useRef<HTMLDivElement>(null)
  const instance = useRef<MergeView | EditorView | null>(null)
  const [wide, setWide] = useState<boolean | null>(null)
  const [layout, setLayout] = useState<'auto' | 'split' | 'unified'>('auto')
  const [position, setPosition] = useState({ current: 0, total: 0 })
  const navigate = useRef<(direction: number, focus?: boolean) => void>(() => {})
  const revertCurrent = useRef<(chunk?: number) => void>(() => {})
  const callbacks = useRef({ onRevert, reverting })
  callbacks.current = { onRevert, reverting }
  const canRevert = !!onRevert
  const split = layout === 'auto' ? wide : layout === 'split'

  useEffect(() => {
    if (!host.current) return
    const observer = new ResizeObserver(entries => {
      const { width, height } = entries[0].contentRect
      // Hidden tabs report zero size. Preserve their layout and view state.
      if (width <= 0 || height <= 0) return
      setWide(width >= 680)
      const view = instance.current
      if (view instanceof MergeView) { view.a.requestMeasure(); view.b.requestMeasure() }
      else view?.requestMeasure()
    })
    observer.observe(host.current)
    return () => observer.disconnect()
  }, [])

  useEffect(() => {
    if (!host.current || split === null) return
    let disposed = false
    const a = new Compartment(), b = new Compartment()
    const theme = EditorView.theme({
      '&': { height: '100%', color: 'var(--ws-ink)', backgroundColor: 'var(--ws-bg)', fontSize: '13px' },
      '.cm-scroller': { overflow: 'auto', fontFamily: 'var(--font-mono)', lineHeight: '1.7' },
      '.cm-content': { padding: '10px 0' },
      '.cm-gutters': { color: 'var(--ws-dim)', backgroundColor: 'var(--ws-bg)', borderColor: 'var(--ws-border)' },
      '.cm-cursor': { borderLeftColor: 'var(--ws-ink)' },
      '&.cm-focused': { outline: 'none' },
    })
    // Read-only views need selection and navigation, not edit history,
    // completion or the rest of basicSetup.
    const extensions = [lineNumbers(), highlightSpecialChars(), drawSelection(), editorSelection, keymap.of(defaultKeymap), colors, theme,
      EditorState.readOnly.of(true), EditorView.editable.of(false),
      EditorView.contentAttributes.of({ 'aria-label': `只读差异 ${path}`, tabindex: '0' })]
    const view = split ? new MergeView({ parent: host.current,
      a: { doc: before, extensions: [...extensions, a.of([])] },
      b: { doc: after, extensions: [...extensions, b.of([])] },
      highlightChanges: true, gutter: true, diffConfig, collapseUnchanged,
      revertControls: canRevert ? 'a-to-b' : undefined,
      renderRevertControl: () => {
        const button = document.createElement('button')
        button.type = 'button'; button.textContent = '→'
        button.title = '还原此处改动：使用左侧旧版本'; button.setAttribute('aria-label', button.title)
        button.disabled = callbacks.current.reverting
        // Keep the upstream control from editing the read-only model directly.
        // The feature callback validates the snapshot and persists through core.
        button.addEventListener('mousedown', event => event.stopPropagation())
        button.addEventListener('click', event => {
          event.preventDefault(); event.stopPropagation()
          const chunk = Number(button.dataset.chunk)
          if (Number.isInteger(chunk)) revertCurrent.current(chunk)
        })
        return button
      } })
      : new EditorView({ parent: host.current, doc: after,
        extensions: [...extensions, b.of([]), unifiedMergeView({ original: before, mergeControls: false,
          highlightChanges: true, gutter: true, diffConfig, collapseUnchanged, syntaxHighlightDeletionsMaxLength: 3000 })] })
    instance.current = view
    const target = view instanceof MergeView ? view.b : view
    const chunks = view instanceof MergeView ? view.chunks : getChunks(target.state)?.chunks ?? []
    let index = -1
    revertCurrent.current = (at = index) => {
      if (!callbacks.current.onRevert || callbacks.current.reverting) return
      const chunk = chunks[at]
      if (!chunk) return
      const original = view instanceof MergeView ? view.a.state.doc : EditorState.create({ doc: before }).doc
      const from = Math.min(chunk.fromB, target.state.doc.length), to = Math.min(chunk.toB, target.state.doc.length)
      // Chunk endpoints include the following newline and may sit one character
      // past EOF. Retain the destination's final-line boundary when replacing.
      let insert = original.sliceString(chunk.fromA, Math.max(chunk.fromA, chunk.toA - 1))
      if (chunk.fromA !== chunk.toA && chunk.toB <= target.state.doc.length) insert += '\n'
      callbacks.current.onRevert(target.state.doc.sliceString(0, from) + insert + target.state.doc.sliceString(to), { path, before, after })
    }
    setPosition({ current: 0, total: chunks.length })
    navigate.current = (direction, focus = true) => {
      if (!chunks.length) return
      index = index < 0 ? direction < 0 ? chunks.length - 1 : 0 : (index + direction + chunks.length) % chunks.length
      const anchor = Math.min(chunks[index].fromB, target.state.doc.length)
      target.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: 'center' }) })
      setPosition({ current: index + 1, total: chunks.length })
      if (focus) target.focus()
    }
    const frame = requestAnimationFrame(() => { if (!disposed) navigate.current(0, false) })
    // Very long lines and large snapshots remain plain text to avoid expensive parsing.
    const syntax = before.length + after.length <= 300000 && ![before, after].some(text => text.split('\n').some(line => line.length >= 20000))
    if (syntax) void loadEditorLanguage(path).then(language => {
      if (disposed) return
      if (view instanceof MergeView) { view.a.dispatch({ effects: a.reconfigure(language) }); view.b.dispatch({ effects: b.reconfigure(language) }) }
      else view.dispatch({ effects: b.reconfigure(language) })
    }).catch(() => { /* Unknown languages remain plain text. */ })
    return () => { disposed = true; cancelAnimationFrame(frame); navigate.current = () => {}; revertCurrent.current = () => {}; instance.current = null; view.destroy() }
  }, [path, before, after, split, canRevert])
  useEffect(() => {
    host.current?.querySelectorAll<HTMLButtonElement>('.cm-merge-revert button').forEach(button => { button.disabled = reverting })
  }, [reverting])

  return <div className="git-diff-editor" aria-label={`只读比较 ${path}`}>
    <div className="git-diff-controls">
      <div role="group" aria-label="差异布局">
        <button type="button" title="左右对比" aria-label="左右对比" aria-pressed={split === true} onClick={() => setLayout('split')}><Columns2 size={14} /></button>
        <button type="button" title="行内对比" aria-label="行内对比" aria-pressed={split === false} onClick={() => setLayout('unified')}><Rows3 size={14} /></button>
      </div>
      {canRevert && <button type="button" title="还原当前差异块为旧版本" aria-label="还原当前差异块为旧版本" disabled={reverting || !position.current} onClick={() => revertCurrent.current()}><Undo2 size={14} /></button>}
      <span role="status">{split === null ? '准备差异视图…' : position.total ? `${position.current} / ${position.total} 处变化` : '无文本差异'}</span>
      <div role="group" aria-label="跳转差异">
        <button type="button" title="上一处变化" aria-label="上一处变化" disabled={!position.total} onClick={() => navigate.current(-1)}><ArrowUp size={14} /></button>
        <button type="button" title="下一处变化" aria-label="下一处变化" disabled={!position.total} onClick={() => navigate.current(1)}><ArrowDown size={14} /></button>
      </div>
    </div>
    {split && <div className="git-diff-labels"><span>{beforeLabel}</span><span>{afterLabel}</span></div>}
    <div ref={host} className="git-diff-surface" />
  </div>
}
