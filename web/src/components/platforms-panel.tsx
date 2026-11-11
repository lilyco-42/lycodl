import { useEffect, useState } from "react";
import type { ReactNode } from "react";
import {
  CircleAlert,
  Globe,
  LoaderCircle,
  Search,
  ShieldCheck,
  Terminal,
  Wrench,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Skeleton } from "@/components/ui/skeleton";
import { ErrorAlert } from "@/components/error-alert";
import { useCaps } from "@/hooks/useCaps";
import { useSites } from "@/hooks/useSites";
import {
  api,
  errorMessage,
  type CapTool,
  type ExtractorItem,
  type ExtractorsResponse,
  type SiteCategory,
  type SiteInfo,
  type SiteStatus,
} from "@/lib/api";
import { formatBytes } from "@/lib/format";
import { cn } from "@/lib/utils";

const CATEGORY_LABEL: Record<SiteCategory, string> = {
  video: "视频",
  music: "音乐",
  live: "直播",
  social: "社交",
  anime: "番剧",
  other: "其他",
};

const STATUS_META: Record<SiteStatus, { label: string; className: string }> = {
  ready: { label: "可直接用", className: "" },
  "needs-cookies": { label: "需 Cookie", className: "border-amber-500/30 bg-amber-500/10 text-amber-300" },
  "needs-login": { label: "需登录", className: "border-amber-500/30 bg-amber-500/10 text-amber-300" },
  experimental: { label: "不保证", className: "border-amber-500/30 bg-amber-500/10 text-amber-300" },
};

interface PlatformsPanelProps {
  /** 点「用示例链接试试」：把示例链接回填到下载页并自动解析。 */
  onUseExample: (url: string) => void;
}

