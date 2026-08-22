import { ConversationShell } from "@/features/conversation/conversation-shell";
import { content } from "@/lib/content";
import type { Metadata } from "next";
import { Suspense } from "react";
import type { ReactNode } from "react";

export const metadata: Metadata = {
  title: `对话 — ${content.zh.footer.rights}`,
  description: content.zh.hero.sub,
};

export default function ConversationPage(): ReactNode {
  return (
    <Suspense>
      <ConversationShell />
    </Suspense>
  );
}
