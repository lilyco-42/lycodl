import { useEffect, useMemo, useState } from "react";
import {
  AudioLines,
  Check,
  ChevronDown,
  Clock,
  Film,
  Info,
  Link2,
  ListVideo,
  LoaderCircle,
  Search,
  Sparkles,
  Video,
} from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import {
  Select,
  SelectContent,
  SelectItem,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Separator } from "@/components/ui/separator";
import { ErrorAlert } from "@/components/error-alert";
import {
  api,
  errorMessage,
  type DownloadMode,
  type DownloadQuality,
  type FormatInfo,
  type ProbeStrategy,
  type SiteInfo,
  type SiteStatus,
  type VideoInfo,
} from "@/lib/api";
import { formatBytes, formatDuration } from "@/lib/format";
import { cn } from "@/lib/utils";

const MODE_OPTIONS: Array<{ value: DownloadMode; label: string; icon: typeof Video }> = [
  { value: "video", label: "视频", icon: Video },
  { value: "audio", label: "音频", icon: AudioLines },
  { value: "custom", label: "自定义格式", icon: Film },
];

const QUALITY_OPTIONS: Array<{ value: DownloadQuality; label: string }> = [
  { value: "best", label: "最佳画质" },
  { value: "2160", label: "2160p (4K)" },
  { value: "1440", label: "1440p (2K)" },
  { value: "1080", label: "1080p" },
  { value: "720", label: "720p" },
  { value: "480", label: "480p" },
];

function codecLabel(value: string | null): string {
  if (value === null || value === "none") {
    return "无";
  }
  return value.split(".")[0] ?? value;
}

function formatLabel(format: FormatInfo): string {
  const parts = [format.formatId, format.ext];
  if (format.resolution) {
    parts.push(format.resolution);
  } else if (format.height !== null) {
    parts.push(format.height + "p");
  }
  parts.push("v:" + codecLabel(format.vcodec));
  if (format.note) {
    parts.push(format.note);
  }
  return parts.join(" · ");
}

function guessBestFormatId(formats: FormatInfo[]): string | null {
  const withVideoAndAudio = formats.filter(
    (format) => format.vcodec !== null && format.vcodec !== "none" && format.acodec !== null && format.acodec !== "none",
  );
  const pool = withVideoAndAudio.length > 0 ? withVideoAndAudio : formats;
  let best: FormatInfo | null = null;
  for (const format of pool) {
    if (best === null) {
      best = format;
      continue;
    }
    const bestHeight = best.height ?? 0;
    const height = format.height ?? 0;
    if (height > bestHeight || (height === bestHeight && (format.tbr ?? 0) > (best.tbr ?? 0))) {
      best = format;
    }
  }
  return best ? best.formatId : null;
}

/** 最近一次已自动解析过的示例链接（跨组件卸载存活）。 */
let lastHandledExample: string | null = null;

interface DownloadPanelProps {
  onJobCreated: (jobId: string) => void;
  /** 从「平台」页点示例链接回填过来的 URL，变化即自动解析。 */
  exampleUrl?: string;
}

