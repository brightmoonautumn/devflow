import { EditorView } from '@codemirror/view'

/** Active-line backgrounds sit above drawSelection's layer. Hide them while
 * any range selects text so the head line cannot obscure the selection. */
export const editorSelection = [
  EditorView.editorAttributes.compute(['selection'], state => ({
    class: state.selection.ranges.some(range => !range.empty) ? 'df-has-selection' : '',
  })),
  EditorView.theme({
    '&.df-has-selection .cm-activeLine': { backgroundColor: 'transparent' },
    '.cm-selectionBackground, &.cm-focused .cm-selectionBackground': { backgroundColor: 'var(--ws-editor-selection) !important' },
    // Occurrence marks live above the selection layer. Keep them transparent
    // so syntax colors and any underlying selection remain readable.
    '.cm-selectionMatch': {
      backgroundColor: 'transparent',
      boxShadow: 'inset 0 0 0 1px color-mix(in srgb, var(--ws-accent) 55%, transparent)',
    },
  }),
]
