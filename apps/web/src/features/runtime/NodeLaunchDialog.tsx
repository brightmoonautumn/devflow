import { useEffect, useId, useRef, useState } from 'react'
import { Save, Settings2, X } from 'lucide-react'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseField } from '../../components/form/BaseField'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseTextarea } from '../../components/form/BaseTextarea'
import { getNodeOptions, type NodeOptions, type Project } from '../../lib/api'
import { readNodeLaunchConfig, saveNodeLaunchConfig, type NodeLaunchConfig } from './nodeLaunchConfig'

export function NodeLaunchDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const id = useId()
  const [config, setConfig] = useState(() => readNodeLaunchConfig(project))
  const [options, setOptions] = useState<NodeOptions | null>(null)
  const [error, setError] = useState('')
  useEffect(() => { const current = dialog.current; current?.showModal(); return () => current?.close() }, [])
  useEffect(() => {
    const controller = new AbortController()
    getNodeOptions(project, controller.signal).then(value => {
      if (controller.signal.aborted) return
      setOptions(value)
      setConfig(current => ({ ...current, script: value.scripts.includes(current.script) ? current.script : value.scripts[0] ?? '' }))
    }).catch(err => { if (!controller.signal.aborted) setError(String(err)) })
    return () => controller.abort()
  }, [project])
  function save() {
    if (!options?.scripts.includes(config.script)) return
    try { saveNodeLaunchConfig(project, config); onClose() } catch { setError('无法保存启动配置，请检查浏览器是否允许本地存储。') }
  }
  return <dialog ref={dialog} className="ws-dialog ws-location-dialog java-launch-dialog" aria-labelledby={`${id}-title`} onCancel={onClose}>
    <header className="ws-location-heading"><div className="ws-dialog-symbol"><Settings2 size={21} /></div><div><h2 id={`${id}-title`}>启动配置</h2><p>{project.name}</p></div><button className="ws-icon-button" aria-label="关闭启动配置" onClick={onClose}><X size={17} /></button></header>
    <form onSubmit={event => { event.preventDefault(); save() }}>
      <div className="ws-location-body java-launch-fields">
        <BaseField label="启动脚本" htmlFor={`${id}-script`} hint="只列出 package.json 中识别到的服务启动脚本。">
          <BaseSelect id={`${id}-script`} aria-describedby={`${id}-script-hint`} value={config.script} onValueChange={script => setConfig({ ...config, script })} disabled={!options} placeholder="正在读取启动脚本…" options={(options?.scripts ?? []).map(script => ({ value: script, label: script }))} />
        </BaseField>
        <BaseField label="包管理器" htmlFor={`${id}-manager`}>
          <BaseSelect id={`${id}-manager`} value={config.manager} onValueChange={manager => setConfig({ ...config, manager: manager as NodeLaunchConfig['manager'] })} options={[{ value: 'auto', label: `自动识别${options ? `（${options.packageManager}）` : ''}` }, ...['npm', 'pnpm', 'yarn'].map(value => ({ value, label: value }))]} />
        </BaseField>
        <BaseField label="附加参数" htmlFor={`${id}-args`} hint="每行一个完整参数，直接传给所选脚本；不填写时使用脚本自身配置。">
          <BaseTextarea mono id={`${id}-args`} aria-describedby={`${id}-args-hint`} rows={3} value={config.args} onChange={event => setConfig({ ...config, args: event.target.value })} placeholder={'--host=127.0.0.1\n--port=5174'} />
        </BaseField>
        <p className="java-launch-hint">在根 package.json 所在目录启动，共享该工程的依赖。DevFlow 不自动安装依赖；保存后下次启动生效。</p>
        {error && <p className="java-console-error" role="alert">{error}</p>}
      </div>
      <footer className="ws-location-footer java-launch-footer"><BaseButton onClick={onClose}>取消</BaseButton><BaseButton type="submit" variant="primary" disabled={!options?.scripts.includes(config.script)}><Save size={13} />保存配置</BaseButton></footer>
    </form>
  </dialog>
}
