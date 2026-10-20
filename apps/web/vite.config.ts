import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { vditorAssets } from './vditorAssets'

export default defineConfig({
  plugins: [react(), tailwindcss(), vditorAssets()],
  server: {
    port: 5173,
    strictPort: true,
    proxy: { '/api': 'http://127.0.0.1:31080', '/mcp': 'http://127.0.0.1:31080' }
  }
})
