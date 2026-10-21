import { useEffect, useRef, useState } from 'react'
import { ArrowRight, Clock3, FolderOpen, FolderPlus, Layers3, Save, X } from 'lucide-react'
import { ApiError, cancelWorkspacePick, executeWorkspace, getWorkspaceCatalog, pickWorkspaceLocation, type WorkspaceCatalog, type WorkspaceOperation, type WorkspaceResponse } from '../../lib/api'
import { WorkspaceBrowser } from './WorkspaceBrowser'

export type WorkspaceManagerMode = 'open' | 'create' | 'add_folder' | 'save' | 'close'
const titles = { open: '打开工作区', create: '新建工作区', add_folder: '添加文件夹到工作区', save: '保存工作区', close: '关闭工作区' }
const descriptions = { open: '选择工作区文件，或直接以文件夹开始。', create: '从文件夹建立工作区，也可以创建一个空工作区。', add_folder: '选择现有文件夹，将它加入当前工作区。', save: '选择保存位置，保留工作区的配置与注释。', close: '关闭后可以随时重新打开，运行中的服务会继续运行。' }
type Props = { mode: WorkspaceManagerMode; data: WorkspaceResponse | null; onChange: (data: WorkspaceResponse) => void; onRefresh: () => void; onClose: () => void }

