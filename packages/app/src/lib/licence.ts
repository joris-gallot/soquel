import type { ActivationReason } from '@/lib/bindings'

/// The service says what happened, the app says what to do about it. Keyed on the
/// generated union so a new reason stops compiling until it has an answer here.
export const ACTIVATION_MESSAGES: Record<ActivationReason, string> = {
  'offline': 'No answer from the licence server. Check your connection and try again.',
  'unknown-key': 'That key is not one of ours. Copy it again from your Polar account.',
  'wrong-product': 'That key belongs to another product.',
  'revoked': 'That key is no longer valid. Get in touch and we will sort it out.',
  'activation-limit': 'This key has been activated on as many machines as it allows. Free one in your Polar account, or paste the licence file from another machine.',
  // Nothing is wrong with the purchase here, and the wording has to say so: Polar
  // rate limits activation, so pasting the same key twice in a row lands on this.
  'upstream-unavailable': 'The licence server is having trouble reaching Polar. Your key is fine, try again in a minute.',
}
