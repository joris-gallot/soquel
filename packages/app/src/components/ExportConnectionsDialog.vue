<script setup lang="ts">
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
import { Input } from '@/components/ui/input'
import { Label } from '@/components/ui/label'
import { Switch } from '@/components/ui/switch'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'
import { exportSummaryMessage, passphraseIssue, pickExportPath } from '@/lib/transfer'

const open = defineModel<boolean>('open', { required: true })

const includeSecrets = ref(false)
const passphrase = ref('')
const confirmation = ref('')
const busy = ref(false)
const error = ref<string | null>(null)

watch(open, (isOpen) => {
  if (isOpen) {
    includeSecrets.value = false
    passphrase.value = ''
    confirmation.value = ''
    error.value = null
  }
})

const issue = computed(() =>
  includeSecrets.value ? passphraseIssue(passphrase.value, confirmation.value) : null,
)

async function run() {
  if (issue.value) {
    error.value = issue.value
    return
  }
  const path = await pickExportPath()
  if (!path)
    return
  busy.value = true
  error.value = null
  try {
    const summary = unwrap(await commands.exportConnections(
      path,
      includeSecrets.value,
      includeSecrets.value ? passphrase.value : null,
    ))
    open.value = false
    toast.success(exportSummaryMessage(summary))
  }
  catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
  finally {
    busy.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          Export connections
        </DialogTitle>
        <DialogDescription>
          Every connection, group and SSH tunnel in one file. Host keys stay on this
          machine: you confirm them again on the first connect.
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <div class="flex items-start justify-between gap-4 rounded-md border p-3">
          <div class="space-y-0.5">
            <Label for="export-secrets" class="text-sm">Include passwords</Label>
            <p class="text-xs text-muted-foreground">
              {{ includeSecrets
                ? 'The file is encrypted with your passphrase. Lose it and the file is unreadable.'
                : 'Off: the file is plain text and safe to share, passwords get re-entered on the other side.' }}
            </p>
          </div>
          <Switch id="export-secrets" v-model="includeSecrets" data-testid="export-include-secrets" />
        </div>

        <div v-if="includeSecrets" class="space-y-3">
          <div class="space-y-1.5">
            <Label for="export-passphrase">Passphrase</Label>
            <Input
              id="export-passphrase"
              v-model="passphrase"
              data-testid="export-passphrase"
              type="password"
              autocomplete="new-password"
            />
          </div>
          <div class="space-y-1.5">
            <Label for="export-passphrase-confirm">Confirm passphrase</Label>
            <Input
              id="export-passphrase-confirm"
              v-model="confirmation"
              data-testid="export-passphrase-confirm"
              type="password"
              autocomplete="new-password"
            />
          </div>
        </div>

        <p v-if="error" data-testid="export-error" class="font-mono text-xs text-destructive">
          {{ error }}
        </p>
      </div>

      <DialogFooter class="gap-2">
        <Button type="button" variant="outline" @click="open = false">
          Cancel
        </Button>
        <Button type="button" data-testid="run-export" :disabled="busy" @click="run">
          {{ busy ? 'Exporting…' : 'Choose a file…' }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
