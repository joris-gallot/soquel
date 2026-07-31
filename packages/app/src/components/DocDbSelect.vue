<script setup lang="ts">
import type { DocDatabase } from '@/lib/bindings'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { commands } from '@/lib/bindings'
import { formatBytes } from '@/lib/docs'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string }>()
const emit = defineEmits<{ switched: [db: string] }>()

// The db is a per-request argument (no reconnect): switching is just state.
const db = defineModel<string | null>({ required: true })
const databases = ref<DocDatabase[]>([])

async function load() {
  try {
    databases.value = unwrap(await commands.docDatabases(props.connectionId))
    if (db.value === null || !databases.value.some(entry => entry.name === db.value))
      db.value = databases.value[0]?.name ?? null
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

watch(() => props.connectionId, load, { immediate: true })

function select(name: string) {
  if (name === db.value)
    return
  db.value = name
  emit('switched', name)
}

defineExpose({ refresh: load })
</script>

<template>
  <Select
    v-if="databases.length > 0 && db !== null"
    :model-value="db"
    @update:model-value="value => select(String(value))"
  >
    <SelectTrigger class="h-6! gap-1 border-none px-2 font-mono text-xs" data-testid="doc-db">
      <SelectValue>{{ db }}</SelectValue>
    </SelectTrigger>
    <SelectContent position="popper" align="end">
      <SelectItem
        v-for="entry in databases"
        :key="entry.name"
        :value="entry.name"
        class="font-mono text-xs"
        :data-testid="`doc-db-option-${entry.name}`"
      >
        {{ entry.name }}
        <span v-if="entry.sizeBytes !== null" class="ml-1 text-muted-foreground">
          {{ formatBytes(entry.sizeBytes) }}
        </span>
      </SelectItem>
    </SelectContent>
  </Select>
</template>
