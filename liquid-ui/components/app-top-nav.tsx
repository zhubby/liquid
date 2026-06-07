"use client";

import { type ReactNode } from "react";
import Image from "next/image";
import { useTheme } from "next-themes";
import { Languages, Monitor, Moon, Sun } from "lucide-react";

import { Button } from "@/components/ui/button";
import { useI18n } from "@/lib/i18n";

type AppTopNavProps = {
  title: string;
  children?: ReactNode;
};

type ThemeShortcut = "system" | "light" | "dark";

const themeShortcutOrder: ThemeShortcut[] = ["system", "light", "dark"];

export function AppTopNav({ title, children }: AppTopNavProps) {
  const { locale, setLocale, t } = useI18n();
  const { theme, setTheme } = useTheme();
  const selectedTheme: ThemeShortcut =
    theme === "light" || theme === "dark" || theme === "system"
      ? theme
      : "system";
  const ThemeShortcutIcon =
    selectedTheme === "dark" ? Moon : selectedTheme === "light" ? Sun : Monitor;
  const nextLocale = locale === "zh-CN" ? "en-US" : "zh-CN";
  const nextLanguageLabel = nextLocale === "zh-CN" ? "中文" : "English";
  const themeShortcutLabel = t.databasePicker.themeShortcutLabel(
    t.settings.preferences.themeBadges[selectedTheme],
  );
  const languageShortcutLabel =
    t.databasePicker.languageShortcutLabel(nextLanguageLabel);

  const handleThemeShortcut = () => {
    const currentIndex = themeShortcutOrder.indexOf(selectedTheme);
    const nextTheme =
      themeShortcutOrder[(currentIndex + 1) % themeShortcutOrder.length];

    setTheme(nextTheme);
  };

  const handleLanguageShortcut = () => {
    setLocale(nextLocale);
  };

  return (
    <header className="sticky top-0 z-30 border-b bg-background">
      <nav className="flex h-16 items-center justify-between gap-3 px-3 sm:px-4 lg:px-5">
        <div className="flex min-w-0 items-center">
          <Image
            src="/banner.png"
            alt="Liquid"
            width={217}
            height={72}
            priority
            unoptimized
            draggable={false}
            className="h-10 w-auto max-w-[150px] select-none object-contain sm:h-11 sm:max-w-[210px]"
          />
          <h1 className="sr-only">{title}</h1>
        </div>

        <div className="flex min-w-0 items-center justify-end gap-2">
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9 rounded-lg"
            title={themeShortcutLabel}
            aria-label={themeShortcutLabel}
            onClick={handleThemeShortcut}
          >
            <ThemeShortcutIcon className="size-4" aria-hidden />
          </Button>
          <Button
            type="button"
            variant="outline"
            size="icon"
            className="size-9 rounded-lg"
            title={languageShortcutLabel}
            aria-label={languageShortcutLabel}
            onClick={handleLanguageShortcut}
          >
            <Languages className="size-4" aria-hidden />
          </Button>
          {children}
        </div>
      </nav>
    </header>
  );
}
