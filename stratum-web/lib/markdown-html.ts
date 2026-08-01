import { unified } from "unified"
import remarkGfm from "remark-gfm"
import remarkParse from "remark-parse"
import remarkRehype from "remark-rehype"
import rehypeStringify from "rehype-stringify"

/**
 * renderMarkdownToHtml —— markdown → HTML 字符串。
 * 与 Streamdown 同源的 remark 管线（parse + GFM + rehype），
 * 供「渲染后对比」这类需要 HTML 字符串而非 React 树的场景使用。
 */
export function renderMarkdownToHtml(markdown: string): string {
  return String(
    unified()
      .use(remarkParse)
      .use(remarkGfm)
      .use(remarkRehype)
      .use(rehypeStringify)
      .processSync(markdown)
  )
}
