import { z } from 'zod'

/// Matches the core's floor: below 1024, binding needs root on unix.
export const mcpPortSchema = z.coerce
  .number()
  .int('Port must be a whole number')
  .min(1024, 'Port must be 1024 or above')
  .max(65535, 'Port must be below 65536')
