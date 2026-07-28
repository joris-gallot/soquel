import { createRouter, createWebHistory } from 'vue-router'

export const router = createRouter({
  history: createWebHistory(),
  routes: [
    {
      path: '/',
      name: 'connections',
      component: () => import('@/pages/ConnectionsPage.vue'),
    },
  ],
})
