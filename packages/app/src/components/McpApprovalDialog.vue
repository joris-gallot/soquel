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

const { pending, queue, listen, resolve } = useAgentApprovals()

onMounted(listen)

// Dismissing the dialog is a refusal: silence must never mean yes.
const open = computed({
  get: () => pending.value !== null,
  set: (value) => {
    if (!value)
      resolve(false)
  },
})
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

      <DialogFooter class="gap-2">
        <Button variant="outline" data-testid="approval-deny" @click="resolve(false)">
          Deny
        </Button>
        <Button variant="destructive" data-testid="approval-allow" @click="resolve(true)">
          Run this write
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
