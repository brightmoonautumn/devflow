import { useEffect, useRef, useState } from 'react'
import { Play, X } from 'lucide-react'
import { executeJava, getJavaOptions, type JavaOptions, type Project } from '../../lib/api'

type Config = { jvm: string; args: string; main: string; profiles: string[]; custom: string }
export function JavaLaunchDialog({ project, revision, onClose, onStarted }: { project: Project; revision: number; onClose: () => void; onStarted: () => void }) {
  const dialog = useRef<HTMLDialogElement>(null)
  const [options, setOptions] = useState<JavaOptions | null>(null)
  const [error, setError] = useState('')
  const [busy, setBusy] = useState(false)
  const [config, setConfig] = useState<Config>(() => {
    try { const saved = JSON.parse(localStorage.getItem(`devflow.java.${project.path}`) ?? 'null') as Config | null; if (saved && typeof saved.jvm === 'string' && typeof saved.args === 'string' && typeof saved.main === 'string' && typeof saved.custom === 'string' && Array.isArray(saved.profiles) && saved.profiles.every(value => typeof value === 'string')) return saved } catch { /* Use defaults when preferences are unavailable. */ }
    return { jvm: '', args: '', main: project.mainClasses?.length === 1 ? project.mainClasses[0] : '', profiles: [], custom: '' }
  })
  useEffect(() => { const current = dialog.current; current?.showModal(); return () => current?.close() }, [])
  useEffect(() => { const controller = new AbortController(); getJavaOptions(project.path, controller.signal).then(value => { if (!controller.signal.aborted) setOptions(value) }).catch(err => { if (!controller.signal.aborted) setError(String(err)) }); return () => controller.abort() }, [project.path])
  async function start() {
    if (busy || !options || !options.mainClasses.includes(config.main)) return
    setBusy(true); setError('')
    try {
      const lines = (value: string) => value.split(/\r?\n/).map(line => line.trim()).filter(Boolean)
      await executeJava(project.path, revision, { type: 'start', mainClass: config.main, jvmArgs: lines(config.jvm), args: lines(config.args), profiles: [...new Set([...config.profiles, ...config.custom.split(/[,，\s]+/).filter(Boolean)])] })
      try { localStorage.setItem(`devflow.java.${project.path}`, JSON.stringify(config)) } catch { /* Run does not depend on local storage. */ }
      onStarted()
    } catch (err) { setError(err instanceof Error ? err.message : String(err)) } finally { setBusy(false) }
  }
  return <dialog ref={dialog} className="ws-dialog ws-location-dialog java-launch-dialog" aria-labelledby="java-launch-title" onCancel={event => { if (busy) event.preventDefault(); else onClose() }}>
    <header className="ws-location-heading"><div className="ws-dialog-symbol"><Play size={21} /></div><div><h2 id="java-launch-title">启动 Java 服务</h2><p>{project.name}</p></div><button className="ws-icon-button" disabled={busy} aria-label="关闭启动配置" onClick={onClose}><X size={17} /></button></header>
    <form onSubmit={event => { event.preventDefault(); void start() }}>
      <div className="ws-location-body java-launch-fields">
        <label>启动主类<select required value={config.main} onChange={event => setConfig({ ...config, main: event.target.value })}><option value="">请选择启动主类</option>{(options?.mainClasses ?? project.mainClasses ?? []).map(main => <option key={main}>{main}</option>)}</select></label>
        <label>JVM 参数<textarea rows={3} value={config.jvm} onChange={event => setConfig({ ...config, jvm: event.target.value })} placeholder={'-Xms256m\n-Xmx1024m\n-Dname=中文 空格'} /><small>每行一个完整参数，空格属于参数值，无须加引号。</small></label>
        <label>程序参数<textarea rows={3} value={config.args} onChange={event => setConfig({ ...config, args: event.target.value })} placeholder={'--server.port=8080\n--example.name=hello world'} /></label>
        <fieldset><legend>Spring Profile</legend><div className="java-profile-list">{options?.profiles.map(profile => <label key={profile}><input type="checkbox" checked={config.profiles.includes(profile)} onChange={event => setConfig({ ...config, profiles: event.target.checked ? [...config.profiles, profile] : config.profiles.filter(value => value !== profile) })} />{profile}</label>)}</div><label>其他 Profile<input value={config.custom} onChange={event => setConfig({ ...config, custom: event.target.value })} placeholder="多个名称用逗号分隔" /></label><small>不选择时沿用项目默认配置；这里激活的是 Spring Profile。</small></fieldset>
        <p className="java-launch-hint">启动前由 Maven 编译并解析运行依赖，随后直接运行所选 Java 主类。构建和程序输出都会显示在下方面板。</p>
        {error && <p className="java-console-error" role="alert">{error}</p>}
      </div>
      <footer className="ws-location-footer"><button type="button" className="ws-outline-button" disabled={busy} onClick={onClose}>取消</button><button type="submit" className="ws-launch is-start" disabled={busy || !options || !options.mainClasses.includes(config.main)}><Play size={12} fill="currentColor" />{busy ? '正在提交…' : '启动'}</button></footer>
    </form>
  </dialog>
}