/** 平台页：运行环境自检 + 主流平台 + 全部提取器。 */
export function PlatformsPanel({ onUseExample }: PlatformsPanelProps) {
  const { caps, error: capsError } = useCaps();
  const { sites, error: sitesError } = useSites();

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <ShieldCheck className="size-4 text-muted-foreground" />
            运行环境自检
          </CardTitle>
          <CardDescription>缺什么这里会直接标红，先看它再排查解析失败。</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {capsError !== null ? <ErrorAlert title="读取运行环境失败" message={capsError} /> : null}
          {caps === null ? (
            <div className="space-y-2">
              <Skeleton className="h-5 w-full" />
              <Skeleton className="h-5 w-4/5" />
              <Skeleton className="h-5 w-3/5" />
            </div>
          ) : (
            <>
              <dl className="divide-y divide-border">
                <CapRow
                  label="ffmpeg"
                  ok={caps.ffmpeg.found}
                  missingHint="缺失：视频+音频合并会失败"
                  value={<ToolValue tool={caps.ffmpeg} />}
                />
                <CapRow
                  label="JS 运行时"
                  ok={caps.jsRuntime.found}
                  missingHint="缺失：YouTube 解析会失败"
                  value={<ToolValue tool={caps.jsRuntime} />}
                />
                <CapRow
                  label="指纹伪装"
                  ok={caps.impersonate.available}
                  value={
                    caps.impersonate.available ? (
                      <span className="text-xs text-muted-foreground">
                        可用，{caps.impersonate.targets.length} 个目标
                      </span>
                    ) : (
                      <span className="text-xs text-muted-foreground">不可用</span>
                    )
                  }
                />
                <CapRow
                  label="Cookie 来源"
                  ok={caps.cookies.detected.length > 0}
                  value={
                    caps.cookies.detected.length > 0 ? (
                      <span className="flex flex-wrap items-center justify-end gap-1">
                        {caps.cookies.detected.map((item) => (
                          <Badge key={item.browser} variant="secondary" title={item.path}>
                            {item.label}
                          </Badge>
                        ))}
                      </span>
                    ) : (
                      <span className="text-xs text-muted-foreground">没检测到本机浏览器</span>
                    )
                  }
                />
                <CapRow
                  label="内核"
                  ok={caps.kernel !== null}
                  value={
                    caps.kernel !== null ? (
                      <span className="flex flex-wrap items-center justify-end gap-1.5">
                        <span className="font-mono text-xs">{caps.kernel.version}</span>
                        <Badge variant={caps.kernel.source === "managed" ? "default" : "secondary"}>
                          {caps.kernel.source === "managed" ? "托管" : "系统"}
                        </Badge>
                      </span>
                    ) : (
                      <Badge variant="destructive">未就绪</Badge>
                    )
                  }
                />
                <CapRow
                  label="extractor 覆盖"
                  ok={caps.extractors.error === null}
                  value={
                    <span className="flex flex-wrap items-center justify-end gap-1.5">
                      <span className="text-xs text-muted-foreground">
                        {caps.extractors.total} 个站点
                      </span>
                      {caps.extractors.broken > 0 ? (
                        <span className="text-xs text-amber-300">
                          其中 {caps.extractors.broken} 个上游标注不可用
                        </span>
                      ) : null}
                      {caps.extractors.error !== null ? (
                        <span className="text-xs text-destructive">{caps.extractors.error}</span>
                      ) : null}
                    </span>
                  }
                />
                <CapRow
                  label="磁盘"
                  ok={caps.disk.freeBytes !== null}
                  value={
                    <span className="text-xs text-muted-foreground">
                      可用 {formatBytes(caps.disk.freeBytes)}
                      {caps.disk.totalBytes !== null ? " / " + formatBytes(caps.disk.totalBytes) : ""}
                    </span>
                  }
                />
              </dl>

              {caps.warnings.length > 0 ? (
                <div className="space-y-1.5 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2.5">
                  {caps.warnings.map((warning) => (
                    <p
                      key={warning}
                      className="flex items-start gap-1.5 text-xs text-amber-200/90"
                    >
                      <CircleAlert className="mt-0.5 size-3.5 shrink-0" />
                      <span className="break-words">{warning}</span>
                    </p>
                  ))}
                </div>
              ) : null}
            </>
          )}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Globe className="size-4 text-muted-foreground" />
            主流平台
          </CardTitle>
          <CardDescription>
            点卡片右下角可以直接拿示例链接试一次，看看这个平台在你的机器上通不通。
            {sites !== null && sites.matched !== null ? (
              <span className="ml-1 text-foreground">当前链接匹配到 {sites.matched}。</span>
            ) : null}
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {sitesError !== null ? <ErrorAlert title="读取平台清单失败" message={sitesError} /> : null}
          {sites === null ? (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              <Skeleton className="h-32 w-full" />
              <Skeleton className="h-32 w-full" />
              <Skeleton className="h-32 w-full" />
            </div>
          ) : (
            <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3">
              {sites.curated.map((site) => (
                <SiteCard
                  key={site.key}
                  site={site}
                  strategy={sites.strategies[site.key] ?? null}
                  onUseExample={onUseExample}
                />
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      <ExtractorsCard />
    </div>
  );
}

function CapRow({
  label,
  ok,
  value,
  missingHint,
}: {
  label: string;
  ok: boolean;
  value: ReactNode;
  missingHint?: string;
}) {
  return (
    <div className="flex items-start justify-between gap-3 py-2 first:pt-0 last:pb-0">
      <dt className="flex shrink-0 items-center gap-2 text-sm text-muted-foreground">
        <span
          className={cn(
            "size-1.5 shrink-0 rounded-full",
            ok ? "bg-emerald-500" : "bg-destructive",
          )}
        />
        {label}
      </dt>
      <dd className="min-w-0 text-right">
        {value}
        {!ok && missingHint !== undefined ? (
          <span className="mt-0.5 block text-xs text-destructive">{missingHint}</span>
        ) : null}
      </dd>
    </div>
  );
}

function ToolValue({ tool }: { tool: CapTool }) {
  if (!tool.found) {
    return <Badge variant="destructive">缺失</Badge>;
  }
  return (
    <span className="flex flex-wrap items-center justify-end gap-1.5">
      {tool.name !== null && tool.name !== "" ? (
        <span className="text-xs text-foreground">{tool.name}</span>
      ) : null}
      <span className="font-mono text-xs text-muted-foreground">{tool.version ?? "版本未知"}</span>
      {tool.path !== null && tool.path !== "" ? (
        <span className="block w-full truncate font-mono text-[11px] text-muted-foreground/70">
          {tool.path}
        </span>
      ) : null}
    </span>
  );
}

function SiteCard({
  site,
  strategy,
  onUseExample,
}: {
  site: SiteInfo;
  strategy: string | null;
  onUseExample: (url: string) => void;
}) {
  const status = STATUS_META[site.status];
  const visibleDomains = site.domains.slice(0, 3);
  const extraDomains = site.domains.length - visibleDomains.length;
  const visibleTips = site.tips.slice(0, 2);

  return (
    <div className="flex flex-col gap-2 rounded-xl border border-border bg-card p-3">
      <div className="flex flex-wrap items-center gap-1.5">
        <span className="truncate text-sm font-medium text-foreground">{site.name}</span>
        <Badge variant="outline">{CATEGORY_LABEL[site.category]}</Badge>
        <Badge
          variant={site.status === "ready" ? "secondary" : "outline"}
          className={cn("ml-auto", status.className)}
        >
          {status.label}
        </Badge>
      </div>

      <p className="font-mono text-[11px] leading-relaxed break-all text-muted-foreground">
        {visibleDomains.join(" · ")}
        {extraDomains > 0 ? " · +" + extraDomains : ""}
      </p>

      {visibleTips.length > 0 ? (
        <ul className="space-y-0.5">
          {visibleTips.map((tip) => (
            <li key={tip} className="text-xs leading-relaxed text-muted-foreground">
              · {tip}
            </li>
          ))}
        </ul>
      ) : null}

      {strategy !== null && strategy !== "default" ? (
        <p className="text-[11px] text-muted-foreground/70">已记录策略：{strategy}</p>
      ) : null}

      <div className="mt-auto flex justify-end pt-0.5">
        <Button
          variant="ghost"
          size="sm"
          className="h-7 px-2 text-xs text-muted-foreground"
          onClick={() => onUseExample(site.sampleUrl)}
        >
          <Terminal className="size-3.5" />
          用示例链接试试
        </Button>
      </div>
    </div>
  );
}

function ExtractorsCard() {
  const [query, setQuery] = useState("");
  const [result, setResult] = useState<ExtractorsResponse | null>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);

  // 250ms 防抖，输入停顿后才打后端
  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    const timer = window.setTimeout(() => {
      api
        .extractors(query)
        .then((data) => {
          if (!cancelled) {
            setResult(data);
            setError(null);
          }
        })
        .catch((cause: unknown) => {
          if (!cancelled) {
            setError(errorMessage(cause));
          }
        })
        .finally(() => {
          if (!cancelled) {
            setLoading(false);
          }
        });
    }, 250);
    return () => {
      cancelled = true;
      window.clearTimeout(timer);
    };
  }, [query]);

  const items: ExtractorItem[] = result?.items ?? [];

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Wrench className="size-4 text-muted-foreground" />
          全部提取器
          {result !== null ? (
            <Badge variant="secondary" className="font-mono">
              {result.total}
            </Badge>
          ) : null}
        </CardTitle>
        <CardDescription>
          这里是 yt-dlp 认得的所有站点。标注「不保证」的是上游自己标成不可用的提取器。
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="relative">
          <Search className="pointer-events-none absolute top-1/2 left-2.5 size-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            value={query}
            onChange={(event) => setQuery(event.target.value)}
            placeholder="搜索站点，如 tiktok / bili / porn…"
            className="pl-8 text-sm"
            spellCheck={false}
            autoComplete="off"
            aria-label="搜索提取器"
          />
          {loading ? (
            <LoaderCircle className="absolute top-1/2 right-2.5 size-4 -translate-y-1/2 animate-spin text-muted-foreground" />
          ) : null}
        </div>

        {result !== null && result.truncated ? (
          <p className="text-xs text-amber-300">仅显示前 200 条，输入关键字继续缩小范围。</p>
        ) : null}

        {error !== null ? <ErrorAlert title="读取提取器列表失败" message={error} /> : null}

        {items.length === 0 && !loading && error === null ? (
          <p className="py-6 text-center text-sm text-muted-foreground">没有匹配的站点</p>
        ) : (
          <div className="flex flex-wrap gap-1.5">
            {items.map((item) => (
              <Badge
                key={item.name}
                variant="secondary"
                className={cn(
                  "font-mono",
                  item.broken && "border-amber-500/30 bg-amber-500/10 text-amber-300",
                )}
                title={item.broken ? "上游标注该提取器不可用" : undefined}
              >
                {item.broken ? item.name + " · 不保证" : item.name}
              </Badge>
            ))}
          </div>
        )}
      </CardContent>
    </Card>
  );
}