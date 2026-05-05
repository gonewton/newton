import { defineConfig } from '@playwright/test';

const appPort = Number(process.env.VITE_PORT || 3100);

export default defineConfig({
  testDir: './e2e',
  use: {
    baseURL: `http://127.0.0.1:${appPort}`,
  },
  webServer: [
    {
      command: 'pnpm --filter newton-ui-mock-server start',
      port: 3101,
      reuseExistingServer: !process.env.CI,
    },
    {
      command: `pnpm dev`,
      port: appPort,
      reuseExistingServer: !process.env.CI,
    },
  ],
});
