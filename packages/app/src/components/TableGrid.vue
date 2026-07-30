<script setup lang="ts">
import type { ColumnFilter, ConnectorKind, ExportFormat, ExportProgress, FilterOp, QueryColumn, RowsChunk, SortSpec, TableInfo } from '@/lib/bindings'
import type { StagedChanges } from '@/lib/staged'
import { ArrowDown, ArrowUp, ArrowUpRight, Copy, CopyPlus, Funnel, Plus, RefreshCw, Trash2, X } from '@lucide/vue'
import { Channel } from '@tauri-apps/api/core'
import { useClipboard, useEventListener, useScroll } from '@vueuse/core'
import { computed, nextTick, ref, shallowRef, triggerRef, useTemplateRef, watch } from 'vue'
import { toast } from 'vue-sonner'
import CellEditor from '@/components/CellEditor.vue'
import ExportMenu from '@/components/ExportMenu.vue'
import { Badge } from '@/components/ui/badge'
import { Button } from '@/components/ui/button'
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from '@/components/ui/dialog'
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
import { useVirtualRows } from '@/composables/useVirtualRows'
import { commands } from '@/lib/bindings'
import { nextEditablePosition } from '@/lib/cell-editing'
import { EXPORT_FORMATS, pickExportPath } from '@/lib/export'
import { FILTER_OP_LABELS, FILTER_OPS_BY_KIND, filterLabel, OP_NEEDS_VALUE } from '@/lib/filters'
import { formatEstimatedRows } from '@/lib/format'
import { highlightJson, highlightSql } from '@/lib/highlight'
import { unwrap } from '@/lib/result'
import { buildTableChanges, emptyStaged, previewSql, stagedCount } from '@/lib/staged'

const props = defineProps<{
  connectionId: string
  kind: ConnectorKind
  schema: string
  table: TableInfo
  initialFilters?: ColumnFilter[]
}>()

const emit = defineEmits<{ hop: [schema: string, table: string, filters: ColumnFilter[]] }>()

// ctid rescue and the xmin guard are postgres system columns.
const isPostgres = computed(() => props.kind === 'postgres')

const FETCH_SIZE = 2000

const columns = ref<QueryColumn[]>([])
// shallowRef + explicit triggers: rows can reach 6 digits, deep proxying costs.
const rows = shallowRef<(string | null)[][]>([])
const durationMs = ref(0)
const sort = ref<SortSpec | null>(null)
const loading = ref(false)
const fetchedAll = ref(false)
const filters = ref<ColumnFilter[]>([])
let generation = 0

const scroller = useTemplateRef('scroller')
const rowCount = computed(() => rows.value.length)
const { window: virtualWindow } = useVirtualRows(scroller, rowCount)
const { y: scrollY } = useScroll(scroller)

const openFilterColumn = ref<string | null>(null)
const draftOp = ref<FilterOp>('eq')
const draftValue = ref('')

const selectedCell = ref<{ rowIndex: number, columnIndex: number } | null>(null)
const { copy, copied } = useClipboard()

const gridView = ref<'data' | 'ddl'>('data')
const ddl = ref<string | null>(null)
const ddlLoading = ref(false)
const ddlHtml = computed(() => (ddl.value === null ? null : highlightSql(ddl.value)))
const { copy: copyDdl, copied: ddlCopied } = useClipboard()

const staged = ref<StagedChanges>(emptyStaged())
const ctidMode = ref(false)
const editingCell = ref<{ rowIndex: number, columnIndex: number } | null>(null)
const previewOpen = ref(false)
const applying = ref(false)

// Views and matviews are read-only; PK-less tables need the ctid opt-in.
const keyColumns = computed(() =>
  ctidMode.value ? ['ctid'] : props.table.primaryKey,
)
const canEverEdit = computed(() => props.table.kind === 'table')
const editable = computed(() => canEverEdit.value && keyColumns.value.length > 0)
const pending = computed(() => stagedCount(staged.value))

// System columns (ctid key, xmin guard) are fetched for keying but never displayed.
const displayColumns = computed(() =>
  columns.value
    .map((column, index) => ({ column, index }))
    .filter(({ column }) => column.name !== 'ctid' && column.name !== 'xmin'),
)

const hasXmin = computed(() => columns.value.some(column => column.name === 'xmin'))

const nullableColumns = computed(() =>
  new Map(props.table.columns.map(column => [column.name, column.nullable])),
)

