<script setup lang="ts">
import type { SecretSubject } from '@/lib/bindings'
import { computed, ref, watch } from 'vue'
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
import { Switch } from '@/components/ui/switch'
import { useSecretPrompt } from '@/composables/useSecretPrompt'

const { pending, showAsDialog, unlock, dismiss } = useSecretPrompt()

const SUBJECT_TITLES: Record<SecretSubject, string> = {
  connection: 'Password for',
  tunnel: 'Credential for the tunnel',
}

const SUBJECT_HINTS: Record<SecretSubject, string> = {
  connection: 'This connection asks for its password every time. Nothing is written to the keychain.',
  tunnel: 'This tunnel asks for its ssh password or key passphrase every time. Nothing is written to the keychain.',
}

const secret = ref('')
const remember = ref(false)
const unlocking = ref(false)

const open = computed({
  get: () => showAsDialog.value,
  set: (value) => {
    if (!value)
      dismiss()
  },
})

watch(open, (isOpen) => {
  if (isOpen) {
    secret.value = ''
    remember.value = false
  }
})

async function confirm() {
  unlocking.value = true
  try {
    await unlock(secret.value, remember.value)
  }
  catch (error) {
    toast.error(error instanceof Error ? error.message : String(error))
  }
  finally {
    unlocking.value = false
    secret.value = ''
  }
}
</script>

<template>
  <Dialog v-model:open="open">
    <DialogContent class="sm:max-w-md" data-testid="secret-prompt-dialog">
      <DialogHeader>
        <DialogTitle class="font-mono font-medium">
          {{ pending ? SUBJECT_TITLES[pending.subject] : '' }} {{ pending?.targetName }}
        </DialogTitle>
        <DialogDescription>
          {{ pending ? SUBJECT_HINTS[pending.subject] : '' }}
        </DialogDescription>
      </DialogHeader>

      <form class="space-y-3" @submit.prevent="confirm">
        <div class="space-y-1.5">
          <Label for="prompt-secret">Password</Label>
          <Input
            id="prompt-secret"
            v-model="secret"
            data-testid="field-prompt-secret"
            type="password"
            autofocus
          />
        </div>
        <div class="flex items-center gap-2">
          <Switch id="prompt-remember" v-model="remember" data-testid="field-prompt-remember" />
          <Label for="prompt-remember" class="text-xs font-normal text-muted-foreground">
            Keep it until I disconnect
          </Label>
        </div>
      </form>

      <DialogFooter class="gap-2">
        <Button variant="outline" @click="dismiss">
          Cancel
        </Button>
        <Button
          data-testid="submit-prompt-secret"
          :disabled="unlocking || secret === ''"
          @click="confirm"
        >
          Connect
        </Button>
      </DialogFooter>
    </DialogContent>
  </Dialog>
</template>
