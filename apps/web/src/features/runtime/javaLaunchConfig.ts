import type { JavaOperation, Project } from '../../lib/api'

export type JavaLaunchConfig = { jvm: string; args: string; main: string; profiles: string[]; custom: string }

export function readJavaLaunchConfig(project: Project): JavaLaunchConfig {
  try {
    const saved = JSON.parse(localStorage.getItem(`devflow.java.${project.path}`) ?? 'null') as JavaLaunchConfig | null
    if (saved && typeof saved.jvm === 'string' && typeof saved.args === 'string' && typeof saved.main === 'string' && typeof saved.custom === 'string' && Array.isArray(saved.profiles) && saved.profiles.every(value => typeof value === 'string')) return saved
  } catch { /* Use defaults when preferences are unavailable. */ }
  return { jvm: '', args: '', main: project.mainClasses?.length === 1 ? project.mainClasses[0] : '', profiles: [], custom: '' }
}

export function saveJavaLaunchConfig(project: Project, config: JavaLaunchConfig) {
  localStorage.setItem(`devflow.java.${project.path}`, JSON.stringify(config))
}

export function javaStartOperation(config: JavaLaunchConfig): Extract<JavaOperation, { type: 'start' }> {
  const lines = (value: string) => value.split(/\r?\n/).map(line => line.trim()).filter(Boolean)
  return { type: 'start', mainClass: config.main, jvmArgs: lines(config.jvm), args: lines(config.args), profiles: [...new Set([...config.profiles, ...config.custom.split(/[,，\s]+/).filter(Boolean)])] }
}
