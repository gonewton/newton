import { test, expect } from '@playwright/test';

test('executions page renders', async ({ page }) => {
  await page.goto('/executions');
  await expect(page).toHaveTitle(/Newton Console/);
});
