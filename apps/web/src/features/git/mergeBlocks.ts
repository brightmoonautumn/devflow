export type MergeBlock = { from: number; to: number; local: string; incoming: string }

/** Git generates the merge result; this parser only locates its unresolved blocks.
 * Supports merge/diff3/zdiff3 and custom conflict-marker-size attributes. */
export function mergeBlocks(content: string): MergeBlock[] {
  const lines = content.match(/[^\n]*\n|[^\n]+$/g) ?? []
  const blocks: MergeBlock[] = []
  let offset = 0, from = -1, size = 0, side: 'local' | 'base' | 'incoming' = 'local', local = '', incoming = ''
  for (const line of lines) {
    const start = /^(<{7,})(?: |\r?$)/.exec(line.replace(/\n$/, ''))
    if (from < 0 && start) { from = offset; size = start[1].length; side = 'local'; local = ''; incoming = '' }
    else if (from >= 0) {
      const clean = line.replace(/\r?\n$/, '')
      if (clean.startsWith('|'.repeat(size)) && side === 'local') side = 'base'
      else if (clean === '='.repeat(size)) side = 'incoming'
      else if (clean.startsWith('>'.repeat(size)) && side === 'incoming') { blocks.push({ from, to: offset + line.length, local, incoming }); from = -1 }
      else if (side === 'local') local += line
      else if (side === 'incoming') incoming += line
    }
    offset += line.length
  }
  return blocks
}
export const hasMergeMarkers = (content: string) => /^(?:<{7,}|={7,}|>{7,}|\|{7,})/m.test(content)
