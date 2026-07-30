<script setup lang="ts">
import type { QueryColumn } from '@/lib/bindings'
import { Ban } from '@lucide/vue'
import { computed, onMounted, ref, useTemplateRef } from 'vue'
import {
  editorMode,
  editorValueValid,
  initialEditorValue,
  NULL_OPTION,
  stagedValue,
} from '@/lib/cell-editing'

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

const mode = computed(() => editorMode(props.column))
const value = ref(initialEditorValue(mode.value, props.initial))
const jsonValid = computed(() => editorValueValid(mode.value, value.value))

const field = useTemplateRef<HTMLElement>('field')
onMounted(() => field.value?.focus())

function stage() {
  if (!jsonValid.value)
    return
  emit('stage', stagedValue(mode.value, value.value))
}

function blurStage() {
  // Invalid JSON never reaches the staging area: blur reverts like escape.
  if (!jsonValid.value) {
    emit('cancel')
    return
  }
  emit('stage', stagedValue(mode.value, value.value))
}

function navigate(direction: 1 | -1) {
  if (!jsonValid.value)
    return
  emit('navigate', stagedValue(mode.value, value.value), direction)
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
