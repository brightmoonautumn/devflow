export type IdeaNode = {
  id: string; title: string; body: string; kind: 'note' | 'group'; x: number; y: number
  parentId: string | null; groupId: string | null; collapsed: boolean; image: string; html: string
}
export type IdeaEdge = { id: string; source: string; target: string; label: string }
export type IdeaCanvas = { id: string; name: string; nodes: IdeaNode[]; edges: IdeaEdge[]; updatedAt: number }
export type IdeasSnapshot = { workspaceKey: string; revision: number; canvases: IdeaCanvas[] }
export type IdeasOperation = { type: 'canvas_save'; canvas: IdeaCanvas } | { type: 'canvas_delete'; id: string }
export const nodeWidth = 228
export function nodeHeight(node: IdeaNode) { return 70 + (node.body ? 42 : 0) + (node.image || node.html ? 120 : 0) }
export function makeNode(x: number, y: number, extra: Partial<IdeaNode> = {}): IdeaNode {
  return { id: crypto.randomUUID(), title: '新节点', body: '', kind: 'note', x, y, parentId: null, groupId: null, collapsed: false, image: '', html: '', ...extra }
}
export function descendants(canvas: IdeaCanvas, id: string): Set<string> {
  const ids = new Set([id])
  let size = 0
  while (size !== ids.size) {
    size = ids.size
    canvas.nodes.forEach(n => { if ((n.parentId && ids.has(n.parentId)) || (n.groupId && ids.has(n.groupId))) ids.add(n.id) })
  }
  return ids
}
export function visibleNodes(canvas: IdeaCanvas) {
  const hidden = new Set<string>()
  canvas.nodes.filter(n => n.collapsed).forEach(n => descendants(canvas, n.id).forEach(id => { if (id !== n.id) hidden.add(id) }))
  return canvas.nodes.filter(n => !hidden.has(n.id))
}
export function removeNode(canvas: IdeaCanvas, id: string): IdeaCanvas {
  return { ...canvas, nodes: canvas.nodes.filter(n => n.id !== id).map(n => ({ ...n, parentId: n.parentId === id ? null : n.parentId, groupId: n.groupId === id ? null : n.groupId })), edges: canvas.edges.filter(e => e.source !== id && e.target !== id) }
}
// Tidy only a selected subtree (or the selected group's roots); free siblings stay put.
export function layoutBranch(canvas: IdeaCanvas, rootId: string): IdeaCanvas {
  const positions = new Map<string, { x: number; y: number }>()
  const root = canvas.nodes.find(n => n.id === rootId)
  if (!root) return canvas
  const children = (id: string) => canvas.nodes.filter(n => n.parentId === id && n.kind !== 'group')
  const height = (node: IdeaNode): number => node.collapsed ? nodeHeight(node) : Math.max(nodeHeight(node), children(node.id).reduce((sum, n) => sum + height(n) + 32, -32))
  const place = (node: IdeaNode, x: number, top: number) => {
    const h = height(node)
    positions.set(node.id, { x, y: top + (h - nodeHeight(node)) / 2 })
    if (node.collapsed) return
    let y = top
    children(node.id).forEach(n => { place(n, x + 318, y); y += height(n) + 32 })
  }
  if (root.kind === 'group') {
    let y = root.y + 64
    canvas.nodes.filter(n => n.groupId === rootId && !n.parentId && n.kind !== 'group').forEach(n => { place(n, root.x + 32, y); y += height(n) + 48 })
  } else place(root, root.x, root.y - (height(root) - nodeHeight(root)) / 2)
  return { ...canvas, nodes: canvas.nodes.map(n => positions.has(n.id) ? { ...n, ...positions.get(n.id)! } : n) }
}
export function groupRect(canvas: IdeaCanvas, group: IdeaNode) {
  const ids = descendants(canvas, group.id)
  const nodes = visibleNodes(canvas).filter(n => ids.has(n.id) && n.id !== group.id && n.kind !== 'group')
  if (!nodes.length || group.collapsed) return { x: group.x, y: group.y, width: 320, height: group.collapsed ? 60 : 220 }
  const x = Math.min(group.x, ...nodes.map(n => n.x - 28))
  const y = Math.min(group.y, ...nodes.map(n => n.y - 56))
  return { x, y, width: Math.max(320, ...nodes.map(n => n.x + nodeWidth + 28 - x)), height: Math.max(220, ...nodes.map(n => n.y + nodeHeight(n) + 28 - y)) }
}
export function prototypeDocument(html: string) {
  // Opaque origin and restrictive resource CSP; inline JS supports local interactions.
  return `<!doctype html><html><head><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="default-src 'none'; script-src 'unsafe-inline'; style-src 'unsafe-inline'; img-src data:; font-src data:; connect-src 'none'; frame-src 'none'; form-action 'none'; base-uri 'none'"><meta name="viewport" content="width=device-width,initial-scale=1"></head><body>${html}</body></html>`
}
