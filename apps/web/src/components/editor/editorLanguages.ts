import { LanguageDescription, StreamLanguage } from '@codemirror/language'
import { languages } from '@codemirror/language-data'
import type { Extension } from '@codemirror/state'
import { tags } from '@lezer/highlight'

const logLanguage = StreamLanguage.define({
  name: 'Log',
  tokenTable: { error: tags.invalid, warning: tags.annotation, info: tags.labelName },
  token(stream) {
    if (stream.eatSpace()) return null
    if (stream.match(/^\[(?=(?:(?:ERROR|FATAL|CRITICAL|SEVERE|WARN|WARNING|INFO|DEBUG|TRACE|VERBOSE)\b|\d{4}[-/]\d{2}[-/]\d{2}|\d{2}:\d{2}:\d{2}))/i)) return null
    if (stream.match(/^(?:\d{4}[-/]\d{2}[-/]\d{2}[T ]\d{2}:\d{2}:\d{2}(?:[.,]\d+)?(?:Z|[+-]\d{2}:?\d{2})?|\d{2}:\d{2}:\d{2}(?:[.,]\d+)?)/)) return 'number'
    if (stream.match(/^(?:ERROR|FATAL|CRITICAL|SEVERE)\b/i)) return 'error'
    if (stream.match(/^(?:WARN|WARNING)\b/i)) return 'warning'
    if (stream.match(/^INFO\b/i)) return 'info'
    if (stream.match(/^(?:DEBUG|TRACE|VERBOSE)\b/i)) return 'comment'
    if (stream.match(/^(?:Caused by:|Suppressed:|Traceback\b|[\w.$]+(?:Exception|Error)\b)/)) return 'error'
    if (stream.match(/^at\s+[\w.$<>]+\([^)]*\)/)) return 'typeName'
    if (stream.match(/^https?:\/\/[^\s<>"']+/)) return 'url'
    if (stream.match(/^"(?:[^"\\]|\\.)*"|^'(?:[^'\\]|\\.)*'/)) return 'string'
    if (stream.match(/^\[[^\]\r\n]*\]/)) return 'meta'
    if (stream.match(/^\b\d+(?:\.\d+)*\b/)) return 'number'
    // Consume words together so level names inside identifiers aren't highlighted.
    if (!stream.match(/^[\w.$/-]+/)) stream.next()
    return null
  },
})

/** Match the basename on Windows and Unix; language parsers load only when opened. */
export async function loadEditorLanguage(path: string): Promise<Extension> {
  const name = path.split(/[\\/]/).at(-1) ?? path
  const lower = name.toLowerCase()
  if (/\.(?:log|out|err)(?:[.-].*)?$/.test(lower)) return logLanguage

  // Aliases cover common configuration files and shell startup files without extensions.
  const alias = lower === '.env' || lower.startsWith('.env.') ? 'settings.properties'
    : /^(?:dockerfile(?:\..+)?|.+\.dockerfile)$/.test(lower) ? 'Dockerfile'
    : /^(?:\.?(?:bashrc|bash_profile|bash_login|zshrc|zprofile|profile)|.+\.zsh)$/.test(lower) ? 'script.sh'
    : /\.(?:jsonc|json5|code-workspace)$/.test(lower) ? 'settings.json'
    : /\.(?:mts|cts)$/.test(lower) ? 'module.ts'
    : /\.(?:pyi|pyw)$/.test(lower) ? 'module.py'
    : /\.(?:svg|pom|xsd|xsl|xslt|wsdl)$/.test(lower) ? 'document.xml'
    : lower
  const description = LanguageDescription.matchFilename(languages, alias) ?? LanguageDescription.matchFilename(languages, name)
  return description ? description.load() : []
}
