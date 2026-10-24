<script setup lang="ts">
import { computed, onMounted, ref, watch } from 'vue'
import { storeToRefs } from 'pinia'
import { useI18n } from 'vue-i18n'
import { useHead } from '@unhead/vue'

import { authFetch } from '@/composables/authFetch'
import { useAuth } from '@/stores/auth'
import { useConfig } from '@/stores/config'
import { useIndex } from '@/stores/index'

type Programme = {
    start: Date
    stop: Date
    title: string
}

const { locale, t } = useI18n()
const authStore = useAuth()
const configStore = useConfig()
const indexStore = useIndex()
const { i } = storeToRefs(configStore)

useHead({
    title: computed(() => t('button.epg')),
})

const days = ref(3)
const xml = ref('')
const loading = ref(false)

// XMLTV time: "20260719120000 +0200"
function parseXmltvTime(value: string): Date {
    const m = /^(\d{4})(\d{2})(\d{2})(\d{2})(\d{2})(\d{2})\s*([+-]\d{2})(\d{2})$/.exec(value.trim())

    if (!m) {
        return new Date(NaN)
    }

    return new Date(`${m[1]}-${m[2]}-${m[3]}T${m[4]}:${m[5]}:${m[6]}${m[7]}:${m[8]}`)
}

const programmes = computed<Programme[]>(() => {
    if (!xml.value) {
        return []
    }

    const doc = new DOMParser().parseFromString(xml.value, 'application/xml')

    return Array.from(doc.querySelectorAll('programme')).map((node) => ({
        start: parseXmltvTime(node.getAttribute('start') ?? ''),
        stop: parseXmltvTime(node.getAttribute('stop') ?? ''),
        title: node.querySelector('title')?.textContent ?? '',
    }))
})

const dayFormat = computed(
    () =>
        new Intl.DateTimeFormat(locale.value, {
            weekday: 'long',
            day: 'numeric',
            month: 'long',
            timeZone: configStore.timezone,
        }),
)

const timeFormat = computed(
    () =>
        new Intl.DateTimeFormat(locale.value, {
            hour: '2-digit',
            minute: '2-digit',
            second: '2-digit',
            hourCycle: 'h23',
            timeZone: configStore.timezone,
        }),
)

const groups = computed(() => {
    const result: { day: string; items: Programme[] }[] = []

    for (const item of programmes.value) {
        const day = dayFormat.value.format(item.start)
        const last = result[result.length - 1]

        if (last?.day === day) {
            last.items.push(item)
        } else {
            result.push({ day, items: [item] })
        }
    }

    return result
})

function duration(item: Programme): string {
    const sec = Math.max(0, Math.round((item.stop.getTime() - item.start.getTime()) / 1000))
    const h = Math.floor(sec / 3600)
    const m = Math.floor((sec % 3600) / 60)
    const s = sec % 60

    return `${String(h).padStart(2, '0')}:${String(m).padStart(2, '0')}:${String(s).padStart(2, '0')}`
}

function isNow(item: Programme): boolean {
    const now = Date.now()

    return item.start.getTime() <= now && now < item.stop.getTime()
}

async function getEpg() {
    const id = configStore.channels[configStore.i]?.id

    if (!id) {
        return
    }

    loading.value = true

    await authFetch<string>(`/api/epg/${id}?days=${days.value}`, {
        method: 'GET',
        headers: authStore.authHeader,
        responseType: 'text',
    })
        .then((data) => {
            xml.value = data
        })
        .catch((e: Error) => {
            xml.value = ''
            indexStore.msgAlert('error', e.message, 4)
        })
        .finally(() => {
            loading.value = false
        })
}

function downloadXmltv() {
    const blob = new Blob([xml.value], { type: 'application/xml' })
    const link = document.createElement('a')

    link.href = URL.createObjectURL(blob)
    link.download = `epg-${configStore.channels[configStore.i]?.id ?? 1}.xml`
    link.click()
    URL.revokeObjectURL(link.href)
}

onMounted(getEpg)
watch([i, days], getEpg)
</script>

<template>
    <div class="h-full p-2 flex flex-col gap-2">
        <section class="vz-panel h-full">
            <header>
                <h3>{{ t('epg.title') }}</h3>
                <label class="vz-label flex items-center gap-2">
                    {{ t('epg.days') }}
                    <select v-model.number="days" class="select select-xs w-16">
                        <option v-for="d in [1, 2, 3, 7, 14]" :key="d" :value="d">{{ d }}</option>
                    </select>
                </label>
                <button class="btn btn-xs btn-primary" :disabled="loading" @click="getEpg">
                    <i class="bi bi-arrow-repeat" /> {{ t('epg.refresh') }}
                </button>
                <button class="btn btn-xs btn-accent" :disabled="!xml" @click="downloadXmltv">
                    <i class="bi bi-download" /> {{ t('epg.download') }}
                </button>
            </header>

            <div class="overflow-auto grow min-h-0">
                <p v-if="!loading && programmes.length === 0" class="p-4 vz-dim">{{ t('epg.empty') }}</p>

                <template v-for="group in groups" :key="group.day">
                    <div class="sticky top-0 z-1 px-3 py-1 vz-label bg-base-200 border-b border-(--vz-line)">
                        {{ group.day }}
                    </div>
                    <table class="table table-xs vz-table">
                        <thead>
                            <tr>
                                <th class="w-24">{{ t('epg.start') }}</th>
                                <th class="w-24">{{ t('epg.stop') }}</th>
                                <th class="w-24">{{ t('epg.duration') }}</th>
                                <th>{{ t('epg.programme') }}</th>
                            </tr>
                        </thead>
                        <tbody>
                            <tr
                                v-for="(item, index) in group.items"
                                :key="index"
                                :class="isNow(item) ? 'bg-(--vz-sel)' : ''"
                            >
                                <td class="font-mono">{{ timeFormat.format(item.start) }}</td>
                                <td class="font-mono vz-dim">{{ timeFormat.format(item.stop) }}</td>
                                <td class="font-mono vz-dim">{{ duration(item) }}</td>
                                <td>
                                    <span v-if="isNow(item)" class="vz-pill on me-2">ON AIR</span>
                                    {{ item.title }}
                                </td>
                            </tr>
                        </tbody>
                    </table>
                </template>
            </div>

            <footer class="px-3 py-1 text-xs vz-dim border-t border-(--vz-line)">{{ t('epg.hint') }}</footer>
        </section>
    </div>
</template>
