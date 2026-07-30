<script setup lang="ts">
import type { ExportFormat } from '@/lib/bindings'
import { Download } from '@lucide/vue'
import { Button } from '@/components/ui/button'
import {
  DropdownMenu,
  DropdownMenuContent,
  DropdownMenuItem,
  DropdownMenuLabel,
  DropdownMenuSeparator,
  DropdownMenuTrigger,
} from '@/components/ui/dropdown-menu'
import { EXPORT_FORMAT_KEYS, EXPORT_FORMATS } from '@/lib/export'

defineProps<{ disabled?: boolean }>()
defineEmits<{ copy: [format: ExportFormat], save: [format: ExportFormat] }>()
</script>

<template>
  <DropdownMenu>
    <DropdownMenuTrigger as-child>
      <Button
        size="icon-sm"
        variant="ghost"
        aria-label="Export rows"
        title="Export rows"
        data-testid="export-menu"
        :disabled="disabled"
      >
        <Download />
      </Button>
    </DropdownMenuTrigger>
    <DropdownMenuContent align="end" class="font-mono text-xs">
      <DropdownMenuLabel class="text-[10px] text-muted-foreground">
        Copy as
      </DropdownMenuLabel>
      <DropdownMenuItem
        v-for="format in EXPORT_FORMAT_KEYS"
        :key="`copy-${format}`"
        class="text-xs"
        :data-testid="`export-copy-${format}`"
        @click="$emit('copy', format)"
      >
        {{ EXPORT_FORMATS[format].label }}
      </DropdownMenuItem>
      <DropdownMenuSeparator />
      <DropdownMenuLabel class="text-[10px] text-muted-foreground">
        Save as
      </DropdownMenuLabel>
      <DropdownMenuItem
        v-for="format in EXPORT_FORMAT_KEYS"
        :key="`save-${format}`"
        class="text-xs"
        :data-testid="`export-save-${format}`"
        @click="$emit('save', format)"
      >
        {{ EXPORT_FORMATS[format].label }}…
      </DropdownMenuItem>
    </DropdownMenuContent>
  </DropdownMenu>
</template>
