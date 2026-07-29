<script setup lang="ts">
import type { ConnectionInput, ConnectionProfile } from '@/lib/bindings'
import type { ConnectionFormValues } from '@/lib/connections'
import { ref, watch } from 'vue'
import { toast } from 'vue-sonner'
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
import { Label } from '@/components/ui/label'
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from '@/components/ui/select'
import { useConnections } from '@/composables/useConnections'
import { connectionSchema, ENVS, parsePostgresUrl, SSL_MODES, toConnectionInput } from '@/lib/connections'
import { zodFieldErrors } from '@/lib/validation'

const props = defineProps<{ profile?: ConnectionProfile | null }>()
const emit = defineEmits<{ saved: [] }>()
const open = defineModel<boolean>('open', { required: true })

const { create, update, test } = useConnections()

function emptyValues(): ConnectionFormValues {
  return { name: '', env: 'dev', kind: 'postgres', host: 'localhost', port: 5432, database: '', user: '', sslMode: 'prefer', password: '' }
}

const values = ref<ConnectionFormValues>(emptyValues())
const errors = ref<Record<string, string>>({})
const importUrl = ref('')
const testing = ref(false)
const saving = ref(false)
const testResult = ref<{ ok: boolean, message: string } | null>(null)

watch(open, (isOpen) => {
  if (!isOpen)
    return
  errors.value = {}
  testResult.value = null
  importUrl.value = ''
  values.value = props.profile
    ? {
        name: props.profile.name,
        env: props.profile.env,
        kind: props.profile.kind,
        host: props.profile.host,
        port: props.profile.port,
        database: props.profile.database,
        user: props.profile.user,
        sslMode: props.profile.sslMode ?? 'prefer',
        password: '',
      }
    : emptyValues()
})

function applyUrl() {
  const parsed = parsePostgresUrl(importUrl.value)
  if (!parsed) {
    toast.error('Not a valid postgres:// URL')
    return
  }
  values.value = { ...values.value, ...parsed }
  importUrl.value = ''
}

function parse(): ConnectionInput | null {
  const result = connectionSchema.safeParse(values.value)
  if (!result.success) {
    errors.value = zodFieldErrors(result.error)
    return null
  }
  errors.value = {}
  return toConnectionInput(result.data)
}

async function runTest() {
  const input = parse()
  if (!input)
    return
  testing.value = true
  testResult.value = null
  try {
    await test(input, props.profile?.id)
    testResult.value = { ok: true, message: 'Connection OK' }
  }
  catch (error) {
    testResult.value = { ok: false, message: error instanceof Error ? error.message : String(error) }
  }
  finally {
    testing.value = false
  }
}

async function save() {
  const input = parse()
  if (!input)
    return
  saving.value = true
  try {
    if (props.profile)
      await update(props.profile.id, input)
    else
      await create(input)
    open.value = false
    emit('saved')
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    saving.value = false
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          {{ profile ? 'Edit connection' : 'New connection' }}
        </DialogTitle>
        <DialogDescription>
          {{ profile ? 'Update the connection details.' : 'Point soquel at a postgres database.' }}
        </DialogDescription>
      </DialogHeader>

      <form class="space-y-4" @submit.prevent="save">
        <div v-if="!profile" class="flex gap-2">
          <Input
            v-model="importUrl"
            data-testid="import-url"
            placeholder="paste a postgres:// url to prefill"
            class="font-mono text-xs"
          />
          <Button type="button" variant="secondary" :disabled="!importUrl" @click="applyUrl">
            Fill
          </Button>
        </div>

        <div class="grid grid-cols-[1fr_8rem] gap-3">
          <div class="space-y-1.5">
            <Label for="conn-name">Name</Label>
            <Input id="conn-name" v-model="values.name" data-testid="field-name" placeholder="local dev" />
            <p v-if="errors.name" class="text-xs text-destructive">
              {{ errors.name }}
            </p>
          </div>
          <div class="space-y-1.5">
            <Label>Environment</Label>
            <Select v-model="values.env">
              <SelectTrigger data-testid="field-env" class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="env in ENVS" :key="env" :value="env">
                  {{ env }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div class="grid grid-cols-[1fr_8rem] gap-3">
          <div class="space-y-1.5">
            <Label for="conn-host">Host</Label>
            <Input id="conn-host" v-model="values.host" data-testid="field-host" class="font-mono" />
            <p v-if="errors.host" class="text-xs text-destructive">
              {{ errors.host }}
            </p>
          </div>
          <div class="space-y-1.5">
            <Label for="conn-port">Port</Label>
            <Input id="conn-port" v-model="values.port" data-testid="field-port" type="number" class="font-mono" />
            <p v-if="errors.port" class="text-xs text-destructive">
              {{ errors.port }}
            </p>
          </div>
        </div>

        <div class="grid grid-cols-[1fr_8rem] gap-3">
          <div class="space-y-1.5">
            <Label for="conn-database">Database</Label>
            <Input id="conn-database" v-model="values.database" data-testid="field-database" class="font-mono" />
            <p v-if="errors.database" class="text-xs text-destructive">
              {{ errors.database }}
            </p>
          </div>
          <div class="space-y-1.5">
            <Label>SSL mode</Label>
            <Select v-model="values.sslMode">
              <SelectTrigger data-testid="field-ssl-mode" class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem v-for="mode in SSL_MODES" :key="mode" :value="mode">
                  {{ mode }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <Label for="conn-user">User</Label>
            <Input id="conn-user" v-model="values.user" data-testid="field-user" class="font-mono" />
            <p v-if="errors.user" class="text-xs text-destructive">
              {{ errors.user }}
            </p>
          </div>
          <div class="space-y-1.5">
            <Label for="conn-password">Password</Label>
            <Input
              id="conn-password"
              v-model="values.password"
              data-testid="field-password"
              type="password"
              :placeholder="profile ? 'unchanged' : ''"
            />
          </div>
        </div>

        <p
          v-if="testResult"
          data-testid="test-result"
          class="font-mono text-xs"
          :class="testResult.ok ? 'text-emerald-500' : 'text-destructive'"
        >
          {{ testResult.message }}
        </p>

        <DialogFooter class="gap-2 sm:justify-between">
          <Button
            type="button"
            variant="outline"
            data-testid="test-connection"
            :disabled="testing"
            @click="runTest"
          >
            {{ testing ? 'Testing…' : 'Test connection' }}
          </Button>
          <Button type="submit" data-testid="save-connection" :disabled="saving">
            {{ profile ? 'Save changes' : 'Create connection' }}
          </Button>
        </DialogFooter>
      </form>
    </DialogContent>
  </Dialog>
</template>
