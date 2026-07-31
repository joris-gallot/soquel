<script setup lang="ts">
import type { DocDetail, DocEntry } from '@/lib/bindings'
import { Check, Copy, Pencil, Trash2 } from '@lucide/vue'
import { useClipboard } from '@vueuse/core'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Textarea } from '@/components/ui/textarea'
import { commands } from '@/lib/bindings'
import { docIdLabel } from '@/lib/docs'
import { highlightJson } from '@/lib/highlight'
import { unwrap } from '@/lib/result'

const props = defineProps<{
  connectionId: string
  db: string | null
  collection: string | null
  entry: DocEntry | null
}>()
const emit = defineEmits<{ saved: [], deleted: [] }>()

const detail = ref<DocDetail | null>(null)
const loading = ref(false)
const editing = ref(false)
const draft = ref('')
const saving = ref(false)
const confirmingDelete = ref(false)
const { copy, copied } = useClipboard()

const addressable = computed(() => props.entry?.id != null)

// A view can project _id away: the row itself is all we have, read-only.
const relaxed = computed(() => detail.value?.relaxed ?? props.entry?.doc ?? null)

const pretty = computed(() => {
  if (relaxed.value === null)
    return null
  try {
    return JSON.stringify(JSON.parse(relaxed.value), null, 2)
  }
  catch {
    return relaxed.value
  }
})
const html = computed(() => pretty.value === null ? null : highlightJson(pretty.value))

async function load() {
  confirmingDelete.value = false
  editing.value = false
  detail.value = null
  if (!props.entry || props.entry.id === null || props.db === null || props.collection === null)
    return
  loading.value = true
  try {
    detail.value = unwrap(await commands.docDetail(props.connectionId, props.db, props.collection, props.entry.id))
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    loading.value = false
  }
}

watch(() => [props.connectionId, props.db, props.collection, props.entry], load, { immediate: true })

function startEdit() {
  if (!detail.value)
    return
  // Canonical extjson: the lossless form (relaxed collapses Int32/Int64/Double).
  try {
    draft.value = JSON.stringify(JSON.parse(detail.value.canonical), null, 2)
  }
  catch {
    draft.value = detail.value.canonical
  }
  editing.value = true
}

async function save() {
  if (!detail.value?.id || props.db === null || props.collection === null)
    return
  saving.value = true
  try {
    unwrap(await commands.docReplace(props.connectionId, props.db, props.collection, detail.value.id, draft.value))
    toast.success('Document saved')
    editing.value = false
    emit('saved')
    await load()
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    saving.value = false
  }
}

async function removeDoc() {
  if (!detail.value?.id || props.db === null || props.collection === null)
    return
  if (!confirmingDelete.value) {
    confirmingDelete.value = true
    return
  }
  try {
    unwrap(await commands.docDelete(props.connectionId, props.db, props.collection, detail.value.id))
    toast.success('Document deleted')
    emit('deleted')
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
  <div class="flex min-h-0 flex-1 flex-col" data-testid="doc-detail">
    <div v-if="!entry" class="flex flex-1 items-center justify-center font-mono text-sm text-muted-foreground">
      <p>soquel=#<span class="ml-1 text-muted-foreground/60">select a document</span></p>
    </div>
    <template v-else>
      <header class="flex items-center gap-2 border-b px-3 py-2">
        <span class="min-w-0 flex-1 truncate font-mono text-sm" data-testid="detail-id">
          {{ docIdLabel(entry.id) }}
        </span>
        <Button size="icon-sm" variant="ghost" data-testid="copy-doc" @click="copy(pretty ?? '')">
          <component :is="copied ? Check : Copy" class="size-3" />
        </Button>
        <template v-if="addressable">
          <Button
            v-if="!editing"
            size="sm"
            variant="ghost"
            class="h-6 text-[11px]"
            data-testid="edit-doc"
            :disabled="!detail"
            @click="startEdit"
          >
            <Pencil class="size-3" /> edit
          </Button>
          <Button
            size="sm"
            :variant="confirmingDelete ? 'destructive' : 'ghost'"
            class="h-6 text-[11px]"
            data-testid="delete-doc"
            :disabled="!detail"
            @click="removeDoc"
            @blur="confirmingDelete = false"
          >
            <Trash2 class="size-3" /> {{ confirmingDelete ? 'sure?' : 'delete' }}
          </Button>
        </template>
      </header>
      <p v-if="!addressable" class="border-b px-3 py-1.5 font-mono text-[11px] text-muted-foreground italic">
        no _id on this document - read-only
      </p>
      <div v-if="editing" class="flex min-h-0 flex-1 flex-col p-3">
        <Textarea v-model="draft" class="min-h-0 flex-1 font-mono text-xs" data-testid="doc-editor" />
        <div class="mt-2 flex gap-2">
          <Button
            size="sm"
            class="h-6 text-[11px]"
            data-testid="save-doc"
            :disabled="saving"
            @click="save"
          >
            {{ saving ? 'Saving…' : 'Save document' }}
          </Button>
          <Button
            size="sm"
            variant="ghost"
            class="h-6 text-[11px]"
            data-testid="cancel-edit"
            @click="editing = false"
          >
            Cancel
          </Button>
        </div>
      </div>
      <div v-else class="min-h-0 flex-1 overflow-auto p-3">
        <p v-if="loading" class="font-mono text-xs text-muted-foreground">
          loading document…
        </p>
        <!-- eslint-disable-next-line vue/no-v-html -- highlightJson escapes every text node -->
        <pre v-else-if="html" class="font-mono text-xs break-all whitespace-pre-wrap" data-testid="doc-json" v-html="html" />
      </div>
    </template>
  </div>
</template>
