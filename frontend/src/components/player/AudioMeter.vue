<script setup lang="ts">
// The level is read through a getter, so frequent meter updates re-render only this component.
const props = defineProps<{
    level: () => AudioLevel | null
    smooth: boolean
}>()

function meterValue() {
    const peak = props.level()?.peak_db ?? -100

    return Math.min(100, Math.max(0, ((peak + 60) / 60) * 100))
}
</script>
<template>
    <div
        class="relative h-full min-h-20 w-4 rounded-sm overflow-hidden bg-linear-to-t from-success from-70% via-warning via-80% to-error to-100%"
    >
        <div
            class="absolute top-0 left-0 w-full bg-base-300"
            :class="smooth ? 'transition-[height] duration-200' : ''"
            :style="{ height: `${100 - meterValue()}%` }"
        />
    </div>
</template>
