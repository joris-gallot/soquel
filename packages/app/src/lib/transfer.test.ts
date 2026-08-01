import type { ImportPreview, PreviewEntry } from '@/lib/bindings'
import { describe, expect, it } from 'vitest'
import { exportSummaryMessage, importOutcomeMessage, importPlan, passphraseIssue } from '@/lib/transfer'

function entry(overrides: Partial<PreviewEntry> = {}): PreviewEntry {
  return {
    name: 'prod',
    target: 'db.internal:5432/app',
    hasSecret: false,
    hasCommand: false,
    duplicate: false,
    problem: null,
    ...overrides,
  }
}

function preview(overrides: Partial<ImportPreview> = {}): ImportPreview {
  return {
    encrypted: false,
    needsPassphrase: false,
    connections: [],
    tunnels: [],
    ...overrides,
  }
}

describe('passphraseIssue', () => {
  it('asks for a usable length', () => {
    expect(passphraseIssue('short', 'short')).toContain('8 characters')
  })

  it('catches a typo in the confirmation', () => {
    expect(passphraseIssue('correct horse', 'correct hose')).toContain('do not match')
  })

  it('accepts a confirmed passphrase', () => {
    expect(passphraseIssue('correct horse', 'correct horse')).toBeNull()
  })
})

describe('importPlan', () => {
  it('reads connections and tunnels as one list, tagged by kind', () => {
    const plan = importPlan(preview({
      connections: [entry(), entry({ name: 'cache', target: 'localhost:6379/2' })],
      tunnels: [entry({ name: 'bastion', target: 'deploy@bastion.internal:22' })],
    }))
    expect(plan.entries).toHaveLength(3)
    expect(plan.entries.map(item => item.kind)).toEqual(['connection', 'connection', 'tunnel'])
  })

  it('counts duplicates, secrets, commands and problems', () => {
    const plan = importPlan(preview({
      connections: [
        entry({ duplicate: true, hasSecret: true }),
        entry({ name: 'iam', hasCommand: true }),
        entry({ name: 'broken', problem: 'the host is empty' }),
      ],
      tunnels: [entry({ name: 'bastion', duplicate: true, hasCommand: true })],
    }))
    expect(plan.duplicates).toBe(2)
    expect(plan.secrets).toBe(1)
    // Connections and tunnels alike: the dialog warns about both.
    expect(plan.commands).toBe(2)
    expect(plan.problems.map(item => item.name)).toEqual(['broken'])
  })
})

describe('summaries', () => {
  it('leaves out what an export did not carry', () => {
    expect(exportSummaryMessage({ connections: 1, tunnels: 0, secrets: 0 })).toBe('Exported 1 connection')
    expect(exportSummaryMessage({ connections: 4, tunnels: 2, secrets: 3 }))
      .toBe('Exported 4 connections, 2 tunnels, 3 passwords')
  })

  it('reports only the import outcomes that happened', () => {
    expect(importOutcomeMessage({ created: 2, replaced: 0, skipped: 1, tunnelsCreated: 0 }))
      .toBe('Imported: 2 added, 1 skipped')
    expect(importOutcomeMessage({ created: 0, replaced: 0, skipped: 0, tunnelsCreated: 0 }))
      .toBe('Nothing to import')
  })
})
