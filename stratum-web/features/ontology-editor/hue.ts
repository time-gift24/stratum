/**
 * 节点色相：把 Object Type ID 稳定散列到 0–360 色环。
 * FNV-1a 收集后接 murmur3 fmix32 终混——UUID 前缀相同、差异集中在尾部时，
 * 单靠 FNV 低位会让 mod 360 的色相簇在一起，终混保证雪崩、色相自然散开。
 * 同一节点跨渲染、跨会话颜色一致，供 .aurora 渐变经 --node-hue 消费。
 */
export function nodeHue(id: string): number {
  let hash = 0x811c9dc5
  for (let index = 0; index < id.length; index += 1) {
    hash ^= id.charCodeAt(index)
    hash = Math.imul(hash, 0x01000193)
  }
  hash ^= hash >>> 16
  hash = Math.imul(hash, 0x85ebca6b)
  hash ^= hash >>> 13
  hash = Math.imul(hash, 0xc2b2ae35)
  hash ^= hash >>> 16
  return (hash >>> 0) % 360
}
