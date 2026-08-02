<script setup lang="ts">
import { computed, ref } from 'vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Progress } from '@/components/ui/progress'
import { useUpdater } from '@/composables/useUpdater'

const open = defineModel<boolean>('open', { required: true })

const { available, downloading, progress, installing, install } = useUpdater()

const error = ref<string | null>(null)

const publishedOn = computed(() => {
  const raw = available.value?.pubDate
  if (!raw)
    return null
  const date = new Date(raw)
  return Number.isNaN(date.getTime()) ? null : date.toLocaleDateString()
})

async function run() {
  error.value = null
  try {
    await install()
  }
  catch (err) {
    error.value = err instanceof Error ? err.message : String(err)
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          soquel {{ available?.version }}
        </DialogTitle>
        <DialogDescription>
          You are on {{ available?.currentVersion }}<span v-if="publishedOn">, this one was published on {{ publishedOn }}</span>.
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <p
          v-if="available?.notes"
          data-testid="update-notes"
          class="max-h-48 overflow-y-auto whitespace-pre-line text-sm text-muted-foreground"
        >
          {{ available.notes }}
        </p>

        <div v-if="downloading" class="space-y-1.5">
          <Progress :model-value="progress === null ? undefined : progress * 100" />
          <p class="font-mono text-xs text-muted-foreground">
            {{ installing ? 'Installing…' : 'Downloading…' }}
          </p>
        </div>

        <p v-if="error" data-testid="update-error" class="font-mono text-xs text-destructive">
          {{ error }}
        </p>
      </div>

      <DialogFooter class="gap-2">
        <Button type="button" variant="outline" :disabled="downloading" @click="open = false">
          Later
        </Button>
        <Button type="button" data-testid="run-update" :disabled="downloading" @click="run">
          Install and restart
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
