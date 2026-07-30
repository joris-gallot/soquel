<script setup lang="ts">
import type { ExplainPlan, PlanNode } from '@/lib/explain'
import { ChevronRight } from '@lucide/vue'
import { computed, ref } from 'vue'
import { flattenPlan, formatMs, formatRows, hiddenByCollapse } from '@/lib/explain'
import { highlightJson } from '@/lib/highlight'

const props = defineProps<{ plans: ExplainPlan[], raw: string }>()

const view = ref<'tree' | 'raw'>('tree')
const collapsed = ref<Set<string>>(new Set())

const rowsByPlan = computed(() =>
  props.plans.map(plan =>
    flattenPlan(plan.root).filter(node => !hiddenByCollapse(node.id, collapsed.value)),
  ),
)

function toggle(id: string) {
  const next = new Set(collapsed.value)
  if (!next.delete(id))
    next.add(id)
  collapsed.value = next
}

const rawHtml = computed(() => {
  try {
    return highlightJson(JSON.stringify(JSON.parse(props.raw), null, 2))
  }
  catch {
    return highlightJson(props.raw)
  }
})

// Heat buckets keep the palette quiet: only real time sinks light up.
function heatClass(node: PlanNode): string {
  if (node.heat >= 0.4)
    return 'bg-red-500'
  if (node.heat >= 0.1)
    return 'bg-amber-500'
  return 'bg-muted-foreground/40'
}

function timing(plan: ExplainPlan, node: PlanNode): string {
  if (!plan.analyzed)
    return `cost ${node.totalCost.toFixed(node.totalCost >= 100 ? 0 : 2)}`
  return node.inclusiveMs === null ? '' : formatMs(node.inclusiveMs)
}
</script>

<template>
  <div class="flex min-h-0 flex-1 flex-col" data-testid="explain-tree">
    <div class="flex h-8 items-center gap-3 border-b px-3 font-mono text-[11px] text-muted-foreground">
      <template v-for="(plan, planIndex) in plans" :key="planIndex">
        <span v-if="plan.planningMs !== null">planning {{ formatMs(plan.planningMs) }}</span>
        <span v-if="plan.executionMs !== null" data-testid="explain-execution">
          execution {{ formatMs(plan.executionMs) }}
        </span>
        <span v-if="!plan.analyzed" class="italic">estimated costs only</span>
      </template>
      <span class="flex-1" />
      <button
        type="button"
        class="rounded px-1.5 py-0.5"
        :class="view === 'tree' ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
        data-testid="explain-view-tree"
        @click="view = 'tree'"
      >
        tree
      </button>
      <button
        type="button"
        class="rounded px-1.5 py-0.5"
        :class="view === 'raw' ? 'bg-accent text-accent-foreground' : 'hover:text-foreground'"
        data-testid="explain-view-raw"
        @click="view = 'raw'"
      >
        raw
      </button>
    </div>

    <div v-if="view === 'tree'" class="min-h-0 flex-1 overflow-auto py-1">
      <template v-for="(rows, planIndex) in rowsByPlan" :key="planIndex">
        <div
          v-for="node in rows"
          :key="node.id"
          class="group flex min-w-0 items-center gap-1.5 px-3 py-0.5 font-mono text-xs hover:bg-muted/40"
          :style="{ paddingLeft: `${node.depth * 16 + 12}px` }"
          data-testid="explain-node"
        >
          <button
            v-if="node.children.length > 0"
            type="button"
            class="shrink-0 text-muted-foreground hover:text-foreground"
            :aria-label="collapsed.has(node.id) ? 'Expand node' : 'Collapse node'"
            :data-testid="`explain-toggle-${node.id}`"
            @click="toggle(node.id)"
          >
            <ChevronRight
              class="size-3 transition-transform"
              :class="collapsed.has(node.id) ? '' : 'rotate-90'"
            />
          </button>
          <span v-else class="w-3 shrink-0" />

          <span class="shrink-0 font-medium">{{ node.nodeType }}</span>
          <span v-if="node.target" class="shrink-0 text-muted-foreground">{{ node.target }}</span>
          <span
            v-if="node.condition"
            class="min-w-0 truncate text-muted-foreground/70"
            :title="node.condition"
          >
            {{ node.condition }}
          </span>

          <span class="ml-auto flex shrink-0 items-center gap-3 pl-3 text-[11px] tabular-nums">
            <span v-if="node.neverExecuted" class="text-muted-foreground/60 italic">never executed</span>
            <template v-else>
              <span
                :class="node.estimateOff ? 'text-amber-500' : 'text-muted-foreground'"
                :title="node.estimateOff ? 'planner estimate off by 10x or more' : 'estimated -> actual rows'"
              >
                rows {{ formatRows(node.planRows) }}<template v-if="node.actualRows !== null">&rarr;{{ formatRows(node.actualRows) }}</template>
              </span>
              <span :class="node.heat >= 0.4 ? 'text-red-500' : node.heat >= 0.1 ? 'text-amber-500' : 'text-muted-foreground'">
                {{ timing(plans[planIndex], node) }}
              </span>
            </template>
            <!-- Exclusive share of total: the folded-flamegraph signature. -->
            <span class="h-1 w-10 overflow-hidden rounded-full bg-muted">
              <span
                class="block h-full rounded-full"
                :class="heatClass(node)"
                :style="{ width: `${Math.max(node.heat * 100, node.heat > 0 ? 4 : 0)}%` }"
              />
            </span>
          </span>
        </div>
      </template>
    </div>

    <!-- eslint-disable-next-line vue/no-v-html -- highlightJson escapes every text node -->
    <pre v-else class="min-h-0 flex-1 overflow-auto p-3 font-mono text-xs leading-5" v-html="rawHtml" />
  </div>
</template>
