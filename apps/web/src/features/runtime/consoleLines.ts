import type { JavaLogLine } from '../../lib/api'

type LogLevel = 'normal' | 'info' | 'warning' | 'error' | 'debug' | 'trace' | 'success' | 'system' | 'arthas'

/** Render terminal control sequences as plain text; never interpret log HTML. */
export function formatConsoleLines(lines: JavaLogLine[]) {
  let previousLevel: LogLevel = 'normal'
  return lines.map(line => {
    const text = line.text.replace(/\x1b\[[0-?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)/g, '')
    const match = text.match(/(?:^|[\s[(|])(ERROR|FATAL|WARN(?:ING)?|INFO|DEBUG|TRACE)(?=$|[\s\])|:])/i)
    const levels: Record<string, LogLevel> = { ERROR: 'error', FATAL: 'error', WARN: 'warning', WARNING: 'warning', INFO: 'info', DEBUG: 'debug', TRACE: 'trace' }
    let level: LogLevel = match ? levels[match[1].toUpperCase()] : line.stream === 'stderr' ? 'error' : line.stream === 'system' ? 'system' : line.stream === 'arthas' ? 'arthas' : 'normal'
    if (/\bBUILD FAILURE\b|\b[\w.$]*(?:Exception|Error)(?::|$)/.test(text) || (line.stream === 'system' && /失败|超时|无法|错误|拒绝/.test(text))) level = 'error'
    else if (/\bBUILD SUCCESS\b|\bStarted .+ in [\d.]+ seconds\b|Arthas 已附加/.test(text)) level = 'success'
    else if (/^\s*(?:at\s|Caused by:|Suppressed:|\.\.\. \d+ (?:more|common frames omitted))/.test(text) && ['error', 'warning'].includes(previousLevel)) level = previousLevel
    if (text.trim()) previousLevel = level
    return { ...line, text, level }
  })
}
