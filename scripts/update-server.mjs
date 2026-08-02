// Fake update endpoint for driving the updater by hand: one hardcoded release,
// no licence logic. The production server is a separate service, this one must not grow into it.

import { once } from 'node:events'
import { createReadStream, readdirSync, readFileSync, statSync } from 'node:fs'
import { createServer } from 'node:http'
import { join } from 'node:path'
import process from 'node:process'
import { setTimeout as sleep } from 'node:timers/promises'

const dir = process.argv[2] ?? 'update-stub'
const port = Number(process.env.PORT ?? 9010)
const slow = process.env.SLOW !== '0'

const bundle = readdirSync(dir).find(name => name.endsWith('.AppImage'))
if (!bundle)
  throw new Error(`no .AppImage in ${dir}/ (build one with \`pnpm tauri build --debug\`)`)

const path = join(dir, bundle)
const size = statSync(path).size
const signature = readFileSync(`${path}.sig`, 'utf8').trim()
const version = process.env.VERSION ?? bundle.match(/_(\d+\.\d+\.\d+)_/)?.[1]
if (!version)
  throw new Error(`no version in ${bundle}: pass VERSION=x.y.z`)

function isNewer(offered, current) {
  const [a, b] = [offered, current].map(value => value.split('.').map(Number))
  for (let i = 0; i < 3; i++) {
    if (a[i] !== b[i])
      return a[i] > b[i]
  }
  return false
}

async function sendBundle(res) {
  res.writeHead(200, {
    // Without a length the client cannot compute a ratio and the bar stays indeterminate.
    'content-length': String(size),
    'content-type': 'application/octet-stream',
  })
  const stream = createReadStream(path, { highWaterMark: 256 * 1024 })
  for await (const chunk of stream) {
    if (!res.write(chunk))
      await once(res, 'drain')
    // Loopback delivers the whole bundle in a blink; pace it so the progress bar is watchable.
    if (slow)
      await sleep(15)
  }
  res.end()
}

const server = createServer((req, res) => {
  const url = new URL(req.url, 'http://127.0.0.1')
  if (url.pathname === '/download') {
    console.log(`-> serving ${bundle} (${(size / 1e6).toFixed(1)} MB)`)
    sendBundle(res).catch(error => console.error(error))
    return
  }
  // Endpoint shape: /{target}/{arch}/{current_version}
  const current = url.pathname.split('/').filter(Boolean).pop() ?? ''
  const licence = req.headers['x-soquel-license'] ?? 'none'
  if (!/^\d+\.\d+\.\d+$/.test(current)) {
    console.log(`?? ${url.pathname} has no version in its last segment, answering 204`)
    res.writeHead(204).end()
    return
  }
  if (!isNewer(version, current)) {
    console.log(`-> ${current} is up to date (licence: ${licence})`)
    res.writeHead(204).end()
    return
  }
  console.log(`-> offering ${version} to ${current} (licence: ${licence})`)
  res.writeHead(200, { 'content-type': 'application/json' })
  res.end(JSON.stringify({
    notes: `Local stub build ${version}.`,
    pub_date: new Date().toISOString(),
    signature,
    url: `http://127.0.0.1:${port}/download`,
    version,
  }))
})

server.listen(port, '127.0.0.1', () => {
  console.log(`serving ${bundle} as ${version} on http://127.0.0.1:${port}`)
  console.log(`run the older build with:\n  SOQUEL_UPDATE_ENDPOINT='http://127.0.0.1:${port}/{{target}}/{{arch}}/{{current_version}}' ./soquel_<older>.AppImage`)
})
