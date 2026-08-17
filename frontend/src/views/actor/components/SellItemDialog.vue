<template>
  <el-dialog
    :model-value="modelValue"
    @update:model-value="$emit('update:modelValue', $event)"
    title="售出道具"
    width="460px"
  >
    <div v-if="sellableItems.length === 0" class="empty-state">
      没有可售出的道具（只有已配置售出价格的武器和防具可售）
    </div>
    <el-radio-group v-else v-model="selected" class="radio-list">
      <el-radio v-for="it in sellableItems" :key="it.id" :value="it.id">
        {{ it.name }}
        <span class="price">→ {{ formatPrice(it.price) }} 币</span>
      </el-radio>
    </el-radio-group>
    <template #footer>
      <el-button @click="$emit('update:modelValue', false)">取消</el-button>
      <el-button
        type="primary"
        :disabled="!selected || sellableItems.length === 0"
        @click="handleConfirm"
      >
        确认售出
      </el-button>
    </template>
  </el-dialog>
</template>

<script setup lang="ts">
import { computed, ref, watch } from 'vue'
import { useGameStateStore } from '@/stores/gameState'
import type { Item } from '@/types/gameStateTypes'

const props = defineProps<{
  modelValue: boolean
  inventory: Item[]
}>()

const emit = defineEmits<{
  'update:modelValue': [boolean]
}>()

const store = useGameStateStore()
const selected = ref('')

interface SellableItem {
  id: string
  name: string
  price: number
}

const sellableItems = computed<SellableItem[]>(() => {
  const priceOf = (rarity: string | null) =>
    rarity ? store.sellPrices.find((e) => e.rarity === rarity)?.price : undefined
  return props.inventory
    .filter(
      (it) =>
        (it.item_type?.type === 'weapon' || it.item_type?.type === 'armor') &&
        priceOf(it.rarity) !== undefined
    )
    .map((it) => ({ id: it.id, name: it.name, price: priceOf(it.rarity) as number }))
})

const formatPrice = (p: number) => String(p)

watch(
  () => props.modelValue,
  (open) => {
    if (open) selected.value = ''
  }
)

const handleConfirm = () => {
  if (!selected.value) return
  store.sellItem(selected.value)
  emit('update:modelValue', false)
}
</script>

<style scoped>
.radio-list {
  display: flex;
  flex-direction: column;
  align-items: flex-start;
  gap: 8px;
}
.radio-list :deep(.el-radio) {
  margin-right: 0;
}
.price {
  margin-left: 8px;
  color: #e6a23c;
  font-size: 12px;
}
.empty-state {
  color: #909399;
  text-align: center;
  padding: 16px;
}
</style>
