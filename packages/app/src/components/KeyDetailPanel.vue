<script setup lang="ts">
import type { KeyDetail } from '@/lib/bindings'
import { Trash2 } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Textarea } from '@/components/ui/textarea'
import { commands } from '@/lib/bindings'
import { formatTtl, KEY_KIND_BADGE } from '@/lib/kv'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string, keyName: string | null }>()
const emit = defineEmits<{ deleted: [key: string], changed: [] }>()

const detail = ref<KeyDetail | null>(null)
const loading = ref(false)
const draft = ref('')
const saving = ref(false)
const ttlSeconds = ref('')
const confirmingDelete = ref(false)

const dirty = computed(() =>
  detail.value?.value.kind === 'string' && draft.value !== detail.value.value.value,
)

const sampleTruncated = computed(() => {
  const value = detail.value?.value
  if (!value || value.kind === 'string' || value.kind === 'other')
    return null
  return value.entries.length < (detail.value?.size ?? 0)
    ? { shown: value.entries.length, total: detail.value!.size }
    : null
})

async function load() {
  confirmingDelete.value = false
  ttlSeconds.value = ''
  if (props.keyName === null) {
    detail.value = null
    return
  }
  loading.value = true
  try {
    detail.value = unwrap(await commands.keyDetail(props.connectionId, props.keyName))
    draft.value = detail.value.value.kind === 'string' ? detail.value.value.value : ''
    ttlSeconds.value = detail.value.ttlMs === null ? '' : String(Math.ceil(detail.value.ttlMs / 1000))
  }
  catch (error) {
    detail.value = null
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    loading.value = false
  }
}

watch(() => [props.connectionId, props.keyName], load, { immediate: true })

async function saveString() {
  if (!detail.value)
    return
  saving.value = true
  try {
    unwrap(await commands.kvSetString(props.connectionId, detail.value.key, draft.value))
    toast.success('Value saved')
    emit('changed')
    await load()
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    saving.value = false
  }
}

