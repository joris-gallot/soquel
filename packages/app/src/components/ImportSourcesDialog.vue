<script setup lang="ts">
import type { ImportSource, ImportSourceSummary } from '@/lib/bindings'
import { FileJson, HardDrive } from '@lucide/vue'
import { ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'
import { IMPORT_SOURCE_LABELS, importSourceStatus, pickImportFile, soquelFileSource } from '@/lib/transfer'

const emit = defineEmits<{ chosen: [source: ImportSource] }>()
const open = defineModel<boolean>('open', { required: true })

const summaries = ref<ImportSourceSummary[]>([])
const error = ref<string | null>(null)

watch(open, async (isOpen) => {
  if (!isOpen)
    return
  error.value = null
  summaries.value = []
  try {
    summaries.value = unwrap(await commands.scanImportSources())
  }
  catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
})

function choose(source: ImportSource) {
  open.value = false
  emit('chosen', source)
}

/// The soquel file is the one source with no home of its own: it is picked.
async function chooseFile() {
  const path = await pickImportFile()
  if (path)
    choose(soquelFileSource(path))
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md" data-testid="import-sources-dialog">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          Import connections
        </DialogTitle>
        <DialogDescription>
          What this machine already knows about, and any soquel file you point at.
        </DialogDescription>
      </DialogHeader>

      <ul class="divide-y rounded-md border">
        <li v-for="summary in summaries" :key="summary.kind">
          <button
            type="button"
            class="flex w-full items-center gap-3 px-3 py-2 text-left enabled:hover:bg-accent disabled:opacity-50"
            :data-testid="`import-source-${summary.kind}`"
            :disabled="!summary.entries"
            @click="choose(summary.source)"
          >
            <HardDrive class="size-3.5 shrink-0 text-muted-foreground" />
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm">
                {{ IMPORT_SOURCE_LABELS[summary.kind] }}
              </p>
              <p class="truncate font-mono text-xs text-muted-foreground">
                {{ summary.path }}
              </p>
            </div>
            <span class="shrink-0 font-mono text-xs text-muted-foreground">
              {{ importSourceStatus(summary) }}
            </span>
          </button>
        </li>
        <li>
          <button
            type="button"
            class="flex w-full items-center gap-3 px-3 py-2 text-left hover:bg-accent"
            data-testid="import-source-file"
            @click="chooseFile"
          >
            <FileJson class="size-3.5 shrink-0 text-muted-foreground" />
            <div class="min-w-0 flex-1">
              <p class="truncate text-sm">
                A soquel file
              </p>
              <p class="truncate font-mono text-xs text-muted-foreground">
                exported from soquel, here or elsewhere
              </p>
            </div>
            <span class="shrink-0 font-mono text-xs text-muted-foreground">choose…</span>
          </button>
        </li>
      </ul>

      <p v-if="error" data-testid="import-sources-error" class="font-mono text-xs text-destructive">
        {{ error }}
      </p>

      <DialogFooter>
        <Button type="button" variant="outline" @click="open = false">
          Cancel
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
