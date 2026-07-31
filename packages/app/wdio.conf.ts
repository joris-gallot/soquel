import type { ChildProcess } from 'node:child_process'
import { spawn, spawnSync } from 'node:child_process'
import fs from 'node:fs'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'
import { browser } from '@wdio/globals'

let tauriDriver: ChildProcess | undefined
let exiting = false

// Isolate e2e runs from the real app data and the OS keychain.
const E2E_DATA_DIR = path.join(os.tmpdir(), 'soquel-e2e-data')
process.env.SOQUEL_DATA_DIR = E2E_DATA_DIR
process.env.SOQUEL_EPHEMERAL_SECRETS = '1'

export const config: WebdriverIO.Config = {
  hostname: '127.0.0.1',
  port: 4444,
  specs: ['./e2e/**/*.spec.ts'],
  maxInstances: 1,
  capabilities: [
    {
      // @ts-expect-error tauri-driver capability, unknown to wdio types
      'tauri:options': {
        application: path.resolve(import.meta.dirname, '../../src-tauri/target/debug/soquel'),
      },
    },
  ],
  reporters: ['spec'],
  framework: 'mocha',
  mochaOpts: { ui: 'bdd', timeout: 60_000 },

  // The webdriver session drives a built binary, not a dev server.
  onPrepare: () => {
    fs.rmSync(E2E_DATA_DIR, { recursive: true, force: true })
    fs.mkdirSync(path.join(import.meta.dirname, 'e2e/screenshots'), { recursive: true })
    spawnSync('pnpm', ['tauri', 'build', '--debug', '--no-bundle'], {
      cwd: path.resolve(import.meta.dirname, '../..'),
      stdio: 'inherit',
    })
  },

  beforeSession: () => {
    tauriDriver = spawn(path.resolve(os.homedir(), '.cargo/bin/tauri-driver'), [], {
      stdio: [null, process.stdout, process.stderr],
    })
    tauriDriver.on('error', (error) => {
      console.error('tauri-driver error:', error)
      process.exit(1)
    })
    tauriDriver.on('exit', (code) => {
      if (!exiting) {
        console.error('tauri-driver exited with code:', code)
        process.exit(1)
      }
    })
  },

  afterTest: async (test, _context, { passed }) => {
    if (!passed) {
      const slug = test.title.replace(/\W+/g, '-').toLowerCase()
      await browser.saveScreenshot(`./e2e/screenshots/FAIL-${slug}.png`)
    }
  },

  afterSession: () => closeTauriDriver(),
}

function closeTauriDriver() {
  exiting = true
  tauriDriver?.kill()
}

// afterSession does not run when the session fails to start.
for (const signal of ['exit', 'SIGINT', 'SIGTERM', 'SIGHUP'] as const)
  process.on(signal, closeTauriDriver)
