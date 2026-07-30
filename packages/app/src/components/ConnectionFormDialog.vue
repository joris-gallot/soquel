<script setup lang="ts">
import type { ConnectionInput, ConnectionProfile } from '@/lib/bindings'
import type { ConnectionFormValues, EngineChoice } from '@/lib/connections'
import { open as openFileDialog } from '@tauri-apps/plugin-dialog'
import { computed, ref, watch } from 'vue'
import { toast } from 'vue-sonner'
import HostKeyTrustPanel from '@/components/HostKeyTrustPanel.vue'
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
import { Switch } from '@/components/ui/switch'
import { useConnections } from '@/composables/useConnections'
import { useTunnels } from '@/composables/useTunnels'
import { connectionSchema, ENGINE_CHOICES, ENVS, formValuesFromProfile, NO_TUNNEL, parseConnectionUrl, portForKindChange, SSL_MODES, toConnectionInput } from '@/lib/connections'
import { CommandError } from '@/lib/result'
import { zodFieldErrors } from '@/lib/validation'

const props = defineProps<{ profile?: ConnectionProfile | null }>()
const emit = defineEmits<{ saved: [] }>()
const open = defineModel<boolean>('open', { required: true })

const { connections, create, update, test } = useConnections()
const { tunnels, refresh: refreshTunnels } = useTunnels()

const knownGroups = computed(() =>
  [...new Set(connections.value.map(profile => profile.group).filter((group): group is string => group !== null))].sort(),
)

const NO_GROUP = 'none'
const NEW_GROUP = '__new__'
const groupChoice = ref<string>(NO_GROUP)
const newGroup = ref('')

function groupValue(): string {
  if (groupChoice.value === NEW_GROUP)
    return newGroup.value
  return groupChoice.value === NO_GROUP ? '' : groupChoice.value
}

function emptyValues(): ConnectionFormValues {
  return { name: '', env: 'dev', kind: 'postgres', host: 'localhost', port: 5432, database: '', user: '', sslMode: 'prefer', sslRootCert: '', tunnelId: NO_TUNNEL, group: '', password: '', path: '', dbIndex: 0, tls: false }
}

const values = ref<ConnectionFormValues>(emptyValues())
const engineChoice = ref<EngineChoice>('postgres')

// The select shows engines (MariaDB included); the profile stores the kind.
watch(engineChoice, (id) => {
  const choice = ENGINE_CHOICES.find(entry => entry.id === id)!
  const previous = values.value.kind
  values.value.kind = choice.kind
  values.value.port = portForKindChange(values.value.port, previous, choice.kind)
})

const isSqlite = computed(() => values.value.kind === 'sqlite')
const isRedis = computed(() => values.value.kind === 'redis')
// postgres/mysql: the full server shape (database, user, ssl).
const isSqlServer = computed(() => !isSqlite.value && !isRedis.value)

async function browsePath() {
  const selected = await openFileDialog({
    filters: [
      { name: 'SQLite database', extensions: ['db', 'sqlite', 'sqlite3', 'db3'] },
      { name: 'All files', extensions: ['*'] },
    ],
  })
  if (typeof selected === 'string')
    values.value.path = selected
}

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
  refreshTunnels()
  const group = props.profile?.group ?? null
  groupChoice.value = group === null ? NO_GROUP : group
  newGroup.value = ''
  values.value = props.profile ? formValuesFromProfile(props.profile) : emptyValues()
  // A stored mariadb profile reads back as mysql: same kind by design.
  engineChoice.value = values.value.kind
})

function applyUrl() {
  const parsed = parseConnectionUrl(importUrl.value)
  if (!parsed) {
    toast.error('Not a valid postgres:// or mysql:// URL')
    return
  }
  values.value = { ...values.value, ...parsed }
  if (parsed.kind)
    engineChoice.value = parsed.kind
  importUrl.value = ''
}

