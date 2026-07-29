import type { Error as CoreError } from '@/lib/bindings'

type CommandResult<T>
  = | { status: 'ok', data: T }
    | { status: 'error', error: CoreError }

export class CommandError extends Error {
  constructor(public readonly raw: CoreError) {
    super(raw.message)
  }

  get kind(): CoreError['kind'] {
    return this.raw.kind
  }
}

export function unwrap<T>(result: CommandResult<T>): T {
  if (result.status === 'error')
    throw new CommandError(result.error)
  return result.data
}
