import type { Metadata } from "next";
import "./globals.css";

export const metadata: Metadata = {
  title: "Liquid",
  description: "SQL AI audit and BI dashboard",
};

export default function RootLayout({
  children,
}: Readonly<{
  children: React.ReactNode;
}>) {
  return (
    <html lang="en">
      <body>{children}</body>
    </html>
  );
}
