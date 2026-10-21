import { useEffect, useRef, useState } from 'react'
import { createPortal } from 'react-dom'
import { basicSetup } from 'codemirror'
import { Compartment, EditorState, Prec } from '@codemirror/state'
import { EditorView, GutterMarker, gutter, keymap } from '@codemirror/view'
import { indentWithTab } from '@codemirror/commands'
import { HighlightStyle, syntaxHighlighting } from '@codemirror/language'
import { tags } from '@lezer/highlight'
import { loadEditorLanguage } from './editorLanguages'
import { editorPhrases } from './editorPhrases'
import { editorSearch } from './editorSearch'
import { editorSelection } from './editorSelection'
import './source-editor.css'

export const colors = syntaxHighlighting(HighlightStyle.define([
  { tag: [tags.keyword, tags.modifier], color: 'var(--tree-purple)' },
  { tag: [tags.string, tags.attributeValue], color: 'var(--tree-green)' },
  { tag: [tags.number, tags.bool, tags.null], color: 'var(--tree-orange)' },
  { tag: [tags.comment, tags.meta], color: 'var(--ws-dim)' },
  { tag: [tags.typeName, tags.className, tags.tagName], color: 'var(--tree-blue)' },
  { tag: [tags.attributeName, tags.propertyName], color: 'var(--tree-teal)' },
  { tag: tags.invalid, color: 'var(--ws-error)' },
  { tag: tags.annotation, color: 'var(--tree-orange)' },
  { tag: [tags.labelName, tags.url], color: 'var(--tree-blue)' },
  { tag: [tags.operator, tags.punctuation], color: 'var(--ws-muted)' },
  { tag: [tags.variableName, tags.name], color: 'var(--ws-ink)' },
]))

export type LineAnnotation = { from: number; to: number; label: string; id?: string }
class AuthorMarker extends GutterMarker {
  constructor(readonly annotation: LineAnnotation, readonly select: (id: string) => void) { super() }
  eq(other: AuthorMarker) { return this.annotation.label === other.annotation.label && this.annotation.id === other.annotation.id }
  toDOM() {
    const element = document.createElement(this.annotation.id ? 'button' : 'span')
    element.textContent = this.annotation.label
    element.className = 'df-line-author'
    if (element instanceof HTMLButtonElement) {
      element.type = 'button'
      element.title = `${this.annotation.label} · 点击查看 Git History 提交详情`
      element.addEventListener('click', event => { event.preventDefault(); event.stopPropagation(); this.select(this.annotation.id!) })
    }
    return element
  }
}
export type SourceEditorProps = { annotations?: LineAnnotation[]; annotationLoading?: boolean; onToggleAnnotations?: () => void; onSelectAnnotation?: (id: string) => void; path: string; value: string; readonly: boolean; location?: { line: number; column: number; sequence: number }; onChange: (value: string) => void; onSave: () => void; onPosition: (line: number, column: number) => void }

