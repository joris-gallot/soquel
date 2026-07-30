<script setup lang="ts">
import type { QueryColumn } from '@/lib/bindings'
import { ArrowUpRight, Copy, X } from '@lucide/vue'
import { useClipboard } from '@vueuse/core'
import { computed } from 'vue'
import { Button } from '@/components/ui/button'
import { Tooltip, TooltipContent, TooltipTrigger } from '@/components/ui/tooltip'
import { highlightJson } from '@/lib/highlight'

const props = defineProps<{
  column: QueryColumn
  value: string | null
  canHop: boolean
}>()

defineEmits<{ hop: [], close: [] }>()

const { copy, copied } = useClipboard()

const pretty = computed(() => {
  if (props.value === null)
    return null
  if (props.column.kind !== 'json')
    return props.value
  try {
    return JSON.stringify(JSON.parse(props.value), null, 2)
  }
  catch {
    return props.value
  }
})

const html = computed(() =>
  props.column.kind === 'json' && pretty.value !== null ? highlightJson(pretty.value) : null,
)
</script>

<template>
  <aside class="flex h-full min-h-0 flex-col" data-testid="cell-inspector">
    <header class="flex items-center gap-2 border-b px-3 py-1.5">
      <span class="min-w-0 truncate font-mono text-xs font-medium">{{ column.name }}</span>
      <span class="font-mono text-[10px] text-muted-foreground">{{ column.dataType }}</span>
      <span class="flex-1" />
      <!-- Controlled open: the tooltip only flashes "Copied" after a click. -->
      <Tooltip v-if="value !== null" :open="copied">
        <TooltipTrigger as-child>
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label="Copy value"
            data-testid="inspector-copy"
            @click="copy(value)"
          >
            <Copy />
          </Button>
        </TooltipTrigger>
        <TooltipContent>Copied</TooltipContent>
      </Tooltip>
      <Tooltip v-if="canHop && value !== null">
        <TooltipTrigger as-child>
          <Button
            size="icon-xs"
            variant="ghost"
            aria-label="Open referenced row"
            data-testid="inspector-hop"
            @click="$emit('hop')"
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
        @click="$emit('close')"
      >
        <X />
      </Button>
    </header>
    <div class="min-h-0 flex-1 overflow-auto p-3">
      <span v-if="value === null" class="font-mono text-xs text-muted-foreground/60 italic">NULL</span>
      <!-- eslint-disable-next-line vue/no-v-html -- highlightJson escapes every text node -->
      <pre v-else-if="html" class="font-mono text-xs break-all whitespace-pre-wrap" data-testid="inspector-value" v-html="html" />
      <pre v-else class="font-mono text-xs break-all whitespace-pre-wrap" data-testid="inspector-value">{{ pretty }}</pre>
    </div>
  </aside>
</template>
