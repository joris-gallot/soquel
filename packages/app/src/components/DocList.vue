<script setup lang="ts">
import type { DocEntry } from '@/lib/bindings'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { commands } from '@/lib/bindings'
import { docIdLabel, docPreview, formatDocCount } from '@/lib/docs'
import { CommandError, unwrap } from '@/lib/result'

const props = defineProps<{
  connectionId: string
  db: string | null
  collection: string | null
  selected: DocEntry | null
}>()
const emit = defineEmits<{ select: [entry: DocEntry] }>()

const PAGE = 100

const filter = ref('')
// The filter that produced the current list; typing only applies on run.
const applied = ref('')
const docs = ref<DocEntry[]>([])
const cursor = ref<string | null>(null)
const count = ref<string | null>(null)
const filterError = ref<string | null>(null)
const loading = ref(false)
const loadedOnce = ref(false)

// Latest-wins: a newer find supersedes in-flight responses.
let findSeq = 0

function appliedFilter(): string | null {
  return applied.value.trim() === '' ? null : applied.value
}

async function find(reset: boolean) {
  const seq = ++findSeq
  if (props.db === null || props.collection === null) {
    docs.value = []
    cursor.value = null
    count.value = null
    return
  }
  loading.value = true
  if (reset)
    filterError.value = null
  try {
    const page = unwrap(await commands.docFind(props.connectionId, {
      db: props.db,
      collection: props.collection,
      filter: appliedFilter(),
      sort: null,
      limit: PAGE,
      cursor: reset ? null : cursor.value,
    }))
    if (seq !== findSeq)
      return
    docs.value = reset ? page.docs : [...docs.value, ...page.docs]
    cursor.value = page.cursor
    loadedOnce.value = true
    if (reset)
      void loadCount(seq)
  }
  catch (error) {
    if (seq !== findSeq)
      return
    // A bad filter is the panel's own input: inline feedback, not a toast.
    if (error instanceof CommandError && error.kind === 'unsupported')
      filterError.value = error.message
    else
      toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    if (seq === findSeq)
      loading.value = false
  }
}

async function loadCount(seq: number) {
  if (props.db === null || props.collection === null)
    return
  try {
    const result = unwrap(await commands.docCount(props.connectionId, props.db, props.collection, appliedFilter()))
    if (seq !== findSeq)
      return
    count.value = result.count === null ? null : formatDocCount(result.count, result.exact)
  }
  catch {
    // The count is decoration; the list already told the story.
    if (seq === findSeq)
      count.value = null
  }
}

function run() {
  applied.value = filter.value
  find(true)
}

watch(() => [props.connectionId, props.db, props.collection], () => {
  filter.value = ''
  applied.value = ''
  find(true)
}, { immediate: true })

function isSelected(entry: DocEntry): boolean {
  if (!props.selected)
    return false
  return entry.id !== null ? entry.id === props.selected.id : entry.doc === props.selected.doc
}

/// Re-run with the current filter; parents call this after writes.
function refresh() {
  find(true)
}

defineExpose({ refresh })
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div v-if="collection === null" class="flex flex-1 items-center justify-center font-mono text-sm text-muted-foreground">
      <p>soquel=#<span class="ml-1 text-muted-foreground/60">select a collection</span></p>
    </div>
    <template v-else>
      <div class="border-b px-2 py-1.5">
        <div class="flex items-center gap-1.5">
          <Input
            v-model="filter"
            placeholder="filter { &quot;plan&quot;: &quot;pro&quot; }"
            class="h-6 font-mono text-[11px]"
            data-testid="doc-filter"
            @keydown.enter="run"
          />
          <Button
            size="sm"
            variant="ghost"
            class="h-6 text-[11px]"
            data-testid="doc-run"
            :disabled="loading"
            @click="run"
          >
            find
          </Button>
        </div>
        <p v-if="filterError" class="mt-1 font-mono text-[11px] text-destructive" data-testid="doc-filter-error">
          {{ filterError }}
        </p>
      </div>
      <ScrollArea class="min-h-0 flex-1">
        <button
          v-for="(entry, index) in docs"
          :key="entry.id ?? `row-${index}`"
          type="button"
          class="block w-full cursor-pointer border-b border-border/40 px-2 py-1 text-left font-mono text-xs hover:bg-accent/50"
          :class="isSelected(entry) && 'bg-accent text-accent-foreground'"
          :data-testid="`doc-row-${index}`"
          @click="emit('select', entry)"
        >
          <span class="block truncate">{{ docIdLabel(entry.id) }}</span>
          <span class="block truncate text-[11px] text-muted-foreground">{{ docPreview(entry.doc) }}</span>
        </button>
        <div class="px-2 py-2">
          <Button
            v-if="cursor !== null"
            size="sm"
            variant="ghost"
            class="h-6 w-full text-[11px]"
            data-testid="doc-more"
            :disabled="loading"
            @click="find(false)"
          >
            {{ loading ? 'loading…' : 'load more' }}
          </Button>
          <p v-else-if="loadedOnce && docs.length === 0" class="font-mono text-[11px] text-muted-foreground">
            no documents match
          </p>
        </div>
      </ScrollArea>
      <footer
        v-if="count"
        class="border-t px-2 py-1 font-mono text-[10px] text-muted-foreground"
        data-testid="doc-count"
      >
        {{ count }}
      </footer>
    </template>
  </div>
</template>
