/** B 站图片接口可能返回 HTTP 或省略协议；统一使用 HTTPS 兼容安装版 CSP。 */
export function imageUrl(value: string | null | undefined): string {
  const url = value?.trim() ?? ''
  if (url.startsWith('//')) return `https:${url}`
  return url.replace(/^http:\/\//i, 'https://')
}
