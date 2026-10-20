import type { JavaLogLine } from '../../lib/api'

type LogLevel = 'normal' | 'info' | 'warning' | 'error' | 'debug' | 'trace' | 'success' | 'system' | 'arthas'
type LogPart = { text: string; tone: string; href?: string }
const levels: Record<string, LogLevel> = { ERROR: 'error', FATAL: 'error', WARN: 'warning', WARNING: 'warning', INFO: 'info', DEBUG: 'debug', TRACE: 'trace' }

/** Keep log text intact, including punctuation around a URL. Only web URLs become links. */
function linkLogPart(part: LogPart): LogPart[] {
  const result: LogPart[] = []
  let cursor = 0
  for (const match of part.text.matchAll(/\bhttps?:\/\/[^\s<>"'`]+/gi)) {
    let address = match[0].replace(/[.,;:!?，。；：！？、]+$/, '')
    // A sentence's closing bracket is not part of its URL; balanced URL brackets are.
    while (/[)\]}]$/.test(address)) {
      const closing = address.at(-1)!
      const opening = closing === ')' ? '(' : closing === ']' ? '[' : '{'
      if (address.split(closing).length <= address.split(opening).length) break
      address = address.slice(0, -1).replace(/[.,;:!?，。；：！？、]+$/, '')
    }
    try {
      const url = new URL(address)
      if (!['http:', 'https:'].includes(url.protocol) || !url.hostname) continue
    } catch { continue }
    const start = match.index!
    if (start > cursor) result.push({ text: part.text.slice(cursor, start), tone: part.tone })
    result.push({ text: address, tone: part.tone, href: address })
    cursor = start + address.length
  }
  if (cursor < part.text.length) result.push({ text: part.text.slice(cursor), tone: part.tone })
  return result.length ? result : [part]
}

/** Preserve every character while coloring the fields of Spring Boot's console format. */
function splitLogParts(text: string, level: LogLevel, levelMatch: RegExpMatchArray | null): LogPart[] {
  const spring = text.match(/^(\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?)(\s+)(ERROR|FATAL|WARN(?:ING)?|INFO|DEBUG|TRACE)(\s+)(\d+)(\s+---\s+)((?:\[[^\]\r\n]*\]\s*)*)([\w.$/]+)(\s*:\s?)(.*)$/i)
  const messageTone = ['warning', 'error', 'success'].includes(level) ? level : 'normal'
  if (spring) return [
    { text: spring[1], tone: 'timestamp' },
    { text: spring[2], tone: 'normal' },
    { text: spring[3], tone: levels[spring[3].toUpperCase()] },
    { text: spring[4], tone: 'normal' },
    { text: spring[5], tone: 'pid' },
    { text: spring[6] + spring[7], tone: 'context' },
    { text: spring[8], tone: 'logger' },
    { text: spring[9], tone: 'context' },
    { text: spring[10], tone: messageTone },
  ]
  if (levelMatch) {
    const start = (levelMatch.index ?? 0) + levelMatch[0].indexOf(levelMatch[1])
    const end = start + levelMatch[1].length
    return [
      { text: text.slice(0, start), tone: 'normal' },
      { text: text.slice(start, end), tone: levels[levelMatch[1].toUpperCase()] },
      { text: text.slice(end), tone: messageTone },
    ]
  }
  return [{ text, tone: level }]
}

/** Render terminal control sequences as plain text; never interpret log HTML. */
export function formatConsoleLines(lines: JavaLogLine[]) {
  let previousLevel: LogLevel = 'normal'
  return lines.map(line => {
    const text = line.text.replace(/\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
    const match = text.match(/(?:^|[\s[(|])(ERROR|FATAL|WARN(?:ING)?|INFO|DEBUG|TRACE)(?=$|[\s\])|:])/i)
    let level: LogLevel = match ? levels[match[1].toUpperCase()] : line.stream === 'stderr' ? 'error' : line.stream === 'system' ? 'system' : line.stream === 'arthas' ? 'arthas' : 'normal'
    if (/\bBUILD FAILURE\b|\b[\w.$]*(?:Exception|Error)(?::|$)/.test(text) || (line.stream === 'system' && /失败|超时|无法|错误|拒绝/.test(text))) level = 'error'
    else if (/\bBUILD SUCCESS\b|\bStarted .+ in [\d.]+ seconds\b|Arthas 已附加/.test(text)) level = 'success'
    else if (/^\s*(?:at\s|Caused by:|Suppressed:|\.\.\. \d+ (?:more|common frames omitted))/.test(text) && ['error', 'warning'].includes(previousLevel)) level = previousLevel
    if (text.trim()) previousLevel = level
    return { ...line, text, level, parts: splitLogParts(text, level, match).flatMap(linkLogPart) }
  })
}
