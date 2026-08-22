import { Gallery } from "@/features/gallery/gallery";
import type { Metadata } from "next";
import type { ReactNode } from "react";

export const metadata: Metadata = {
  title: "组件与样式 — 运筹 Stratum",
  description: "stratum-site 组件库实拍：色板、字体与全部公共组件。",
};

export default function ComponentsPage(): ReactNode {
  return <Gallery />;
}
