<script setup lang="ts">
import type { StatementResult } from '@/lib/bindings'
import { computed } from 'vue'

const props = defineProps<{ statement: StatementResult }>()

const ROW_CAP = 1000

const rows = computed(() => props.statement.rows.slice(0, ROW_CAP))
const truncated = computed(() => props.statement.rows.length > ROW_CAP)
const numericColumns = computed(() => props.statement.columns.map(column => column.kind === 'number'))
</script>

<template>
  <div class="min-h-0 flex-1 overflow-auto">
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
        <tr v-for="(row, rowIndex) in rows" :key="rowIndex" class="hover:bg-muted/40">
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
      </tbody>
    </table>
    <p v-else class="px-4 py-6 font-mono text-xs text-muted-foreground">
      {{ statement.rowsAffected ?? 0 }} row{{ statement.rowsAffected === 1 ? '' : 's' }} affected
    </p>
    <p v-if="truncated" class="px-3 py-2 font-mono text-[11px] text-muted-foreground">
      showing first {{ ROW_CAP }} of {{ statement.rows.length }} rows
    </p>
  </div>
</template>
