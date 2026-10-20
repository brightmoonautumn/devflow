import { useEffect, useRef, useState } from 'react'
import { basicSetup } from 'codemirror'
import { Compartment, EditorState } from '@codemirror/state'
import { EditorView } from '@codemirror/view'
import { MergeView, unifiedMergeView } from '@codemirror/merge'
import { colors } from './SourceEditor'
import { loadEditorLanguage } from './editorLanguages'

export default function GitDiffEditor({ path, before, after }: { path: string; before: string; after: string }) {
  const host = useRef<HTMLDivElement>(null)
  const [wide, setWide] = useState(false)
  useEffect(() => {
    if (!host.current) return
    const observer = new ResizeObserver(entries => setWide(entries[0].contentRect.width >= 680))
    observer.observe(host.current)
    return () => observer.disconnect()
  }, [])
  useEffect(() => {
    if (!host.current) return
    let disposed = false
    const a = new Compartment(), b = new Compartment()
    const theme = EditorView.theme({
      '&': { height: '100%', color: 'var(--ws-ink)', backgroundColor: 'var(--ws-bg)', fontSize: '12px' },
      '.cm-scroller': { overflow: 'auto', fontFamily: 'var(--font-mono)' },
      '.cm-gutters': { color: 'var(--ws-dim)', backgroundColor: 'var(--ws-bg)', borderColor: 'var(--ws-border)' },
      '&.cm-focused': { outline: 'none' },
    })
    const extensions = [basicSetup, colors, theme, EditorState.readOnly.of(true), EditorView.editable.of(false)]
    const view = wide ? new MergeView({ parent: host.current, a: { doc: before, extensions: [...extensions, a.of([])] }, b: { doc: after, extensions: [...extensions, b.of([])] }, highlightChanges: true, gutter: true })
      : new EditorView({ parent: host.current, doc: after, extensions: [...extensions, b.of([]), unifiedMergeView({ original: before, mergeControls: false, highlightChanges: true, gutter: true })] })
    void loadEditorLanguage(path).then(language => {
      if (disposed) return
      if (view instanceof MergeView) { view.a.dispatch({ effects: a.reconfigure(language) }); view.b.dispatch({ effects: b.reconfigure(language) }) }
      else view.dispatch({ effects: b.reconfigure(language) })
    }).catch(() => { /* Unknown languages remain plain text. */ })
    return () => { disposed = true; view.destroy() }
  }, [path, before, after, wide])
  return <div ref={host} className="git-diff-editor" aria-label={`只读比较 ${path}`} />
}
