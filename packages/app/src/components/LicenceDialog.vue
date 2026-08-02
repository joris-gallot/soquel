<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Textarea } from '@/components/ui/textarea'
import { useLicence } from '@/composables/useLicence'
import { formatDay } from '@/lib/format'

const { status, install } = useLicence()
const open = defineModel<boolean>('open', { required: true })

const token = ref('')
const error = ref<string | null>(null)
const busy = ref(false)

watch(open, (isOpen) => {
  if (isOpen) {
    token.value = ''
    error.value = null
  }
})

const until = computed(() =>
  formatDay(status.value.kind === 'free' ? null : status.value.updatesUntil))

async function apply() {
  if (token.value.trim() === '')
    return
  busy.value = true
  error.value = null
  try {
    await install(token.value)
    token.value = ''
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
          Licence
        </DialogTitle>
        <DialogDescription>
          <template v-if="status.kind === 'licensed'">
            Licensed to {{ status.email }}<span v-if="until">, with updates through {{ until }}</span>.
          </template>
          <!-- Expired and free both limit the app; only saying so tells the
               difference between a lapsed window and a bug. -->
          <template v-else-if="status.kind === 'expired'">
            Your updates ran until {{ until }}, and this build came out after that, so it runs on
            the free tier. Earlier builds keep working with this licence, and renewing reopens the
            newer ones.
          </template>
          <template v-else>
            The free tier opens two tabs per connection, with everything else included, agent
            access and all. A licence lifts the limit for good.
          </template>
        </DialogDescription>
      </DialogHeader>

      <!-- min-w-0: the token is one unbroken string, and the textarea sizes to its
           content, so without a break point it widens the dialog off screen. -->
      <div class="min-w-0 space-y-2">
        <Textarea
          v-model="token"
          data-testid="licence-token"
          class="h-24 max-h-24 w-full resize-none font-mono text-xs break-all"
          placeholder="Paste your licence here"
        />
        <p v-if="error" data-testid="licence-error" class="font-mono text-xs text-destructive">
          {{ error }}
        </p>
      </div>

      <DialogFooter class="gap-2">
        <Button type="button" variant="outline" @click="open = false">
          Close
        </Button>
        <Button
          type="button"
          data-testid="apply-licence"
          :disabled="busy || token.trim() === ''"
          @click="apply"
        >
          {{ busy ? 'Checking…' : 'Add licence' }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
