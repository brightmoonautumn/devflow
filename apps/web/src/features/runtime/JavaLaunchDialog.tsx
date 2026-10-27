import { useEffect, useId, useRef, useState } from 'react'
import { Save, Settings2, X } from 'lucide-react'
import { executeJava, getJavaOptions, type JavaOptions, type LocalJavaDependency, type Project } from '../../lib/api'
import { readJavaLaunchConfig, saveJavaLaunchConfig } from './javaLaunchConfig'
import { BaseButton } from '../../components/form/BaseButton'
import { BaseCheckbox } from '../../components/form/BaseCheckbox'
import { BaseField } from '../../components/form/BaseField'
import { BaseInput } from '../../components/form/BaseInput'
import { BaseSelect } from '../../components/form/BaseSelect'
import { BaseTextarea } from '../../components/form/BaseTextarea'

export function JavaLaunchDialog({ project, revision, onClose }: { project: Project; revision: number; onClose: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const formId = useId()
  const [options, setOptions] = useState<JavaOptions | null>(null)
  const [error, setError] = useState('')
  const [dependencies, setDependencies] = useState<LocalJavaDependency[]>([])
  const [dependencyDraft, setDependencyDraft] = useState('')
  const [saving, setSaving] = useState(false)
  const [config, setConfig] = useState(() => readJavaLaunchConfig(project))
  useEffect(() => { const current = dialog.current; current?.showModal(); return () => current?.close() }, [])
  useEffect(() => { const controller = new AbortController(); getJavaOptions(project.path, controller.signal).then(value => { if (!controller.signal.aborted) { setOptions(value); setDependencies(value.localDependencies ?? []) } }).catch(err => { if (!controller.signal.aborted) setError(String(err)) }); return () => controller.abort() }, [project.path])
  function addDependency() {
    const value = dependencyDraft.trim()
    if (value && !dependencies.some(item => item.value === value)) setDependencies(current => [...current, { value, enabled: true }])
    setDependencyDraft('')
  }
  async function save() {
    if (!options || !options.mainClasses.includes(config.main)) return
    setError('')
    setSaving(true)
    try {
      saveJavaLaunchConfig(project, config)
      const value = dependencyDraft.trim()
      const values = value && !dependencies.some(item => item.value === value) ? [...dependencies, { value, enabled: true }] : dependencies
      await executeJava(project.path, revision, { type: 'configure_dependencies', dependencies: values })
      onClose()
    } catch (err) { setError(`无法保存启动配置：${String(err)}`) }
    finally { setSaving(false) }
  }
  return <dialog ref={dialog} className="ws-dialog ws-location-dialog java-launch-dialog" aria-labelledby="java-launch-title" onCancel={onClose}>
    <header className="ws-location-heading"><div className="ws-dialog-symbol"><Settings2 size={21} /></div><div><h2 id="java-launch-title">启动配置</h2><p>{project.name}</p></div><button className="ws-icon-button" aria-label="关闭启动配置" onClick={onClose}><X size={17} /></button></header>
    <form onSubmit={event => { event.preventDefault(); if (!saving) void save() }}>
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
        <BaseField htmlFor={`${formId}-build`} label="启动前构建" hint={config.buildMode === 'install' ? '按需执行 install，可能触发项目中的打包、复制依赖等插件。' : '按需构建变化模块和受影响的依赖链；没有变化时直接启动。'}>
          <BaseSelect id={`${formId}-build`} value={config.buildMode} onValueChange={value => setConfig(current => ({ ...current, buildMode: value === 'install' ? 'install' : 'workspace' }))} options={[{ value: 'workspace', label: '工作区按需构建（推荐）' }, { value: 'install', label: '构建并安装（兼容特殊产物）' }]} />
        </BaseField>
        <BaseField htmlFor={`${formId}-maven-profiles`} label="Maven Profile" hint="多个名称用逗号分隔，与下方 Spring Profile 独立；留空沿用 Maven 默认配置。">
          <BaseInput id={`${formId}-maven-profiles`} value={config.mavenProfiles} onChange={event => setConfig({ ...config, mavenProfiles: event.target.value })} placeholder="例如 local-build" />
        </BaseField>
        <BaseCheckbox label="每次重新构建目标及上游（忽略缓存）" checked={config.forceBuild} onCheckedChange={forceBuild => setConfig(current => ({ ...current, forceBuild }))} />
        <fieldset className="java-profile-fields" disabled={saving || !options}><legend>本地运行依赖</legend>
          <p className="df-field-hint">仅本机启动时追加，Web 与 MCP 共用；不修改 POM，不参与编译和打包。本地 JAR 须位于当前工作区项目目录内。</p>
          {dependencies.map((item, index) => <div key={item.value} className="local-dependency-row">
            <BaseCheckbox label={item.value} checked={item.enabled} onCheckedChange={enabled => setDependencies(current => current.map((entry, i) => i === index ? { ...entry, enabled } : entry))} />
            <BaseButton aria-label={`删除 ${item.value}`} onClick={() => setDependencies(current => current.filter((_, i) => i !== index))}><X size={13} /></BaseButton>
          </div>)}
          <BaseField htmlFor={`${formId}-dependency`} label="Maven 坐标或 JAR 绝对路径" hint="Maven 坐标会解析运行时传递依赖；取消勾选可临时禁用。">
            <BaseInput id={`${formId}-dependency`} value={dependencyDraft} onChange={event => setDependencyDraft(event.target.value)} onKeyDown={event => { if (event.key === 'Enter') { event.preventDefault(); addDependency() } }} placeholder="dm.jdbc.driver:dm8:20250513" />
          </BaseField>
          <BaseButton onClick={addDependency} disabled={!dependencyDraft.trim() || dependencies.length >= 64}>添加依赖</BaseButton>
        </fieldset>
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
      <footer className="ws-location-footer java-launch-footer"><BaseButton onClick={onClose} disabled={saving}>取消</BaseButton><BaseButton type="submit" variant="primary" disabled={saving || !options || !options.mainClasses.includes(config.main)}><Save size={13} />{saving ? '正在保存…' : '保存配置'}</BaseButton></footer>
    </form>
  </dialog>
}
