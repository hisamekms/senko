import { defineConfig } from 'vite'
import { devtools } from '@tanstack/devtools-vite'
import { tanstackStart } from '@tanstack/react-start/plugin/vite'
import viteReact from '@vitejs/plugin-react'

const port = Number(process.env.WEB_DEV_PORT ?? 3000)

const config = defineConfig({
  resolve: { tsconfigPaths: true },
  server: { port, strictPort: false },
  preview: { port, strictPort: false },
  plugins: [
    devtools(),
    tanstackStart({ server: { entry: './server-entry' } }),
    viteReact(),
  ],
})

export default config
