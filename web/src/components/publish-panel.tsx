import { useEffect, useRef, useState } from "react";
import { LoaderCircle, RefreshCw, Send, UploadCloud } from "lucide-react";
import { toast } from "sonner";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
import { ErrorAlert } from "@/components/error-alert";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Progress } from "@/components/ui/progress";
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { api, errorMessage, type PublishLedgerEntry, type PublishSubmission } from "@/lib/api";
import { formatBytes, formatDateTime } from "@/lib/format";
import { PUBLISH_DESTS, PUBLISH_LABELS, publishStatusMeta } from "@/lib/publish";
import { useSettings } from "@/hooks/useSettings";

interface PublishPanelProps {
  /** 从「文件」页点发布时自动填入本机路径 */
  prefillPath?: string;
  /** token 递增保证同一路径可重复触发 */
  prefillToken?: number;
  /** 建任务后跳到「任务」页看实时结果 */
  onTaskCreated?: () => void;
}

function baseName(path: string): string {
  const slash = path.lastIndexOf("/");
  const backslash = path.lastIndexOf(String.fromCharCode(92));
  const index = Math.max(slash, backslash);
  return index >= 0 ? path.slice(index + 1) : path;
}

/** 本机视频手动发布：浏览器选择文件上传，或直接填后端所在机器的绝对路径。 */
export function PublishPanel({ prefillPath, prefillToken, onTaskCreated }: PublishPanelProps) {
  const { settings, loading: settingsLoading } = useSettings();
  const [file, setFile] = useState<File | null>(null);
  const [pickerKey, setPickerKey] = useState(0);
  const [localPath, setLocalPath] = useState("");
  const [title, setTitle] = useState("");
  const [targets, setTargets] = useState<string[]>(() => PUBLISH_DESTS.slice());
  const [dryRun, setDryRun] = useState(false);
  const [force, setForce] = useState(false);
  const [busy, setBusy] = useState(false);
  const [uploadPercent, setUploadPercent] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const [ledger, setLedger] = useState<PublishLedgerEntry[]>([]);
  const [ledgerLoading, setLedgerLoading] = useState(false);
  const [ledgerError, setLedgerError] = useState<string | null>(null);
  const [ledgerTick, setLedgerTick] = useState(0);
  const defaultsApplied = useRef(false);

  useEffect(() => {
    if (settings === null || defaultsApplied.current) {
      return;
    }
    defaultsApplied.current = true;
    const configured = PUBLISH_DESTS.filter((d) => settings.publishTargets.includes(d));
    setTargets(configured.length > 0 ? configured : PUBLISH_DESTS.slice());
    setDryRun(settings.publishDryRun);
  }, [settings]);

  useEffect(() => {
    if (prefillPath === undefined || prefillPath === "") {
      return;
    }
    setLocalPath(prefillPath);
    setTitle(baseName(prefillPath));
    setFile(null);
    setPickerKey((key) => key + 1);
  }, [prefillPath, prefillToken]);

  useEffect(() => {
    let cancelled = false;
    setLedgerLoading(true);
    setLedgerError(null);
    api
      .publishLedger(8)
      .then((data) => {
        if (!cancelled) {
          setLedger(data.entries);
        }
      })
      .catch((cause: unknown) => {
        if (!cancelled) {
          setLedgerError(errorMessage(cause));
        }
      })
      .finally(() => {
        if (!cancelled) {
          setLedgerLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [ledgerTick]);

  function toggleTarget(dest: string, checked: boolean) {
    setTargets((prev) =>
      checked
        ? PUBLISH_DESTS.filter((d) => d === dest || prev.includes(d))
        : prev.filter((d) => d !== dest),
    );
  }

  async function handleSubmit() {
    if (busy) {
      return;
    }
    const selected = PUBLISH_DESTS.filter((d) => targets.includes(d));
    if (selected.length === 0) {
      setError("请至少勾选一个发布目的地");
      return;
    }
    setError(null);
    setBusy(true);
    setUploadPercent(0);
    try {
      const options: PublishSubmission = {
        title: title.trim(),
        targets: selected,
        dryRun,
        force,
      };
      let jobId: string;
      if (file !== null) {
        const result = await api.publishUpload(file, options, (percent) => setUploadPercent(percent));
        jobId = result.jobId;
      } else {
        const path = localPath.trim();
        if (path === "") {
          setError("请选择视频文件，或填写本机视频的绝对路径");
          return;
        }
        const result = await api.publishLocal({ file: path, ...options });
        jobId = result.jobId;
      }
      toast.success("发布任务已创建", {
        description: "任务 ID：" + jobId + (dryRun ? "（演练，不会外发）" : ""),
      });
      setLedgerTick((tick) => tick + 1);
      onTaskCreated?.();
    } catch (cause: unknown) {
      setError(errorMessage(cause));
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="space-y-4">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <UploadCloud className="size-4 text-muted-foreground" />
            发布本机视频
          </CardTitle>
          <CardDescription>
            选择浏览器里的视频文件，或直接填写运行 lycodl 的本机绝对路径。发布记录会写入任务列表和发布台账。
          </CardDescription>
        </CardHeader>
        <CardContent className="space-y-5">
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="space-y-2">
              <Label htmlFor="publish-file">方式一：选择本机文件</Label>
              <Input
                key={pickerKey}
                id="publish-file"
                type="file"
                accept="video/*,.mp4,.mov,.mkv,.webm,.m4v,.ts"
                disabled={busy}
                onChange={(event) => {
                  const picked = event.target.files?.[0] ?? null;
                  setFile(picked);
                  if (picked !== null) {
                    setLocalPath("");
                    if (title.trim() === "") {
                      setTitle(picked.name);
                    }
                  }
                }}
              />
              <div className="flex min-h-6 flex-wrap items-center gap-2 text-xs text-muted-foreground">
                {file !== null ? (
                  <>
                    <span className="min-w-0 truncate">{file.name} · {formatBytes(file.size)}</span>
                    <Button
                      variant="ghost"
                      size="xs"
                      disabled={busy}
                      onClick={() => {
                        setFile(null);
                        setPickerKey((key) => key + 1);
                      }}
                    >
                      清除
                    </Button>
                  </>
                ) : (
                  <span>文件会流式保存到输出目录，不整份读进内存。</span>
                )}
              </div>
            </div>
            <div className="space-y-2">
              <Label htmlFor="publish-local-path">方式二：本机绝对路径</Label>
              <Input
                id="publish-local-path"
                value={localPath}
                disabled={busy}
                placeholder="/home/lilyco/视频/demo.mp4"
                onChange={(event) => {
                  setLocalPath(event.target.value);
                  if (event.target.value.trim() !== "") {
                    setFile(null);
                    setPickerKey((key) => key + 1);
                  }
                }}
              />
              <p className="min-h-6 text-xs text-muted-foreground">
                {settingsLoading
                  ? "正在读取输出目录…"
                  : settings !== null
                    ? "输出目录：" + settings.outputDir + "；也可从「文件」页点「发布」自动填入。"
                    : "路径是后端所在机器的路径，不是浏览器的虚拟路径。"}
              </p>
            </div>
          </div>

          <div className="space-y-2">
            <Label htmlFor="publish-title">发布标题</Label>
            <Input
              id="publish-title"
              value={title}
              disabled={busy}
              placeholder="留空则使用文件名"
              onChange={(event) => setTitle(event.target.value)}
            />
          </div>

          <Separator />

          <div className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <div>
                <p className="text-sm font-medium">发布目的地</p>
                <p className="text-xs text-muted-foreground">已选 {targets.length} / {PUBLISH_DESTS.length} 个</p>
              </div>
              <div className="flex items-center gap-1.5">
                <Button variant="ghost" size="xs" disabled={busy} onClick={() => setTargets(PUBLISH_DESTS.slice())}>
                  全选
                </Button>
                <Button variant="ghost" size="xs" disabled={busy} onClick={() => setTargets([])}>
                  清空
                </Button>
              </div>
            </div>
            <div className="grid gap-2 sm:grid-cols-2 lg:grid-cols-3">
              {PUBLISH_DESTS.map((dest) => (
                <div key={dest} className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
                  <Label htmlFor={"publish-target-" + dest} className="cursor-pointer text-sm font-normal">
                    {PUBLISH_LABELS[dest] ?? dest}
                  </Label>
                  <Switch
                    id={"publish-target-" + dest}
                    aria-label={PUBLISH_LABELS[dest] ?? dest}
                    checked={targets.includes(dest)}
                    disabled={busy}
                    onCheckedChange={(checked) => toggleTarget(dest, checked)}
                  />
                </div>
              ))}
            </div>
          </div>

          <Separator />

          <div className="grid gap-3 sm:grid-cols-2">
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
              <div className="space-y-0.5">
                <Label htmlFor="publish-dry-run" className="cursor-pointer text-sm font-normal">只演练（不真正外发）</Label>
                <p className="text-xs text-muted-foreground">适合先检查链路与任务记录</p>
              </div>
              <Switch id="publish-dry-run" checked={dryRun} disabled={busy} onCheckedChange={setDryRun} />
            </div>
            <div className="flex items-center justify-between gap-3 rounded-lg border border-border px-3 py-2">
              <div className="space-y-0.5">
                <Label htmlFor="publish-force" className="cursor-pointer text-sm font-normal">强制重发</Label>
                <p className="text-xs text-muted-foreground">忽略台账里已成功发布的去重记录</p>
              </div>
              <Switch id="publish-force" checked={force} disabled={busy} onCheckedChange={setForce} />
            </div>
          </div>

          {dryRun ? (
            <div className="rounded-lg border border-sky-500/30 bg-sky-500/5 px-3 py-2 text-xs text-sky-700 dark:text-sky-300">
              演练模式：只生成任务与台账记录，不会上传夸克或提交到各平台。
            </div>
          ) : (
            <div className="rounded-lg border border-amber-500/30 bg-amber-500/5 px-3 py-2 text-xs text-amber-700 dark:text-amber-300">
              真发模式：提交后会向已勾选平台外发；仅检查流程请打开「只演练」。
            </div>
          )}

          {file !== null && busy ? (
            <div className="space-y-1.5">
              <Progress value={uploadPercent} className="h-1.5" />
              <p className="text-xs text-muted-foreground">上传进度 {uploadPercent}%</p>
            </div>
          ) : null}

          {error !== null ? <ErrorAlert title="提交失败" message={error} /> : null}

          <div className="flex flex-wrap items-center justify-end gap-2">
            <Button onClick={() => void handleSubmit()} disabled={busy}>
              {busy ? <LoaderCircle className="animate-spin" /> : <Send />}
              {busy ? "提交中…" : "创建发布任务"}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader className="flex flex-row items-start justify-between gap-3">
          <div className="space-y-1">
            <CardTitle className="text-base">最近发布台账</CardTitle>
            <CardDescription>每个目的地一条结果；点击任务页可看实时进度和日志。</CardDescription>
          </div>
          <Button
            variant="outline"
            size="sm"
            disabled={ledgerLoading}
            onClick={() => setLedgerTick((tick) => tick + 1)}
          >
            {ledgerLoading ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
            刷新
          </Button>
        </CardHeader>
        <CardContent>
          {ledgerError !== null ? <ErrorAlert title="读取发布记录失败" message={ledgerError} /> : null}
          {ledgerError === null && ledger.length === 0 && !ledgerLoading ? (
            <p className="text-xs text-muted-foreground">暂时没有发布记录。</p>
          ) : null}
          <div className="space-y-2">
            {ledger.map((entry, index) => {
              const meta = publishStatusMeta(entry.status);
              return (
                <div key={index} className="rounded-lg border border-border px-3 py-2 text-xs">
                  <div className="flex flex-wrap items-center gap-2">
                    <Badge variant={meta.variant}>
                      {PUBLISH_LABELS[entry.dest] ?? entry.dest} · {meta.label}
                    </Badge>
                    <span className="min-w-0 flex-1 truncate font-mono" title={entry.file}>
                      {baseName(entry.file)}
                    </span>
                    <span className="shrink-0 text-muted-foreground">{formatDateTime(entry.at)}</span>
                  </div>
                  <div className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-1 break-all text-muted-foreground">
                    <span className="min-w-0 flex-1">{entry.detail}</span>
                    {entry.source !== undefined && entry.source !== "" ? <span>来源 {entry.source}</span> : null}
                    {entry.taskId !== null && entry.taskId !== undefined && entry.taskId !== "" ? (
                      <span className="font-mono">任务 {entry.taskId}</span>
                    ) : null}
                    {entry.url !== null && entry.url !== "" ? (
                      <a className="text-primary underline underline-offset-2" href={entry.url} target="_blank" rel="noreferrer">
                        打开链接
                      </a>
                    ) : null}
                  </div>
                </div>
              );
            })}
          </div>
        </CardContent>
      </Card>
    </div>
  );
}
