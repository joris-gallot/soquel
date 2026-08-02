import { describe, expect, it } from 'vitest'
import { mcpPortSchema } from '@/lib/mcp'

describe('mcpPortSchema', () => {
  it('accepts the usable range', () => {
    expect(mcpPortSchema.parse(1024)).toBe(1024)
    expect(mcpPortSchema.parse(52700)).toBe(52700)
    expect(mcpPortSchema.parse(65535)).toBe(65535)
  })

  it('coerces what a number input hands over', () => {
    expect(mcpPortSchema.parse('52799')).toBe(52799)
  })

  it('refuses privileged ports', () => {
    const result = mcpPortSchema.safeParse(1023)
    expect(result.success).toBe(false)
    expect(result.error?.issues[0].message).toBe('Port must be 1024 or above')
  })

  it('refuses what does not fit a port', () => {
    expect(mcpPortSchema.safeParse(65536).error?.issues[0].message).toBe('Port must be below 65536')
    expect(mcpPortSchema.safeParse(52700.5).error?.issues[0].message).toBe('Port must be a whole number')
    expect(mcpPortSchema.safeParse('').success).toBe(false)
  })
})
