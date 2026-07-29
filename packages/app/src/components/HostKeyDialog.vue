<script setup lang="ts">
import { computed, ref } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useHostKeyTrust } from '@/composables/useHostKeyTrust'

const { pending, showAsDialog, trust, dismiss } = useHostKeyTrust()

const trusting = ref(false)
const open = computed({
  get: () => showAsDialog.value,
  set: (value) => {
    if (!value)
      dismiss()
  },
})

async function confirm() {
  trusting.value = true
  try {
    await trust()
    toast.success('Host key trusted')
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
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md" data-testid="host-key-dialog">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          {{ pending?.previouslyTrusted ? 'Host key changed' : 'Unknown host key' }}
        </DialogTitle>
        <DialogDescription>
          <template v-if="pending?.previouslyTrusted">
            The key for {{ pending?.host }}:{{ pending?.port }} does not match the one trusted before.
            This can mean the server was reinstalled - or that the connection is being intercepted.
          </template>
          <template v-else>
            First connection to {{ pending?.host }}:{{ pending?.port }}. Verify the fingerprint before trusting it.
          </template>
        </DialogDescription>
      </DialogHeader>

      <p class="rounded bg-muted px-3 py-2 font-mono text-xs break-all" data-testid="host-key-fingerprint">
        {{ pending?.fingerprint }}
      </p>

      <DialogFooter class="gap-2">
        <Button variant="outline" @click="dismiss">
          Cancel
        </Button>
        <Button
          :variant="pending?.previouslyTrusted ? 'destructive' : 'default'"
          data-testid="trust-host-key"
          :disabled="trusting"
          @click="confirm"
        >
          Trust and retry
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
