import { test, expect } from '@playwright/test';
import { createServer } from 'node:http';
import { readFile } from 'node:fs/promises';

const dashboard = await readFile(new URL('../src/dashboard.html', import.meta.url));
const magnet = 'magnet:?xt=urn:btih:0000000000000000000000000000000000000001&tr=https%3A%2F%2Ftracker.example%2Fannounce';

function row(overrides = {}) {
  return {
    id: 'test-id',
    info_hash: '0000000000000000000000000000000000000001',
    name: 'sample.bin',
    desired: 'running',
    observed_state: 'live',
    progress_bytes: 4096,
    total_bytes: 8192,
    finished: false,
    download_bytes_per_sec: 64,
    peers: 2,
    eta_secs: 65,
    ...overrides,
  };
}

async function fixture(rows) {
  const requests = [];
  const server = createServer(async (request, response) => {
    const path = new URL(request.url, 'http://127.0.0.1').pathname;
    if (request.method === 'GET' && path === '/case/') {
      response.writeHead(200, { 'content-type': 'text/html; charset=utf-8' });
      response.end(dashboard);
    } else if (request.method === 'GET' && path === '/case/csrf') {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ token: 'fixture-csrf-token' }));
    } else if (request.method === 'GET' && path === '/case/data') {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify(rows));
    } else if (request.method === 'GET' && path === '/case/torrents/test-id/files') {
      response.writeHead(200, { 'content-type': 'application/json' });
      response.end(JSON.stringify({ summary: { total_bytes: 8192, files: [{ path: ['sample.bin'], bytes: 8192 }] }, only_files: null }));
    } else if (request.method === 'POST') {
      const chunks = [];
      for await (const chunk of request) chunks.push(chunk);
      requests.push({ path, headers: request.headers, body: Buffer.concat(chunks).toString() });
      response.writeHead(201, { 'content-type': 'application/json' });
      response.end('{}');
    } else {
      response.writeHead(404).end();
    }
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  return {
    url: `http://127.0.0.1:${server.address().port}/case/`,
    requests,
    close: () => new Promise((resolve, reject) => server.close(error => error ? reject(error) : resolve())),
  };
}

test('shows measured ETA and keeps unavailable ETA explicit', async ({ page }) => {
  const site = await fixture([row(), row({ id: 'paused-id', name: 'paused.bin', desired: 'paused', observed_state: 'paused', eta_secs: null })]);
  try {
    await page.goto(site.url);
    await expect(page.getByText('ETA 1 min 5 s')).toBeVisible();
    await expect(page.getByText('ETA —')).toBeVisible();
  } finally {
    await page.close();
    await site.close();
  }
});

test('shows a measured low download rate without rounding it to zero', async ({ page }) => {
  const site = await fixture([row({ download_bytes_per_sec: 64 })]);
  try {
    await page.goto(site.url);
    await expect(page.locator('.speed')).toHaveText('64 B/s');
  } finally {
    await page.close();
    await site.close();
  }
});

test('keyboard submits magnet with CSRF token and Escape closes file dialog', async ({ page }) => {
  const site = await fixture([row()]);
  try {
    await page.goto(site.url);
    await page.getByLabel('Link magnético').fill(magnet);
    await page.getByLabel('Link magnético').press('Enter');
    await expect.poll(() => site.requests.length).toBe(1);
    expect(site.requests[0].path).toBe('/case/add-magnet');
    expect(site.requests[0].headers['x-csrf-token']).toBe('fixture-csrf-token');
    expect(JSON.parse(site.requests[0].body).magnet).toBe(magnet);
    await page.getByRole('button', { name: 'Arquivos sample.bin' }).focus();
    await page.keyboard.press('Enter');
    await expect(page.getByRole('dialog', { name: 'Arquivos do torrent' })).toBeVisible();
    await page.keyboard.press('Escape');
    await expect(page.getByRole('dialog', { name: 'Arquivos do torrent' })).toBeHidden();
  } finally {
    await page.close();
    await site.close();
  }
});

test('torrent names remain text', async ({ page }) => {
  const site = await fixture([row({ name: '<img src=x onerror=alert(1)>' })]);
  try {
    await page.goto(site.url);
    await expect(page.getByText('<img src=x onerror=alert(1)>')).toBeVisible();
    expect(await page.locator('.transfer img').count()).toBe(0);
  } finally {
    await page.close();
    await site.close();
  }
});