export function WorkspaceManager({ mode: initialMode, data, onChange, onRefresh, onClose }: Props) {
  const dialog = useRef<HTMLDialogElement>(null)
  const pickerRequest = useRef<string | null>(null)
  const cancelPending = useRef(false)
  const [mode, setMode] = useState(initialMode)
  const [path, setPath] = useState<string | null>(null)
  const [selected, setSelected] = useState<string | null>(null)
  const [filename, setFilename] = useState(initialMode === 'save' ? data?.workspaceFile?.split(/[\\/]/).at(-1)?.replace(/\.code-workspace$/i, '') ?? data?.name ?? '我的工作区' : '我的工作区')
  const [sourceFolder, setSourceFolder] = useState<string | null>(null)
  const [folderName, setFolderName] = useState('')
  const [step, setStep] = useState(0)
  const [closeAfterSave, setCloseAfterSave] = useState(false)
  const [catalog, setCatalog] = useState<WorkspaceCatalog | null>(null)
  const [catalogError, setCatalogError] = useState('')
  const [catalogAttempt, setCatalogAttempt] = useState(0)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [picking, setPicking] = useState(false)
  const [cancelling, setCancelling] = useState(false)
  const [discard, setDiscard] = useState(false)
  const [baseRevision, setBaseRevision] = useState(data?.revision)
  const stale = baseRevision !== undefined && data?.revision !== baseRevision
  const needsDiscard = !!data?.dirty && ['open', 'create', 'close'].includes(mode)
  const saving = mode === 'save' || (mode === 'create' && step === 1)
  useEffect(() => { if (baseRevision === undefined && data) setBaseRevision(data.revision) }, [data, baseRevision])
  useEffect(() => { const current = dialog.current; current?.showModal(); return () => current?.close() }, [])
  useEffect(() => {
    const controller = new AbortController()
    setCatalogError('')
    getWorkspaceCatalog(controller.signal).then(value => { if (!controller.signal.aborted) setCatalog(value) }).catch(err => { if (!controller.signal.aborted) setCatalogError(String(err)) })
    return () => controller.abort()
  }, [catalogAttempt])
  function chooseDestination(folder: string | null) {
    setSourceFolder(folder); setStep(1); setSelected(null)
    setPath(catalog?.allowedRoots.find(root => /[\\/]WorkSpace$/i.test(root)) ?? null)
    if (folder) setFilename(folder.split(/[\\/]/).filter(Boolean).at(-1) ?? '我的工作区')
  }
  async function pickLocal(kind: 'workspace_file' | 'folder') {
    if (busy || cancelling || stale || baseRevision === undefined) return
    const requestId = crypto.randomUUID()
    pickerRequest.current = requestId
    setBusy(true); setPicking(true); setError('')
    try {
      const result = await pickWorkspaceLocation(kind, baseRevision, requestId)
      if (result.path) {
        if (kind === 'workspace_file') { setSelected(result.path); setPath(null) }
        else { setPath(result.path); setSelected(null) }
        setCatalogAttempt(value => value + 1)
      }
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      if (err instanceof ApiError && err.code === 'conflict') onRefresh()
    } finally { pickerRequest.current = null; setBusy(false); setPicking(false) }
  }
  async function cancelPick() {
    const requestId = pickerRequest.current
    if (!requestId || cancelPending.current) return
    cancelPending.current = true; setCancelling(true); setError('')
    try { await cancelWorkspacePick(requestId) }
    catch (err) { setError(err instanceof Error ? err.message : String(err)) }
    finally { cancelPending.current = false; setCancelling(false) }
  }
  async function run(operation: WorkspaceOperation, thenClose = false) {
    if (busy || stale || baseRevision === undefined) return
    setBusy(true); setError('')
    try {
      let result = await executeWorkspace(operation, baseRevision, discard)
      if (thenClose) result = await executeWorkspace({ type: 'close' }, result.revision)
      onChange(result); onClose()
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
      if (err instanceof ApiError && err.code === 'conflict') onRefresh()
    } finally { setBusy(false) }
  }
  function submit() {
    if (needsDiscard && !discard) { setError('请先保存，或选择放弃未保存的更改。'); return }
    if (mode === 'close') { void run({ type: 'close' }); return }
    if (mode === 'create' && step === 0) { if (path) chooseDestination(path); return }
    if (saving) {
      const name = filename.trim().replace(/\.code-workspace$/i, '')
      if (!path || !name || /[\\/:*?"<>|\x00-\x1f]/.test(name) || /[. ]$/.test(name) || name === '.' || name === '..') { setError('请选择保存文件夹，并填写有效的工作区名称。'); return }
      const target = `${path.replace(/[\\/]$/, '')}/${name}.code-workspace`
      void run(mode === 'save' ? { type: 'save', path: target } : sourceFolder ? { type: 'create_from_folder', path: target, folder: sourceFolder } : { type: 'create', path: target }, closeAfterSave)
    } else if (mode === 'open' && (selected || path)) void run({ type: 'open', path: selected ?? path! })
    else if (mode === 'add_folder' && path) void run({ type: 'add_folder', path, ...(folderName.trim() ? { name: folderName.trim() } : {}) })
  }
  function saveAndClose() {
    if (data?.workspaceFile) { void run({ type: 'save' }, true); return }
    setMode('save'); setCloseAfterSave(true); setPath(null); setSelected(null)
  }
  const canSubmit = !!data && !busy && !stale && (!needsDiscard || discard) && (mode === 'close' || (mode === 'open' ? !!(selected || path) : !!path))
  return <dialog ref={dialog} className="ws-dialog ws-location-dialog" aria-labelledby="workspace-manager-title" aria-describedby="workspace-manager-description" onCancel={event => { if (busy) { event.preventDefault(); if (picking) void cancelPick() } else onClose() }}>
    <header className="ws-location-heading"><div className="ws-dialog-symbol">{mode === 'add_folder' ? <FolderPlus size={22} /> : <Layers3 size={22} />}</div><div><h2 id="workspace-manager-title">{titles[mode]}</h2><p id="workspace-manager-description">{descriptions[mode]}</p></div><button className="ws-icon-button" type="button" disabled={busy} aria-label="关闭工作区管理" onClick={onClose}><X size={17} /></button></header>
    <div className="ws-location-body">
      {stale && <div className="ws-dialog-notice" role="alert"><p>当前工作区已被其他入口修改。</p><button className="ws-outline-button" disabled={busy} onClick={() => { setBaseRevision(data?.revision); setDiscard(false); setError('') }}>基于当前工作区继续</button></div>}
      {mode === 'create' && <div className="ws-create-steps"><span className={step === 0 ? 'is-current' : ''}>1 选择起始文件夹</span><ArrowRight size={12} /><span className={step === 1 ? 'is-current' : ''}>2 名称与保存位置</span>{step === 0 ? <button type="button" disabled={busy} onClick={() => chooseDestination(null)}>创建空工作区</button> : <button type="button" disabled={busy} onClick={() => { setStep(0); setPath(sourceFolder); setSelected(null) }}>上一步</button>}</div>}
      {mode !== 'close' && <>
        {mode === 'open' && <section className="ws-recent" aria-label="最近打开的工作区">
          <h3><Clock3 size={14} />最近打开</h3>
          {catalog?.recentWorkspaces?.length ? <div className="ws-recent-list">{catalog.recentWorkspaces.map(entry => <button type="button" key={entry.path} disabled={busy || stale || baseRevision === undefined || !entry.available || (needsDiscard && !discard)} title={entry.available ? entry.path : `位置不存在或已改变：${entry.path}`} onClick={() => void run({ type: 'open_recent', path: entry.path })}>
            {entry.kind === 'folder' ? <FolderOpen size={17} /> : <Layers3 size={17} />}<span><strong>{entry.name}</strong><small>{entry.path}</small></span><small>{entry.available ? entry.kind === 'folder' ? '文件夹' : '工作区' : '不可用'}</small><ArrowRight size={13} />
          </button>)}</div> : <p className="ws-manager-hint">{!catalog ? '正在读取最近记录…' : !catalog.recentWorkspaces ? '当前后端尚不支持最近打开列表，请更新并重启 DevFlow。' : '暂无最近记录，打开工作区或文件夹后会自动保留。'}</p>}
          {needsDiscard && !discard && <p className="ws-manager-hint">请先保存，或勾选下方“放弃未保存的更改”后打开最近位置。</p>}
        </section>}
        {saving && <div className="ws-workspace-name"><label htmlFor="workspace-name">工作区名称</label><div><input id="workspace-name" autoFocus value={filename} onChange={event => setFilename(event.target.value)} disabled={busy} autoComplete="off" /><span>.code-workspace</span></div>{mode === 'create' && <p className="ws-manager-hint">{sourceFolder ? `包含文件夹：${sourceFolder}` : '空工作区，可稍后在资源树中添加文件夹。'}</p>}</div>}
        <div className="ws-native-picker">
          {mode === 'open' && <button type="button" className="ws-outline-button" disabled={busy || stale || baseRevision === undefined} onClick={() => void pickLocal('workspace_file')}><Layers3 size={15} />选择工作区文件…</button>}
          <button type="button" className="ws-outline-button" disabled={busy || stale || baseRevision === undefined} onClick={() => void pickLocal('folder')}><FolderOpen size={15} />{saving ? '选择保存文件夹…' : '选择本机文件夹…'}</button>
          <p className="ws-manager-hint" role="status">{picking ? cancelling ? '正在关闭系统选择窗口…' : '请在系统窗口中选择；若被遮挡，可从任务栏切换到“DevFlow — 选择本机位置”，或在此取消选择。' : '可选择本机任意位置。下方保留已授权位置，方便再次打开。'}</p>
          {picking && <button type="button" className="ws-outline-button" disabled={cancelling} onClick={() => void cancelPick()}>{cancelling ? '正在取消…' : '取消选择'}</button>}
        </div>
        {mode === 'open' && selected && <p className="ws-native-selection" title={selected}>已选择：{selected}</p>}
        {catalogError ? <div className="ws-dialog-notice" role="alert"><p>{catalogError}</p><button className="ws-outline-button" onClick={() => setCatalogAttempt(value => value + 1)}>重新加载位置</button></div> : <WorkspaceBrowser catalog={catalog} path={path} onNavigate={setPath} selected={selected} onSelect={setSelected} foldersOnly={mode !== 'open'} disabled={busy} />}
        {mode === 'add_folder' && <div className="ws-workspace-name ws-folder-alias"><label htmlFor="workspace-folder-name">显示名称（可选）</label><div><input id="workspace-folder-name" value={folderName} onChange={event => setFolderName(event.target.value)} maxLength={256} placeholder="默认使用文件夹名称" disabled={busy} /></div></div>}
      </>}
      {mode === 'close' && <div className="ws-close-summary"><FolderOpen size={20} /><div><strong>{data?.name}</strong><p>{data?.dirty ? '当前工作区有未保存的更改。' : '工作区配置已保存，可安全关闭。'}</p></div></div>}
      {needsDiscard && <label className="ws-discard"><input type="checkbox" checked={discard} onChange={event => setDiscard(event.target.checked)} disabled={busy} />放弃当前工作区未保存的更改</label>}
      {error && <p className="ws-manager-error" role="alert">{error}</p>}
    </div>
    <footer className="ws-location-footer"><span>{mode === 'add_folder' ? '添加已有文件夹，不创建磁盘目录' : saving ? '已有的其他工作区文件不会被覆盖' : mode === 'open' ? '支持 VS Code 工作区文件与本地文件夹' : ''}</span><div><button className="ws-outline-button" disabled={busy} onClick={onClose}>取消</button>{mode === 'close' && data?.dirty && <button className="ws-outline-button" disabled={busy || stale} onClick={saveAndClose}><Save size={14} />保存并关闭</button>}<button className="ws-primary-button" disabled={!canSubmit} onClick={submit}>{busy ? '处理中…' : mode === 'create' && step === 0 ? <>下一步<ArrowRight size={14} /></> : mode === 'create' ? '创建工作区' : mode === 'save' ? '保存工作区' : mode === 'add_folder' ? '添加到工作区' : mode === 'close' ? '关闭工作区' : selected ? '打开工作区文件' : '打开所选文件夹'}</button></div></footer>
  </dialog>
}
