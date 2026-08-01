<script setup lang="ts">
import { computed, onMounted } from 'vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { useAgentApprovals } from '@/composables/useAgentApprovals'
import { refreshTrustWindows } from '@/composables/useTrustWindows'

const { pending, queue, listen, resolve } = useAgentApprovals()

onMounted(listen)

// Dismissing the dialog is a refusal: silence must never mean yes.
const open = computed({
  get: () => pending.value !== null,
  set: (value) => {
    if (!value)
      resolve('deny')
  },
})

async function allowForWindow() {
  await resolve('for-window')
  await refreshTrustWindows()
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-lg" data-testid="approval-dialog">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          An agent wants to write
        </DialogTitle>
        <DialogDescription>
          This changes data on
          <span class="font-medium text-foreground">{{ pending?.connectionName }}</span>. It runs only if you allow it.
        </DialogDescription>
      </DialogHeader>

      <pre
        class="max-h-60 overflow-auto rounded bg-muted px-3 py-2 font-mono text-xs whitespace-pre-wrap"
        data-testid="approval-operation"
      >{{ pending?.operation }}</pre>

      <div v-if="pending?.payload" class="space-y-1.5">
        <p class="text-xs text-muted-foreground">
          What it writes
        </p>
        <pre
          class="max-h-60 overflow-auto rounded bg-muted px-3 py-2 font-mono text-xs whitespace-pre-wrap"
          data-testid="approval-payload"
        >{{ pending.payload }}</pre>
      </div>

      <p v-if="queue.length > 1" class="font-mono text-xs text-muted-foreground">
        {{ queue.length - 1 }} more waiting
      </p>

      <DialogFooter class="gap-2 sm:justify-between">
        <Button variant="outline" data-testid="approval-deny" @click="resolve('deny')">
          Deny
        </Button>
        <div class="flex gap-2">
          <Button variant="secondary" data-testid="approval-allow-window" @click="allowForWindow">
            Allow for 15 min
          </Button>
          <Button variant="destructive" data-testid="approval-allow" @click="resolve('once')">
            Run this write
          </Button>
        </div>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
