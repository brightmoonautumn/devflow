import { lazy } from 'react'
import type { SourceEditorProps } from './SourceEditor'
import type { MarkdownImageContext } from './markdownImages'

const SourceEditor = lazy(() => import('./SourceEditor'))
const MarkdownEditor = lazy(() => import('./MarkdownEditor'))

/** Keep format-specific implementations behind the application's editor boundary. */
export default function FileEditor(props: SourceEditorProps & MarkdownImageContext & { theme: 'dark' | 'light'; active?: boolean; onOpenFile?: (path: string) => void; toolbarContainer?: HTMLElement | null; viewStateKey?: string }) {
  const Editor = /\.(?:md|markdown)$/i.test(props.path) ? MarkdownEditor : SourceEditor
  return <Editor {...props} />
}
