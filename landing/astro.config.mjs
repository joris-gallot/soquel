import tailwindcss from '@tailwindcss/vite'
import { defineConfig } from 'astro/config'

export default defineConfig({
  site: 'https://soquel.dev',
  vite: {
    plugins: [tailwindcss()],
  },
})
