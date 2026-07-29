<script setup lang="ts">
import type { StatementResult } from '@/lib/bindings'
import { computed, useTemplateRef } from 'vue'
import { useVirtualRows } from '@/composables/useVirtualRows'

const props = defineProps<{ statement: StatementResult }>()

const scroller = useTemplateRef('scroller')
const rowCount = computed(() => props.statement.rows.length)
const { window: virtualWindow } = useVirtualRows(scroller, rowCount)

const visibleRows = computed(() =>
  props.statement.rows
    .slice(virtualWindow.value.start, virtualWindow.value.end)
    .map((row, i) => ({ row, rowIndex: virtualWindow.value.start + i })),
)

const numericColumns = computed(() => props.statement.columns.map(column => column.kind === 'number'))
</script>

<template>
  <div ref="scroller" class="min-h-0 flex-1 overflow-auto">
    <table v-if="statement.columns.length > 0" class="w-max min-w-full border-separate border-spacing-0 font-mono text-xs">
      <thead class="sticky top-0 z-10">
        <tr>
          <th
            v-for="(column, columnIndex) in statement.columns"
            :key="column.name"
            class="border-b bg-background px-3 py-1.5 font-medium text-muted-foreground"
            :class="numericColumns[columnIndex] ? 'text-right' : 'text-left'"
            :title="column.dataType ?? undefined"
          >
            {{ column.name }}
          </th>
        </tr>
      </thead>
      <tbody>
        <tr v-if="virtualWindow.padTop > 0" aria-hidden="true" :style="{ height: `${virtualWindow.padTop}px` }" />
        <tr v-for="{ row, rowIndex } in visibleRows" :key="rowIndex" data-row class="hover:bg-muted/40">
          <td
            v-for="(value, columnIndex) in row"
            :key="columnIndex"
            class="max-w-xs truncate border-b border-border/50 px-3 py-1"
            :class="numericColumns[columnIndex] && 'text-right'"
            :title="value ?? undefined"
          >
            <span v-if="value === null" class="text-muted-foreground/60 italic">NULL</span>
            <template v-else>
              {{ value }}
            </template>
          </td>
        </tr>
        <tr v-if="virtualWindow.padBottom > 0" aria-hidden="true" :style="{ height: `${virtualWindow.padBottom}px` }" />
      </tbody>
    </table>
    <p v-else class="px-4 py-6 font-mono text-xs text-muted-foreground">
      {{ statement.rowsAffected ?? 0 }} row{{ statement.rowsAffected === 1 ? '' : 's' }} affected
    </p>
  </div>
</template>
