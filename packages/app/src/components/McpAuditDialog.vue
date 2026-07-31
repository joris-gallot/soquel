<script setup lang="ts">
import type { AuditEntry } from '@/lib/bindings'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const props = defineProps<{ names: Record<string, string> }>()
const open = defineModel<boolean>('open', { required: true })
const entries = ref<AuditEntry[]>([])

watch(open, async (isOpen) => {
  if (!isOpen)
    return
  try {
    entries.value = unwrap(await commands.mcpAuditLog(null))
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
})

function time(ts: number | null) {
  return new Date(ts ?? 0).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit' })
}

function target(entry: AuditEntry) {
  if (entry.connection === null)
    return ''
  return props.names[entry.connection] ?? entry.connection
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-2xl" data-testid="audit-dialog">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          Agent activity
        </DialogTitle>
        <DialogDescription>
          Every tool call agents made through the MCP server, newest first.
        </DialogDescription>
      </DialogHeader>

      <p v-if="entries.length === 0" class="py-8 text-center text-sm text-muted-foreground" data-testid="audit-empty">
        Nothing yet. Calls appear here as agents use your connections.
      </p>

      <ul v-else class="max-h-96 divide-y overflow-auto rounded border">
        <li
          v-for="(entry, index) in entries"
          :key="`${entry.ts}-${index}`"
          class="flex gap-3 px-3 py-2"
          data-testid="audit-row"
        >
          <span
            class="mt-1.5 size-1.5 shrink-0 rounded-full"
            :class="entry.ok ? 'bg-[oklch(0.72_0.11_240)]' : 'bg-destructive'"
          />
          <div class="min-w-0 flex-1">
            <div class="flex items-baseline gap-2 font-mono text-xs">
              <span class="font-medium">{{ entry.tool }}</span>
              <span class="truncate text-muted-foreground">{{ target(entry) }}</span>
              <span class="flex-1" />
              <span class="shrink-0 text-muted-foreground">{{ time(entry.ts) }}</span>
            </div>
            <p v-if="entry.detail" class="truncate font-mono text-xs text-muted-foreground">
              {{ entry.detail }}
            </p>
            <p v-if="entry.error" class="font-mono text-xs text-destructive">
              {{ entry.error }}
            </p>
          </div>
        </li>
      </ul>
    </DialogContent>
  </Dialog>
</template>
