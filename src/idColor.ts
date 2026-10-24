export function idBackground(id: string): string {
  // FNV-1a gives each ID a stable RGB color; mix with white for readable badges.
  let hash = 0x811c9dc5
  for (let i = 0; i < id.length; i++) {
    hash = Math.imul(hash ^ id.charCodeAt(i), 0x01000193) >>> 0
  }
  const channels = [hash & 255, (hash >>> 8) & 255, (hash >>> 16) & 255]
    .map(channel => Math.round(224 + channel * 31 / 255))
  return `rgb(${channels.join(', ')})`
}
