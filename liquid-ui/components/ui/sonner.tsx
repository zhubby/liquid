"use client";

import {
  CircleAlert,
  CircleCheck,
  Info,
  Loader2,
  TriangleAlert,
} from "lucide-react";
import { useTheme } from "next-themes";
import { Toaster as Sonner, type ToasterProps } from "sonner";

import { cn } from "@/lib/utils";

const toastIcons: ToasterProps["icons"] = {
  success: <CircleCheck className="size-4 text-emerald-600 dark:text-emerald-400" />,
  info: <Info className="size-4 text-chart-2" />,
  warning: <TriangleAlert className="size-4 text-chart-5" />,
  error: <CircleAlert className="size-4 text-destructive" />,
  loading: (
    <Loader2
      className="size-4 animate-spin text-muted-foreground"
      aria-hidden
    />
  ),
};

function Toaster({
  className,
  icons,
  toastOptions,
  duration = 3000,
  ...props
}: ToasterProps) {
  const { theme = "system" } = useTheme();

  return (
    <Sonner
      {...props}
      theme={theme as ToasterProps["theme"]}
      richColors={false}
      className={cn("toaster group", className)}
      duration={duration}
      icons={{
        ...toastIcons,
        ...icons,
      }}
      toastOptions={{
        ...toastOptions,
        classNames: {
          toast:
            "group toast !rounded-md !border !border-black/10 !bg-white !px-4 !py-3 !text-sm !font-medium !text-black !shadow-lg dark:!border-white/15 dark:!bg-black dark:!text-white",
          title: "!text-sm !font-medium !leading-5 !text-inherit",
          description: "!text-black/65 dark:!text-white/70",
          icon: "mt-0.5",
          content: "gap-1",
          closeButton:
            "!border-black/10 !bg-white !text-black hover:!bg-black/5 dark:!border-white/15 dark:!bg-black dark:!text-white dark:hover:!bg-white/10",
          actionButton:
            "!bg-black !text-white hover:!bg-black/85 dark:!bg-white dark:!text-black dark:hover:!bg-white/85",
          cancelButton:
            "!bg-black/5 !text-black hover:!bg-black/10 dark:!bg-white/10 dark:!text-white dark:hover:!bg-white/15",
          ...toastOptions?.classNames,
        },
      }}
    />
  );
}

export { Toaster };