async function applyTtl(ms: number | null) {
  if (!detail.value)
    return
  try {
    unwrap(await commands.kvSetTtl(props.connectionId, detail.value.key, ms))
    emit('changed')
    await load()
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

function submitTtl() {
  const seconds = Number(ttlSeconds.value)
  if (!Number.isFinite(seconds) || seconds <= 0) {
    toast.error('TTL must be a positive number of seconds')
    return
  }
  applyTtl(seconds * 1000)
}

async function removeKey() {
  if (!detail.value)
    return
  if (!confirmingDelete.value) {
    confirmingDelete.value = true
    return
  }
  try {
    unwrap(await commands.kvDeleteKey(props.connectionId, detail.value.key))
    toast.success('Key deleted')
    emit('deleted', detail.value.key)
    detail.value = null
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    confirmingDelete.value = false
  }
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col" data-testid="key-detail">
    <div
      v-if="!detail"
      class="flex flex-1 items-center justify-center font-mono text-sm text-muted-foreground"
    >
      <p v-if="loading">
        loading key…
      </p>
      <p v-else>
        soquel=#<span class="ml-1 text-muted-foreground/60">select a key</span>
      </p>
    </div>

    <template v-else>
      <header class="flex items-center gap-2 border-b px-3 py-2">
        <Badge
          variant="outline"
          class="border-transparent font-mono text-[10px]"
          :class="KEY_KIND_BADGE[detail.value.kind === 'other' ? 'other' : detail.value.kind].classes"
        >
          {{ detail.value.kind }}
        </Badge>
        <span class="min-w-0 flex-1 truncate font-mono text-sm" data-testid="detail-key">{{ detail.key }}</span>
        <span class="shrink-0 font-mono text-[11px] text-muted-foreground">
          {{ detail.value.kind === 'string' ? `${detail.size} bytes` : `${detail.size} entries` }}
        </span>
        <Button
          size="sm"
          :variant="confirmingDelete ? 'destructive' : 'ghost'"
          class="h-6 text-[11px]"
          data-testid="delete-key"
          @click="removeKey"
          @blur="confirmingDelete = false"
        >
          <Trash2 class="size-3" />
          {{ confirmingDelete ? 'sure?' : 'delete' }}
        </Button>
      </header>

      <div class="flex items-center gap-2 border-b px-3 py-1.5 font-mono text-[11px] text-muted-foreground">
        <span data-testid="detail-ttl">
          ttl {{ detail.ttlMs === null ? 'none' : formatTtl(detail.ttlMs) }}
        </span>
        <span class="flex-1" />
        <Input
          v-model="ttlSeconds"
          placeholder="seconds"
          type="number"
          class="h-6 w-24 font-mono text-[11px]"
          data-testid="ttl-input"
          @keydown.enter="submitTtl"
        />
        <Button size="sm" variant="ghost" class="h-6 text-[11px]" data-testid="ttl-apply" @click="submitTtl">
          set ttl
        </Button>
        <Button
          v-if="detail.ttlMs !== null"
          size="sm"
          variant="ghost"
          class="h-6 text-[11px]"
          data-testid="ttl-persist"
          @click="applyTtl(null)"
        >
          persist
        </Button>
      </div>

      <div class="min-h-0 flex-1 overflow-auto p-3" data-testid="key-value">
        <template v-if="detail.value.kind === 'string'">
          <Textarea
            v-model="draft"
            class="min-h-40 font-mono text-xs"
            data-testid="string-editor"
          />
          <Button
            size="sm"
            class="mt-2"
            data-testid="save-string"
            :disabled="!dirty || saving"
            @click="saveString"
          >
            {{ saving ? 'Saving…' : 'Save value' }}
          </Button>
        </template>

        <table
          v-else-if="detail.value.kind === 'list' || detail.value.kind === 'set'"
          class="w-full font-mono text-xs"
        >
          <tbody>
            <tr v-for="(entry, index) in detail.value.entries" :key="index" class="border-b border-border/40">
              <td class="w-10 py-1 pr-3 text-right text-muted-foreground">
                {{ index }}
              </td>
              <td class="py-1 break-all">
                {{ entry }}
              </td>
            </tr>
          </tbody>
        </table>

        <table v-else-if="detail.value.kind === 'zset'" class="w-full font-mono text-xs">
          <tbody>
            <tr v-for="entry in detail.value.entries" :key="entry.member" class="border-b border-border/40">
              <td class="w-24 py-1 pr-3 text-right text-muted-foreground tabular-nums">
                {{ entry.score }}
              </td>
              <td class="py-1 break-all">
                {{ entry.member }}
              </td>
            </tr>
          </tbody>
        </table>

        <table v-else-if="detail.value.kind === 'hash'" class="w-full font-mono text-xs">
          <tbody>
            <tr v-for="entry in detail.value.entries" :key="entry.field" class="border-b border-border/40">
              <td class="w-40 py-1 pr-3 align-top break-all text-muted-foreground">
                {{ entry.field }}
              </td>
              <td class="py-1 break-all">
                {{ entry.value }}
              </td>
            </tr>
          </tbody>
        </table>

        <div v-else-if="detail.value.kind === 'stream'" class="space-y-2 font-mono text-xs">
          <div v-for="entry in detail.value.entries" :key="entry.id" class="border-b border-border/40 pb-1.5">
            <p class="text-muted-foreground">
              {{ entry.id }}
            </p>
            <p v-for="field in entry.fields" :key="field.field" class="break-all">
              <span class="text-muted-foreground">{{ field.field }}:</span> {{ field.value }}
            </p>
          </div>
        </div>

        <p v-else class="font-mono text-xs text-muted-foreground">
          unsupported type {{ detail.value.kind === 'other' ? detail.value.typeName : '' }}
        </p>

        <p v-if="sampleTruncated" class="mt-2 font-mono text-[11px] text-muted-foreground italic">
          showing the first {{ sampleTruncated.shown }} of {{ sampleTruncated.total }} entries
        </p>
      </div>
    </template>
  </div>
</template>
