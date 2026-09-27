// Renders public/favicon.svg to public/icon-192.png and public/icon-512.png, the
// web manifest's raster icons (docs/frontends/wasm.md#offline), on the page's light
// background from src/tokens.css so a maskable icon has no transparent corners.
// Uses the browser Playwright already brings: `npm run icons`, output committed.
//
//   PW_CHANNEL=chrome node scripts/icons.mjs   (the installed Chrome instead)
import { readFileSync, writeFileSync } from 'node:fs'
import { dirname, resolve } from 'node:path'
import { fileURLToPath } from 'node:url'
import { chromium } from '@playwright/test'

const here = dirname(fileURLToPath(import.meta.url))
const pub = resolve(here, '..', 'public')
const svg = readFileSync(resolve(pub, 'favicon.svg'), 'utf8')
const background = '#f7f8f7'

const browser = await chromium.launch({ channel: process.env.PW_CHANNEL || undefined })
try {
  for (const size of [192, 512]) {
    const page = await browser.newPage({ viewport: { width: size, height: size }, deviceScaleFactor: 1 })
    // The mark sits inside the safe zone a maskable icon keeps (80% of the side).
    const inset = Math.round(size * 0.1)
    await page.setContent(
      `<html><body style="margin:0;background:${background}"><div style="position:absolute;inset:${inset}px">${svg.replace(
        /width="100"\s+height="100"/,
        'width="100%" height="100%"',
      )}</div></body></html>`,
    )
    const png = await page.screenshot({ type: 'png', clip: { x: 0, y: 0, width: size, height: size } })
    writeFileSync(resolve(pub, `icon-${size}.png`), png)
    console.log(`wrote public/icon-${size}.png (${png.length} bytes)`)
    await page.close()
  }
} finally {
  await browser.close()
}