/** 解析 URL + 选择下载方式。 */
export function DownloadPanel({ onJobCreated, exampleUrl }: DownloadPanelProps) {
  const [url, setUrl] = useState("");
  const [probing, setProbing] = useState(false);
  const [probeError, setProbeError] = useState<string | null>(null);
  const [info, setInfo] = useState<VideoInfo | null>(null);
  const [site, setSite] = useState<SiteInfo | null>(null);
  const [strategy, setStrategy] = useState<ProbeStrategy | null>(null);

  const [mode, setMode] = useState<DownloadMode>("video");
  const [quality, setQuality] = useState<DownloadQuality>("best");
  const [formatId, setFormatId] = useState<string | null>(null);
  const [playlist, setPlaylist] = useState(false);
  const [submitting, setSubmitting] = useState(false);
  const [submitError, setSubmitError] = useState<string | null>(null);

  const formats = useMemo(() => info?.formats ?? [], [info]);
  const selectedFormat = formats.find((format) => format.formatId === formatId) ?? null;

  async function handleProbe(target?: string) {
    const trimmed = (target ?? url).trim();
    if (trimmed === "") {
      toast.error("请先粘贴视频链接");
      return;
    }
    setProbing(true);
    setProbeError(null);
    setSubmitError(null);
    try {
      const result = await api.probe(trimmed);
      setInfo(result.info);
      setSite(result.site);
      setStrategy(result.strategy);
      setMode("video");
      setQuality("best");
      setPlaylist(result.info.isPlaylist);
      setFormatId(guessBestFormatId(result.info.formats));
      toast.success("解析成功", { description: result.info.title });
    } catch (cause) {
      setInfo(null);
      setSite(null);
      setStrategy(null);
      setProbeError(errorMessage(cause));
    } finally {
      setProbing(false);
    }
  }

  // 外部（平台页）塞进来的示例链接：填进输入框并自动解析。
  // 用模块级变量而不是 ref 去重：切页签会让本组件卸载重建，ref 会丢，导致同一示例被重复解析。
  useEffect(() => {
    if (exampleUrl === undefined || exampleUrl.trim() === "") {
      // 置空表示「本轮示例已消费完毕」，允许再次点击同一个示例链接
      lastHandledExample = null;
      return;
    }
    if (lastHandledExample === exampleUrl) {
      return;
    }
    lastHandledExample = exampleUrl;
    setUrl(exampleUrl);
    void handleProbe(exampleUrl);
    // handleProbe 每次渲染都是新的函数，故意不进依赖，去重靠 lastHandledExample
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [exampleUrl]);

  async function handleSubmit() {
    if (info === null) {
      toast.error("请先解析链接");
      return;
    }
    if (mode === "custom" && (formatId === null || formatId === "")) {
      toast.error("请选择一个自定义格式");
      return;
    }
    setSubmitting(true);
    setSubmitError(null);
    try {
      const result = await api.createDownload({
        url: info.webpageUrl !== "" ? info.webpageUrl : url.trim(),
        mode,
        quality,
        formatId: mode === "custom" ? formatId : null,
        playlist,
      });
      toast.success("已加入下载队列", {
        description: "任务 ID：" + result.jobId,
      });
      onJobCreated(result.jobId);
    } catch (cause) {
      const message = errorMessage(cause);
      setSubmitError(message);
      toast.error("创建下载任务失败", { description: message });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Link2 className="size-4 text-muted-foreground" />
            粘贴视频链接
          </CardTitle>
          <CardDescription>支持 yt-dlp 能解析的站点，粘贴后按回车即可解析。</CardDescription>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="flex flex-col gap-2 sm:flex-row">
            <Input
              value={url}
              onChange={(event) => setUrl(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  void handleProbe();
                }
              }}
              placeholder="https://www.youtube.com/watch?v=..."
              className="h-11 flex-1 text-sm"
              spellCheck={false}
              autoComplete="off"
              aria-label="视频链接"
            />
            <Button
              onClick={() => void handleProbe()}
              disabled={probing}
              size="lg"
              className="h-11 shrink-0 px-4"
            >
              {probing ? (
                <LoaderCircle className="animate-spin" />
              ) : (
                <Search />
              )}
              {probing ? "解析中…" : "解析"}
            </Button>
          </div>

          {probing ? <ProbeSkeleton /> : null}

          {probeError !== null ? (
            <ErrorAlert title="解析失败" message={probeError} />
          ) : null}

          {!probing && probeError === null && info !== null ? (
            <VideoPreview info={info} site={site} strategy={strategy} />
          ) : null}
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Sparkles className="size-4 text-muted-foreground" />
            下载选项
          </CardTitle>
          <CardDescription>解析成功后才可提交任务。</CardDescription>
        </CardHeader>
        <CardContent className="space-y-5">
          <div className="space-y-2">
            <Label>模式</Label>
            <div className="grid grid-cols-1 gap-2 sm:grid-cols-3">
              {MODE_OPTIONS.map((option) => {
                const Icon = option.icon;
                const active = mode === option.value;
                const disabled = option.value === "custom" && formats.length === 0;
                return (
                  <button
                    key={option.value}
                    type="button"
                    disabled={disabled || info === null}
                    onClick={() => setMode(option.value)}
                    className={cn(
                      "flex items-center gap-2 rounded-lg border px-3 py-2.5 text-sm transition-colors",
                      "disabled:cursor-not-allowed disabled:opacity-50",
                      active && !disabled
                        ? "border-foreground/30 bg-muted text-foreground"
                        : "border-border bg-background text-muted-foreground hover:bg-muted/60 hover:text-foreground",
                    )}
                    aria-pressed={active}
                  >
                    <Icon className="size-4" />
                    <span className="font-medium">{option.label}</span>
                    {active && !disabled ? <Check className="ml-auto size-4" /> : null}
                  </button>
                );
              })}
            </div>
          </div>

          <Separator />

          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="quality-select">画质</Label>
              <Select
                value={quality}
                onValueChange={(value) => setQuality(value as DownloadQuality)}
                disabled={mode !== "video" || info === null}
              >
                <SelectTrigger id="quality-select" className="w-full">
                  <SelectValue placeholder="选择画质" />
                </SelectTrigger>
                <SelectContent>
                  {QUALITY_OPTIONS.map((option) => (
                    <SelectItem key={option.value} value={option.value}>
                      {option.label}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {mode === "video" ? "仅视频模式可选" : "音频 / 自定义格式模式忽略画质"}
              </p>
            </div>

            <div className="space-y-2">
              <Label htmlFor="format-select">自定义格式</Label>
              <Select
                value={formatId ?? formats[0]?.formatId ?? undefined}
                onValueChange={(value) => setFormatId(value)}
                disabled={mode !== "custom" || formats.length === 0}
              >
                <SelectTrigger id="format-select" className="w-full">
                  <SelectValue placeholder={formats.length === 0 ? "无可用格式" : "选择格式"} />
                </SelectTrigger>
                <SelectContent className="max-w-md">
                  {formats.map((format) => (
                    <SelectItem key={format.formatId} value={format.formatId}>
                      {formatLabel(format)}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
              <p className="text-xs text-muted-foreground">
                {selectedFormat !== null && mode === "custom"
                  ? formatBytes(selectedFormat.filesize) + " · 仅自定义格式模式生效"
                  : "仅自定义格式模式可选"}
              </p>
            </div>
          </div>

          <Separator />

          <div className="flex items-start justify-between gap-4 rounded-lg border border-border bg-muted/30 px-3 py-2.5">
            <div className="space-y-0.5">
              <Label htmlFor="playlist-switch" className="cursor-pointer">
                下载整个播放列表
              </Label>
              <p className="text-xs text-muted-foreground">
                {info !== null && info.isPlaylist
                  ? "该链接是播放列表，共 " + info.entryCount + " 个条目"
                  : "关闭时只下载当前单个视频"}
              </p>
            </div>
            <Switch
              id="playlist-switch"
              checked={playlist}
              onCheckedChange={setPlaylist}
              disabled={info === null}
            />
          </div>

          {submitError !== null ? (
            <ErrorAlert title="提交失败" message={submitError} />
          ) : null}

          <Button
            className="w-full"
            size="lg"
            onClick={() => void handleSubmit()}
            disabled={info === null || submitting}
          >
            {submitting ? <LoaderCircle className="animate-spin" /> : <Film />}
            {submitting ? "提交中…" : "开始下载"}
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

function ProbeSkeleton() {
  return (
    <div className="space-y-3 rounded-xl border border-border p-4">
      <div className="flex flex-col gap-4 sm:flex-row">
        <Skeleton className="aspect-video w-full rounded-lg sm:w-56" />
        <div className="flex-1 space-y-2">
          <Skeleton className="h-5 w-3/4" />
          <Skeleton className="h-4 w-1/3" />
          <Skeleton className="h-4 w-1/2" />
          <Skeleton className="h-4 w-2/5" />
        </div>
      </div>
    </div>
  );
}

const SITE_STATUS_ALERT_TITLE: Partial<Record<SiteStatus, string>> = {
  "needs-cookies": "该平台需要 Cookie",
  "needs-login": "该平台需要登录",
  experimental: "该平台支持不保证",
};

function VideoPreview({
  info,
  site,
  strategy,
}: {
  info: VideoInfo;
  site: SiteInfo | null;
  strategy: ProbeStrategy | null;
}) {
  return (
    <div className="space-y-3 rounded-xl border border-border bg-card p-3">
      <div className="flex flex-col gap-4 sm:flex-row">
        <div className="relative aspect-video w-full shrink-0 overflow-hidden rounded-lg bg-muted sm:w-56">
          {info.thumbnail !== null && info.thumbnail !== "" ? (
            <img
              src={info.thumbnail}
              alt={info.title}
              className="size-full object-cover"
              loading="lazy"
              referrerPolicy="no-referrer"
            />
          ) : (
            <div className="flex size-full items-center justify-center text-muted-foreground">
              <Film className="size-6" />
            </div>
          )}
        </div>
        <div className="min-w-0 flex-1 space-y-2">
          <p className="line-clamp-2 text-sm font-medium text-foreground">{info.title}</p>
          <div className="flex flex-wrap items-center gap-1.5">
            <Badge variant="secondary">{info.extractor}</Badge>
            {site !== null ? <Badge variant="outline">{site.name}</Badge> : null}
            {info.uploader !== null && info.uploader !== "" ? (
              <span className="truncate text-xs text-muted-foreground">{info.uploader}</span>
            ) : null}
          </div>
          <div className="flex flex-wrap items-center gap-3 text-xs text-muted-foreground">
            <span className="inline-flex items-center gap-1">
              <Clock className="size-3.5" />
              {formatDuration(info.duration)}
            </span>
            <span>{info.formats.length} 个格式</span>
          </div>
          {info.isPlaylist ? (
            <Alert className="mt-1 py-1.5">
              <ListVideo />
              <AlertTitle>这是一个播放列表</AlertTitle>
              <AlertDescription>共 {info.entryCount} 个条目，可开启下方开关批量下载。</AlertDescription>
            </Alert>
          ) : null}
        </div>
      </div>

      {info.isPlaylist && info.entries.length > 0 ? (
        <div className="max-h-44 space-y-1 overflow-y-auto rounded-lg border border-border bg-muted/20 p-2">
          {info.entries.map((entry, index) => (
            <div
              key={entry.url + "-" + index}
              className="flex items-center justify-between gap-3 rounded px-1.5 py-1 text-xs"
            >
              <span className="min-w-0 flex-1 truncate text-foreground">
                <span className="mr-1.5 text-muted-foreground">{index + 1}.</span>
                {entry.title}
              </span>
              <span className="shrink-0 text-muted-foreground">{formatDuration(entry.duration)}</span>
            </div>
          ))}
        </div>
      ) : null}

      {site !== null ? (
        site.status !== "ready" ? (
          <div className="space-y-1 rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2">
            <p className="text-xs font-medium text-amber-200">
              {SITE_STATUS_ALERT_TITLE[site.status] ?? "该平台支持不保证"}
            </p>
            {site.tips.map((tip) => (
              <p
                key={tip}
                className="flex items-start gap-1.5 text-xs leading-relaxed text-amber-200/80"
              >
                <Info className="mt-0.5 size-3 shrink-0" />
                <span className="break-words">{tip}</span>
              </p>
            ))}
          </div>
        ) : site.tips.length > 0 ? (
          <ul className="space-y-1">
            {site.tips.map((tip) => (
              <li
                key={tip}
                className="flex items-start gap-1.5 text-xs leading-relaxed text-muted-foreground"
              >
                <Info className="mt-0.5 size-3 shrink-0" />
                <span className="break-words">{tip}</span>
              </li>
            ))}
          </ul>
        ) : null
      ) : null}

      {strategy !== null ? <StrategyTrace strategy={strategy} /> : null}
    </div>
  );
}
/** 默认配置一次成功是常态，没必要打扰用户；只有试过多种方式才展开明细。 */
function shouldShowTrace(strategy: ProbeStrategy): boolean {
  const { attempts } = strategy;
  if (attempts.length === 0) {
    return false;
  }
  if (strategy.applied === "default" && attempts.length === 1 && attempts[0].ok) {
    return false;
  }
  return true;
}

function StrategyTrace({ strategy }: { strategy: ProbeStrategy }) {
  const [open, setOpen] = useState(false);

  if (!shouldShowTrace(strategy)) {
    return null;
  }

  const attempts = strategy.attempts;
  const failed = attempts.filter((attempt) => !attempt.ok).length;

  return (
    <div className="space-y-1.5">
      {strategy.applied !== "default" ? (
        <p className="text-[11px] text-muted-foreground/70">解析策略：{strategy.label}</p>
      ) : null}

      <button
        type="button"
        onClick={() => setOpen((prev) => !prev)}
        aria-expanded={open}
        className="inline-flex items-center gap-1 rounded text-[11px] text-muted-foreground transition-colors hover:text-foreground focus-visible:ring-[3px] focus-visible:ring-ring/50 focus-visible:outline-none"
      >
        试了 {attempts.length} 种方式
        {failed > 0 ? <span className="text-muted-foreground/60">（{failed} 个失败）</span> : null}
        <ChevronDown
          className={cn("size-3 transition-transform duration-200", open && "rotate-180")}
        />
      </button>

      {open ? (
        <ul className="space-y-1.5 rounded-lg border border-border bg-muted/20 p-2">
          {attempts.map((attempt, index) => (
            <li key={attempt.name + "-" + index} className="space-y-1">
              <div className="flex items-center gap-1.5">
                <span className="text-xs text-foreground">{attempt.label}</span>
                <Badge
                  variant="outline"
                  className={cn(
                    "ml-auto",
                    attempt.ok
                      ? "border-emerald-500/30 bg-emerald-500/10 text-emerald-300"
                      : "border-amber-500/30 bg-amber-500/10 text-amber-300",
                  )}
                >
                  {attempt.ok ? "成功" : "失败"}
                </Badge>
              </div>
              {!attempt.ok && attempt.error !== null && attempt.error !== "" ? (
                <p className="line-clamp-2 font-mono text-[11px] break-words whitespace-pre-wrap text-muted-foreground">
                  {attempt.error}
                </p>
              ) : null}
            </li>
          ))}
        </ul>
      ) : null}
    </div>
  );
}
