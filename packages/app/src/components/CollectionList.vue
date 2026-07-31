<script setup lang="ts">
import type { DocCollection } from '@/lib/bindings'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Input } from '@/components/ui/input'
import { ScrollArea } from '@/components/ui/scroll-area'
import { commands } from '@/lib/bindings'
import { compactCount, DOC_KIND_BADGE } from '@/lib/docs'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string, db: string | null, selected: string | null }>()
const emit = defineEmits<{ select: [collection: string] }>()

const filter = ref('')
const collections = ref<DocCollection[]>([])
const loadedOnce = ref(false)

// Latest-wins: a db switch supersedes in-flight listings.
let loadSeq = 0

async function load() {
  const seq = ++loadSeq
  if (props.db === null) {
    collections.value = []
    return
  }
  try {
    const listed = unwrap(await commands.docCollections(props.connectionId, props.db))
    if (seq !== loadSeq)
      return
    collections.value = listed
    loadedOnce.value = true
  }
  catch (error) {
    if (seq === loadSeq)
      toast.error(error instanceof Error ? error.message : String(error))
  }
}

watch(() => [props.connectionId, props.db], load, { immediate: true })

const visible = computed(() => {
  const needle = filter.value.trim().toLowerCase()
  if (needle === '')
    return collections.value
  return collections.value.filter(collection => collection.name.toLowerCase().includes(needle))
})

defineExpose({ refresh: load })
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col">
    <div class="px-2 py-2">
      <Input
        v-model="filter"
        placeholder="filter collections"
        class="h-7 font-mono text-xs"
        data-testid="collection-filter"
      />
    </div>
    <ScrollArea class="min-h-0 flex-1">
      <button
        v-for="collection in visible"
        :key="collection.name"
        type="button"
        class="flex w-full cursor-pointer items-center gap-1.5 px-2 py-1 text-left font-mono text-xs hover:bg-accent/50"
        :class="collection.name === selected && 'bg-accent text-accent-foreground'"
        :data-testid="`collection-${collection.name}`"
        @click="emit('select', collection.name)"
      >
        <span
          class="w-9 shrink-0 rounded px-1 text-center text-[10px]"
          :class="DOC_KIND_BADGE[collection.kind].classes"
        >
          {{ DOC_KIND_BADGE[collection.kind].short }}
        </span>
        <span class="min-w-0 flex-1 truncate">{{ collection.name }}</span>
        <span v-if="collection.estimatedDocs !== null" class="shrink-0 text-[10px] text-muted-foreground">
          ~{{ compactCount(collection.estimatedDocs) }}
        </span>
      </button>
      <div class="px-2 py-2">
        <p
          v-if="loadedOnce"
          class="font-mono text-[11px] text-muted-foreground"
          data-testid="collection-count"
        >
          {{ collections.length }} collection{{ collections.length === 1 ? '' : 's' }}
        </p>
      </div>
    </ScrollArea>
  </div>
</template>
