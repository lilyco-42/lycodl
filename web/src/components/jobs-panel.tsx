import { useState } from "react";
import {
  Ban,
  CircleCheck,
  CircleDashed,
  CircleX,
  CircleSlash,
  FolderOpen,
  Inbox,
  LoaderCircle,
  RefreshCw,
  Clock,
  Gauge,
  Files,
} from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/empty-state";
import { ErrorAlert } from "@/components/error-alert";
import { api } from "@/lib/api";
import type { Job, JobStatus } from "@/lib/api";
import { errorMessage } from "@/lib/api";
import { clampPercent, formatBytes, formatDateTime, formatEta, formatSpeed } from "@/lib/format";
import { copyText } from "@/lib/clipboard";
import { useJobs } from "@/hooks/useJobs";

const STATUS_META: Record<
  JobStatus,
  { label: string; variant: "default" | "secondary" | "destructive" | "outline"; icon: typeof CircleCheck }
> = {
  queued: { label: "排队中", variant: "secondary", icon: CircleDashed },
  running: { label: "下载中", variant: "default", icon: LoaderCircle },
  done: { label: "已完成", variant: "outline", icon: CircleCheck },
  failed: { label: "失败", variant: "destructive", icon: CircleX },
  cancelled: { label: "已取消", variant: "secondary", icon: CircleSlash },
};

const MODE_LABEL: Record<string, string> = {
  video: "视频",
  audio: "音频",
  custom: "自定义格式",
};

function isActive(job: Job): boolean {
  return job.status === "queued" || job.status === "running";
}

/** 任务面板：列表 + SSE 实时进度 + 取消 / 打开位置。 */
export function JobsPanel() {
  const { jobs, loading, error, refresh, cancelJob } = useJobs();
  const [cancelling, setCancelling] = useState<Record<string, boolean>>({});
  const [refreshing, setRefreshing] = useState(false);

  const activeCount = jobs.filter(isActive).length;

  async function handleCancel(job: Job) {
    setCancelling((prev) => ({ ...prev, [job.id]: true }));
    try {
      await cancelJob(job.id);
      toast.success("已请求取消任务");
    } catch (cause) {
      toast.error("取消失败", { description: errorMessage(cause) });
    } finally {
      setCancelling((prev) => {
        const next = { ...prev };
        delete next[job.id];
        return next;
      });
    }
  }

  async function handleReveal(job: Job) {
    const dir = job.output;
    const name = job.fileName;
    // output 是目录、fileName 是文件名，拼成完整路径才能让文件管理器「选中」它
    const path =
      dir !== null && dir !== "" && name !== null && name !== ""
        ? dir.replace(/\/+$/, "") + "/" + name
        : (dir ?? name);
    if (path === null || path === "") {
      toast.error("该任务没有输出路径");
      return;
    }
    // 先请后端拉起系统文件管理器；拉不起来（无桌面会话 / 非本机）就退回复制路径
    try {
      const r = await api.reveal(path);
      toast.success(
        r.via === "filemanager" ? "已在文件管理器中定位" : "已打开所在目录",
        { description: r.dir },
      );
      return;
    } catch {
      /* 落到复制 */
    }
    const ok = await copyText(path);
    if (ok) {
      toast.success("路径已复制到剪贴板", { description: path });
    } else {
      toast.error("复制失败", { description: path });
    }
  }

  async function handleRefresh() {
    setRefreshing(true);
    refresh();
    window.setTimeout(() => setRefreshing(false), 600);
  }

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Files className="size-4" />
          <span>
            共 {jobs.length} 个任务
            {activeCount > 0 ? "，其中 " + activeCount + " 个进行中" : ""}
          </span>
        </div>
        <Button variant="outline" size="sm" onClick={() => void handleRefresh()} disabled={refreshing}>
          {refreshing ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
          刷新
        </Button>
      </div>

      {error !== null ? <ErrorAlert title="获取任务列表失败" message={error} /> : null}

      {loading && jobs.length === 0 ? (
        <div className="space-y-3">
          {[0, 1].map((key) => (
            <Card key={key}>
              <CardHeader>
                <Skeleton className="h-5 w-1/2" />
              </CardHeader>
              <CardContent className="space-y-3">
                <Skeleton className="h-1.5 w-full" />
                <Skeleton className="h-4 w-1/3" />
              </CardContent>
            </Card>
          ))}
        </div>
      ) : null}

      {!loading && jobs.length === 0 ? (
        <EmptyState
          icon={Inbox}
          title="还没有下载任务"
          description="回到「下载」页签粘贴一个链接，解析后点击开始下载，任务会出现在这里。"
        />
      ) : null}

      <div className="space-y-3">
        {jobs.map((job) => (
          <JobCard
            key={job.id}
            job={job}
            cancelling={cancelling[job.id] === true}
            onCancel={() => void handleCancel(job)}
            onReveal={() => void handleReveal(job)}
          />
        ))}
      </div>
    </div>
  );
}

