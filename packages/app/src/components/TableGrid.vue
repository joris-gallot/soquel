<script setup lang="ts">
import type { ColumnFilter, FilterOp, SortSpec, StatementResult, TableInfo } from '@/lib/bindings'
import { ArrowDown, ArrowUp, ArrowUpRight, ChevronLeft, ChevronRight, Copy, Funnel, RefreshCw, X } from '@lucide/vue'
import { useClipboard } from '@vueuse/core'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import { Input } from '@/components/ui/input'
import { Popover, PopoverContent, PopoverTrigger } from '@/components/ui/popover'
import { ResizableHandle, ResizablePanel, ResizablePanelGroup } from '@/components/ui/resizable'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { commands } from '@/lib/bindings'
import { FILTER_OP_LABELS, FILTER_OPS_BY_KIND, filterLabel, OP_NEEDS_VALUE } from '@/lib/filters'
import { formatEstimatedRows } from '@/lib/format'
import { highlightJson } from '@/lib/highlight-json'
import { unwrap } from '@/lib/result'

const props = defineProps<{
  connectionId: string
  schema: string
  table: TableInfo
  initialFilters?: ColumnFilter[]
}>()

const emit = defineEmits<{ hop: [schema: string, table: string, filters: ColumnFilter[]] }>()

const PAGE_SIZE = 100

const statement = ref<StatementResult | null>(null)
const durationMs = ref(0)
const offset = ref(0)
const sort = ref<SortSpec | null>(null)
const loading = ref(false)
const filters = ref<ColumnFilter[]>([])

const openFilterColumn = ref<string | null>(null)
const draftOp = ref<FilterOp>('eq')
const draftValue = ref('')

const selectedCell = ref<{ rowIndex: number, columnIndex: number } | null>(null)
const { copy, copied } = useClipboard()

const numericColumns = computed(() =>
  (statement.value?.columns ?? []).map(column => column.kind === 'number'),
)

// Single-column FKs by column name; composite FKs hop from any of their columns.
const fkByColumn = computed(() => {
  const map = new Map<string, TableInfo['foreignKeys'][number]>()
  for (const fk of props.table.foreignKeys) {
    for (const column of fk.columns)
      map.set(column, fk)
  }
  return map
})

const inspected = computed(() => {
  if (!selectedCell.value || !statement.value)
    return null
  const { rowIndex, columnIndex } = selectedCell.value
  const column = statement.value.columns[columnIndex]
  const row = statement.value.rows[rowIndex]
  if (!column || !row)
    return null
  return { column, value: row[columnIndex], rowIndex, columnIndex }
})

const inspectedPretty = computed(() => {
  const cell = inspected.value
  if (!cell || cell.value === null)
    return null
  if (cell.column.kind !== 'json')
    return cell.value
  try {
    return JSON.stringify(JSON.parse(cell.value), null, 2)
  }
  catch {
    return cell.value
  }
})

const inspectedHtml = computed(() =>
  inspected.value?.column.kind === 'json' && inspectedPretty.value !== null
    ? highlightJson(inspectedPretty.value)
    : null,
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
      filters: filters.value,
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
    filters.value = props.initialFilters ?? []
    selectedCell.value = null
    fetchRows()
  },
  { immediate: true },
)

watch(() => props.initialFilters, (initial) => {
  if (initial && initial.length > 0) {
    filters.value = initial
    offset.value = 0
    fetchRows()
  }
})

function toggleSort(column: string) {
  sort.value = sort.value?.column === column && sort.value.direction === 'asc'
    ? { column, direction: 'desc' }
    : { column, direction: 'asc' }
  offset.value = 0
  fetchRows()
}

function openFilter(column: string, kind: (typeof FILTER_OPS_BY_KIND) extends Record<infer K, unknown> ? K : never) {
  const existing = filters.value.find(f => f.column === column)
  draftOp.value = existing?.op ?? FILTER_OPS_BY_KIND[kind][0]
  draftValue.value = existing?.value ?? ''
  openFilterColumn.value = column
}

function applyFilter(column: string) {
  const filter: ColumnFilter = {
    column,
    op: draftOp.value,
    value: OP_NEEDS_VALUE[draftOp.value] ? draftValue.value : null,
  }
  filters.value = [...filters.value.filter(f => f.column !== column), filter]
  openFilterColumn.value = null
  offset.value = 0
  fetchRows()
}

function removeFilter(column: string) {
  filters.value = filters.value.filter(f => f.column !== column)
  openFilterColumn.value = null
  offset.value = 0
  fetchRows()
}

function clearFilters() {
  filters.value = []
  offset.value = 0
  fetchRows()
}

