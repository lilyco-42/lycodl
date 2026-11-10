import { useEffect, useState } from "react";
import { LoaderCircle, ScrollText } from "lucide-react";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { api, errorMessage } from "@/lib/api";

interface JobLogsButtonProps {
  jobId: string;
}

/** 查看某个任务的原始日志尾（失败排查用：这里能看到 yt-dlp 到底报了什么）。 */
export function JobLogsButton({ jobId }: JobLogsButtonProps) {
  const [open, setOpen] = useState(false);
  const [logs, setLogs] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    if (!open) {
      return;
    }
    let cancelled = false;
    setLoading(true);
    setError(null);
    api
      .jobLogs(jobId)
      .then((data) => {
        if (!cancelled) {
          setLogs(data.logs);
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
    return () => {
      cancelled = true;
    };
  }, [open, jobId]);

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      <DialogTrigger asChild>
        <Button variant="ghost" size="sm">
          <ScrollText />
          日志
        </Button>
      </DialogTrigger>
      <DialogContent className="sm:max-w-3xl">
        <DialogHeader>
          <DialogTitle>任务日志</DialogTitle>
          <DialogDescription className="font-mono text-[11px]">{jobId}</DialogDescription>
        </DialogHeader>
        {loading ? (
          <div className="flex items-center gap-2 py-6 text-sm text-muted-foreground">
            <LoaderCircle className="size-4 animate-spin" />
            正在读取…
          </div>
        ) : error !== null ? (
          <div className="rounded-lg border border-destructive/40 bg-destructive/10 px-3 py-2 text-sm text-destructive">
            {error}
          </div>
        ) : logs.length === 0 ? (
          <div className="py-6 text-sm text-muted-foreground">这个任务还没有日志。</div>
        ) : (
          <pre className="max-h-[60vh] overflow-auto rounded-lg border border-border bg-muted/30 p-3 font-mono text-[11px] leading-relaxed whitespace-pre-wrap break-all">
            {logs.join("\n")}
          </pre>
        )}
      </DialogContent>
    </Dialog>
  );
}
