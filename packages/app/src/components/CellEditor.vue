<script setup lang="ts">
import type { QueryColumn } from '@/lib/bindings'
import { Ban } from '@lucide/vue'
import { computed, onMounted, ref, useTemplateRef } from 'vue'

const props = defineProps<{
  column: QueryColumn
  nullable: boolean
  initial: string | null
}>()

const emit = defineEmits<{
  stage: [value: string | null]
  cancel: []
  navigate: [value: string | null, direction: 1 | -1]
}>()

const NULL_OPTION = '__null__'

type Mode = 'bool' | 'date' | 'json' | 'text'

const mode = computed<Mode>(() => {
  if (props.column.kind === 'bool')
    return 'bool'
  // Only plain dates: datetime-local would drop timezone and microseconds.
  if (props.column.dataType === 'date')
    return 'date'
  if (props.column.kind === 'json')
    return 'json'
  return 'text'
})

const value = ref(
  mode.value === 'bool' && props.initial === null ? NULL_OPTION : props.initial ?? '',
)

const jsonValid = computed(() => {
  if (mode.value !== 'json' || value.value.trim() === '')
    return true
  try {
    JSON.parse(value.value)
    return true
  }
  catch {
    return false
  }
})

const field = useTemplateRef<HTMLElement>('field')
onMounted(() => field.value?.focus())

function staged(): string | null {
  if (mode.value === 'bool')
    return value.value === NULL_OPTION ? null : value.value
  // A cleared date reads as NULL: '' can never cast to date anyway.
  if (mode.value === 'date' && value.value === '')
    return null
  return value.value
}

function stage() {
  if (!jsonValid.value)
    return
  emit('stage', staged())
}

function blurStage() {
  // Invalid JSON never reaches the staging area: blur reverts like escape.
  if (!jsonValid.value) {
    emit('cancel')
    return
  }
  emit('stage', staged())
}

function navigate(direction: 1 | -1) {
  if (!jsonValid.value)
    return
  emit('navigate', staged(), direction)
}
</script>

<template>
  <span class="flex items-center gap-1">
    <select
      v-if="mode === 'bool'"
      ref="field"
      v-model="value"
      class="w-full min-w-24 border-b border-ring bg-background text-foreground font-mono text-xs outline-none [color-scheme:light] dark:[color-scheme:dark]"
      data-testid="cell-editor-bool"
      @change="stage"
      @keydown.enter.prevent="stage"
      @keydown.escape="emit('cancel')"
      @keydown.tab.exact.prevent="navigate(1)"
      @keydown.shift.tab.prevent="navigate(-1)"
      @blur="blurStage"
    >
      <option v-if="nullable" :value="NULL_OPTION">NULL</option>
      <option value="true">true</option>
      <option value="false">false</option>
    </select>

    <input
      v-else-if="mode === 'date'"
      ref="field"
      v-model="value"
      type="date"
      class="w-full min-w-32 border-b border-ring bg-transparent font-mono text-xs outline-none [color-scheme:light] dark:[color-scheme:dark]"
      data-testid="cell-editor-date"
      @keydown.enter="stage"
      @keydown.escape="emit('cancel')"
      @keydown.tab.exact.prevent="navigate(1)"
      @keydown.shift.tab.prevent="navigate(-1)"
      @blur="blurStage"
      @click.stop
    >

    <textarea
      v-else-if="mode === 'json'"
      ref="field"
      v-model="value"
      rows="4"
      class="w-full min-w-64 resize-y border-b bg-transparent font-mono text-xs outline-none"
      :class="jsonValid ? 'border-ring' : 'border-destructive text-destructive'"
      :title="jsonValid ? undefined : 'invalid JSON'"
      data-testid="cell-editor-json"
      @keydown.ctrl.enter="stage"
      @keydown.escape="emit('cancel')"
      @keydown.tab.exact.prevent="navigate(1)"
      @keydown.shift.tab.prevent="navigate(-1)"
      @blur="blurStage"
      @click.stop
    />

    <input
      v-else
      ref="field"
      v-model="value"
      class="w-full min-w-24 border-b border-ring bg-transparent font-mono text-xs outline-none"
      data-testid="cell-editor"
      @keydown.enter="stage"
      @keydown.escape="emit('cancel')"
      @keydown.tab.exact.prevent="navigate(1)"
      @keydown.shift.tab.prevent="navigate(-1)"
      @blur="blurStage"
      @click.stop
    >

    <button
      v-if="nullable && mode !== 'bool'"
      type="button"
      class="shrink-0 text-muted-foreground hover:text-foreground"
      aria-label="Set NULL"
      data-testid="cell-set-null"
      title="Set NULL"
      @mousedown.prevent.stop="emit('stage', null)"
    >
      <Ban class="size-3" />
    </button>
  </span>
</template>
