// UUIDv7（RFC 9562）：48-bit Unix 毫秒时间戳 + version 7 + variant 10 + 74-bit 随机。
// 客户端在首次保存前生成 Object Type / Property / Link Type 的 ID。

const UUID_LENGTH = 16

export function createUuidV7(now: number = Date.now()): string {
  const bytes = new Uint8Array(UUID_LENGTH)
  crypto.getRandomValues(bytes)

  // 48-bit 毫秒时间戳（当前时间戳远小于 2^53，移位无损）
  const high = Math.floor(now / 0x100000000)
  const low = now >>> 0
  bytes[0] = (high >> 8) & 0xff
  bytes[1] = high & 0xff
  bytes[2] = (low >>> 24) & 0xff
  bytes[3] = (low >>> 16) & 0xff
  bytes[4] = (low >>> 8) & 0xff
  bytes[5] = low & 0xff

  bytes[6] = (bytes[6] & 0x0f) | 0x70 // version 7
  bytes[8] = (bytes[8] & 0x3f) | 0x80 // variant 10

  const hex = Array.from(bytes, (byte) => byte.toString(16).padStart(2, "0"))
  return `${hex.slice(0, 4).join("")}-${hex.slice(4, 6).join("")}-${hex
    .slice(6, 8)
    .join("")}-${hex.slice(8, 10).join("")}-${hex.slice(10, 16).join("")}`
}
