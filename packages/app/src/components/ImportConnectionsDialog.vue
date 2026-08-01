<script setup lang="ts">
import type { DuplicateStrategy, ImportPreview, ImportSource } from '@/lib/bindings'
import { Cable, Database, KeyRound, Lock, SquareTerminal, TriangleAlert } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
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
import { RadioGroup, RadioGroupItem } from '@/components/ui/radio-group'
import { Switch } from '@/components/ui/switch'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'
import { DUPLICATE_STRATEGIES, DUPLICATE_STRATEGY_LABELS, importOutcomeMessage, importPlan } from '@/lib/transfer'

const props = defineProps<{ source: ImportSource | null }>()
const emit = defineEmits<{ imported: [] }>()
const open = defineModel<boolean>('open', { required: true })

const preview = ref<ImportPreview | null>(null)
const passphrase = ref('')
const withSecrets = ref(false)
const strategy = ref<DuplicateStrategy>('skip')
const busy = ref(false)
const error = ref<string | null>(null)
// Sticky: a rejected passphrase must leave the field on screen to retry in.
const locked = ref(false)

watch(open, (isOpen) => {
  if (!isOpen)
    return
  preview.value = null
  passphrase.value = ''
  withSecrets.value = false
  strategy.value = 'skip'
  error.value = null
  locked.value = false
  load()
})

async function load() {
  if (!props.source)
    return
  busy.value = true
  error.value = null
  try {
    const result = unwrap(await commands.previewImport(props.source, passphrase.value || null))
    preview.value = result
    locked.value = result.needsPassphrase
  }
  catch (err) {
    preview.value = null
    error.value = err instanceof Error ? err.message : String(err)
  }
  finally {
    busy.value = false
  }
}

const plan = computed(() => (preview.value ? importPlan(preview.value) : null))
const blocked = computed(() => (plan.value?.problems.length ?? 0) > 0)

async function run() {
  if (!props.source || blocked.value)
    return
  busy.value = true
  error.value = null
  try {
    const outcome = unwrap(await commands.runImport(
      props.source,
      passphrase.value || null,
      withSecrets.value,
      strategy.value,
    ))
    open.value = false
    emit('imported')
    toast.success(importOutcomeMessage(outcome))
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
    <DialogContent class="sm:max-w-lg">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          Import connections
        </DialogTitle>
        <DialogDescription class="truncate font-mono text-xs">
          {{ source?.path }}
        </DialogDescription>
      </DialogHeader>

      <div class="space-y-4">
        <div v-if="locked" class="space-y-1.5">
          <Label for="import-passphrase" class="flex items-center gap-1.5">
            <Lock class="size-3.5" />
            This file is encrypted
          </Label>
          <div class="flex gap-2">
            <Input
              id="import-passphrase"
              v-model="passphrase"
              data-testid="import-passphrase"
              type="password"
              autocomplete="off"
              placeholder="Passphrase"
              @keydown.enter.prevent="load"
            />
            <Button type="button" variant="secondary" :disabled="busy || passphrase === ''" @click="load">
              Unlock
            </Button>
          </div>
        </div>

        <template v-if="plan && !locked">
          <div class="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
            <span data-testid="import-counts" class="font-mono">
              {{ preview!.connections.length }} connections, {{ preview!.tunnels.length }} tunnels
            </span>
            <Badge v-if="preview!.encrypted" variant="outline" class="gap-1 font-mono text-[10px]">
              <Lock class="size-2.5" />
              encrypted
            </Badge>
            <Badge v-if="plan.secrets > 0" variant="outline" class="gap-1 font-mono text-[10px]">
              <KeyRound class="size-2.5" />
              {{ plan.secrets }} passwords
            </Badge>
            <Badge
              v-if="plan.commands > 0"
              variant="outline"
              data-testid="import-commands"
              class="gap-1 border-amber-500/30 font-mono text-[10px] text-amber-500"
            >
              <SquareTerminal class="size-2.5" />
              {{ plan.commands }} run a command
            </Badge>
          </div>

          <ul class="max-h-56 divide-y overflow-y-auto rounded-md border">
            <li
              v-for="entry in plan.entries"
              :key="`${entry.name}-${entry.target}`"
              data-testid="import-entry"
              class="flex items-center gap-2 px-3 py-2"
            >
              <component
                :is="entry.kind === 'tunnel' ? Cable : Database"
                class="size-3.5 shrink-0 text-muted-foreground"
              />
              <div class="min-w-0 flex-1">
                <p class="truncate text-sm">
                  {{ entry.name }}
                </p>
                <p class="truncate font-mono text-xs text-muted-foreground">
                  {{ entry.target }}
                </p>
              </div>
              <Badge v-if="entry.problem" variant="outline" data-testid="import-problem" class="gap-1 border-destructive/30 text-[10px] text-destructive">
                <TriangleAlert class="size-2.5" />
                {{ entry.problem }}
              </Badge>
              <Badge
                v-else-if="entry.hasCommand"
                variant="outline"
                data-testid="import-entry-command"
                class="gap-1 border-amber-500/30 font-mono text-[10px] text-amber-500"
              >
                <SquareTerminal class="size-2.5" />
                command
              </Badge>
              <Badge v-else-if="entry.duplicate" variant="outline" class="font-mono text-[10px]" data-testid="import-duplicate">
                exists
              </Badge>
            </li>
          </ul>

          <div v-if="plan.secrets > 0" class="flex items-start justify-between gap-4 rounded-md border p-3">
            <div class="space-y-0.5">
              <Label for="import-secrets" class="text-sm">Bring the passwords</Label>
              <p class="text-xs text-muted-foreground">
                {{ withSecrets
                  ? `${plan.secrets} passwords land in the keychain.`
                  : 'Off: the connections arrive without them, ready to re-enter.' }}
              </p>
            </div>
            <Switch id="import-secrets" v-model="withSecrets" data-testid="import-with-secrets" />
          </div>

          <div v-if="plan.duplicates > 0 && !blocked" class="space-y-2">
            <Label>{{ plan.duplicates }} already here</Label>
            <RadioGroup v-model="strategy" class="gap-2">
              <div v-for="option in DUPLICATE_STRATEGIES" :key="option" class="flex items-start gap-2">
                <RadioGroupItem :id="`strategy-${option}`" :value="option" class="mt-0.5" :data-testid="`strategy-${option}`" />
                <Label :for="`strategy-${option}`" class="grid gap-0.5 font-normal">
                  <span class="text-sm">{{ DUPLICATE_STRATEGY_LABELS[option].label }}</span>
                  <span class="text-xs text-muted-foreground">{{ DUPLICATE_STRATEGY_LABELS[option].hint }}</span>
                </Label>
              </div>
            </RadioGroup>
          </div>

          <p v-if="blocked" class="text-xs text-destructive">
            Nothing is imported while an entry is invalid: fix the file, or remove those entries.
          </p>
          <p v-else class="text-xs text-muted-foreground">
            Imported connections stay hidden from agents whatever the file says.
            <template v-if="plan.commands > 0">
              A credential command runs nothing until you read it and approve it.
            </template>
          </p>
        </template>

        <p v-if="error" data-testid="import-error" class="font-mono text-xs text-destructive">
          {{ error }}
        </p>
      </div>

      <DialogFooter class="gap-2">
        <Button type="button" variant="outline" @click="open = false">
          Cancel
        </Button>
        <Button
          type="button"
          data-testid="run-import"
          :disabled="busy || blocked || plan === null || locked"
          @click="run"
        >
          {{ busy ? 'Working…' : 'Import' }}
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
