import { Providers } from "@/components/providers";
import { SkipToContent } from "@/components/skip-to-content";
import { content } from "@/lib/content";
import type { Metadata, Viewport } from "next";
import { Archivo, Chivo_Mono, Noto_Sans_SC, Source_Serif_4 } from "next/font/google";
import type { ReactNode } from "react";
import "./globals.css";

const archivo = Archivo({
  variable: "--font-archivo",
  subsets: ["latin"],
  display: "swap",
});

const sourceSerif = Source_Serif_4({
  variable: "--font-source-serif",
  subsets: ["latin"],
  style: ["italic"],
  display: "swap",
});

const chivoMono = Chivo_Mono({
  variable: "--font-chivo-mono",
  subsets: ["latin"],
  display: "swap",
});

const notoSansSC = Noto_Sans_SC({
  variable: "--font-noto-sc",
  subsets: ["latin"],
  display: "swap",
});

export const metadata: Metadata = {
  title: "运筹 Stratum — Rust-first Agent Runtime",
  description: content.zh.hero.sub,
};

export const viewport: Viewport = {
  themeColor: "#fbf4e7",
  width: "device-width",
  initialScale: 1,
  maximumScale: 5,
};

export default function RootLayout({
  children,
}: Readonly<{
  children: ReactNode;
}>): ReactNode {
  return (
    <html lang="zh-CN">
      <body
        className={`${archivo.variable} ${sourceSerif.variable} ${chivoMono.variable} ${notoSansSC.variable} min-h-screen bg-paper font-sans text-ink antialiased`}
      >
        <Providers>
          <SkipToContent />
          {children}
        </Providers>
      </body>
    </html>
  );
}
