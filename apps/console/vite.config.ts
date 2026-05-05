import { defineConfig } from 'vite';
import react from '@vitejs/plugin-react';

const appPort = Number(process.env.VITE_PORT || 3000);
const apiProxyTarget = process.env.VITE_API_PROXY_TARGET || 'http://127.0.0.1:3001';

export default defineConfig({
  plugins: [react()],
  server: {
    port: appPort,
    watch: {
      ignored: ['**/node_modules/**', '**/target/**', '**/.git/**', '**/dist/**'],
    },
    proxy: {
      '/api': {
        target: apiProxyTarget,
        changeOrigin: true,
        ws: true,
      },
    },
  },
  preview: {
    port: appPort,
    strictPort: true,
    proxy: {
      '/api': {
        target: apiProxyTarget,
        changeOrigin: true,
        ws: true,
      },
    },
  },
});
