// Prepare the current platform's pinned search binary; never downloads at runtime.
const fs = require('node:fs')
const path = require('node:path')
const crypto = require('node:crypto')

async function prepareRipgrep() {
  const root = path.resolve(__dirname, '..')
  const { rgPath } = await import('@vscode/ripgrep')
  const wrapperPath = path.resolve(path.dirname(require.resolve('@vscode/ripgrep')), '..', 'package.json')
  const wrapper = JSON.parse(fs.readFileSync(wrapperPath, 'utf8'))
  const platformPath = path.resolve(path.dirname(rgPath), '..')
  const platform = JSON.parse(fs.readFileSync(path.join(platformPath, 'package.json'), 'utf8'))
  if (!fs.statSync(rgPath).isFile() || platform.version !== wrapper.version || platform.name !== `@vscode/ripgrep-${process.platform}-${process.arch}`) {
    throw new Error('ripgrep binary package is missing or has a mismatched version. Run pnpm install with optional dependencies enabled.')
  }
  const destination = path.join(root, 'tools', 'ripgrep')
  fs.mkdirSync(destination, { recursive: true })
  const target = path.join(destination, process.platform === 'win32' ? 'rg.exe' : 'rg')
  // Rename a completed copy so concurrent starts never see a partial executable.
  const digest = file => crypto.createHash('sha256').update(fs.readFileSync(file)).digest('hex')
  const sha256 = digest(rgPath)
  if (!fs.existsSync(target) || digest(target) !== sha256) {
    const temporary = `${target}.${process.pid}.tmp`
    try {
      fs.copyFileSync(rgPath, temporary)
      if (process.platform !== 'win32') fs.chmodSync(temporary, 0o755)
      fs.renameSync(temporary, target)
    } finally {
      fs.rmSync(temporary, { force: true })
    }
  }
  fs.copyFileSync(path.join(path.dirname(wrapperPath), 'LICENSE'), path.join(destination, 'LICENSE-vscode-ripgrep.txt'))
  fs.copyFileSync(path.join(platformPath, 'LICENSE'), path.join(destination, 'LICENSE-platform-package.txt'))
  fs.copyFileSync(path.join(__dirname, 'ripgrep-LICENSE-MIT.txt'), path.join(destination, 'LICENSE-ripgrep.txt'))
  fs.writeFileSync(path.join(destination, 'manifest.json'), JSON.stringify({
    package: wrapper.name,
    version: wrapper.version,
    platform: process.platform,
    arch: process.arch,
    sha256,
  }, null, 2) + '\n', 'utf8')
  console.log(`Prepared @vscode/ripgrep ${wrapper.version} (${process.platform}/${process.arch}) in tools/ripgrep`)
}

prepareRipgrep().catch(error => {
  console.error(`Unable to prepare DevFlow search: ${error.message}`)
  process.exitCode = 1
})
