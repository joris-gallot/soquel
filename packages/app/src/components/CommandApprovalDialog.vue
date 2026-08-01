<script setup lang="ts">
import type { SecretSubject } from '@/lib/bindings'
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
import { useCommandApproval } from '@/composables/useCommandApproval'

const { pending, showAsDialog, approve, dismiss } = useCommandApproval()

const SUBJECT_NOUNS: Record<SecretSubject, string> = {
  connection: 'connection',
  tunnel: 'ssh tunnel',
}

const approving = ref(false)
const open = computed({
  get: () => showAsDialog.value,
  set: (value) => {
    if (!value)
      dismiss()
  },
})

async function confirm() {
  approving.value = true
  try {
    await approve()
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    approving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md" data-testid="command-approval-dialog">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          Run a command for {{ pending?.targetName }}?
        </DialogTitle>
        <DialogDescription>
          This {{ pending ? SUBJECT_NOUNS[pending.subject] : '' }} gets its password by running a
          program on this machine. It came from an import, so nothing has run yet. Read it before
          approving.
        </DialogDescription>
      </DialogHeader>

      <!-- One chip per argument, spaced: what runs, and how it splits. -->
      <p class="flex flex-wrap gap-1 rounded bg-muted px-3 py-2 font-mono text-xs break-all" data-testid="command-approval-argv">
        <span v-for="(arg, index) in pending?.argv ?? []" :key="index" class="rounded bg-background px-1 py-0.5">
          {{ arg }}
        </span>
      </p>

      <DialogFooter class="gap-2">
        <Button variant="outline" @click="dismiss">
          Cancel
        </Button>
        <Button
          variant="destructive"
          data-testid="approve-command"
          :disabled="approving"
          @click="confirm"
        >
          Approve and run
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