const numericColumns = computed(() =>
  columns.value.map(column => column.kind === 'number'),
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

const visibleRows = computed(() =>
  rows.value
    .slice(virtualWindow.value.start, virtualWindow.value.end)
    .map((row, i) => ({ row, rowIndex: virtualWindow.value.start + i })),
)

const inspected = computed(() => {
  if (!selectedCell.value)
    return null
  const { rowIndex, columnIndex } = selectedCell.value
  const column = columns.value[columnIndex]
  const row = rows.value[rowIndex]
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

/// Streams one FETCH_SIZE window; `reset` restarts from the top, otherwise the
/// next offset appends (infinite scroll). Stale generations are ignored.
async function fetchRows(reset = true) {
  const mine = ++generation
  loading.value = true
  if (reset) {
    rows.value = []
    triggerRef(rows)
  }
  const offset = reset ? 0 : rows.value.length
  const channel = new Channel<RowsChunk>()
  channel.onmessage = (chunk) => {
    if (mine !== generation)
      return
    if (chunk.columns)
      columns.value = chunk.columns
    rows.value.push(...chunk.rows)
    triggerRef(rows)
  }
  try {
    const summary = unwrap(await commands.streamTableRows(props.connectionId, {
      schema: props.schema,
      table: props.table.name,
      limit: FETCH_SIZE,
      offset,
      sort: sort.value,
      filters: filters.value,
      includeCtid: ctidMode.value,
      includeXmin: canEverEdit.value && isPostgres.value,
    }, channel))
    if (mine === generation) {
      durationMs.value = summary.durationMs ?? 0
      fetchedAll.value = (summary.rows ?? 0) < FETCH_SIZE
    }
  }
  catch (error) {
    if (mine === generation)
      toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    if (mine === generation) {
      loading.value = false
      nextTick(maybeAppend)
    }
  }
}

watch(
  () => [props.connectionId, props.schema, props.table.name],
  () => {
    sort.value = null
    filters.value = props.initialFilters ?? []
    selectedCell.value = null
    staged.value = emptyStaged()
    editingCell.value = null
    ctidMode.value = false
    gridView.value = 'data'
    ddl.value = null
    fetchRows()
  },
  { immediate: true },
)

async function openDdlView() {
  gridView.value = 'ddl'
  if (ddl.value !== null || ddlLoading.value)
    return
  ddlLoading.value = true
  try {
    ddl.value = unwrap(await commands.tableDdl(props.connectionId, props.schema, props.table.name))
  }
  catch (error) {
    gridView.value = 'data'
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    ddlLoading.value = false
  }
}

watch(() => props.initialFilters, (initial) => {
  if (initial && initial.length > 0) {
    filters.value = initial
    fetchRows()
  }
})

// Live measurement (no arrived-state: it goes stale between scroll events).
// Checked on every scroll and again after a fetch lands at the bottom.
function nearBottom(): boolean {
  const el = scroller.value
  return el !== null && el.scrollTop + el.clientHeight >= el.scrollHeight - 600
}

function maybeAppend() {
  if (nearBottom() && !loading.value && !fetchedAll.value && pending.value === 0 && rows.value.length > 0)
    fetchRows(false)
}

watch(scrollY, maybeAppend)

// Staged edits are keyed by row index: any reordering/refetch would corrupt them.
function guardPending(): boolean {
  if (pending.value === 0)
    return false
  toast.warning('Apply or discard the staged changes first')
  return true
}

function toggleSort(column: string) {
  if (guardPending())
    return
  sort.value = sort.value?.column === column && sort.value.direction === 'asc'
    ? { column, direction: 'desc' }
    : { column, direction: 'asc' }
  fetchRows()
}

function openFilter(column: string, kind: (typeof FILTER_OPS_BY_KIND) extends Record<infer K, unknown> ? K : never) {
  const existing = filters.value.find(f => f.column === column)
  draftOp.value = existing?.op ?? FILTER_OPS_BY_KIND[kind][0]
  draftValue.value = existing?.value ?? ''
  openFilterColumn.value = column
}

function applyFilter(column: string) {
  if (guardPending())
    return
  const filter: ColumnFilter = {
    column,
    op: draftOp.value,
    value: OP_NEEDS_VALUE[draftOp.value] ? draftValue.value : null,
  }
  filters.value = [...filters.value.filter(f => f.column !== column), filter]
  openFilterColumn.value = null
  fetchRows()
}

function removeFilter(column: string) {
  if (guardPending())
    return
  filters.value = filters.value.filter(f => f.column !== column)
  openFilterColumn.value = null
  fetchRows()
}

function clearFilters() {
  if (guardPending())
    return
  filters.value = []
  fetchRows()
}

function hopFrom(rowIndex: number, columnName: string) {
  const fk = fkByColumn.value.get(columnName)
  const row = rows.value[rowIndex]
  if (!fk || !row)
    return
  const hopFilters: ColumnFilter[] = fk.columns.map((column, position) => {
    const columnIndex = columns.value.findIndex(c => c.name === column)
    const value = columnIndex === -1 ? null : row[columnIndex]
    return value === null
      ? { column: fk.referencedColumns[position], op: 'is-null', value: null }
      : { column: fk.referencedColumns[position], op: 'eq', value }
  })
  emit('hop', fk.referencedSchema, fk.referencedTable, hopFilters)
}

function displayedValue(rowIndex: number, columnIndex: number): string | null {
  const name = columns.value[columnIndex]?.name
  const edited = name !== undefined ? staged.value.edits[rowIndex]?.[name] : undefined
  return edited !== undefined ? edited : rows.value[rowIndex]?.[columnIndex] ?? null
}

function isEdited(rowIndex: number, columnIndex: number): boolean {
  const name = columns.value[columnIndex]?.name
  return name !== undefined && staged.value.edits[rowIndex]?.[name] !== undefined
}

function beginEdit(rowIndex: number, columnIndex: number) {
  if (!editable.value)
    return
  editingCell.value = { rowIndex, columnIndex }
}

function stageEdit(value: string | null) {
  const cell = editingCell.value
  if (!cell || columns.value.length === 0)
    return
  const name = columns.value[cell.columnIndex].name
  const original = rows.value[cell.rowIndex]?.[cell.columnIndex] ?? null
  const edits = { ...(staged.value.edits[cell.rowIndex] ?? {}) }
  if (value === original)
    delete edits[name]
  else
    edits[name] = value
  const all = { ...staged.value.edits }
  if (Object.keys(edits).length === 0)
    delete all[cell.rowIndex]
  else
    all[cell.rowIndex] = edits
  staged.value = { ...staged.value, edits: all }
  editingCell.value = null
}

/// Tab / shift-tab: stage the current edit and hop to the adjacent editable cell.
function stageAndMove(value: string | null, direction: 1 | -1) {
  const cell = editingCell.value
  stageEdit(value)
  if (!cell)
    return
  const positions = displayColumns.value
  const next = nextEditablePosition(
    {
      rowIndex: cell.rowIndex,
      position: positions.findIndex(({ index }) => index === cell.columnIndex),
    },
    direction,
    positions.length,
    rows.value.length,
  )
  if (next)
    beginEdit(next.rowIndex, positions[next.position].index)
}

function addRow(fromRow?: number) {
  const values: Record<string, string | null> = {}
  if (fromRow !== undefined) {
    for (const { column, index } of displayColumns.value) {
      // PK values stay behind: serials/defaults must produce fresh ones.
      if (!props.table.primaryKey.includes(column.name))
        values[column.name] = rows.value[fromRow]?.[index] ?? null
    }
  }
  staged.value = { ...staged.value, inserts: [...staged.value.inserts, values] }
  // The insert row lives at the bottom: bring it into view.
  nextTick(() => scroller.value?.scrollTo({ top: scroller.value.scrollHeight }))
}

function removeInsert(insertIndex: number) {
  staged.value = {
    ...staged.value,
    inserts: staged.value.inserts.filter((_, index) => index !== insertIndex),
  }
}

function stageInsertCell(insertIndex: number, column: string, value: string | null) {
  const inserts = staged.value.inserts.map((row, index) =>
    index === insertIndex ? { ...row, [column]: value } : row,
  )
  staged.value = { ...staged.value, inserts }
}

function toggleDeleteSelected() {
  const row = selectedCell.value?.rowIndex
  if (row === undefined)
    return
  const deletes = staged.value.deletes.includes(row)
    ? staged.value.deletes.filter(index => index !== row)
    : [...staged.value.deletes, row]
  staged.value = { ...staged.value, deletes }
}

function discardAll() {
  staged.value = emptyStaged()
  editingCell.value = null
}

// xmin rides along as an optimistic-lock guard: a concurrent write bumps it
// and the update/delete then matches nothing instead of overwriting.
const changes = computed(() => buildTableChanges(
  staged.value,
  rows.value,
  columns.value,
  hasXmin.value ? [...keyColumns.value, 'xmin'] : keyColumns.value,
  props.schema,
  props.table.name,
))

const preview = computed(() => previewSql(changes.value))

async function applyChanges() {
  applying.value = true
  try {
    const result = unwrap(await commands.applyTableChanges(props.connectionId, changes.value))
    toast.success(`applied: ${result.updated} updated, ${result.inserted} inserted, ${result.deleted} deleted`)
    previewOpen.value = false
    discardAll()
    selectedCell.value = null
    await fetchRows()
  }
  catch (error) {
    // Staging is kept: the transaction rolled back server-side.
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    applying.value = false
  }
}

// Copy formats the fetched rows; save re-streams the whole filtered/sorted
// table on the Rust side, so it covers rows beyond what the grid loaded.
async function copyRows(format: ExportFormat) {
  const exportColumns = displayColumns.value.map(({ column }) => column)
  const exportRows = rows.value.map(row => displayColumns.value.map(({ index }) => row[index] ?? null))
  try {
    const text = unwrap(await commands.formatStatement(exportColumns, exportRows, format, props.kind, props.table.name))
    await copy(text)
    toast.success(`copied ${exportRows.length} row${exportRows.length === 1 ? '' : 's'} as ${EXPORT_FORMATS[format].label}`)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
}

const exportedRows = ref<number | null>(null)
let cancelingExport = false

async function saveRows(format: ExportFormat) {
  const path = await pickExportPath(format, props.table.name)
  if (path === null)
    return
  const progress = new Channel<ExportProgress>()
  progress.onmessage = (event) => {
    exportedRows.value = event.rows
  }
  exportedRows.value = 0
  cancelingExport = false
  try {
    const summary = unwrap(await commands.exportTableRows(props.connectionId, {
      schema: props.schema,
      table: props.table.name,
      limit: null,
      offset: 0,
      sort: sort.value,
      filters: filters.value,
      includeCtid: false,
    }, format, path, progress))
    toast.success(`exported ${summary.rows} row${summary.rows === 1 ? '' : 's'} to ${path}`)
  }
  catch (error) {
    // The partial file is already gone server-side.
    if (cancelingExport)
      toast.info('export canceled')
    else
      toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    exportedRows.value = null
    cancelingExport = false
  }
}

async function cancelExport() {
  cancelingExport = true
  try {
    unwrap(await commands.cancelQuery(props.connectionId))
  }
  catch {
    // The export may have finished in the meantime; the export call reports.
  }
}

function enableCtidEditing() {
  ctidMode.value = true
  staged.value = emptyStaged()
  fetchRows()
}

useEventListener('keydown', (event) => {
  if (event.ctrlKey && event.key === 's') {
    event.preventDefault()
    if (pending.value > 0)
      previewOpen.value = true
  }
})
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

      <div
        v-if="canEverEdit && !editable"
        class="flex items-center gap-3 border-b px-3 py-1.5 font-mono text-[11px] text-muted-foreground"
        data-testid="no-pk-banner"
      >
        no primary key - editing disabled
        <Button
          v-if="isPostgres"
          size="sm"
          variant="secondary"
          class="h-6 text-[11px]"
          data-testid="enable-ctid"
          @click="enableCtidEditing"
        >
          edit via ctid
        </Button>
      </div>

      <div
        v-if="gridView === 'ddl'"
        class="min-h-0 flex-1 overflow-auto p-3"
        data-testid="ddl-view"
      >
        <p v-if="ddlLoading" class="font-mono text-xs text-muted-foreground">
          loading definition…
        </p>
        <!-- eslint-disable-next-line vue/no-v-html -- highlightSql escapes every text node -->
        <pre v-else-if="ddlHtml" class="font-mono text-xs leading-5 whitespace-pre" v-html="ddlHtml" />
      </div>

      <div v-show="gridView === 'data'" ref="scroller" data-testid="grid-scroller" class="min-h-0 flex-1 overflow-auto">
        <table class="w-max min-w-full border-separate border-spacing-0 font-mono text-xs">
          <thead class="sticky top-0 z-10">
            <tr>
              <th
                v-for="{ column, index: columnIndex } in displayColumns"
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
            <tr v-if="virtualWindow.padTop > 0" aria-hidden="true" :style="{ height: `${virtualWindow.padTop}px` }" />
            <tr
              v-for="{ row, rowIndex } in visibleRows"
              :key="rowIndex"
              data-row
              class="group hover:bg-muted/40"
              :class="staged.deletes.includes(rowIndex) && 'bg-destructive/10 text-destructive/80 line-through'"
            >
              <td
                v-for="{ index: columnIndex } in displayColumns"
                :key="columnIndex"
                class="max-w-xs cursor-default border-b border-border/50 px-3 py-1"
                :class="[
                  numericColumns[columnIndex] && 'text-right',
                  selectedCell?.rowIndex === rowIndex && selectedCell?.columnIndex === columnIndex
                    && 'bg-accent text-accent-foreground',
                  isEdited(rowIndex, columnIndex) && 'text-amber-500',
                  // The editor and its NULL button must not be clipped.
                  (editingCell?.rowIndex !== rowIndex || editingCell?.columnIndex !== columnIndex) && 'truncate',
                ]"
                :title="isEdited(rowIndex, columnIndex)
                  ? `was: ${row[columnIndex] ?? 'NULL'}`
                  : row[columnIndex] ?? undefined"
                @click="selectedCell = { rowIndex, columnIndex }"
                @dblclick="beginEdit(rowIndex, columnIndex)"
              >
                <CellEditor
                  v-if="editingCell?.rowIndex === rowIndex && editingCell?.columnIndex === columnIndex"
                  :column="columns[columnIndex]"
                  :nullable="nullableColumns.get(columns[columnIndex].name) ?? false"
                  :initial="displayedValue(rowIndex, columnIndex)"
                  @stage="stageEdit"
                  @cancel="editingCell = null"
                  @navigate="stageAndMove"
                />
                <span v-else class="inline-flex max-w-full items-center gap-1">
                  <span v-if="displayedValue(rowIndex, columnIndex) === null" class="text-muted-foreground/60 italic">NULL</span>
                  <span v-else class="truncate">{{ displayedValue(rowIndex, columnIndex) }}</span>
                  <button
                    v-if="row[columnIndex] !== null && fkByColumn.has(columns[columnIndex].name)"
                    type="button"
                    class="invisible shrink-0 text-muted-foreground hover:text-foreground group-hover:visible"
                    aria-label="Open referenced row"
                    :data-testid="`fk-hop-${columns[columnIndex].name}`"
                    @click.stop="hopFrom(rowIndex, columns[columnIndex].name)"
                  >
                    <ArrowUpRight class="size-3" />
                  </button>
                </span>
              </td>
            </tr>
            <tr v-if="virtualWindow.padBottom > 0" aria-hidden="true" :style="{ height: `${virtualWindow.padBottom}px` }" />
            <tr
              v-for="(insert, insertIndex) in staged.inserts"
              :key="`insert-${insertIndex}`"
              class="bg-emerald-500/5"
              data-testid="insert-row"
            >
              <td
                v-for="({ column }, position) in displayColumns"
                :key="column.name"
                class="max-w-xs border-b border-border/50 px-3 py-1"
              >
                <span class="flex items-center gap-1">
                  <input
                    :value="insert[column.name] ?? ''"
                    class="w-full min-w-24 border-b border-border/60 bg-transparent font-mono text-xs outline-none placeholder:text-muted-foreground/40"
                    :class="insert[column.name] === null && 'italic'"
                    :placeholder="insert[column.name] === null ? 'NULL' : 'default'"
                    :data-testid="`insert-${column.name}`"
                    @input="stageInsertCell(insertIndex, column.name, ($event.target as HTMLInputElement).value)"
                  >
                  <button
                    v-if="position === displayColumns.length - 1"
                    type="button"
                    class="shrink-0 text-muted-foreground hover:text-destructive"
                    aria-label="Remove new row"
                    data-testid="remove-insert"
                    @click="removeInsert(insertIndex)"
                  >
                    <X class="size-3" />
                  </button>
                </span>
              </td>
            </tr>
          </tbody>
        </table>
        <p
          v-if="!loading && rows.length === 0"
          class="px-4 py-8 text-center font-mono text-xs text-muted-foreground"
        >
          no rows{{ filters.length > 0 ? ' match the filters' : '' }}
        </p>
      </div>

      <footer class="flex h-9 items-center gap-3 border-t px-3 font-mono text-[11px] text-muted-foreground">
        <button
          type="button"
          class="rounded px-1.5 py-0.5"
          :class="gridView === 'data' ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
          data-testid="grid-view-data"
          @click="gridView = 'data'"
        >
          data
        </button>
        <button
          type="button"
          class="rounded px-1.5 py-0.5"
          :class="gridView === 'ddl' ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
          data-testid="grid-view-ddl"
          @click="openDdlView"
        >
          ddl
        </button>
        <Tooltip v-if="gridView === 'ddl' && ddl !== null" :open="ddlCopied">
          <TooltipTrigger as-child>
            <Button
              size="icon-xs"
              variant="ghost"
              aria-label="Copy definition"
              data-testid="copy-ddl"
              @click="copyDdl(ddl)"
            >
              <Copy />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Copied</TooltipContent>
        </Tooltip>
        <template v-if="gridView === 'data'">
          <span data-testid="grid-range">
            {{ rows.length }}{{ fetchedAll ? '' : '+' }} rows
            <template v-if="filters.length > 0">
              filtered
            </template>
            <template v-else-if="formatEstimatedRows(table.estimatedRows)">
              of ~{{ formatEstimatedRows(table.estimatedRows) }}
            </template>
          </span>
          <span>{{ durationMs.toFixed(0) }}ms</span>
        </template>
        <span class="flex-1" />
        <template v-if="editable && gridView === 'data'">
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="Add row"
                data-testid="add-row"
                @click="addRow()"
              >
                <Plus />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Add row</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="Duplicate selected row"
                data-testid="duplicate-row"
                :disabled="!selectedCell"
                @click="addRow(selectedCell!.rowIndex)"
              >
                <CopyPlus />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Duplicate selected row</TooltipContent>
          </Tooltip>
          <Tooltip>
            <TooltipTrigger as-child>
              <Button
                size="icon-sm"
                variant="ghost"
                aria-label="Delete selected row"
                data-testid="delete-row"
                :disabled="!selectedCell"
                @click="toggleDeleteSelected"
              >
                <Trash2 />
              </Button>
            </TooltipTrigger>
            <TooltipContent>Delete selected row</TooltipContent>
          </Tooltip>
          <template v-if="pending > 0">
            <button
              type="button"
              class="text-muted-foreground hover:text-foreground"
              data-testid="discard-changes"
              @click="discardAll"
            >
              discard
            </button>
            <Button size="sm" class="h-6" data-testid="apply-changes" @click="previewOpen = true">
              Apply ({{ pending }})
            </Button>
          </template>
        </template>
        <template v-if="exportedRows !== null">
          <span class="text-muted-foreground" data-testid="export-progress">
            exporting… {{ formatEstimatedRows(exportedRows) }} rows
          </span>
          <Button
            size="icon-sm"
            variant="ghost"
            aria-label="Cancel export"
            data-testid="cancel-export"
            @click="cancelExport"
          >
            <X />
          </Button>
        </template>
        <ExportMenu
          v-if="gridView === 'data'"
          :disabled="rows.length === 0 || exportedRows !== null"
          @copy="copyRows"
          @save="saveRows"
        />
        <Tooltip v-if="gridView === 'data'">
          <TooltipTrigger as-child>
            <Button
              size="icon-sm"
              variant="ghost"
              aria-label="Refresh rows"
              data-testid="grid-refresh"
              :disabled="loading || pending > 0"
              @click="fetchRows()"
            >
              <RefreshCw :class="loading ? 'animate-spin' : ''" />
            </Button>
          </TooltipTrigger>
          <TooltipContent>Refresh rows</TooltipContent>
        </Tooltip>
      </footer>

      <Dialog v-model:open="previewOpen">
        <DialogContent class="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle class="font-mono font-medium">
              Apply {{ pending }} change{{ pending === 1 ? '' : 's' }}
            </DialogTitle>
            <DialogDescription>
              Runs in a single transaction; each update and delete must match exactly one row.
            </DialogDescription>
          </DialogHeader>
          <pre class="max-h-72 overflow-auto rounded bg-muted px-3 py-2 font-mono text-xs break-all whitespace-pre-wrap" data-testid="sql-preview">{{ preview.join('\n') }}</pre>
          <DialogFooter class="gap-2">
            <Button variant="outline" @click="previewOpen = false">
              Cancel
            </Button>
            <Button data-testid="confirm-apply" :disabled="applying" @click="applyChanges">
              {{ applying ? 'Applying…' : 'Apply' }}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
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