/** Public editor boundary: feature code never depends on CodeMirror state or DOM APIs. */
export default function SourceEditor({ path, value, readonly, location, onChange, onSave, onPosition, annotations, annotationLoading, onToggleAnnotations, onSelectAnnotation }: SourceEditorProps) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const editable = useRef(new Compartment())
  const attribution = useRef(new Compartment())
  const [menu, setMenu] = useState<{ x: number; y: number } | null>(null)
  useEffect(() => { if (!onToggleAnnotations) setMenu(null) }, [onToggleAnnotations])
  const annotationCallbacks = useRef({ onToggleAnnotations, onSelectAnnotation })
  annotationCallbacks.current = { onToggleAnnotations, onSelectAnnotation }
  useEffect(() => {
    if (!menu) return
    const close = () => setMenu(null)
    const keyboard = (event: KeyboardEvent) => { if (event.key === 'Escape') close() }
    window.addEventListener('pointerdown', close); window.addEventListener('blur', close); window.addEventListener('resize', close); window.addEventListener('keydown', keyboard)
    return () => { window.removeEventListener('pointerdown', close); window.removeEventListener('blur', close); window.removeEventListener('resize', close); window.removeEventListener('keydown', keyboard) }
  }, [menu])
  const callbacks = useRef({ onChange, onSave, onPosition })
  callbacks.current = { onChange, onSave, onPosition }
  const initial = useRef(value)
  useEffect(() => {
    if (!host.current) return
    const language = new Compartment()
    let disposed = false
    const editor = new EditorView({
      parent: host.current,
      doc: initial.current,
      extensions: [
        basicSetup, editorPhrases, editorSearch, editorSelection, colors, language.of([]), attribution.current.of([]),
        EditorView.domEventHandlers({ scroll: () => { setMenu(null); return false } }),
        /\.(?:md|markdown)$/i.test(path) ? EditorView.lineWrapping : [],
        EditorState.tabSize.of(2),
        editable.current.of([EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)]),
        EditorView.contentAttributes.of({ 'aria-label': `编辑 ${path}`, spellcheck: 'false' }),
        keymap.of([{ key: 'Mod-s', preventDefault: true, run: () => { callbacks.current.onSave(); return true } }, indentWithTab]),
        EditorView.updateListener.of(update => {
          if (update.docChanged) callbacks.current.onChange(update.state.doc.toString())
          if (update.selectionSet || update.docChanged) {
            const position = update.state.selection.main.head
            const line = update.state.doc.lineAt(position)
            callbacks.current.onPosition(line.number, position - line.from + 1)
          }
        }),
        EditorView.theme({
          '&': { height: '100%', backgroundColor: 'var(--ws-bg)', color: 'var(--ws-ink)', fontSize: '13px' },
          '.cm-scroller': { fontFamily: 'var(--font-mono)', lineHeight: '1.7', overflow: 'auto' },
          '.cm-content': { padding: '12px 0', caretColor: 'var(--ws-ink)' },
          '.cm-gutters': { backgroundColor: 'var(--ws-bg)', color: 'var(--ws-dim)', borderRight: '1px solid var(--ws-subtle-border)' },
          '.cm-activeLine, .cm-activeLineGutter': { backgroundColor: 'var(--ws-hover)' },
          '.df-author-gutter': { backgroundColor: 'var(--ws-bg)' },
          '.df-author-gutter .cm-gutterElement': { padding: '0 7px', maxWidth: '200px', overflow: 'hidden' },
          '.df-line-author': { font: 'inherit', color: 'var(--ws-dim)', background: 'none', border: '0', padding: '0', whiteSpace: 'pre', maxWidth: '200px', overflow: 'hidden', textOverflow: 'ellipsis', verticalAlign: 'top' },
          'button.df-line-author': { cursor: 'pointer' },
          'button.df-line-author:hover, button.df-line-author:focus-visible': { color: 'var(--ws-ink)', textDecoration: 'underline' },
          '.cm-cursor': { borderLeftColor: 'var(--ws-ink)' },
          '.cm-panels, .cm-tooltip': { backgroundColor: 'var(--ws-surface)', color: 'var(--ws-ink)', borderColor: 'var(--ws-border)' },
          '&.cm-focused': { outline: 'none' },
        }),
      ],
    })
    view.current = editor
    void loadEditorLanguage(path).then(extension => {
      if (!disposed) editor.dispatch({ effects: language.reconfigure(extension) })
    }).catch(() => { /* Keep plain text editing available if a language chunk cannot load. */ })
    return () => { disposed = true; view.current = null; editor.destroy() }
    // Each tab keeps its editor instance, undo stack and scroll position until closed.
  }, [path])
  useEffect(() => {
    const editor = view.current
    if (editor && editor.state.doc.toString() !== value) {
      const selection = editor.state.selection.main
      const top = editor.scrollDOM.scrollTop, left = editor.scrollDOM.scrollLeft
      editor.dispatch({ changes: { from: 0, to: editor.state.doc.length, insert: value }, selection: { anchor: Math.min(selection.anchor, value.length), head: Math.min(selection.head, value.length) } })
      editor.scrollDOM.scrollTop = top; editor.scrollDOM.scrollLeft = left
    }
  }, [value])
  useEffect(() => {
    view.current?.dispatch({ effects: editable.current.reconfigure([EditorState.readOnly.of(readonly), EditorView.editable.of(!readonly)]) })
  }, [readonly])
  useEffect(() => {
    const editor = view.current
    if (!editor || !location) return
    const line = editor.state.doc.line(Math.max(1, Math.min(location.line, editor.state.doc.lines)))
    const anchor = Math.min(line.to, line.from + Math.max(0, location.column - 1))
    editor.dispatch({ selection: { anchor }, effects: EditorView.scrollIntoView(anchor, { y: 'center' }) })
    editor.focus()
  }, [location])
  useEffect(() => {
    const editor = view.current
    if (!editor) return
    const snapshot = editor.state.doc
    const ranges = annotations ?? []
    const markers = ranges.map(range => new AuthorMarker(range, id => annotationCallbacks.current.onSelectAnnotation?.(id)))
    editor.dispatch({ effects: attribution.current.reconfigure(annotations ? Prec.high(gutter({
      class: 'df-author-gutter',
      lineMarker: (current, line) => {
        if (!current.state.doc.eq(snapshot)) return null
        const number = current.state.doc.lineAt(line.from).number
        let low = 0, high = ranges.length - 1
        while (low <= high) {
          const middle = (low + high) >>> 1, range = ranges[middle]
          if (number < range.from) high = middle - 1
          else if (number > range.to) low = middle + 1
          else return markers[middle]
        }
        return null
      },
      lineMarkerChange: update => update.docChanged,
    })) : []) })
  }, [annotations])
  return <><div ref={host} className="df-source-editor" onContextMenu={event => {
    if (!annotationCallbacks.current.onToggleAnnotations || !(event.target instanceof Element) || !event.target.closest('.cm-gutters')) return
    event.preventDefault()
    setMenu({ x: Math.max(0, Math.min(event.clientX, window.innerWidth - 210)), y: Math.max(0, Math.min(event.clientY, window.innerHeight - 50)) })
  }} />{menu && onToggleAnnotations && createPortal(<div role="menu" aria-label="行提交信息" className="df-source-context-menu" style={{ left: menu.x, top: menu.y }} onPointerDown={event => event.stopPropagation()}>
    <button autoFocus role="menuitem" onClick={() => { setMenu(null); annotationCallbacks.current.onToggleAnnotations?.() }}>{annotationLoading ? '取消读取行提交信息' : annotations ? '隐藏行提交信息' : '显示行提交信息'}</button>
  </div>, host.current?.closest('.workspace-shell') ?? document.body)}</>

}
