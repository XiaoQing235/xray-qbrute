import { defineConfig } from 'vite'
import vue from '@vitejs/plugin-vue'
import AutoImport from 'unplugin-auto-import/vite'
import Components from 'unplugin-vue-components/vite'
import { ElementPlusResolver } from 'unplugin-vue-components/resolvers'
import { fileURLToPath, URL } from 'node:url'

export default defineConfig(({ mode }) => ({
  base: mode === 'production' ? '/xray-qbrute/' : '/',
  plugins: [
    vue(),
    AutoImport({ dts: 'src/auto-imports.d.ts', resolvers: [ElementPlusResolver()] }),
    Components({ dts: 'src/components.d.ts', resolvers: [ElementPlusResolver()] }),
  ],
  build: {
    minify: 'esbuild',
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: 'vue', test: /node_modules\/vue/ },
            { name: 'element-plus', test: /node_modules\/element-plus/ },
            { name: 'vendor', test: /node_modules/ },
          ],
        },
      },
    },
  },
  resolve: {
    alias: {
      '@': fileURLToPath(new URL('./src', import.meta.url)),
    },
  },
  server: {
    headers: {
      'Cross-Origin-Opener-Policy': 'same-origin',
      'Cross-Origin-Embedder-Policy': 'require-corp',
    },
  },
}))
