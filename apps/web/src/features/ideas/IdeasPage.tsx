import { useEffect, useRef, useState, type ButtonHTMLAttributes, type PointerEvent as ReactPointerEvent, type ReactNode } from 'react'
import { ArrowDownToLine, ArrowUpRight, Check, ChevronDown, ChevronRight, Crosshair, Expand, FileCode2, Focus, GitBranch, ImagePlus, LayoutTemplate, Link2, Minus, Pencil, Plus, Redo2, RefreshCw, Shapes, Trash2, Undo2, X } from 'lucide-react'
import { useDialogs } from '../../components/form/BaseDialog'
import { IdeasCanvasMenu } from './IdeasCanvasMenu'
import type { WorkspaceResponse } from '../../lib/api'
import { executeBoard, getBoard } from '../board/api'
import { descendants, groupRect, layoutBranch, makeNode, nodeHeight, nodeWidth, prototypeDocument, removeNode, visibleNodes, type IdeaCanvas, type IdeaNode } from './model'
import { useIdeas } from './useIdeas'
import './ideas.css'

type View = { x: number; y: number; zoom: number }
type Drag = { pointer: number; x: number; y: number; view: View; node?: string; moved: boolean }
function Tool({ label, children, ...props }: { label: string; children: ReactNode } & ButtonHTMLAttributes<HTMLButtonElement>) {
  return <button type="button" title={label} aria-label={label} {...props}>{children}</button>
}
function Modal({ title, children, onClose, large = false }: { title: string; children: ReactNode; onClose: () => void; large?: boolean }) {
  const ref = useRef<HTMLDialogElement>(null)
  useEffect(() => { const el = ref.current; el?.showModal(); return () => el?.close() }, [])
  return <dialog ref={ref} className={`ideas-modal ${large ? 'is-large' : ''}`} aria-label={title} onCancel={e => { e.preventDefault(); onClose() }}>
    <header><strong>{title}</strong><Tool label="关闭" onClick={onClose}><X size={17} /></Tool></header>{children}
  </dialog>
}
function download(canvas: IdeaCanvas) {
  const url = URL.createObjectURL(new Blob([JSON.stringify(canvas, null, 2)], { type: 'application/json;charset=utf-8' }))
  const a = document.createElement('a'); a.href = url; a.download = `${canvas.name.replace(/[<>:"/\\|?*]/g, '_')}.json`; a.click()
  setTimeout(() => URL.revokeObjectURL(url), 1000)
}
function NodeTextEditor({ node, disabled, onChange, onFinish }: { node: IdeaNode; disabled: boolean; onChange: (patch: Partial<IdeaNode>) => void; onFinish: () => void }) {
  return <div className="ideas-inline-editor" onPointerDown={e => e.stopPropagation()} onClick={e => e.stopPropagation()} onDoubleClick={e => e.stopPropagation()} onBlur={e => { if (!e.currentTarget.contains(e.relatedTarget as Node)) onFinish() }} onKeyDown={e => {
    e.stopPropagation()
    if (e.nativeEvent.isComposing) return
    if (e.key === 'Escape' || e.key === 'Enter' && (e.ctrlKey || e.metaKey || node.kind === 'group')) { e.preventDefault(); onFinish() }
  }}>
    <input autoFocus aria-label={node.kind === 'group' ? '分组名称' : '节点标题'} value={node.title} maxLength={240} disabled={disabled} onFocus={e => e.currentTarget.select()} onChange={e => onChange({ title: e.target.value })} />
    {node.kind !== 'group' && <textarea aria-label="节点简述" rows={3} value={node.body} placeholder="简述" maxLength={16000} disabled={disabled} onChange={e => onChange({ body: e.target.value })} />}
    <button type="button" disabled={disabled} onClick={onFinish} aria-label="完成编辑" title="完成编辑 · Ctrl+Enter"><Check size={13} /></button>
  </div>
}
export function IdeasPage({ workspace }: { workspace: WorkspaceResponse | null }) {
  const key = workspace?.workspaceFile ?? workspace?.workspaceRoot
  if (!key) return <section className="ideas-page ideas-no-workspace"><Shapes size={30} /><h2>构思</h2><p>打开工作区后创建画布</p></section>
  return <IdeasWorkspace key={key} workspaceKey={key} revision={workspace!.revision} />
}
function IdeasWorkspace({ workspaceKey, revision }: { workspaceKey: string; revision: number }) {
  const store = useIdeas(workspaceKey, revision)
  const { canvas, data, active, dirty, busy, conflict, error, changed } = store
  const dialogs = useDialogs()
  const [selected, setSelected] = useState(() => new URLSearchParams(window.location.hash.split('?')[1] ?? '').get('node') ?? '')
  const [editingId, setEditingId] = useState('')
  const [edgeId, setEdgeId] = useState('')
  const [linking, setLinking] = useState(false)
  const [view, setView] = useState<View>({ x: 120, y: 100, zoom: 1 })
  const [preview, setPreview] = useState<IdeaNode | null>(null)
  const [previewMedia, setPreviewMedia] = useState<'image' | 'html'>('image')
  const [publish, setPublish] = useState(false)
  const [publishing, setPublishing] = useState(false)
  const [notice, setNotice] = useState('')
  const [dragOffset, setDragOffset] = useState<{ ids: Set<string>; dx: number; dy: number } | null>(null)
  const stage = useRef<HTMLDivElement>(null), imageInput = useRef<HTMLInputElement>(null), htmlInput = useRef<HTMLInputElement>(null)
  const drag = useRef<Drag | null>(null), viewRef = useRef(view), canvasRef = useRef(canvas)
  viewRef.current = view; canvasRef.current = canvas
  const node = canvas?.nodes.find(n => n.id === selected)
  const edge = canvas?.edges.find(e => e.id === edgeId)
  const locked = conflict || publishing
  const nodes = canvas ? visibleNodes(canvas) : []
  const display = nodes.map(n => dragOffset?.ids.has(n.id) ? { ...n, x: n.x + dragOffset.dx, y: n.y + dragOffset.dy } : n)
  const displayCanvas = canvas ? { ...canvas, nodes: canvas.nodes.map(n => display.find(v => v.id === n.id) ?? n) } : null
  const selectedIds = canvas && node ? descendants(canvas, node.id) : new Set<string>()
  const renderHeight = (n: IdeaNode) => editingId === n.id ? Math.max(nodeHeight(n), 158 + (n.image || n.html ? 120 : 0)) : nodeHeight(n)
  const renderRect = (n: IdeaNode) => n.kind === 'group' && displayCanvas ? groupRect(displayCanvas, n) : { x: n.x, y: n.y, width: nodeWidth, height: renderHeight(n) }
  const selectedRect = node ? renderRect(node) : null

  useEffect(() => {
    if (!publish && !preview && !editingId) return
    store.interaction.current = true
    return () => { store.interaction.current = false }
  }, [publish, preview, editingId, store.interaction])

  useEffect(() => {
    setEditingId(''); setEdgeId(''); setLinking(false); setNotice(''); setView({ x: 120, y: 100, zoom: 1 })
  }, [active])
  useEffect(() => {
    if (!canvas?.nodes.length) return
    const linked = new URLSearchParams(window.location.hash.split('?')[1] ?? '').get('node')
    if (linked && canvas.nodes.some(n => n.id === linked)) {
      setSelected(linked)
      const parents = canvas.nodes.filter(n => n.collapsed && descendants(canvas, n.id).has(linked))
      if (parents.length) store.edit({ ...canvas, nodes: canvas.nodes.map(n => parents.some(p => p.id === n.id) ? { ...n, collapsed: false } : n) }, false)
      fit([linked])
    }
    else fit()
  }, [canvas?.id])
  useEffect(() => {
    const navigate = () => {
      if (window.location.hash.split('?')[0] !== '#ideas' || dirty || busy || conflict) return
      const query = new URLSearchParams(window.location.hash.split('?')[1] ?? ''), id = query.get('canvas'), target = query.get('node')
      if (id && id !== active) store.select(id, target ?? undefined)
      else if (target) { setSelected(target); fit([target]) }
    }
    window.addEventListener('hashchange', navigate)
    return () => window.removeEventListener('hashchange', navigate)
  })
  useEffect(() => {
    const guard = (e: BeforeUnloadEvent) => { if (dirty || busy) { e.preventDefault(); e.returnValue = '' } }
    const navigate = (e: MouseEvent) => {
      if ((dirty || busy) && e.target instanceof Element && e.target.closest('.activity-button')) {
        e.preventDefault(); e.stopPropagation(); setNotice(conflict ? '本地编辑尚未保存，请先导出或处理保存错误' : '正在保存画布，请稍后切换')
      }
    }
    const navigateApp = (e: Event) => {
      if (dirty || busy) { e.preventDefault(); setNotice(conflict ? '本地编辑尚未保存，请先导出或处理保存错误' : '正在保存画布，请稍后切换') }
    }
    window.addEventListener('beforeunload', guard); document.addEventListener('click', navigate, true)
    window.addEventListener('devflow-before-navigate', navigateApp)
    return () => { window.removeEventListener('beforeunload', guard); document.removeEventListener('click', navigate, true); window.removeEventListener('devflow-before-navigate', navigateApp) }
  }, [dirty, busy, conflict])
  useEffect(() => {
    const el = stage.current
    if (!el) return
    const wheel = (e: WheelEvent) => {
      if ((e.target as Element).closest('button,input,textarea,select')) return
      e.preventDefault()
      const rect = el.getBoundingClientRect(), old = viewRef.current
      if (!e.ctrlKey && !e.metaKey) { setView({ ...old, x: old.x - e.deltaX, y: old.y - e.deltaY }); return }
      const zoom = Math.max(.2, Math.min(2, old.zoom * Math.exp(-e.deltaY * .008)))
      const x = e.clientX - rect.left, y = e.clientY - rect.top
      setView({ x: x - (x - old.x) * zoom / old.zoom, y: y - (y - old.y) * zoom / old.zoom, zoom })
    }
    el.addEventListener('wheel', wheel, { passive: false })
    return () => el.removeEventListener('wheel', wheel)
  }, [!!canvas])
  useEffect(() => {
    const handle = (e: KeyboardEvent) => {
      if ((e.target as Element)?.closest('input,textarea,select,dialog')) return
      if ((e.ctrlKey || e.metaKey) && e.key.toLowerCase() === 'z') { e.preventDefault(); store.undo(e.shiftKey) }
      if (e.key === 'Escape') { setLinking(false); setEditingId(''); setEdgeId('') }
    }
    window.addEventListener('keydown', handle)
    return () => window.removeEventListener('keydown', handle)
  })

  function change(next: IdeaCanvas) { if (!locked) { store.edit(next); setNotice('') } }
  function updateNode(id: string, patch: Partial<IdeaNode>) {
    if (canvas) change({ ...canvas, nodes: canvas.nodes.map(n => n.id === id ? { ...n, ...patch } : n) })
  }
  function beginEdit(id: string) {
    if (locked || linking) return
    setSelected(id); setEdgeId(''); setEditingId(id)
  }
  function fit(ids?: string[]) {
    if (!canvas || !stage.current) return
    // Reveal all ancestors before locating an AI update hidden inside a collapsed branch.
    const targets = ids ? canvas.nodes.filter(n => ids.includes(n.id)) : visibleNodes(canvas)
    if (!targets.length) return
    const rects = targets.map(n => n.kind === 'group' ? groupRect(canvas, n) : { x: n.x, y: n.y, width: nodeWidth, height: nodeHeight(n) })
    const left = Math.min(...rects.map(r => r.x)), top = Math.min(...rects.map(r => r.y))
    const width = Math.max(...rects.map(r => r.x + r.width)) - left, height = Math.max(...rects.map(r => r.y + r.height)) - top
    const box = stage.current.getBoundingClientRect(), zoom = Math.max(.2, Math.min(1.25, (box.width - 100) / Math.max(width, 1), (box.height - 120) / Math.max(height, 1)))
    setView({ x: (box.width - width * zoom) / 2 - left * zoom, y: (box.height - height * zoom) / 2 - top * zoom, zoom })
  }
  function zoomBy(factor: number) {
    const box = stage.current?.getBoundingClientRect(); if (!box) return
    const zoom = Math.max(.2, Math.min(2, view.zoom * factor))
    setView({ x: box.width / 2 - (box.width / 2 - view.x) * zoom / view.zoom, y: box.height / 2 - (box.height / 2 - view.y) * zoom / view.zoom, zoom })
  }
  async function createCanvas() {
    if (dirty || busy || conflict) return
    const next: IdeaCanvas = { id: crypto.randomUUID(), name: `构思 ${1 + (data?.canvases.length ?? 0)}`, nodes: [], edges: [], updatedAt: 0 }
    if (await store.persist({ type: 'canvas_save', canvas: next })) { store.select(next.id); setSelected('') }
  }
  function addNode(kind: 'note' | 'group' = 'note', child = false, attachment?: { image?: string; html?: string }, source = node) {
    if (!canvas || locked) return
    const box = stage.current?.getBoundingClientRect()
    const x = ((box?.width ?? 500) / 2 - view.x) / view.zoom - nodeWidth / 2
    const y = ((box?.height ?? 400) / 2 - view.y) / view.zoom - 40
    const siblingCount = canvas.nodes.filter(n => n.parentId === source?.id).length
    const next = makeNode(child && source ? source.x + 318 : x, child && source ? source.y + siblingCount * 130 : y, {
      title: kind === 'group' ? '新分组' : attachment?.image ? '参考图片' : attachment?.html ? '页面原型' : canvas.nodes.length ? '新节点' : '功能规划', kind,
      parentId: child && source?.kind !== 'group' ? source?.id ?? null : null,
      groupId: child && source?.kind === 'group' ? source.id : source?.groupId ?? null,
      ...attachment,
    })
    const updated = { ...canvas, nodes: [...canvas.nodes.map(n => child && n.id === source?.id ? { ...n, collapsed: false } : n), next] }
    change(child && source ? layoutBranch(updated, source.id) : updated); setSelected(next.id); setEditingId(next.id); setEdgeId('')
  }
  async function readFile(file: File, html = false) {
    if (!canvasRef.current || locked) return
    const targetCanvasId = canvasRef.current.id, targetNodeId = selected
    if (html ? file.size > 512 * 1024 : !['image/png', 'image/jpeg', 'image/webp', 'image/gif'].includes(file.type) || file.size > 3 * 1024 * 1024) {
      setNotice(html ? 'HTML 文件最多 512 KiB' : '支持 3 MiB 以内的 PNG、JPEG、WebP、GIF 图片'); return
    }
    try {
      const content = html ? await file.text() : await new Promise<string>((resolve, reject) => { const reader = new FileReader(); reader.onload = () => resolve(String(reader.result)); reader.onerror = () => reject(reader.error); reader.readAsDataURL(file) })
      const current = canvasRef.current
      if (!current || current.id !== targetCanvasId) return
      const attachment = html ? { html: content } : { image: content }
      if (current.nodes.some(n => n.id === targetNodeId && n.kind === 'note')) change({ ...current, nodes: current.nodes.map(n => n.id === targetNodeId ? { ...n, ...attachment } : n) })
      else {
        const box = stage.current?.getBoundingClientRect(), v = viewRef.current
        const next = makeNode(((box?.width ?? 500) / 2 - v.x) / v.zoom - nodeWidth / 2, ((box?.height ?? 400) / 2 - v.y) / v.zoom - 40, { title: html ? '页面原型' : '参考图片', ...attachment })
        change({ ...current, nodes: [...current.nodes, next] }); setSelected(next.id)
      }
    } catch (e) { setNotice(String(e)) }
  }
  function openPreview(value: IdeaNode, html = false) { setPreview(value); setPreviewMedia(html || !value.image ? 'html' : 'image') }
  function choose(id: string) {
    if (linking && selected && id !== selected && canvas) {
      if (!canvas.edges.some(e => e.source === selected && e.target === id)) {
        const edge = { id: crypto.randomUUID(), source: selected, target: id, label: '' }
        change({ ...canvas, edges: [...canvas.edges, edge] }); setEdgeId(edge.id); setSelected(''); setEditingId('')
      }
      setLinking(false)
    } else { setSelected(id); setEdgeId(''); if (id !== editingId) setEditingId('') }
  }
  function startDrag(e: ReactPointerEvent, id?: string) {
    if (e.button !== 0 && e.button !== 1 || drag.current || locked && id || linking && id) return
    if ((e.target as Element).closest('button,input,textarea,iframe')) return
    e.preventDefault(); e.stopPropagation()
    if (id) choose(id)
    else { setSelected(''); setEditingId(''); setEdgeId('') }
    stage.current?.setPointerCapture(e.pointerId)
    drag.current = { pointer: e.pointerId, x: e.clientX, y: e.clientY, view, node: id, moved: false }
    store.interaction.current = true
  }
  function moveDrag(e: ReactPointerEvent) {
    const current = drag.current
    if (!current || current.pointer !== e.pointerId) return
    const dx = e.clientX - current.x, dy = e.clientY - current.y
    if (Math.abs(dx) + Math.abs(dy) < 4 && !current.moved) return
    current.moved = true
    if (current.node && canvas) setDragOffset({ ids: descendants(canvas, current.node), dx: dx / view.zoom, dy: dy / view.zoom })
    else setView({ ...current.view, x: current.view.x + dx, y: current.view.y + dy })
  }
  function finishDrag(e: ReactPointerEvent, cancel = false) {
    const current = drag.current
    if (!current || current.pointer !== e.pointerId) return
    drag.current = null; store.interaction.current = !!editingId || publish || !!preview
    if (!cancel && current.node && dragOffset && canvas) change({ ...canvas, nodes: canvas.nodes.map(n => dragOffset.ids.has(n.id) ? { ...n, x: n.x + dragOffset.dx, y: n.y + dragOffset.dy } : n) })
    if (cancel && !current.node) setView(current.view)
    setDragOffset(null)
    if (stage.current?.hasPointerCapture(e.pointerId)) stage.current.releasePointerCapture(e.pointerId)
  }
  async function deleteSelected() {
    if (!canvas || locked) return
    if (edge) { change({ ...canvas, edges: canvas.edges.filter(e => e.id !== edge.id) }); setEdgeId(''); return }
    if (!node) return
    store.interaction.current = true
    const accepted = await dialogs.confirm({ title: '删除节点', description: `删除「${node.title}」及其连线？子节点将保留并解除归属。`, tone: 'danger', confirmText: '删除' })
    store.interaction.current = !!editingId || publish || !!preview
    if (!accepted || !canvasRef.current) return
    change(removeNode(canvasRef.current, node.id)); setSelected(''); setEditingId('')
  }
  const branch = canvas && node ? canvas.nodes.filter(n => selectedIds.has(n.id) && n.kind !== 'group') : []
  async function publishBranch() {
    if (!canvas || !node || locked || dirty) return
    setPublishing(true)
    try {
      const board = await getBoard(workspaceKey, revision)
      const digest = await crypto.subtle.digest('SHA-256', new TextEncoder().encode(JSON.stringify([canvas.id, node.id])))
      const featureId = `idea:${Array.from(new Uint8Array(digest), value => value.toString(16).padStart(2, '0')).join('')}`
      const source = (n: IdeaNode) => `${n.body}${n.body ? '\n\n' : ''}[查看构思](#ideas?canvas=${encodeURIComponent(canvas.id)}&node=${encodeURIComponent(n.id)})`
      await executeBoard(workspaceKey, revision, board.revision, { type: 'idea_branch', feature: { id: featureId, name: node.title, description: source(node), goal: '', x: node.x, y: node.y }, tasks: branch.filter(n => n.id !== node.id).map(n => ({ title: n.title, description: source(n) })) })
      setPublish(false); setNotice('已转入看板，任务进入待开始')
    } catch (e) { setNotice(String(e)) } finally { setPublishing(false) }
  }

  return <section className="ideas-page" aria-label="构思画布" onPaste={e => {
    if ((e.target as Element).closest('input,textarea') || locked) return
    const file = Array.from(e.clipboardData.files).find(f => f.type.startsWith('image/'))
    if (file) { e.preventDefault(); void readFile(file) }
  }}>
    <header className="ideas-header">
      <Shapes size={17} /><IdeasCanvasMenu canvases={data?.canvases ?? []} value={active} busy={!data || dirty || busy || locked} revision={data?.revision ?? 0} interaction={store.interaction} onChange={store.select} onSave={(operation, expected) => store.persist(operation, null, expected)} />
      <span className="ideas-save" role="status">{busy ? '保存中' : conflict ? '未保存' : dirty ? '待保存' : data ? '已保存' : '加载中'}</span>
      <div className="ideas-header-actions">
        <Tool label="撤销 · Ctrl+Z" disabled={!store.history.past.length || locked} onClick={() => store.undo()}><Undo2 size={16} /></Tool>
        <Tool label="重做 · Ctrl+Shift+Z" disabled={!store.history.future.length || locked} onClick={() => store.undo(true)}><Redo2 size={16} /></Tool>
        <Tool label="导出画布 JSON" disabled={!canvas} onClick={() => canvas && download(canvas)}><ArrowDownToLine size={16} /></Tool>
      </div>
    </header>
    {(error || notice) && <div className={`ideas-notice ${error ? 'is-error' : ''}`} role={error ? 'alert' : 'status'}><span>{error || notice}</span>
      {error && <button onClick={async () => { if (!dirty || await dialogs.confirm({ title: '重新读取画布', description: '将放弃本地未保存的修改。可先导出 JSON 保留内容，再重新读取。', confirmText: '放弃修改并读取' })) await store.reload() }}><RefreshCw size={13} />重新读取</button>}
      {!error && <Tool label="关闭提示" onClick={() => setNotice('')}><X size={14} /></Tool>}
    </div>}
    {!canvas ? <div className="ideas-empty"><Shapes size={38} strokeWidth={1} /><h2>构思</h2><button className="ideas-primary" disabled={!data || locked} onClick={() => void createCanvas()}><Plus size={15} />新建画布</button></div> : <>
      <div className={`ideas-stage ${linking ? 'is-linking' : ''}`} ref={stage} tabIndex={0} aria-label="画布，拖动空白平移，Ctrl 加滚轮缩放" onPointerDown={e => { if (!(e.target as Element).closest('.ideas-node,.ideas-group,.ideas-float,.ideas-edge')) startDrag(e) }} onPointerMove={moveDrag} onPointerUp={e => finishDrag(e)} onPointerCancel={e => finishDrag(e, true)} onLostPointerCapture={e => finishDrag(e, true)} onDragOver={e => { if (e.dataTransfer.types.includes('Files')) e.preventDefault() }} onDrop={e => { e.preventDefault(); const file = e.dataTransfer.files[0]; if (file) void readFile(file, /\.html?$/i.test(file.name)) }}>
        <div className="ideas-world" style={{ transform: `translate(${view.x}px,${view.y}px) scale(${view.zoom})` }}>
          {display.filter(n => n.kind === 'group').map(n => {
            const rect = renderRect(n)
            return <div key={n.id} className={`ideas-group ${selected === n.id ? 'is-selected' : ''}`} style={{ left: rect.x, top: rect.y, width: rect.width, height: rect.height }} onPointerDown={e => startDrag(e, n.id)} onClick={() => choose(n.id)}>
            {editingId === n.id ? <NodeTextEditor node={n} disabled={locked} onChange={patch => updateNode(n.id, patch)} onFinish={() => setEditingId('')} /> : <button onDoubleClick={() => beginEdit(n.id)} onKeyDown={e => { if (e.key === 'F2' || e.key === 'Enter') { e.preventDefault(); beginEdit(n.id) } }}>{n.title}</button>}
            <Tool className="ideas-add-child" label="添加组内节点" disabled={locked} onClick={e => { e.stopPropagation(); addNode('note', true, undefined, n) }}><Plus size={14} /></Tool>
          </div>})}
          <svg className="ideas-wires" aria-label="节点关系"><defs><marker id="ideas-arrow" markerWidth="8" markerHeight="8" refX="7" refY="4" orient="auto"><path d="M0 0L8 4L0 8" fill="currentColor" /></marker></defs>
            {display.filter(n => n.parentId).map(n => {
              const parent = display.find(p => p.id === n.parentId); if (!parent) return null
              const a = renderRect(parent), b = renderRect(n)
              const x = a.x + a.width, y = a.y + a.height / 2, endY = b.y + b.height / 2
              return <path className="ideas-branch" key={`parent-${n.id}`} d={`M${x},${y} C${x + 64},${y} ${b.x - 64},${endY} ${b.x},${endY}`} />
            })}
            {canvas.edges.map(e => {
              const a = display.find(n => n.id === e.source), b = display.find(n => n.id === e.target)
              if (!a || !b) return null
              const ar = renderRect(a), br = renderRect(b)
              const forward = br.x + br.width / 2 >= ar.x + ar.width / 2, x = ar.x + (forward ? ar.width : 0), endX = br.x + (forward ? 0 : br.width), y = ar.y + ar.height / 2, endY = br.y + br.height / 2, bend = Math.max(70, Math.abs(endX - x) / 2) * (forward ? 1 : -1)
              const path = `M${x},${y} C${x + bend},${y} ${endX - bend},${endY} ${endX},${endY}`
              return <g className={`ideas-edge ${edgeId === e.id ? 'is-selected' : ''}`} key={e.id} tabIndex={0} role="button" aria-label={`连线：${a.title} 到 ${b.title}${e.label ? `，${e.label}` : ''}`} onClick={event => { event.stopPropagation(); setEdgeId(e.id); setSelected(''); setEditingId('') }} onKeyDown={event => { if (event.key === 'Enter') { setEdgeId(e.id); setSelected(''); setEditingId('') } }}><path className="ideas-edge-hit" d={path} /><path className="ideas-edge-line" d={path} markerEnd="url(#ideas-arrow)" />{e.label && <text x={(x + endX) / 2} y={(y + endY) / 2 - 9} textAnchor="middle">{e.label}</text>}</g>
            })}
          </svg>
          {display.filter(n => n.kind !== 'group').map(n => <article key={n.id} tabIndex={0} aria-label={n.title} className={`ideas-node ${selected === n.id ? 'is-selected' : ''} ${changed.includes(n.id) ? 'is-updated' : ''}`} style={{ left: n.x, top: n.y, height: renderHeight(n) }} onPointerDown={e => startDrag(e, n.id)} onClick={() => choose(n.id)} onDoubleClick={() => beginEdit(n.id)} onKeyDown={e => {
            if (e.target !== e.currentTarget) return
            if (e.key === 'Enter' || e.key === 'F2') { e.preventDefault(); beginEdit(n.id) }
            const step = e.shiftKey ? 40 : 10
            const delta: Record<string, [number, number]> = { ArrowLeft: [-step, 0], ArrowRight: [step, 0], ArrowUp: [0, -step], ArrowDown: [0, step] }
            if (delta[e.key] && !locked) { e.preventDefault(); const [dx, dy] = delta[e.key]; const ids = descendants(canvas, n.id); change({ ...canvas, nodes: canvas.nodes.map(v => ids.has(v.id) ? { ...v, x: v.x + dx, y: v.y + dy } : v) }) }
          }}>
            {editingId === n.id ? <NodeTextEditor node={n} disabled={locked} onChange={patch => updateNode(n.id, patch)} onFinish={() => setEditingId('')} /> : <>
              <header><span className="ideas-node-dot" /><strong>{n.title}</strong></header>
              {n.body && <p>{n.body}</p>}
            </>}
            <Tool className="ideas-add-child" label="添加子节点" disabled={locked} onClick={e => { e.stopPropagation(); addNode('note', true, undefined, n) }}><Plus size={14} /></Tool>
            {(n.image || n.html) && <div className="ideas-node-media"><button className="ideas-media-open" aria-label={`预览${n.title}`} onClick={e => { e.stopPropagation(); openPreview(n) }}>{n.image ? <img alt={n.title} src={n.image} draggable={false} /> : <iframe title={`${n.title} 缩略图`} sandbox="" srcDoc={prototypeDocument(n.html)} tabIndex={-1} loading="lazy" />}{n.image && n.html && <span><FileCode2 size={12} />含原型</span>}</button><Tool className="ideas-media-remove" label={n.image ? '移除图片' : '移除原型'} disabled={locked} onClick={e => { e.stopPropagation(); updateNode(n.id, n.image ? { image: '' } : { html: '' }) }}><X size={12} /></Tool></div>}
            {canvas.nodes.some(child => child.parentId === n.id) && <button className="ideas-fold" aria-label={n.collapsed ? '展开分支' : '折叠分支'} aria-expanded={!n.collapsed} onClick={e => { e.stopPropagation(); change({ ...canvas, nodes: canvas.nodes.map(v => v.id === n.id ? { ...v, collapsed: !v.collapsed } : v) }) }}>{n.collapsed ? <ChevronRight size={12} /> : <ChevronDown size={12} />}</button>}
          </article>)}
        </div>
        {node && selectedRect && !editingId && !dragOffset && !linking && <div className="ideas-node-toolbar ideas-float" role="toolbar" aria-label="选中节点操作" style={{ left: Math.max(130, Math.min((stage.current?.clientWidth ?? 550) - 130, view.x + (selectedRect.x + selectedRect.width / 2) * view.zoom)), top: Math.max(48, view.y + selectedRect.y * view.zoom - 10) }}>
          <Tool label="编辑文字 · 双击或 F2" disabled={locked} onClick={() => beginEdit(node.id)}><Pencil size={15} /></Tool>
          <Tool label="添加或替换图片" disabled={locked || node.kind === 'group'} onClick={() => imageInput.current?.click()}><ImagePlus size={15} /></Tool>
          <Tool label="添加或替换 HTML 原型" disabled={locked || node.kind === 'group'} onClick={() => htmlInput.current?.click()}><FileCode2 size={15} /></Tool>
          <span />
          <Tool label="连接到其他节点" disabled={locked} onClick={() => setLinking(true)}><Link2 size={15} /></Tool>
          <Tool label="整理分支或分组" disabled={locked} onClick={() => change(layoutBranch(canvas, node.id))}><Shapes size={15} /></Tool>
          {node.kind === 'group' && <Tool label={node.collapsed ? '展开分组' : '折叠分组'} disabled={locked} onClick={() => updateNode(node.id, { collapsed: !node.collapsed })}>{node.collapsed ? <ChevronRight size={15} /> : <ChevronDown size={15} />}</Tool>}
          <Tool label="转为看板功能" disabled={locked || dirty} onClick={() => setPublish(true)}><ArrowUpRight size={15} /></Tool>
          <Tool label="删除节点" disabled={locked} onClick={() => void deleteSelected()}><Trash2 size={15} /></Tool>
        </div>}
        {edge && (() => {
          const a = display.find(n => n.id === edge.source), b = display.find(n => n.id === edge.target)
          if (!a || !b) return null
          const ar = renderRect(a), br = renderRect(b)
          const forward = br.x + br.width / 2 >= ar.x + ar.width / 2
          const middleX = (ar.x + (forward ? ar.width : 0) + br.x + (forward ? 0 : br.width)) / 2
          const middleY = (ar.y + ar.height / 2 + br.y + br.height / 2) / 2
          return <div className="ideas-edge-editor ideas-float" style={{ left: Math.max(110, Math.min((stage.current?.clientWidth ?? 550) - 110, view.x + middleX * view.zoom)), top: Math.max(24, view.y + middleY * view.zoom - 12) }} onFocusCapture={() => { store.interaction.current = true }} onBlurCapture={e => { if (!e.currentTarget.contains(e.relatedTarget as Node)) store.interaction.current = false }} onKeyDown={e => { if (e.key === 'Escape' || e.key === 'Enter' && !e.nativeEvent.isComposing) { e.stopPropagation(); setEdgeId(''); store.interaction.current = false } }}>
            <input autoFocus aria-label="连线说明" placeholder="连线说明" maxLength={240} disabled={locked} value={edge.label} onChange={e => change({ ...canvas, edges: canvas.edges.map(v => v.id === edge.id ? { ...v, label: e.target.value } : v) })} />
            <Tool label="删除连线" disabled={locked} onClick={() => { void deleteSelected(); store.interaction.current = false }}><Trash2 size={14} /></Tool>
          </div>
        })()}
        <div className="ideas-tools ideas-float" role="toolbar" aria-label="画布工具">
          <Tool label="新增节点" disabled={locked} onClick={() => addNode()}><Plus size={18} /></Tool>
          <Tool label="添加图片" disabled={locked} onClick={() => imageInput.current?.click()}><ImagePlus size={18} /></Tool>
          <Tool label="添加 HTML 原型" disabled={locked} onClick={() => htmlInput.current?.click()}><FileCode2 size={18} /></Tool>
          <Tool label="新建分组" disabled={locked} onClick={() => addNode('group')}><LayoutTemplate size={18} /></Tool>
        </div>
        {!canvas.nodes.length && <div className="ideas-canvas-empty"><GitBranch size={34} strokeWidth={1} /><button className="ideas-primary" disabled={locked} onClick={() => addNode()}>添加中心节点</button></div>}
        <div className="ideas-zoom ideas-float"><Tool label="缩小" onClick={() => zoomBy(.8)}><Minus size={16} /></Tool><span>{Math.round(view.zoom * 100)}%</span><Tool label="放大" onClick={() => zoomBy(1.25)}><Plus size={16} /></Tool><Tool label="适应全部" onClick={() => fit()}><Expand size={16} /></Tool><Tool label="定位选中节点" disabled={!node} onClick={() => fit([selected])}><Crosshair size={16} /></Tool></div>
        {!!changed.length && <button className="ideas-updates ideas-float" onClick={() => {
          const hiddenParents = canvas.nodes.filter(n => n.collapsed && [...descendants(canvas, n.id)].some(id => changed.includes(id)))
          if (hiddenParents.length) change({ ...canvas, nodes: canvas.nodes.map(n => hiddenParents.some(p => p.id === n.id) ? { ...n, collapsed: false } : n) })
          fit(changed); setSelected(changed[0]); store.setChanged([])
        }}><Focus size={14} />定位本次更新 · {changed.length}</button>}
        {linking && <div className="ideas-link-hint ideas-float" role="status">点击目标节点 <button onClick={() => setLinking(false)}>取消</button></div>}
      </div>
    </>}
    <input ref={imageInput} type="file" accept="image/png,image/jpeg,image/webp,image/gif" hidden onChange={e => { const file = e.target.files?.[0]; if (file) void readFile(file); e.target.value = '' }} />
    <input ref={htmlInput} type="file" accept=".html,.htm,text/html" hidden onChange={e => { const file = e.target.files?.[0]; if (file) void readFile(file, true); e.target.value = '' }} />
    {preview && <Modal title={preview.title} large onClose={() => setPreview(null)}>{preview.image && preview.html && <div className="ideas-preview-tabs"><button aria-pressed={previewMedia === 'image'} onClick={() => setPreviewMedia('image')}>图片</button><button aria-pressed={previewMedia === 'html'} onClick={() => setPreviewMedia('html')}>原型</button></div>}<div className="ideas-preview">{previewMedia === 'html' ? <iframe title={preview.title} sandbox="allow-scripts" referrerPolicy="no-referrer" srcDoc={prototypeDocument(preview.html)} /> : <img alt={preview.title} src={preview.image} />}</div></Modal>}
    {publish && node && <Modal title="转为看板功能" onClose={() => { if (!publishing) setPublish(false) }}><div className="ideas-publish"><h3>{node.title}</h3><p>{node.body}</p><ul>{branch.filter(n => n.id !== node.id).map(n => <li key={n.id}>{n.title}</li>)}</ul><p>{branch.filter(n => n.id !== node.id).length} 个任务进入待开始，保留构思链接。</p><button className="ideas-primary" disabled={publishing || dirty || conflict} onClick={() => void publishBranch()}>{publishing ? '创建中' : '确认创建'}</button></div></Modal>}
  </section>
}
