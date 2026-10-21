import { useEffect, useRef } from 'react'
import { basicSetup } from 'codemirror'
import { Compartment, EditorState } from '@codemirror/state'
import { EditorView, keymap } from '@codemirror/view'
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

export type SourceEditorProps = { path: string; value: string; readonly: boolean; location?: { line: number; column: number; sequence: number }; onChange: (value: string) => void; onSave: () => void; onPosition: (line: number, column: number) => void }

/** Public editor boundary: feature code never depends on CodeMirror state or DOM APIs. */
export default function SourceEditor({ path, value, readonly, location, onChange, onSave, onPosition }: SourceEditorProps) {
  const host = useRef<HTMLDivElement>(null)
  const view = useRef<EditorView | null>(null)
  const editable = useRef(new Compartment())
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
        basicSetup, editorPhrases, editorSearch, editorSelection, colors, language.of([]),
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
  return <div ref={host} className="df-source-editor" />
}
