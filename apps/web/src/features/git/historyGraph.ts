import type { HistoryCommit } from './api'

type Lane = { id: string; color: number }
export type HistoryGraphRow = { column: number; color: number; width: number; edges: { from: number; to: number; color: number }[] }

/** A small topological lane layout; parents come from Git, never inferred by date. */
export function historyGraph(commits: HistoryCommit[]): HistoryGraphRow[] {
  const lanes: Lane[] = []
  let nextColor = 0
  return commits.map(commit => {
    let column = lanes.findIndex(lane => lane.id === commit.id)
    if (column < 0) { column = lanes.length; lanes.push({ id: commit.id, color: nextColor++ % 6 }) }
    const before = [...lanes], color = lanes[column].color
    lanes.splice(column, 1)
    commit.parents.forEach((id, index) => {
      if (!lanes.some(lane => lane.id === id)) lanes.splice(index === 0 ? Math.min(column, lanes.length) : lanes.length, 0, { id, color: index === 0 ? color : nextColor++ % 6 })
    })
    const edges = before.flatMap((lane, from) => {
      const destinations = from === column ? commit.parents : [lane.id]
      return destinations.flatMap(id => {
        const to = lanes.findIndex(item => item.id === id)
        return to < 0 ? [] : [{ from, to, color: from === column ? lanes[to].color : lane.color }]
      })
    })
    return { column, color, edges, width: Math.max(before.length, lanes.length, 1) }
  })
}
