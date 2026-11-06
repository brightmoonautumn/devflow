import { jsonLanguage } from '@codemirror/lang-json'
import { classHighlighter, highlightTree } from '@lezer/highlight'
import type { ReactNode } from 'react'

// Format tokens without converting numbers through JavaScript's floating-point type.
export function formatQaJson(input: string): string | null {
  try { JSON.parse(input) } catch { return null }
  let result = '', depth = 0, inString = false, escaped = false, previous = ''
  const indent = () => '  '.repeat(depth)
  for (let i = 0; i < input.length; i++) {
    const char = input[i]
    if (inString) {
      result += char
      if (escaped) escaped = false
      else if (char === '\\') escaped = true
      else if (char === '"') { inString = false; previous = char }
      continue
    }
    if (char === '"') { inString = true; result += char }
    else if (char === '{' || char === '[') {
      result += char
      let nextIndex = i + 1
      while (nextIndex < input.length && /\s/.test(input[nextIndex])) nextIndex++
      const next = input[nextIndex]
      if (next !== '}' && next !== ']') { depth++; result += `\n${indent()}` } else depth++
    } else if (char === '}' || char === ']') {
      depth--
      if (previous !== '{' && previous !== '[') result += `\n${indent()}`
      result += char
    } else if (char === ',') result += `,\n${indent()}`
    else if (char === ':') result += ': '
    else if (!/\s/.test(char)) result += char
    if (!/\s/.test(char)) previous = char
  }
  return result
}

export function QaJson({ value }: { value: string }) {
  const tree = jsonLanguage.parser.parse(value)
  const pieces: ReactNode[] = []
  let position = 0
  highlightTree(tree, classHighlighter, (from, to, classes) => {
    if (from > position) pieces.push(value.slice(position, from))
    pieces.push(<span key={from} className={classes}>{value.slice(from, to)}</span>)
    position = to
  })
  if (position < value.length) pieces.push(value.slice(position))
  return <pre className="qa-json"><code>{pieces}</code></pre>
}

export function QaCurl({ value }: { value: string }) {
  const tokens = /'[^']*'|"(?:\\.|[^"\\])*"|--?[a-zA-Z][\w-]*|\bcurl\b/g
  const pieces: ReactNode[] = []
  let position = 0
  for (const match of value.matchAll(tokens)) {
    const from = match.index
    if (from > position) pieces.push(value.slice(position, from))
    const token = match[0]
    const kind = token[0] === "'" || token[0] === '"' ? 'string' : token === 'curl' ? 'command' : 'flag'
    pieces.push(<span key={from} className={`qa-curl-${kind}`}>{token}</span>)
    position = from + token.length
  }
  if (position < value.length) pieces.push(value.slice(position))
  return <pre className="qa-curl"><code>{pieces}</code></pre>
}
