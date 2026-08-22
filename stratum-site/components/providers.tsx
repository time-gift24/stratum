"use client";

import { SmoothScroll } from "@/components/smooth-scroll";
import { LanguageProvider } from "@/lib/i18n";
import { ReducedMotionProvider } from "@/lib/motion";
import type { ReactNode } from "react";

export function Providers({ children }: { children: ReactNode }): ReactNode {
  return (
    <ReducedMotionProvider>
      <LanguageProvider>
        <SmoothScroll>{children}</SmoothScroll>
      </LanguageProvider>
    </ReducedMotionProvider>
  );
}
