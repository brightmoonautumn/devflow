import { copyText } from '../../lib/clipboard'
import { useEffect, useId, useRef, useState } from 'react'
import { Copy, LoaderCircle, Share2, X } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import type { WorkspaceResponse } from '../../lib/api'
import { createShare, listShares, revokeShare, type ShareCatalog } from './api'
import './sharing.css'
export function ShareDialog({ workspace, onClose }: { workspace: WorkspaceResponse | null; onClose: () => void }) {
  const id = useId()
  const [catalog, setCatalog] = useState<ShareCatalog | null>(null)
  const [minutes, setMinutes] = useState(60)
  const [base, setBase] = useState('')
  const [token, setToken] = useState('')
  const [createdId, setCreatedId] = useState('')
  const [notice, setNotice] = useState('')
  const [busy, setBusy] = useState(false)
  const dialog = useRef<HTMLDialogElement>(null)
  const mounted = useRef(true)
  async function load() { const value = await listShares(); if (mounted.current) { setCatalog(value); setBase(old => old || value.network.suggestedUrl || '') } }
  useEffect(() => {
    mounted.current = true
    const current = dialog.current
    current?.showModal()
    void load().catch(error => { if (mounted.current) setNotice(error instanceof Error ? error.message : String(error)) })
    return () => { mounted.current = false; current?.close() }
  }, [])
  function link() {
    if (!token || !base || !catalog?.invitations.some(invite => invite.id === createdId)) return ''
    try {
      const url = new URL(base)
      const port = catalog.network.address?.split(':').at(-1)
      if (url.protocol !== 'http:' || url.username || url.password || (url.port || '80') !== port || !/^\d+\.\d+\.\d+\.\d+$/.test(url.hostname)) return ''
      url.pathname = '/share/'; url.search = ''; url.hash = token
      return url.href
    } catch { return '' }
  }
  const url = link()
  async function act(action: () => Promise<void>) {
    if (busy) return
    setBusy(true); setNotice('')
    try { await action(); await load() } catch (error) { if (mounted.current) setNotice(error instanceof Error ? error.message : String(error)) }
    finally { if (mounted.current) setBusy(false) }
  }
  return <dialog ref={dialog} aria-labelledby={`${id}-title`} aria-describedby={`${id}-description`} className="ws-dialog ws-location-dialog share-dialog" onCancel={event => { event.preventDefault(); if (!busy) onClose() }} onClick={event => { if (event.target === event.currentTarget && !busy) onClose() }} onKeyDown={event => { if (event.key === 'Escape') event.stopPropagation() }}>
    <header className="ws-location-heading">
      <div className="ws-dialog-symbol"><Share2 size={21} /></div>
      <div><h2 id={`${id}-title`}>只读分享</h2><p id={`${id}-description`}>邀请局域网成员查看当前工作区。</p></div>
      <BaseButton className="share-dialog-close" disabled={busy} onClick={onClose} aria-label="关闭分享设置" title="关闭分享设置"><X size={16} /></BaseButton>
    </header>
    <form aria-busy={busy} onSubmit={event => {
      event.preventDefault()
      if (busy || !workspace?.folders.length || !catalog?.network.address) return
      void act(async () => {
        const result = await createShare(workspace.revision, minutes)
        if (mounted.current) { setToken(result.token); setCreatedId(result.invitation.id) }
      })
    }}>
      <div className="ws-location-body share-dialog-body">
        <section className="share-scope" aria-labelledby={`${id}-scope`}>
          <h3 id={`${id}-scope`}>分享范围</h3>
          <p>受邀者可查看目录、文件、搜索结果、Git Diff 和服务日志，无法修改文件或运行服务。</p>
          <ul>{workspace?.folders.map(folder => <li key={folder.path} title={folder.path}>{folder.path}</li>)}</ul>
          <p className="share-muted">链接持有者均可访问这些内容。请分享给可信的局域网成员；重启后台后所有邀请失效。</p>
        </section>
        <div className="share-dialog-fields">
          <BaseField htmlFor={`${id}-lifetime`} label="有效期">
            <BaseSelect id={`${id}-lifetime`} value={String(minutes)} disabled={busy} onValueChange={value => setMinutes(Number(value))} options={[
              { value: '15', label: '15 分钟' }, { value: '60', label: '1 小时' },
              { value: '240', label: '4 小时' }, { value: '1440', label: '24 小时' },
            ]} />
          </BaseField>
          <BaseField htmlFor={`${id}-address`} label="本机局域网访问地址" hint={`使用分享端口 ${catalog?.network.address?.split(':').at(-1) ?? '31082'}。自动识别的地址可按实际网卡修改；若无法连接，请检查 Windows 防火墙和网络是否互通。`}>
            <BaseInput mono id={`${id}-address`} aria-describedby={`${id}-address-hint`} inputMode="url" autoComplete="off" spellCheck={false} value={base} disabled={busy} onChange={event => setBase(event.target.value)} placeholder="http://192.168.1.xxx:31082" />
          </BaseField>
          {token && <BaseField htmlFor={`${id}-link`} label="邀请链接">
            <div className="share-link-row">
              <BaseInput mono id={`${id}-link`} readOnly value={url} placeholder="请填写有效的局域网地址和分享端口" onFocus={event => event.target.select()} />
              <BaseButton disabled={busy || !url} onClick={() => {
                void copyText(url).then(() => { if (mounted.current) setNotice('链接已复制') }).catch(() => { if (mounted.current) setNotice('请选中上方链接手动复制') })
              }}><Copy size={14} />复制链接</BaseButton>
            </div>
          </BaseField>}
        </div>
        {catalog?.network.error && <p className="share-notice is-error" role="alert">{catalog.network.error}</p>}
        <section className="share-invitations" aria-labelledby={`${id}-invitations`}>
          <h3 id={`${id}-invitations`}>有效邀请</h3>
          {catalog?.invitations.map(invite => <div className="share-invitation" key={invite.id}>
            <span title={invite.folders.join('\n')}>{invite.name}<small>到期：{new Date(invite.expiresAt * 1000).toLocaleString()}</small></span>
            <BaseButton variant="danger" disabled={busy} aria-label={`撤销 ${invite.name} 的只读邀请`} onClick={() => void act(async () => { await revokeShare(invite.id); if (invite.id === createdId && mounted.current) setToken('') })}>撤销</BaseButton>
          </div>)}
          {catalog && !catalog.invitations.length && <p className="share-muted">暂无有效邀请</p>}
        </section>
        {notice && <p className="share-notice" role="status">{notice}</p>}
      </div>
      <footer className="ws-location-footer"><div>
        <BaseButton disabled={busy} onClick={onClose}>关闭</BaseButton>
        <BaseButton type="submit" variant="primary" disabled={busy || !workspace?.folders.length || !catalog?.network.address}>
          {busy ? <LoaderCircle size={14} className="spinning" /> : <Share2 size={14} />}{busy ? '处理中…' : '创建只读邀请'}
        </BaseButton>
      </div></footer>
    </form>
  </dialog>
}
