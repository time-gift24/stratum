import { Geist, Geist_Mono, Roboto_Slab } from "next/font/google"

import "./globals.css"
import { ThemeProvider } from "@/components/theme-provider"
import { cn } from "@/lib/utils";

const robotoSlabHeading = Roboto_Slab({subsets:['latin'],variable:'--font-heading'});

const fontSans = Geist({
  subsets: ["latin"],
  variable: "--font-sans",
})

const geistMono = Geist_Mono({subsets:['latin'],variable:'--font-mono'})

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode
}>) {
  return (
    <html
      lang="en"
      suppressHydrationWarning
      className={cn("antialiased", fontSans.variable, "font-mono", geistMono.variable, robotoSlabHeading.variable)}
    >
      <body>
        <ThemeProvider>{children}</ThemeProvider>
      </body>
    </html>
  )
}
