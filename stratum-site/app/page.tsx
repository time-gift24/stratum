/*
 * DIRECTION CONTRACT — 流体雾场 · 纸墨（下潜叙事）
 * THESIS: 滚动即下潜——纸面（对话入口）沉入墨黑（架构真相）再浮回纸面（行动）；
 *   拒绝品类默认的"深色 hero + 浅色特性栅格"拼贴，明暗变化本身就是渐进式透明的演示。
 * OWN-WORLD: 宣纸米白 #fbf4e7 与暖墨黑 #16120e 的垂直潜程（半透明罩纱，见下方渐变 alpha）；
 *   MoltenMetal 熔墨 shader 为全页固定背景（InkField：雾边纸色 → 深褐丝缕 → 朱砂红芯），
 *   浅处墨痕透出、深处红芯如火种，滚动无割裂；
 *   中文黑体巨型标题混排英文衬线斜体单词；朱砂 #b24731 配给制 + 古铜 #be9563 辅助，禁红绿并置；有机圆角，无 hairline。
 * STORY: 访客第一眼知道"把任务交给 Agent，它真的会把事做完"；下潜后相信机制真实
 *   （审批/取消/恢复/事件溯源/OTLP），最后带着三条命令去自托管。
 * FIRST VIEWPORT: 中轴线上——mono 眉题、巨型标题、一句副文案、悬浮对话 pill（可真实输入，
 *   提交跳 /conversation）；熔墨在标题四周缓慢游动，随指针漂移；底部"下潜"提示。
 * FORM: 方向 A「流体雾场」的纸墨变体（comp-a3 + t1 排版），用户在效果图评审中选定；
 *   下潜结构、WebGL 真流体、无界面截图均为用户拍板。seed key: cd6c1128（重掷后用户改以图选定）。
 */
import { InkField } from "@/components/ink-field";
import { SiteFooter } from "@/components/site-footer";
import { SiteNav } from "@/components/site-nav";
import { Depth } from "@/features/landing/depth";
import { FinalCta } from "@/features/landing/final-cta";
import { Hero } from "@/features/landing/hero";
import { Mechanism } from "@/features/landing/mechanism";
import type { ReactNode } from "react";

export default function Page(): ReactNode {
  return (
    <div className="bg-[linear-gradient(180deg,rgb(251_244_231/0.7)_0%,rgb(251_244_231/0.7)_12%,rgb(241_232_214/0.74)_22%,rgb(168_157_136/0.84)_34%,rgb(74_66_56/0.9)_46%,rgb(22_18_14/0.93)_54%,rgb(22_18_14/0.93)_68%,rgb(111_99_85/0.9)_76%,rgb(168_157_136/0.84)_82%,rgb(241_232_214/0.74)_88%,rgb(251_244_231/0.7)_94%)]">
      <InkField />
      <SiteNav />
      <main id="main" tabIndex={-1}>
        <Hero />
        <Mechanism />
        <Depth />
        <FinalCta />
      </main>
      <SiteFooter />
    </div>
  );
}