function parse(): ConnectionInput | null {
  values.value.group = groupValue()
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
    // The host key panel owns this failure mode.
    if (error instanceof CommandError && error.kind === 'host-key-untrusted')
      testResult.value = null
    else
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
          {{ profile ? 'Update the connection details.' : 'Point soquel at a database.' }}
        </DialogDescription>
      </DialogHeader>

      <form class="space-y-4" @submit.prevent="save">
        <div v-if="!profile" class="flex gap-2">
          <Input
            v-model="importUrl"
            data-testid="import-url"
            placeholder="paste a postgres://, mysql:// or redis:// url to prefill"
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

        <div class="space-y-1.5">
          <Label>Database engine</Label>
          <Select v-model="engineChoice">
            <SelectTrigger data-testid="field-kind" class="w-full">
              <SelectValue />
            </SelectTrigger>
            <SelectContent>
              <SelectItem v-for="choice in ENGINE_CHOICES" :key="choice.id" :value="choice.id">
                {{ choice.label }}
              </SelectItem>
            </SelectContent>
          </Select>
        </div>

        <div v-if="isSqlite" class="space-y-1.5">
          <Label for="conn-path">Database file</Label>
          <div class="flex gap-2">
            <Input
              id="conn-path"
              v-model="values.path"
              data-testid="field-path"
              class="font-mono text-xs"
              placeholder="/path/to/app.db"
            />
            <Button type="button" variant="secondary" @click="browsePath">
              Browse
            </Button>
          </div>
          <p v-if="errors.path" class="text-xs text-destructive">
            {{ errors.path }}
          </p>
        </div>

        <div v-if="!isSqlite" class="grid grid-cols-[1fr_8rem] gap-3">
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

        <div v-if="isSqlServer" class="grid grid-cols-[1fr_8rem] gap-3">
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

        <!-- The db lives in the workspace selector; a url prefill still carries it. -->
        <div v-if="isRedis" class="flex items-center gap-2">
          <Switch id="conn-tls" v-model="values.tls" data-testid="field-tls" />
          <Label for="conn-tls" class="cursor-pointer">TLS (rediss)</Label>
        </div>

        <div v-if="values.sslMode === 'verify-full'" class="space-y-1.5">
          <Label for="conn-ssl-root-cert">CA certificate</Label>
          <Input
            id="conn-ssl-root-cert"
            v-model="values.sslRootCert"
            data-testid="field-ssl-root-cert"
            class="font-mono text-xs"
            placeholder="/path/to/ca.pem (empty = system trust store)"
          />
        </div>

        <div v-if="!isSqlite" class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <Label for="conn-user">User</Label>
            <Input
              id="conn-user"
              v-model="values.user"
              data-testid="field-user"
              class="font-mono"
              :placeholder="isRedis ? 'default (ACL user, optional)' : ''"
            />
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

        <div class="grid grid-cols-2 gap-3">
          <div class="space-y-1.5">
            <Label>Group</Label>
            <Select v-model="groupChoice">
              <SelectTrigger data-testid="field-group" class="w-full">
                <SelectValue />
              </SelectTrigger>
              <SelectContent>
                <SelectItem :value="NO_GROUP">
                  none
                </SelectItem>
                <SelectItem v-for="group in knownGroups" :key="group" :value="group">
                  {{ group }}
                </SelectItem>
                <SelectItem :value="NEW_GROUP" data-testid="new-group-option">
                  + new group
                </SelectItem>
              </SelectContent>
            </Select>
            <Input
              v-if="groupChoice === NEW_GROUP"
              v-model="newGroup"
              data-testid="field-new-group"
              placeholder="group name"
            />
          </div>
          <div v-if="!isSqlite" class="space-y-1.5">
            <Label>SSH tunnel</Label>
            <Select v-model="values.tunnelId">
              <SelectTrigger data-testid="field-tunnel" class="w-full">
                <SelectValue placeholder="none" />
              </SelectTrigger>
              <SelectContent>
                <SelectItem :value="NO_TUNNEL">
                  none
                </SelectItem>
                <SelectItem v-for="tunnel in tunnels" :key="tunnel.id" :value="tunnel.id">
                  {{ tunnel.name }}
                </SelectItem>
              </SelectContent>
            </Select>
          </div>
        </div>

        <HostKeyTrustPanel />

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
