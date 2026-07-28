import type { Error as CoreError } from '@/lib/bindings'

type CommandResult<T>
  = | { status: 'ok', data: T }
    | { status: 'error', error: CoreError }

export class CommandError extends Error {
  constructor(public readonly kind: CoreError['kind'], message: string) {
    super(message)
  }
}

export function unwrap<T>(result: CommandResult<T>): T {
  if (result.status === 'error')
    throw new CommandError(result.error.kind, result.error.message)
  return result.data
}
