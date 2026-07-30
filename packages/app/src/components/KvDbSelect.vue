<script setup lang="ts">
import type { KvDatabases } from '@/lib/bindings'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from '@/components/ui/select'
import { commands } from '@/lib/bindings'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string }>()
const emit = defineEmits<{ switched: [] }>()

const databases = ref<KvDatabases | null>(null)
const switching = ref(false)

async function load() {
  try {
    databases.value = unwrap(await commands.kvDatabases(props.connectionId))
  }
  catch {
    databases.value = null
  }
}

watch(() => props.connectionId, load, { immediate: true })

function keyCount(db: number): number | undefined {
  // f64 binds as number | null: a real count is never null.
  return databases.value?.used.find(entry => entry.db === db)?.keys ?? undefined
}

async function select(db: number) {
  if (!databases.value || db === databases.value.current || switching.value)
    return
  switching.value = true
  try {
    unwrap(await commands.kvSelectDb(props.connectionId, db))
    await load()
    emit('switched')
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    switching.value = false
  }
}

defineExpose({ refresh: load })
</script>

<template>
  <Select
    v-if="databases"
    :model-value="String(databases.current)"
    :disabled="switching"
    @update:model-value="value => select(Number(value))"
  >
    <SelectTrigger class="h-6! gap-1 border-none px-2 font-mono text-xs" data-testid="kv-db">
      <SelectValue>db {{ databases.current }}</SelectValue>
    </SelectTrigger>
    <SelectContent position="popper" align="end">
      <SelectItem
        v-for="index in databases.total"
        :key="index"
        :value="String(index - 1)"
        class="font-mono text-xs"
        :data-testid="`kv-db-option-${index - 1}`"
      >
        db {{ index - 1 }}
        <span v-if="keyCount(index - 1) !== undefined" class="ml-1 text-muted-foreground">
          {{ keyCount(index - 1) }} keys
        </span>
      </SelectItem>
    </SelectContent>
  </Select>
</template>
