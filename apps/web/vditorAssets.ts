import { readFile, readdir } from 'node:fs/promises'
import { fileURLToPath } from 'node:url'
import type { Plugin } from 'vite'

/** Serve/package only the editor's named runtime assets, never workspace files. */
export function vditorAssets(): Plugin {
  const root = new URL('./node_modules/vditor/', import.meta.url)
  const prefix = '/vendor/vditor/'
  const files = [
    'LICENSE', 'dist/js/lute/lute.min.js', 'dist/js/i18n/zh_CN.js', 'dist/js/icons/ant.js',
    'dist/js/highlight.js/highlight.min.js', 'dist/js/highlight.js/third-languages.js', 'dist/js/highlight.js/LICENSE',
    'dist/js/highlight.js/styles/github.min.css', 'dist/js/highlight.js/styles/github-dark.min.css',
    'dist/css/content-theme/light.css', 'dist/css/content-theme/dark.css',
    'dist/js/katex/katex.min.js', 'dist/js/katex/katex.min.css', 'dist/js/katex/mhchem.min.js',
  ]
  let manifest: Map<string, URL> | undefined
  let building = false
  async function assets() {
    if (!manifest) {
      // One known font directory; no recursive dependency or project discovery.
      const fonts = await readdir(new URL('dist/js/katex/fonts/', root))
      manifest = new Map([...files, ...fonts.filter(name => /\.(?:woff2?|ttf)$/.test(name)).map(name => `dist/js/katex/fonts/${name}`)].map(path => [path, new URL(path, root)]))
    }
    return manifest
  }
  return {
    name: 'devflow-vditor-assets',
    configResolved(config) { building = config.command === 'build' },
    async configureServer(server) {
      const allowed = await assets()
      server.middlewares.use((req, res, next) => {
        const pathname = (req.url ?? '').split('?')[0]
        if (!pathname.startsWith(prefix)) return next()
        const file = allowed.get(pathname.slice(prefix.length))
        if (!file || !['GET', 'HEAD'].includes(req.method ?? '')) { res.statusCode = 404; res.end(); return }
        void readFile(file).then(bytes => {
          res.setHeader('Content-Type', file.pathname.endsWith('.js') ? 'text/javascript; charset=utf-8' : file.pathname.endsWith('.css') ? 'text/css; charset=utf-8' : file.pathname.endsWith('.woff2') ? 'font/woff2' : file.pathname.endsWith('.woff') ? 'font/woff' : file.pathname.endsWith('.ttf') ? 'font/ttf' : 'text/plain; charset=utf-8')
          res.end(req.method === 'HEAD' ? undefined : bytes)
        }).catch(next)
      })
    },
    async buildStart() {
      if (!building) return
      for (const [path, file] of await assets()) {
        this.addWatchFile(fileURLToPath(file))
        this.emitFile({ type: 'asset', fileName: `vendor/vditor/${path}`, source: await readFile(file) })
      }
    },
  }
}
