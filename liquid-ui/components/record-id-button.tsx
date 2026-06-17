"use client";

import { ChevronRight } from "lucide-react";

import { cn } from "@/lib/utils";

type RecordIdButtonProps = {
  id: string;
  label: string;
  onOpen: () => void;
  className?: string;
};

export function RecordIdButton({
  id,
  label,
  onOpen,
  className,
}: RecordIdButtonProps) {
  return (
    <button
      type="button"
      className={cn(
        "group -mx-1 inline-flex max-w-36 items-center gap-1 rounded-md px-1 py-0.5 font-mono text-xs font-semibold text-foreground underline-offset-4 outline-none transition-colors hover:bg-accent hover:text-primary hover:underline focus-visible:bg-accent focus-visible:text-primary focus-visible:ring-[3px] focus-visible:ring-ring/50",
        className,
      )}
      title={id}
      onClick={onOpen}
    >
      <span className="min-w-0 truncate">{label}</span>
      <ChevronRight
        className="size-3.5 shrink-0 text-muted-foreground/70 transition-all group-hover:translate-x-0.5 group-hover:text-primary group-focus-visible:translate-x-0.5 group-focus-visible:text-primary"
        aria-hidden
      />
    </button>
  );
}
