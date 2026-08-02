<script setup lang="ts">
import { Check, Copy, FolderOpen } from '@lucide/vue'
import { useClipboard } from '@vueuse/core'
import { computed, ref, watch } from 'vue'
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
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const open = defineModel<boolean>('open', { required: true })

const block = ref('')
const error = ref<string | null>(null)
const { copy, copied } = useClipboard({ legacy: true })
const { copy: copyPath, copied: pathCopied } = useClipboard({ legacy: true })

/// The block already names it; pulling it out saves fishing for it by hand when
/// opening the folder leads nowhere.
const logPath = computed(() =>
  block.value.split('\n').find(line => line.startsWith('log: '))?.slice(5) ?? '',
)

watch(open, async (isOpen) => {
  if (!isOpen)
    return
  error.value = null
  try {
    block.value = unwrap(await commands.diagnostics())
  }
  catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
})

async function openFolder() {
  try {
    // The opener spawns detached: a session with no file manager opens nothing
    // and still reports success. The path above is the fallback, on screen already.
    unwrap(await commands.openLogFolder())
  }
  catch (err) {
    toast.error(err instanceof Error ? err.message : String(err))
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          Diagnostics
        </DialogTitle>
        <DialogDescription>
          Safe to paste into a bug report: no connection names, no hosts, and never
          the contents of the log.
        </DialogDescription>
      </DialogHeader>

      <pre
        v-if="block"
        data-testid="diagnostics-block"
        class="max-h-64 overflow-auto rounded-md border bg-muted/40 p-3 font-mono text-xs leading-relaxed select-text"
      >{{ block }}</pre>
      <p v-if="error" data-testid="diagnostics-error" class="font-mono text-xs text-destructive">
        {{ error }}
      </p>

      <DialogFooter class="gap-2 sm:justify-between">
        <div class="flex gap-2">
          <Button type="button" variant="outline" data-testid="open-log-folder" @click="openFolder">
            <FolderOpen />
            Open log folder
          </Button>
          <Button
            v-if="logPath"
            type="button"
            variant="ghost"
            data-testid="copy-log-path"
            @click="copyPath(logPath)"
          >
            {{ pathCopied ? 'Path copied' : 'Copy path' }}
          </Button>
        </div>
        <Button type="button" data-testid="copy-diagnostics" :disabled="!block" @click="copy(block)">
          <component :is="copied ? Check : Copy" />
          {{ copied ? 'Copied' : 'Copy' }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
