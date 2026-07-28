import type { ChildProcess } from 'node:child_process'
import { spawn, spawnSync } from 'node:child_process'
import os from 'node:os'
import path from 'node:path'
import process from 'node:process'

let tauriDriver: ChildProcess | undefined
let exiting = false

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

  afterSession: () => closeTauriDriver(),
}

function closeTauriDriver() {
  exiting = true
  tauriDriver?.kill()
}

// afterSession does not run when the session fails to start.
for (const signal of ['exit', 'SIGINT', 'SIGTERM', 'SIGHUP'] as const)
  process.on(signal, closeTauriDriver)
