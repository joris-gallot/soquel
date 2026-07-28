<script setup lang="ts">
import type { FunctionalComponent } from 'vue'
import type { SchemaSnapshot, TableInfo, TableKind } from '@/lib/bindings'
import { ChevronDown, ChevronRight, Eye, Layers, Table2 } from '@lucide/vue'
import { computed, ref } from 'vue'
import { formatEstimatedRows } from '@/lib/format'

const props = defineProps<{ snapshot: SchemaSnapshot, filter: string }>()
const emit = defineEmits<{ select: [schema: string, table: TableInfo] }>()

const TABLE_ICONS: Record<TableKind, FunctionalComponent> = {
  'table': Table2,
  'view': Eye,
  'materialized-view': Layers,
}

const collapsed = ref<Set<string>>(new Set())
const selected = ref<string | null>(null)

const filtered = computed(() => {
  const needle = props.filter.trim().toLowerCase()
  return props.snapshot.schemas
    .map(schema => ({
      ...schema,
      tables: needle
        ? schema.tables.filter(t => t.name.toLowerCase().includes(needle))
        : schema.tables,
    }))
    .filter(schema => schema.tables.length > 0)
})

function toggle(name: string) {
  if (collapsed.value.has(name))
    collapsed.value.delete(name)
  else
    collapsed.value.add(name)
}

function select(schema: string, table: TableInfo) {
  selected.value = `${schema}.${table.name}`
  emit('select', schema, table)
}
</script>

<template>
  <nav class="space-y-1 px-2 pb-4 font-mono text-[13px]">
    <div v-for="schema in filtered" :key="schema.name">
      <button
        type="button"
        class="flex w-full items-center gap-1 rounded px-2 py-1 text-xs text-muted-foreground hover:text-foreground"
        :data-testid="`schema-${schema.name}`"
        @click="toggle(schema.name)"
      >
        <component :is="collapsed.has(schema.name) ? ChevronRight : ChevronDown" class="size-3" />
        {{ schema.name }}
      </button>
      <ul v-if="!collapsed.has(schema.name)" class="space-y-px">
        <li v-for="table in schema.tables" :key="table.name">
          <button
            type="button"
            class="flex w-full items-center gap-2 rounded px-2 py-1 text-left hover:bg-sidebar-accent"
            :class="selected === `${schema.name}.${table.name}`
              ? 'bg-sidebar-accent text-sidebar-accent-foreground'
              : 'text-sidebar-foreground/90'"
            :data-testid="`table-${schema.name}.${table.name}`"
            @click="select(schema.name, table)"
          >
            <component :is="TABLE_ICONS[table.kind]" class="size-3.5 shrink-0 text-muted-foreground" />
            <span class="min-w-0 flex-1 truncate">{{ table.name }}</span>
            <span class="text-[10px] text-muted-foreground/70">
              {{ formatEstimatedRows(table.estimatedRows) }}
            </span>
          </button>
        </li>
      </ul>
    </div>
    <p v-if="filtered.length === 0" class="px-2 py-4 text-xs text-muted-foreground">
      No tables match.
    </p>
  </nav>
</template>
