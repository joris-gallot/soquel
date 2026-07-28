import { useColorMode } from '@vueuse/core'

const mode = useColorMode({ initialValue: 'dark' })

export function useTheme() {
  function toggle() {
    mode.value = mode.value === 'dark' ? 'light' : 'dark'
  }
  return { mode, toggle }
}
