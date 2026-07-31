import type { Metadata } from "next";
import { Geist, Geist_Mono } from "next/font/google";
import { HitRegionProvider } from "@/lib/hit-regions";
import { OverlayLifecycle } from "@/lib/overlay-lifecycle";
import "./globals.css";

const geistSans = Geist({
  variable: "--font-geist-sans",
  subsets: ["latin"],
});

const geistMono = Geist_Mono({
  variable: "--font-geist-mono",
  subsets: ["latin"],
});

export const metadata: Metadata = {
  title: "Tauri Overlay",
  description: "Always-on-top, click-through-capable desktop overlay (Tauri v2 + Next.js)",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html
      lang="en"
      className={`${geistSans.variable} ${geistMono.variable} h-full antialiased`}
    >
      <body className="h-full">
        <HitRegionProvider>
          <OverlayLifecycle />
          {children}
        </HitRegionProvider>
      </body>
    </html>
  );
}