function hopFrom(rowIndex: number, columnName: string) {
  const fk = fkByColumn.value.get(columnName)
  const row = statement.value?.rows[rowIndex]
  const columns = statement.value?.columns
  if (!fk || !row || !columns)
    return
  const hopFilters: ColumnFilter[] = fk.columns.map((column, position) => {
    const columnIndex = columns.findIndex(c => c.name === column)
    const value = columnIndex === -1 ? null : row[columnIndex]
    return value === null
      ? { column: fk.referencedColumns[position], op: 'is-null', value: null }
      : { column: fk.referencedColumns[position], op: 'eq', value }
  })
  emit('hop', fk.referencedSchema, fk.referencedTable, hopFilters)
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
  <ResizablePanelGroup direction="horizontal" auto-save-id="soquel-grid" class="h-full min-h-0" data-testid="grid">
    <ResizablePanel id="grid-rows" :min-size="30" class="flex min-w-0 flex-col">
      <div
        v-if="filters.length > 0"
        class="flex flex-wrap items-center gap-1.5 border-b px-3 py-1.5"
        data-testid="filter-chips"
      >
        <Badge
          v-for="filter in filters"
          :key="filter.column"
          variant="outline"
          class="gap-1 font-mono text-[11px]"
        >
          {{ filterLabel(filter) }}
          <button
            type="button"
            class="text-muted-foreground hover:text-foreground"
            :aria-label="`Remove filter on ${filter.column}`"
            :data-testid="`remove-filter-${filter.column}`"
            @click="removeFilter(filter.column)"
          >
            <X class="size-3" />
          </button>
        </Badge>
        <button
          type="button"
          class="font-mono text-[11px] text-muted-foreground hover:text-foreground"
          data-testid="clear-filters"
          @click="clearFilters"
        >
          clear all
        </button>
      </div>

      <div class="min-h-0 flex-1 overflow-auto">
        <table class="w-max min-w-full border-separate border-spacing-0 font-mono text-xs">
          <thead class="sticky top-0 z-10">
            <tr>
              <th
                v-for="(column, columnIndex) in statement?.columns ?? []"
                :key="column.name"
                class="border-b bg-background px-3 py-1.5 font-medium text-muted-foreground select-none"
                :class="numericColumns[columnIndex] ? 'text-right' : 'text-left'"
                :data-testid="`grid-header-${column.name}`"
                :title="column.dataType ?? undefined"
              >
                <span class="inline-flex items-center gap-1">
                  <button
                    type="button"
                    class="cursor-pointer hover:text-foreground"
                    :data-testid="`sort-${column.name}`"
                    @click="toggleSort(column.name)"
                  >
                    {{ column.name }}
                  </button>
                  <component
                    :is="sort.direction === 'asc' ? ArrowUp : ArrowDown"
                    v-if="sort?.column === column.name"
                    class="size-3"
                  />
                  <Popover
                    :open="openFilterColumn === column.name"
                    @update:open="(open: boolean) => (openFilterColumn = open ? column.name : null)"
                  >
                    <PopoverTrigger as-child>
                      <button
                        type="button"
                        class="cursor-pointer"
                        :class="filters.some(f => f.column === column.name) ? 'text-foreground' : 'text-muted-foreground/50 hover:text-foreground'"
                        :aria-label="`Filter ${column.name}`"
                        :data-testid="`filter-${column.name}`"
                        @click="openFilter(column.name, column.kind)"
                      >
                        <Funnel class="size-3" />
                      </button>
                    </PopoverTrigger>
                    <PopoverContent class="w-64 space-y-2 p-2" align="start">
                      <div class="flex items-center gap-2">
                        <Select v-model="draftOp">
                          <SelectTrigger size="sm" class="w-full font-mono text-xs" data-testid="filter-op">
                            <SelectValue />
                          </SelectTrigger>
                          <SelectContent>
                            <SelectItem
                              v-for="op in FILTER_OPS_BY_KIND[column.kind]"
                              :key="op"
                              :value="op"
                              class="font-mono text-xs"
                            >
                              {{ FILTER_OP_LABELS[op] }}
                            </SelectItem>
                          </SelectContent>
                        </Select>
                      </div>
                      <Input
                        v-if="OP_NEEDS_VALUE[draftOp]"
                        v-model="draftValue"
                        class="h-7 font-mono text-xs"
                        data-testid="filter-value"
                        @keydown.enter="applyFilter(column.name)"
                      />
                      <div class="flex justify-between">
                        <Button
                          size="sm"
                          variant="ghost"
                          data-testid="filter-clear"
                          :disabled="!filters.some(f => f.column === column.name)"
                          @click="removeFilter(column.name)"
                        >
                          Clear
                        </Button>
                        <Button size="sm" data-testid="filter-apply" @click="applyFilter(column.name)">
                          Apply
                        </Button>
                      </div>
                    </PopoverContent>
                  </Popover>
                </span>
              </th>
            </tr>
          </thead>
          <tbody data-testid="grid-body">
            <tr
              v-for="(row, rowIndex) in statement?.rows ?? []"
              :key="offset + rowIndex"
              class="group hover:bg-muted/40"
            >
              <td
                v-for="(value, columnIndex) in row"
                :key="columnIndex"
                class="max-w-xs cursor-default truncate border-b border-border/50 px-3 py-1"
                :class="[
                  numericColumns[columnIndex] && 'text-right',
                  selectedCell?.rowIndex === rowIndex && selectedCell?.columnIndex === columnIndex
                    && 'bg-accent text-accent-foreground',
                ]"
                :title="value ?? undefined"
                @click="selectedCell = { rowIndex, columnIndex }"
              >
                <span class="inline-flex max-w-full items-center gap-1">
                  <span v-if="value === null" class="text-muted-foreground/60 italic">NULL</span>
                  <span v-else class="truncate">{{ value }}</span>
                  <button
                    v-if="value !== null && fkByColumn.has(statement!.columns[columnIndex].name)"
                    type="button"
                    class="invisible shrink-0 text-muted-foreground hover:text-foreground group-hover:visible"
                    aria-label="Open referenced row"
                    :data-testid="`fk-hop-${statement!.columns[columnIndex].name}`"
                    @click.stop="hopFrom(rowIndex, statement!.columns[columnIndex].name)"
                  >
                    <ArrowUpRight class="size-3" />
                  </button>
                </span>
              </td>
            </tr>
          </tbody>
        </table>
        <p
          v-if="statement && statement.rows.length === 0"
          class="px-4 py-8 text-center font-mono text-xs text-muted-foreground"
        >
          no rows{{ filters.length > 0 ? ' match the filters' : '' }}
        </p>
      </div>

      <footer class="flex items-center gap-3 border-t px-3 py-1 font-mono text-[11px] text-muted-foreground">
        <span data-testid="grid-range">
          {{ statement && statement.rows.length > 0 ? `${offset + 1}–${offset + statement.rows.length}` : '0' }}
          <template v-if="filters.length > 0">
            filtered
          </template>
          <template v-else-if="formatEstimatedRows(table.estimatedRows)">
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
    </ResizablePanel>

    <template v-if="inspected">
      <ResizableHandle with-handle />
      <ResizablePanel id="grid-inspector" :default-size="28" :min-size="15" :max-size="60">
        <aside class="flex h-full min-h-0 flex-col" data-testid="cell-inspector">
          <header class="flex items-center gap-2 border-b px-3 py-1.5">
            <span class="min-w-0 truncate font-mono text-xs font-medium">{{ inspected.column.name }}</span>
            <span class="font-mono text-[10px] text-muted-foreground">{{ inspected.column.dataType }}</span>
            <span class="flex-1" />
            <!-- Controlled open: the tooltip only flashes "Copied" after a click. -->
            <Tooltip v-if="inspected.value !== null" :open="copied">
              <TooltipTrigger as-child>
                <Button
                  size="icon-xs"
                  variant="ghost"
                  aria-label="Copy value"
                  data-testid="inspector-copy"
                  @click="copy(inspected.value)"
                >
                  <Copy />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Copied</TooltipContent>
            </Tooltip>
            <Tooltip v-if="fkByColumn.has(inspected.column.name) && inspected.value !== null">
              <TooltipTrigger as-child>
                <Button
                  size="icon-xs"
                  variant="ghost"
                  aria-label="Open referenced row"
                  data-testid="inspector-hop"
                  @click="hopFrom(inspected.rowIndex, inspected.column.name)"
                >
                  <ArrowUpRight />
                </Button>
              </TooltipTrigger>
              <TooltipContent>Open referenced row</TooltipContent>
            </Tooltip>
            <Button
              size="icon-xs"
              variant="ghost"
              aria-label="Close inspector"
              data-testid="inspector-close"
              @click="selectedCell = null"
            >
              <X />
            </Button>
          </header>
          <div class="min-h-0 flex-1 overflow-auto p-3">
            <span v-if="inspected.value === null" class="font-mono text-xs text-muted-foreground/60 italic">NULL</span>
            <!-- eslint-disable-next-line vue/no-v-html -- highlightJson escapes every text node -->
            <pre v-else-if="inspectedHtml" class="font-mono text-xs break-all whitespace-pre-wrap" data-testid="inspector-value" v-html="inspectedHtml" />
            <pre v-else class="font-mono text-xs break-all whitespace-pre-wrap" data-testid="inspector-value">{{ inspectedPretty }}</pre>
          </div>
        </aside>
      </ResizablePanel>
    </template>
  </ResizablePanelGroup>
</template>
