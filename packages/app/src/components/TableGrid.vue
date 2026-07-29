<script setup lang="ts">
import type { SortSpec, StatementResult, TableInfo } from '@/lib/bindings'
import { ArrowDown, ArrowUp, ChevronLeft, ChevronRight, RefreshCw } from '@lucide/vue'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Button } from '@/components/ui/button'
import { commands } from '@/lib/bindings'
import { formatEstimatedRows } from '@/lib/format'
import { unwrap } from '@/lib/result'

const props = defineProps<{ connectionId: string, schema: string, table: TableInfo }>()

const PAGE_SIZE = 100

const statement = ref<StatementResult | null>(null)
const durationMs = ref(0)
const offset = ref(0)
const sort = ref<SortSpec | null>(null)
const loading = ref(false)

const numericColumns = computed(() =>
  (statement.value?.columns ?? []).map(column => column.kind === 'number'),
)

async function fetchRows() {
  loading.value = true
  try {
    const result = unwrap(await commands.tableRows(props.connectionId, {
      schema: props.schema,
      table: props.table.name,
      limit: PAGE_SIZE,
      offset: offset.value,
      sort: sort.value,
    }))
    statement.value = result.statements[0] ?? null
    durationMs.value = result.durationMs ?? 0
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    loading.value = false
  }
}

watch(
  () => [props.connectionId, props.schema, props.table.name],
  () => {
    offset.value = 0
    sort.value = null
    fetchRows()
  },
  { immediate: true },
)

function toggleSort(column: string) {
  sort.value = sort.value?.column === column && sort.value.direction === 'asc'
    ? { column, direction: 'desc' }
    : { column, direction: 'asc' }
  offset.value = 0
  fetchRows()
}

function nextPage() {
  offset.value += PAGE_SIZE
  fetchRows()
}

function previousPage() {
  offset.value = Math.max(0, offset.value - PAGE_SIZE)
  fetchRows()
}
</script>

<template>
  <div class="flex h-full min-h-0 flex-col" data-testid="grid">
    <div class="min-h-0 flex-1 overflow-auto">
      <table class="w-max min-w-full border-separate border-spacing-0 font-mono text-xs">
        <thead class="sticky top-0 z-10">
          <tr>
            <th
              v-for="(column, columnIndex) in statement?.columns ?? []"
              :key="column.name"
              class="cursor-pointer border-b bg-background px-3 py-1.5 font-medium text-muted-foreground select-none hover:text-foreground"
              :class="numericColumns[columnIndex] ? 'text-right' : 'text-left'"
              :data-testid="`grid-header-${column.name}`"
              :title="column.dataType ?? undefined"
              @click="toggleSort(column.name)"
            >
              <span class="inline-flex items-center gap-1">
                {{ column.name }}
                <component
                  :is="sort.direction === 'asc' ? ArrowUp : ArrowDown"
                  v-if="sort?.column === column.name"
                  class="size-3"
                />
              </span>
            </th>
          </tr>
        </thead>
        <tbody data-testid="grid-body">
          <tr
            v-for="(row, rowIndex) in statement?.rows ?? []"
            :key="offset + rowIndex"
            class="hover:bg-muted/40"
          >
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
      <p
        v-if="statement && statement.rows.length === 0"
        class="px-4 py-8 text-center font-mono text-xs text-muted-foreground"
      >
        no rows
      </p>
    </div>

    <footer class="flex items-center gap-3 border-t px-3 py-1 font-mono text-[11px] text-muted-foreground">
      <span data-testid="grid-range">
        {{ statement && statement.rows.length > 0 ? `${offset + 1}–${offset + statement.rows.length}` : '0' }}
        <template v-if="formatEstimatedRows(table.estimatedRows)">
          of ~{{ formatEstimatedRows(table.estimatedRows) }}
        </template>
      </span>
      <span>{{ durationMs.toFixed(0) }}ms</span>
      <span class="flex-1" />
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Refresh rows"
        data-testid="grid-refresh"
        :disabled="loading"
        @click="fetchRows"
      >
        <RefreshCw :class="loading ? 'animate-spin' : ''" />
      </Button>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Previous page"
        data-testid="grid-prev"
        :disabled="loading || offset === 0"
        @click="previousPage"
      >
        <ChevronLeft />
      </Button>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Next page"
        data-testid="grid-next"
        :disabled="loading || (statement?.rows.length ?? 0) < PAGE_SIZE"
        @click="nextPage"
      >
        <ChevronRight />
      </Button>
    </footer>
  </div>
</template>
