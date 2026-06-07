"use client";

import { useEffect, useMemo, useState } from "react";
import { useParams } from "next/navigation";
import { CircleAlert, Loader2 } from "lucide-react";

import { AppTopNav } from "@/components/app-top-nav";
import { DatapanelReadonlyPanel } from "@/components/datapanel";
import { type DatapanelPreview, apiRequest } from "@/lib/api";
import { useI18n } from "@/lib/i18n";

type PreviewState =
  | { status: "loading"; preview: null; error: null }
  | { status: "ready"; preview: DatapanelPreview; error: null }
  | { status: "error"; preview: null; error: string };

export function DatapanelPreviewPage() {
  const { t } = useI18n();
  const params = useParams<{ slug?: string | string[] }>();
  const slug = useMemo(() => {
    const value = params.slug;

    return Array.isArray(value) ? value[0] : value;
  }, [params.slug]);
  const [state, setState] = useState<PreviewState>({
    status: "loading",
    preview: null,
    error: null,
  });

  useEffect(() => {
    if (!slug) {
      setState({
        status: "error",
        preview: null,
        error: t.dashboard.previewUnavailableDescription,
      });
      return;
    }

    let cancelled = false;

    setState({ status: "loading", preview: null, error: null });
    void apiRequest<DatapanelPreview>(
      `/api/v1/datapanel-previews/${encodeURIComponent(slug)}`,
    )
      .then((preview) => {
        if (!cancelled) {
          setState({ status: "ready", preview, error: null });
        }
      })
      .catch((error) => {
        if (!cancelled) {
          setState({
            status: "error",
            preview: null,
            error:
              error instanceof Error
                ? error.message
                : t.dashboard.previewUnavailableDescription,
          });
        }
      });

    return () => {
      cancelled = true;
    };
  }, [slug, t.dashboard.previewUnavailableDescription]);

  return (
    <main className="min-h-screen bg-background text-foreground">
      <div className="flex min-h-screen flex-col">
        <AppTopNav title={t.dashboard.previewPageTitle} />
        <section className="flex min-w-0 flex-1 flex-col bg-muted/30 p-3 sm:p-4 lg:p-5">
          {state.status === "loading" ? (
            <div className="flex min-h-[calc(100vh-6.5rem)] items-center justify-center rounded-lg border bg-card text-sm text-muted-foreground shadow-sm">
              <Loader2 className="mr-2 size-4 animate-spin" aria-hidden />
              {t.dashboard.previewLoading}
            </div>
          ) : null}

          {state.status === "error" ? (
            <div className="flex min-h-[calc(100vh-6.5rem)] items-center justify-center rounded-lg border bg-card p-6 text-card-foreground shadow-sm">
              <div className="w-full max-w-sm text-center">
                <div className="mx-auto flex size-10 items-center justify-center rounded-md border bg-secondary text-secondary-foreground">
                  <CircleAlert className="size-5" aria-hidden />
                </div>
                <h2 className="mt-4 text-base font-semibold">
                  {t.dashboard.previewUnavailableTitle}
                </h2>
                <p className="mt-2 text-sm leading-6 text-muted-foreground">
                  {state.error}
                </p>
              </div>
            </div>
          ) : null}

          {state.status === "ready" ? (
            <DatapanelReadonlyPanel preview={state.preview} />
          ) : null}
        </section>
      </div>
    </main>
  );
}
