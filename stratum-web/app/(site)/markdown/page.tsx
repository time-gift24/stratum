import { MarkdownDockNav } from "@/components/chrome/site-chrome"
import { MarkdownArticle } from "@/components/stratum/markdown/markdown-article"
import { MarkdownDiff } from "@/components/stratum/markdown/markdown-diff"
import { MarkdownStream } from "@/components/stratum/markdown/markdown-stream"
import {
  ShowcaseDemo,
  ShowcaseSection,
} from "@/components/stratum/showcase/showcase-section"
import { ScrollReveal } from "@/components/stratum/showcase/scroll-reveal"

/**
 * DIRECTION CONTRACT —— /markdown 展示页
 * THESIS: 同一份 markdown 的三种生命形态——成稿阅读、流式生成、版本对比；
 *         拒绝把 markdown 展示做成截图或干巴巴的代码块。
 * OWN-WORLD: 跟随站点主题的 Medium 阅读排版（Charter 衬线正文 / Geist 无衬线标题 /
 *            居中三点分节符）；diff 语义色沿用画布绿（primary）与 destructive 红。
 * STORY: 访客依次读懂三个组件各自解决的问题：排版、流式、对比。
 * FIRST VIEWPORT: 悬浮双 nav 之下，页标题 + MarkdownArticle 的衬线窄栏文章直接入眼。
 * FORM: 首页同款 ShowcaseSection 序列——既有展示面的延伸，非新世界。
 */

const ARTICLE = `# 流式时代的 Markdown 渲染

Markdown 的假设一直很朴素：**拿到全文，再排版**。这个假设在 AI 时代失效了——大模型把文章切成一个个 token 吐出来，渲染器看到的永远是"没写完"的文本。

## 未闭合的语法

流式输出的每一帧都可能停在尴尬的位置：加粗只有开头的 \`**\`，链接的右括号还在路上，代码块的围栏缺了收尾。传统渲染器遇到这些会原样输出，或者更糟——把后续内容全部吞进一个错误的节点里。

[Streamdown](https://github.com/vercel/streamdown) 的思路是"边收边修"：底层用 remend 自动补全未闭合的语法，让中间帧也能正确排版。接入只需要一个 prop：

\`\`\`tsx
<Streamdown mode="streaming" caret="block">
  {partialMarkdown}
</Streamdown>
\`\`\`

> 普通渲染器是拿到全文才排版，流式渲染器是边接收边排版，排错了自动修正。

## 排版是另一件事

渲染正确只是底线，**读起来舒服**才是目标。Medium 把这件事做成了范式：

- 窄栏居中，正文约 680px，视线不用长途跋涉
- 衬线大字号正文，行高 1.58 起步
- 标题回到无衬线，和正文形成材质对比
- 分节不用横线，三个居中的点就够了

这套规则与渲染器完全正交——Streamdown 负责"边流边渲染"，样式表负责"渲染出来长什么样"。

---

## 对比是第三种形态

文章写完还会改。词级 diff 把两版源文对齐：新增染绿、删除划红线，改动一目了然；并排渲染则回答另一个问题——**看起来**差多少。

diff 库的 \`diffWordsWithSpace\` 足够应付大多数场景，按块切分还能进一步降噪。`

const STREAM_SOURCE = `## 流式渲染演示

这段文字正在**逐字到达**。如果渲染器等到全文就绪才排版，你会先看到一片空白，然后是整屏内容的跳变。

流式渲染把等待变成阅读：第一个 token 到达时排版就开始了，未闭合的 \`语法\` 会被自动补全。

- 边生成，边排版
- 中间帧同样正确
- 没有最后的"整段跳变"

> 等待被消灭之后，生成过程本身就是内容。`

const DIFF_BEFORE = `## MarkdownArticle

Medium 风格的文章渲染组件，基于 react-markdown 实现。

- 衬线正文，窄栏居中
- 标题使用无衬线字体
- 支持 GFM 语法

适合博客与文档场景。`

const DIFF_AFTER = `## MarkdownArticle

Medium 风格的文章渲染组件，基于 Streamdown 实现，天然支持流式输出。

- 衬线正文，窄栏居中
- 标题使用无衬线字体，与正文形成材质对比
- 支持 GFM 语法与代码高亮
- 颜色只消费外层 token，随主题切换

适合博客、文档与 AI 对话场景。`

export default function MarkdownPage() {
  return (
    <div className="min-h-svh pt-28 font-sans sm:pt-32">
      <MarkdownDockNav />

      <div className="mx-auto max-w-4xl px-6 pt-2 pb-10 md:pl-24 xl:pl-6">
        <main className="flex min-w-0 flex-col gap-12">
          <ScrollReveal>
            <header className="flex flex-col gap-3">
              <h1 className="font-heading text-3xl tracking-tight">Markdown</h1>
              <p className="max-w-prose text-sm leading-relaxed text-muted-foreground">
                同一份 markdown 的三种形态：Medium 风格的成稿排版、AI
                流式渲染、原文版本对比。渲染层统一用
                Streamdown，排版只消费全局 token，随主题切换。
              </p>
            </header>
          </ScrollReveal>

          <ShowcaseSection
            id="markdown-article"
            title="MarkdownArticle"
            description="Medium 风格的文章排版：衬线正文 + 无衬线标题 + 居中三点分节符。排版规则集中在组件自带的 prose-medium CSS Module，颜色只消费 token、随主题切换；渲染层是 Streamdown（mode=static）。"
          >
            <ShowcaseDemo className="block px-6 py-10 sm:px-10">
              <MarkdownArticle className="mx-auto max-w-2xl">
                {ARTICLE}
              </MarkdownArticle>
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            id="markdown-stream"
            title="MarkdownStream"
            description="AI 流式输出的 Markdown 渲染：模拟 token 逐块到达，Streamdown 以 mode=streaming 边收边排，未闭合语法由 remend 自动补全，生成中显示块状 caret，完成后可重新播放。"
          >
            <ShowcaseDemo className="block p-0">
              <MarkdownStream
                source={STREAM_SOURCE}
                className="mx-auto max-w-2xl"
              />
            </ShowcaseDemo>
          </ShowcaseSection>

          <ShowcaseSection
            id="markdown-diff"
            title="MarkdownDiff"
            description="Markdown 版本对比：原文 diff（diff 库词级对比）、内联渲染（两版渲染成 HTML 后 htmldiff 合并，单文档内联看增删，GitHub rich diff 形态）、并排渲染三种视图；新增染 primary 绿、删除染 destructive 红。"
          >
            <ShowcaseDemo className="block p-0">
              <MarkdownDiff before={DIFF_BEFORE} after={DIFF_AFTER} />
            </ShowcaseDemo>
          </ShowcaseSection>
        </main>
      </div>
    </div>
  )
}
