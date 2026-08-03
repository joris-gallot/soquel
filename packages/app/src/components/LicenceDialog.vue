<script setup lang="ts">
import { ChevronRight } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { Button } from '@/components/ui/button'
import { Collapsible, CollapsibleContent, CollapsibleTrigger } from '@/components/ui/collapsible'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { useLicence } from '@/composables/useLicence'
import { formatDay } from '@/lib/format'
import { ACTIVATION_MESSAGES } from '@/lib/licence'
import { CommandError } from '@/lib/result'

const { status, install, activate } = useLicence()
const open = defineModel<boolean>('open', { required: true })

const key = ref('')
const token = ref('')
const pasting = ref(false)
const outcome = ref<{ ok: boolean, message: string } | null>(null)
const busy = ref(false)

watch(open, (isOpen) => {
  if (isOpen) {
    key.value = ''
    token.value = ''
    pasting.value = false
    outcome.value = null
  }
})

const until = computed(() =>
  formatDay(status.value.kind === 'free' ? null : status.value.updatesUntil))

/// A refused activation carries why, and each reason asks something different of
/// the buyer. Anything else is already a sentence.
function explain(thrown: unknown): string {
  if (thrown instanceof CommandError && thrown.raw.kind === 'activation')
    return ACTIVATION_MESSAGES[thrown.raw.reason]
  return thrown instanceof Error ? thrown.message : String(thrown)
}

/// A licence can install and still unlock nothing, so success cannot be one phrase.
function installed(): string {
  return status.value.kind === 'licensed'
    ? 'Licence added. Tabs are unlimited from here.'
    : 'Licence added, and it does not cover this build.'
}

async function run(action: () => Promise<void>) {
  busy.value = true
  outcome.value = null
  try {
    await action()
    outcome.value = { ok: status.value.kind === 'licensed', message: installed() }
  }
  catch (thrown) {
    outcome.value = { ok: false, message: explain(thrown) }
  }
  finally {
    busy.value = false
  }
}

async function apply() {
  if (key.value.trim() === '')
    return
  await run(async () => {
    await activate(key.value)
    key.value = ''
  })
}

async function applyFile() {
  if (token.value.trim() === '')
    return
  await run(async () => {
    await install(token.value)
    token.value = ''
  })
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

      <div class="min-w-0 space-y-3">
        <Input
          v-model="key"
          data-testid="licence-key"
          class="font-mono text-xs"
          placeholder="SOQUEL-XXXX-XXXX-XXXX"
          @keydown.enter="apply"
        />

        <!-- Kept, and kept second: it needs no network, it outlives this service,
             and it is how a licence gets handed out with no order behind it. -->
        <Collapsible v-model:open="pasting">
          <CollapsibleTrigger
            data-testid="licence-file-toggle"
            class="text-muted-foreground hover:text-foreground flex items-center gap-1 font-mono text-xs"
          >
            <ChevronRight class="size-3 transition-transform" :class="{ 'rotate-90': pasting }" />
            I have a licence file
          </CollapsibleTrigger>
          <CollapsibleContent class="space-y-2 pt-2">
            <!-- min-w-0: the token is one unbroken string, and the textarea sizes to
                 its content, so without a break point it widens the dialog off screen. -->
            <Textarea
              v-model="token"
              data-testid="licence-token"
              class="h-24 max-h-24 w-full min-w-0 resize-none font-mono text-xs break-all"
              placeholder="Paste your licence file"
            />
            <Button
              type="button"
              variant="outline"
              size="sm"
              data-testid="apply-licence-file"
              :disabled="busy || token.trim() === ''"
              @click="applyFile"
            >
              Add licence file
            </Button>
          </CollapsibleContent>
        </Collapsible>

        <p
          v-if="outcome"
          data-testid="licence-outcome"
          class="font-mono text-xs"
          :class="outcome.ok ? 'text-emerald-500' : 'text-destructive'"
        >
          {{ outcome.message }}
        </p>
      </div>

      <DialogFooter class="gap-2">
        <Button type="button" variant="outline" @click="open = false">
          Close
        </Button>
        <Button
          type="button"
          data-testid="apply-licence"
          :disabled="busy || key.trim() === ''"
          @click="apply"
        >
          {{ busy ? 'Checking…' : 'Activate' }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
