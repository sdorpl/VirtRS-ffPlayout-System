<script setup lang="ts">
import { onBeforeUnmount, onMounted, ref } from 'vue'
import dayjs from 'dayjs'
import timezone from 'dayjs/plugin/timezone.js'
import utc from 'dayjs/plugin/utc.js'
import { useRoute, useRouter } from 'vue-router'
import { useI18n } from 'vue-i18n'

import SvgIcon from '@/components/utils/SvgIcon.vue'

dayjs.extend(utc)
dayjs.extend(timezone)

import { useAuth } from '@/stores/auth'
import { useIndex } from '@/stores/index'
import { useConfig } from '@/stores/config'

const { t } = useI18n()
const router = useRouter()
const route = useRoute()

const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()

const menuDropdown = ref()
const isOpen = ref(false)
const clock = ref('')
let clockTimer: ReturnType<typeof setInterval> | undefined

function tick() {
    clock.value = dayjs().tz(configStore.timezone).format('HH:mm:ss')
}

onMounted(() => {
    tick()
    clockTimer = setInterval(tick, 1000)
})

onBeforeUnmount(() => {
    clearInterval(clockTimer)
})

const menuItems = ref([
    { label: 'home', name: t('button.home'), link: '/' },
    { label: 'player', name: t('button.player'), link: '/player' },
    { label: 'media', name: t('button.media'), link: '/media' },
    { label: 'epg', name: t('button.epg'), link: '/epg' },
    { label: 'message', name: t('button.message'), link: '/message' },
    { label: 'logging', name: t('button.logging'), link: '/logging' },
])

function closeMenu() {
    setTimeout(() => {
        isOpen.value = false
        menuDropdown.value?.removeAttribute('open')
    }, 200)
}

function clickMenu() {
    isOpen.value = !isOpen.value

    if (!isOpen.value) {
        menuDropdown.value?.removeAttribute('open')
    }
}

function blurMenu() {
    if (isOpen.value) {
        isOpen.value = !isOpen.value
    } else {
        setTimeout(() => {
            menuDropdown.value?.removeAttribute('open')
        }, 200)
    }
}

function closeDropdown($event: FocusEvent) {
    setTimeout(() => {
        const parent = ($event.target as HTMLElement).parentNode
        if (parent && parent instanceof HTMLElement) {
            parent.removeAttribute('open')
        }
    }, 200)
}

async function logout() {
    await authStore.logout()
    await router.push({ name: 'login' })
}

function channelLink(path: string) {
    const channelId = configStore.channels[configStore.i]?.id

    return channelId ? { path, query: { channel: channelId } } : path
}

async function selectChannel(channelId: number) {
    await router.push({ path: route.path, query: { ...route.query, channel: channelId } })
}

function toggleTheme() {
    indexStore.darkMode = !indexStore.darkMode

    if (indexStore.darkMode) {
        localStorage.setItem('theme', 'dark')
        document.documentElement.setAttribute('data-theme', 'vizrs-dark')
    } else {
        localStorage.setItem('theme', 'light')
        document.documentElement.setAttribute('data-theme', 'vizrs-light')
    }
}
</script>
<template>
    <header class="vz-topbar">
        <RouterLink class="vz-brand flex items-center gap-2" to="/">
            <span>Viz<b>RS</b><small>Playout</small></span>
        </RouterLink>
        <span class="w-px h-5 bg-[#2e323a] hidden 2sm:block" />

        <nav class="vz-nav hidden 2sm:flex items-center gap-1 grow">
            <RouterLink
                v-for="item in menuItems"
                :key="item.label"
                :to="channelLink(item.link)"
                active-class="is-active"
            >
                {{ item.name }}
            </RouterLink>
        </nav>
        <div class="grow 2sm:hidden" />

        <div class="vz-nav flex items-center gap-1">
            <details v-if="configStore.channels.length > 1" class="dropdown dropdown-end" @focusout="closeDropdown">
                <summary class="vz-pill !h-auto">
                    <i class="bi bi-broadcast" />
                    {{ configStore.channels[configStore.i]?.name }}
                </summary>
                <ul
                    class="menu menu-sm dropdown-content mt-1 z-50 p-1 bg-base-100 border border-(--vz-line) rounded-box w-44"
                >
                    <li v-for="channel in configStore.channels" :key="channel.id">
                        <a class="text-base-content" @click="selectChannel(channel.id)">{{ channel.name }}</a>
                    </li>
                </ul>
            </details>
            <span v-else class="vz-pill hidden xs:inline">{{ configStore.channels[configStore.i]?.name }}</span>

            <span class="vz-clock hidden xs:inline" :title="configStore.timezone">{{ clock }}</span>

            <RouterLink
                class="hidden 2sm:inline-flex"
                :to="channelLink('/configure')"
                active-class="is-active"
                :title="t('button.configure')"
            >
                <i class="bi bi-gear text-[16px]" />
            </RouterLink>
            <button class="hidden 2sm:inline-flex" :title="indexStore.darkMode ? 'Light' : 'Dark'" @click="toggleTheme">
                <i :class="indexStore.darkMode ? 'bi bi-brightness-high' : 'bi bi-moon'" class="text-[16px]" />
            </button>
            <button class="hidden 2sm:inline-flex" :title="t('button.logout')" @click="logout()">
                <i class="bi bi-box-arrow-right text-[16px]" />
            </button>

            <details ref="menuDropdown" tabindex="0" class="dropdown dropdown-end 2sm:hidden">
                <summary @click="clickMenu()" @blur="blurMenu()">
                    <SvgIcon name="burger" classes="w-5 h-5" />
                </summary>
                <ul
                    class="menu menu-sm dropdown-content mt-1 z-50 p-1 bg-base-100 border border-(--vz-line) rounded-box w-52"
                >
                    <li v-for="item in menuItems" :key="item.label">
                        <RouterLink
                            :to="channelLink(item.link)"
                            class="text-base-content"
                            exact-active-class="is-active"
                            @click="closeMenu()"
                        >
                            {{ item.name }}
                        </RouterLink>
                    </li>
                    <li>
                        <RouterLink :to="channelLink('/configure')" class="text-base-content" @click="closeMenu()">
                            <i class="bi bi-gear" /> {{ t('button.configure') }}
                        </RouterLink>
                    </li>
                    <li>
                        <a class="text-base-content" @click="toggleTheme">
                            <i :class="indexStore.darkMode ? 'bi bi-brightness-high' : 'bi bi-moon'" />
                            {{ indexStore.darkMode ? 'Light' : 'Dark' }}
                        </a>
                    </li>
                    <li>
                        <a class="text-base-content" @click="logout()">
                            <i class="bi bi-box-arrow-right" /> {{ t('button.logout') }}
                        </a>
                    </li>
                </ul>
            </details>
        </div>
    </header>
</template>
