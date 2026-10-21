import { useEffect, useRef, useState } from 'react'
import { Compartment, EditorState, StateEffect, StateField } from '@codemirror/state'
import { Decoration, EditorView, keymap, type DecorationSet } from '@codemirror/view'
import { basicSetup } from 'codemirror'
import { defaultKeymap, historyKeymap } from '@codemirror/commands'
import { colors } from './SourceEditor'
import { loadEditorLanguage } from './editorLanguages'

type Range = { from: number; to: number }
const highlight = StateEffect.define<Range[]>()
const highlights = StateField.define<DecorationSet>({
  create: () => Decoration.none,
  update(value, transaction) {
    value = value.map(transaction.changes)
    for (const effect of transaction.effects) if (effect.is(highlight)) {
      const lines = new Set<number>()
      for (const range of effect.value) {
        const end = Math.min(Math.max(range.from, range.to - 1), transaction.state.doc.length)
        let line = transaction.state.doc.lineAt(Math.min(range.from, end))
        while (line.from <= end) { lines.add(line.from); if (line.number === transaction.state.doc.lines) break; line = transaction.state.doc.line(line.number + 1) }
      }
      value = Decoration.set([...lines].sort((a, b) => a - b).map(at => Decoration.line({ class: 'merge-conflict-line' }).range(at)))
    }
    return value
  },
  provide: field => EditorView.decorations.from(field),
})

/** Editor-only boundary: no Git, file writes, or business requests. */
export default function ThreeWayMergeEditor({ path, local, incoming, value, ranges, selected, disabled, onChange, onSave }: {
  path: string; local: string; incoming: string; value: string; ranges: Range[]; selected: (Range & { local?: string; incoming?: string }) | null; disabled: boolean;
  onChange: (value: string) => void; onSave: () => void;
}) {
  const hosts = useRef<(HTMLDivElement | null)[]>([]), views = useRef<EditorView[]>([])
  const callbacks = useRef({ onChange, onSave }); callbacks.current = { onChange, onSave }
  const initial = useRef({ local, incoming, value })
  const [pane, setPane] = useState(1), [linked, setLinked] = useState(true)
  const linking = useRef(true), scrolling = useRef(false)
  linking.current = linked
  const editable = useRef(new Compartment())
  useEffect(() => {
    let disposed = false, frame = 0
    const data = [initial.current.local, initial.current.value, initial.current.incoming]
    const labels = ['本地 · 当前分支', '合并结果 · 可编辑', '远端 · 上游分支']
    const editors = data.map((doc, index) => {
      const language = new Compartment()
      const extensions = [basicSetup, colors, language.of([]), highlights,
        keymap.of([...defaultKeymap, ...historyKeymap, { key: 'Mod-s', preventDefault: true, run: () => { if (index === 1) callbacks.current.onSave(); return true } }]),
        EditorView.contentAttributes.of({ 'aria-label': `${labels[index]} ${path}`, spellcheck: 'false' }),
        EditorView.updateListener.of(update => { if (index === 1 && update.docChanged) callbacks.current.onChange(update.state.doc.toString()) }),
        EditorView.domEventHandlers({ scroll: (_event, view) => {
          if (!linking.current || scrolling.current) return false
          scrolling.current = true
          const fraction = view.scrollDOM.scrollTop / Math.max(1, view.scrollDOM.scrollHeight - view.scrollDOM.clientHeight)
          for (const other of views.current) if (other !== view) other.scrollDOM.scrollTop = fraction * (other.scrollDOM.scrollHeight - other.scrollDOM.clientHeight)
          frame = requestAnimationFrame(() => { scrolling.current = false }); return false
        } }),
        EditorView.theme({
          '&': { height: '100%', backgroundColor: 'var(--ws-bg)', color: 'var(--ws-ink)', fontSize: '12px' },
          '.cm-scroller': { overflow: 'auto', fontFamily: 'var(--font-mono)', lineHeight: '1.7' },
          '.cm-gutters': { backgroundColor: 'var(--ws-bg)', color: 'var(--ws-dim)', borderColor: 'var(--ws-border)' },
          '.cm-content': { padding: '8px 0' }, '.cm-cursor': { borderLeftColor: 'var(--ws-ink)' },
          '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'var(--ws-accent-bg) !important' },
          '.merge-conflict-line': { backgroundColor: 'color-mix(in srgb,var(--tree-orange) 13%,transparent)' },
          '&.cm-focused': { outline: 'none' },
        }),
      ]
      const readOnly = [EditorState.readOnly.of(index !== 1 || disabled), EditorView.editable.of(index === 1 && !disabled)]
      const editor = new EditorView({ parent: hosts.current[index]!, doc, extensions: [...extensions, index === 1 ? editable.current.of(readOnly) : readOnly] })
      if (doc.length < 300000 && !doc.split('\n').some(line => line.length > 20000)) void loadEditorLanguage(path).then(extension => { if (!disposed) editor.dispatch({ effects: language.reconfigure(extension) }) }).catch(() => {})
      return editor
    })
    views.current = editors
    const observer = new ResizeObserver(() => editors.forEach(view => view.requestMeasure()))
    hosts.current.forEach(host => { if (host) observer.observe(host) })
    return () => { disposed = true; cancelAnimationFrame(frame); observer.disconnect(); editors.forEach(view => view.destroy()); views.current = [] }
  }, [path])
  useEffect(() => {
    const editor = views.current[1]
    if (editor && editor.state.doc.toString() !== value) editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value } })
  }, [value])
  useEffect(() => { views.current[1]?.dispatch({ effects: editable.current.reconfigure([EditorState.readOnly.of(disabled), EditorView.editable.of(!disabled)]) }) }, [disabled])
  useEffect(() => {
    views.current[1]?.dispatch({ effects: highlight.of(ranges) })
    for (const index of [0, 2]) {
      const editor = views.current[index]
      if (!editor) continue
      const content = editor.state.doc.toString(), snippet = index === 0 ? selected?.local : selected?.incoming
      const at = snippet ? content.indexOf(snippet) : -1
      // Never guess a source location when identical code appears in multiple places.
      const unique = at >= 0 && !!snippet && content.indexOf(snippet, at + 1) < 0
      editor.dispatch({ effects: highlight.of(unique ? [{ from: at, to: at + snippet!.length }] : []) })
    }
  }, [ranges, selected, value.length])
  useEffect(() => {
    const editor = views.current[1]
    if (selected && editor) { const anchor = Math.min(selected.from, editor.state.doc.length); editor.dispatch({ effects: EditorView.scrollIntoView(anchor, { y: 'center' }), selection: { anchor } }) }
  }, [selected?.from, selected?.to])
  return <div className="merge-editor">
    <div className="merge-editor-options"><div className="merge-pane-switch" role="group" aria-label="合并面板">{['本地', '结果', '远端'].map((label, index) => <button key={label} aria-pressed={pane === index} onClick={() => setPane(index)}>{label}</button>)}</div><label><input type="checkbox" checked={linked} onChange={event => setLinked(event.target.checked)} />关联滚动</label></div>
    <div className="merge-columns">{['本地 · 当前分支', '合并结果 · 可编辑', '远端 · 上游分支'].map((label, index) => <section key={label} className={`merge-column ${pane === index ? 'is-selected' : ''}`}><h3>{label}</h3><div className="merge-code" ref={host => { hosts.current[index] = host }} /></section>)}</div>
  </div>
}
