"use client";

/**
 * 全页熔墨背景：固定视口层，贯穿整页消除段落割裂。
 * 纸墨配色 + 朱砂热芯：雾边（纸色）→ 深褐丝缕 → 朱砂红芯；
 * 页面上方盖半透明下潜渐变（page.tsx），浅处墨痕透出、深处红芯如火种。
 * reduced-motion 时不渲染，由渐变罩纱与 paper 底色保底。
 */
import MoltenMetal from "@/components/react-bits/molten-metal";
import { useReducedMotion } from "@/lib/motion";
import type { ReactNode } from "react";

export function InkField(): ReactNode {
  const prefersReducedMotion = useReducedMotion();
  if (prefersReducedMotion) return null;

  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 -z-10">
      <MoltenMetal
        color1="#e2d9c5"
        color2="#4a4238"
        color3="#b24731"
        speed={0.25}
        scale={2.6}
        detail={3}
        glow={2.2}
        coreSize={0.07}
        swirl={1}
        fold={-0.2}
        blackPoint={0.03}
        brightness={1.35}
        grain
        grainIntensity={0.04}
        mouseInteraction
        mouseStrength={0.3}
        opacity={0.9}
        className="absolute inset-0"
      />
    </div>
  );
}
