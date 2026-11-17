import { resolve } from 'node:path'
import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'

const variablesFile = resolve(__dirname, 'src/assets/styles/variables.scss').replace(/\\/g, '/')

// Tauri 前端构建配置：开发端口固定为 1420，构建产物输出到 dist
export default defineConfig({
  plugins: [vue()],
  clearScreen: false,
  resolve: {
    alias: {
      '@': resolve(__dirname, 'src')
    }
  },
  server: {
    port: 1420,
    strictPort: true,
    watch: {
      ignored: ['**/src-tauri/**']
    }
  },
  envPrefix: ['VITE_', 'TAURI_'],
  css: {
    preprocessorOptions: {
      scss: {
        additionalData: `@use "${variablesFile}" as *;\n`
      }
    }
  },
  build: {
    target: 'chrome110',
    sourcemap: false
  }
})