interface JobCardProps {
  job: Job;
  cancelling: boolean;
  onCancel: () => void;
  onReveal: () => void;
}

function JobCard({ job, cancelling, onCancel, onReveal }: JobCardProps) {
  const meta = STATUS_META[job.status];
  const StatusIcon = meta.icon;
  const percent = clampPercent(job.percent);
  const active = isActive(job);

  return (
    <Card>
      <CardHeader className="gap-2">
        <CardTitle className="flex flex-wrap items-center gap-2 text-sm">
          <span className="min-w-0 flex-1 truncate">{job.title !== "" ? job.title : job.url}</span>
          <Badge variant={meta.variant} className="shrink-0">
            <StatusIcon className={job.status === "running" ? "animate-spin" : undefined} />
            {meta.label}
          </Badge>
        </CardTitle>
        <div className="flex flex-wrap items-center gap-2 text-xs text-muted-foreground">
          <Badge variant="outline">{MODE_LABEL[job.mode] ?? job.mode}</Badge>
          {job.playlistIndex !== null ? (
            <Badge variant="secondary">
              第 {job.playlistIndex}
              {job.playlistCount !== null ? " / " + job.playlistCount : ""} 项
            </Badge>
          ) : null}
          <span className="inline-flex items-center gap-1">
            <Clock className="size-3" />
            {formatDateTime(job.createdAt)}
          </span>
          <span className="truncate font-mono text-[11px] opacity-70">{job.id}</span>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="space-y-1.5">
          <Progress value={percent} className="h-1.5" />
          <div className="flex flex-wrap items-center justify-between gap-2 text-xs text-muted-foreground">
            <span className="inline-flex items-center gap-1">
              <Gauge className="size-3" />
              {formatSpeed(job.speed)}
            </span>
            <span>
              {formatBytes(job.downloadedBytes)}
              {job.totalBytes !== null ? " / " + formatBytes(job.totalBytes) : ""}
            </span>
            <span>剩余 {formatEta(job.eta)}</span>
            <span className="font-medium text-foreground">{percent.toFixed(1)}%</span>
          </div>
        </div>

        {job.error !== null && job.error !== "" ? (
          <ErrorAlert title="任务出错" message={job.error} />
        ) : null}

        {job.status === "done" && (job.output ?? job.fileName) !== null ? (
          <div className="rounded-lg border border-border bg-muted/30 px-2.5 py-1.5 font-mono text-[11px] break-all text-muted-foreground">
            {job.output ?? job.fileName}
          </div>
        ) : null}

        {active || job.status === "done" ? (
          <>
            <Separator />
            <div className="flex items-center justify-end gap-2">
              {active ? (
                <Button
                  variant="destructive"
                  size="sm"
                  onClick={onCancel}
                  disabled={cancelling}
                >
                  {cancelling ? <LoaderCircle className="animate-spin" /> : <Ban />}
                  {cancelling ? "取消中…" : "取消"}
                </Button>
              ) : null}
              {job.status === "done" ? (
                <Button variant="outline" size="sm" onClick={onReveal}>
                  <FolderOpen />
                  打开位置
                </Button>
              ) : null}
            </div>
          </>
        ) : null}
      </CardContent>
    </Card>
  );
}
