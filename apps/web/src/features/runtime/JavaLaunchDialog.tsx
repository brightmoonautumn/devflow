import { useEffect, useId, useRef, useState } from 'react'
import { Save, Settings2, X } from 'lucide-react'
import { getJavaOptions, type JavaOptions, type Project } from '../../lib/api'
import { readJavaLaunchConfig, saveJavaLaunchConfig } from './javaLaunchConfig'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseCheckbox } from '../../components/form/BaseCheckbox'
import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseTextarea } from '../../components/form/BaseTextarea'

export function JavaLaunchDialog({ project, onClose }: { project: Project; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const formId = useId()
  const [options, setOptions] = useState<JavaOptions | null>(null)
  const [error, setError] = useState('')
  const [config, setConfig] = useState(() => readJavaLaunchConfig(project))
  useEffect(() => { const current = dialog.current; current?.showModal(); return () => current?.close() }, [])
  useEffect(() => { const controller = new AbortController(); getJavaOptions(project.path, controller.signal).then(value => { if (!controller.signal.aborted) setOptions(value) }).catch(err => { if (!controller.signal.aborted) setError(String(err)) }); return () => controller.abort() }, [project.path])
  function save() {
    if (!options || !options.mainClasses.includes(config.main)) return
    setError('')
    try {
      saveJavaLaunchConfig(project, config)
      onClose()
    } catch { setError('无法保存启动配置，请检查浏览器是否允许本地存储。') }
  }
  return <dialog ref={dialog} className="ws-dialog ws-location-dialog java-launch-dialog" aria-labelledby="java-launch-title" onCancel={onClose}>
    <header className="ws-location-heading"><div className="ws-dialog-symbol"><Settings2 size={21} /></div><div><h2 id="java-launch-title">启动配置</h2><p>{project.name}</p></div><button className="ws-icon-button" aria-label="关闭启动配置" onClick={onClose}><X size={17} /></button></header>
    <form onSubmit={event => { event.preventDefault(); save() }}>
      <div className="ws-location-body java-launch-fields">
        <BaseField htmlFor={`${formId}-main`} label="启动主类">
          <BaseSelect id={`${formId}-main`} required mono value={config.main} disabled={!options} onValueChange={main => setConfig(current => ({ ...current, main }))} placeholder={!options ? error ? '启动主类读取失败' : '正在读取启动主类…' : options.mainClasses.length ? '请选择启动主类' : '未发现启动主类'} options={(options?.mainClasses ?? project.mainClasses ?? []).map(main => ({ value: main, label: main }))} />
        </BaseField>
        <BaseField htmlFor={`${formId}-jvm`} label="JVM 参数" hint="每行一个完整参数，空格属于参数值，无须加引号。">
          <BaseTextarea id={`${formId}-jvm`} aria-describedby={`${formId}-jvm-hint`} mono rows={3} value={config.jvm} onChange={event => setConfig({ ...config, jvm: event.target.value })} placeholder={'-Xms256m\n-Xmx1024m\n-Dname=中文 空格'} />
        </BaseField>
        <BaseField htmlFor={`${formId}-args`} label="程序参数">
          <BaseTextarea id={`${formId}-args`} mono rows={3} value={config.args} onChange={event => setConfig({ ...config, args: event.target.value })} placeholder={'--server.port=8080\n--example.name=hello world'} />
        </BaseField>
        <fieldset className="java-profile-fields"><legend>Spring Profile</legend>
          <div className="java-profile-list">{options?.profiles.map(profile => <BaseCheckbox key={profile} label={profile} checked={config.profiles.includes(profile)} onCheckedChange={checked => setConfig(current => ({ ...current, profiles: checked ? [...current.profiles, profile] : current.profiles.filter(value => value !== profile) }))} />)}</div>
          {options && !options.profiles.length && <p className="df-field-hint java-profile-empty">未发现预设 Profile，可在下方填写。</p>}
          <BaseField htmlFor={`${formId}-profiles`} label="其他 Profile" hint="多个名称用逗号分隔；不选择时沿用项目默认配置。">
            <BaseInput id={`${formId}-profiles`} aria-describedby={`${formId}-profiles-hint`} value={config.custom} onChange={event => setConfig({ ...config, custom: event.target.value })} placeholder="例如 dev, local" />
          </BaseField>
        </fieldset>
        <p className="java-launch-hint">保存后，下次点击服务的启动按钮时使用这些配置；正在运行的服务不受影响。</p>
        {error && <p className="java-console-error" role="alert">{error}</p>}
      </div>
      <footer className="ws-location-footer java-launch-footer"><BaseButton onClick={onClose}>取消</BaseButton><BaseButton type="submit" variant="primary" disabled={!options || !options.mainClasses.includes(config.main)}><Save size={13} />保存配置</BaseButton></footer>
    </form>
  </dialog>
}
