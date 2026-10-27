import { useEffect, useId, useRef, useState, type FormEvent } from 'react'
import { GitBranch, X } from 'lucide-react'
import { request } from '../../lib/api'
import { useDialogs } from '../../components/form/BaseDialog'

import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseCheckbox } from '../../components/form/BaseCheckbox'
import { BaseButton } from '../../components/form/BaseButton'

type Settings = { owner: string; repo: string; branch: string; boardId: string; automatic: boolean }
type Status = { settings: Settings | null; syncedRevision: number | null; lastSynced: number | null; error: string | null; pending: boolean; credentialReady: boolean }
type Props = { workspaceKey: string; workspaceRevision: number; boardRevision: number; onClose: () => void }

export function BoardSyncButton({ workspaceKey, workspaceRevision, boardRevision, onClick, disabled }: Omit<Props, 'onClose'> & { onClick: () => void; disabled: boolean }) {
  const [label, setLabel] = useState('Gitee 同步')
  useEffect(() => {
    let active = true
    let reading = false
    const refresh = async () => {
      if (reading) return
      reading = true
      try {
        const value = await request<Status>('/api/workspace/board/sync', { method: 'POST', body: JSON.stringify({ workspaceKey, expectedRevision: workspaceRevision, boardRevision, action: 'status' }), signal: AbortSignal.timeout(10000) })
        if (active) setLabel(!value.settings ? 'Gitee 同步' : value.error ? 'Gitee · 同步失败' : value.pending ? 'Gitee · 待同步' : 'Gitee · 已同步')
      } catch { /* A worker may hold the sync lock. Keep the last known label. */ }
      finally { reading = false }
    }
    void refresh()
    const timer = setInterval(() => { if (document.visibilityState === 'visible') void refresh() }, 5000)
    return () => { active = false; clearInterval(timer) }
  }, [workspaceKey, workspaceRevision, boardRevision])
  return <BaseButton type="button" disabled={disabled} title={label} aria-label={label} onClick={onClick}><GitBranch size={16} /></BaseButton>
}

export function BoardSyncPanel({ workspaceKey, workspaceRevision, boardRevision, onClose }: Props) {
  const id = useId()
  const dialog = useRef<HTMLDialogElement>(null)
  const dialogs = useDialogs()
  const [status, setStatus] = useState<Status | null>(null)
  const [settings, setSettings] = useState<Settings>({ owner: '', repo: '', branch: 'main', boardId: crypto.randomUUID(), automatic: false })
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState('')
  const [saved, setSaved] = useState(false)
  const [loaded, setLoaded] = useState(false)
  const guard = useRef(false)
  const call = (action: string, config?: Settings) => request<Status>('/api/workspace/board/sync', { method: 'POST', body: JSON.stringify({ workspaceKey, expectedRevision: workspaceRevision, boardRevision, action, settings: config }), signal: AbortSignal.timeout(60000) })
  useEffect(() => {
    const previous = document.activeElement as HTMLElement | null
    dialog.current?.showModal()
    let active = true
    void call('status').then(value => {
      if (!active) return
      setStatus(value); if (value.settings) { setSettings(value.settings); setSaved(true) }; setLoaded(true)
    }).catch(e => { if (active) setError(e instanceof Error ? e.message : '读取同步设置失败') })
    return () => { active = false; dialog.current?.close(); previous?.focus() }
    // The panel uses the board revision at open; conflicts require reopening after refresh.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])
  async function execute(action: string) {
    if (guard.current) return
    guard.current = true
    if (action === 'restore' && !await dialogs.confirm({ title: '从 Gitee 恢复', description: '用远端快照替换当前看板。恢复前保存本地备份；执行者会清除，进行中的任务回到待开始。', tone: 'danger', confirmText: '备份并恢复' })) { guard.current = false; return }
    setBusy(true); setError('')
    try {
      const value = await call(action, action === 'configure' ? settings : undefined)
      setStatus(value); setSaved(true)
      if (action === 'restore' && !value.error) onClose()
    } catch (e) { setError(e instanceof Error ? e.message : '同步操作失败') }
    finally { setBusy(false); guard.current = false }
  }
  function save(event: FormEvent) { event.preventDefault(); void execute('configure') }
  function change(next: Partial<Settings>) { setSettings(current => ({ ...current, ...next })); setSaved(false) }
  return <dialog ref={dialog} className="kb-create-dialog" aria-labelledby={`${id}-title`} onKeyDown={e => e.stopPropagation()} onCancel={e => { e.preventDefault(); if (!busy) onClose() }}>
    <header><h2 id={`${id}-title`}>Gitee 同步</h2><BaseButton type="button" aria-label="关闭" disabled={busy} onClick={onClose}><X size={18} /></BaseButton></header>
    <form onSubmit={save}>
      {(error || status?.error) && <p className="kb-error" role="alert">{error || status?.error}</p>}
      <fieldset className="kb-editor-fields" disabled={busy || !loaded}>
        <p className="kb-muted">使用已有私有仓库及分支。保存后每 30 秒检查当前工作区的修改并上传完整快照。</p>
        <div className="kb-form-row"><BaseField htmlFor={`${id}-field-1`} label="仓库拥有者"><BaseInput id={`${id}-field-1`} required maxLength={100} value={settings.owner} onChange={e => change({ owner: e.target.value.trim() })} placeholder="用户名或组织" /></BaseField><BaseField htmlFor={`${id}-field-2`} label="仓库路径"><BaseInput id={`${id}-field-2`} required maxLength={100} value={settings.repo} onChange={e => change({ repo: e.target.value.trim() })} placeholder="devflow-board" /></BaseField></div>
        <BaseField htmlFor={`${id}-field-3`} label="分支"><BaseInput id={`${id}-field-3`} required maxLength={200} value={settings.branch} onChange={e => change({ branch: e.target.value })} /></BaseField>
        <BaseField htmlFor={`${id}-board-id`} label="看板 ID" hint={`恢复其他设备的数据时，填写原设备的看板 ID。远端文件：boards/${settings.boardId}.json`}><BaseInput id={`${id}-board-id`} aria-describedby={`${id}-board-id-hint`} required value={settings.boardId} onChange={e => change({ boardId: e.target.value.trim() })} /></BaseField>
        <BaseCheckbox disabled={busy || !loaded} label="自动上传到 Gitee" checked={settings.automatic} onCheckedChange={automatic => change({ automatic })} />
        <p className="kb-muted">凭据：{status?.credentialReady ? '后端已配置' : '请设置后端环境变量 DEVFLOW_GITEE_TOKEN 并重启服务'}。Token 需有目标仓库的读写权限。</p>
        <p role="status">{!status?.settings ? '尚未配置同步' : status.error ? '同步失败，可重试' : status.pending ? '有本地变更待同步' : '已同步'}{status?.lastSynced && ` · ${new Date(status.lastSynced * 1000).toLocaleString()}`}</p>
        <p className="kb-muted">同步任务正文、验收、交付、功能和布局；活动记录仅保留在本地，不上传。移除工作区路径与执行身份。任务正文中你手动填写的内容会一同上传。远端变更时暂停覆盖。</p>
      </fieldset>
      <footer><BaseButton type="button" disabled={busy || !saved || !status?.credentialReady} onClick={() => void execute('restore')}>从远端恢复</BaseButton><BaseButton type="button" disabled={busy || !saved || !status?.credentialReady} onClick={() => void execute('push')}>立即同步 / 重试</BaseButton><BaseButton variant="primary" type="submit" disabled={busy || !loaded}>{busy ? '处理中…' : '保存设置'}</BaseButton></footer>
    </form>
  </dialog>
}
