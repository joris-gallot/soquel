<script setup lang="ts">
import { ShieldAlert } from '@lucide/vue'
import { ref } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { useHostKeyTrust } from '@/composables/useHostKeyTrust'

const { pending, trust, dismiss, claimInline } = useHostKeyTrust()

claimInline()

const trusting = ref(false)

async function confirm() {
  trusting.value = true
  try {
    await trust()
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    trusting.value = false
  }
}
</script>

<template>
  <div
    v-if="pending"
    data-testid="host-key-panel"
    class="space-y-2 rounded-md border border-amber-500/40 bg-amber-500/5 p-3"
    :class="pending.previouslyTrusted && 'border-destructive/40 bg-destructive/5'"
  >
    <p class="flex items-center gap-2 text-xs font-medium">
      <ShieldAlert class="size-3.5" :class="pending.previouslyTrusted ? 'text-destructive' : 'text-amber-500'" />
      {{ pending.previouslyTrusted ? 'Host key changed' : 'Unknown host key' }}
    </p>
    <p class="text-xs text-muted-foreground">
      <template v-if="pending.previouslyTrusted">
        {{ pending.host }}:{{ pending.port }} presents a different key than the one trusted before. The server may
        have been reinstalled, or the connection is being intercepted.
      </template>
      <template v-else>
        First contact with {{ pending.host }}:{{ pending.port }}. Verify the fingerprint before trusting it.
      </template>
    </p>
    <p class="font-mono text-[11px] break-all" data-testid="host-key-fingerprint">
      {{ pending.fingerprint }}
    </p>
    <div class="flex gap-2">
      <Button
        size="sm"
        :variant="pending.previouslyTrusted ? 'destructive' : 'default'"
        data-testid="trust-host-key"
        :disabled="trusting"
        @click="confirm"
      >
        Trust and retry
      </Button>
      <Button size="sm" variant="ghost" @click="dismiss">
        Cancel
      </Button>
    </div>
  </div>
</template>
