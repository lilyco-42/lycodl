import { useState } from "react";
import { Download, FolderOpen, Layers3, LoaderCircle, RefreshCw, Send, WandSparkles } from "lucide-react";
import { toast } from "sonner";
import { api, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
import { Switch } from "@/components/ui/switch";
import { Label } from "@/components/ui/label";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { EmptyState } from "@/components/empty-state";
import { ErrorAlert } from "@/components/error-alert";
import { useFiles } from "@/hooks/useFiles";
import { useSettings } from "@/hooks/useSettings";
import { formatBytes, formatDateTime } from "@/lib/format";

/** 输出目录文件列表。 */
export function FilesPanel({
  onPublish,
  onTaskCreated,
}: {
  onPublish?: (path: string) => void;
  onTaskCreated?: () => void;
}) {
  const { files, loading, error, refresh } = useFiles();
  // 文件接口只给名字，定位到文件需要输出目录，所以顺带读一次设置
  const { settings } = useSettings();
  const [refreshing, setRefreshing] = useState(false);
  const [revealing, setRevealing] = useState<string | null>(null);
  const [oneClicking, setOneClicking] = useState<string | null>(null);
  const [publishAllOpen, setPublishAllOpen] = useState(false);
  const [publishAllDryRun, setPublishAllDryRun] = useState(true);
  const [publishAllRiskAck, setPublishAllRiskAck] = useState(false);
  const [publishAllBusy, setPublishAllBusy] = useState(false);

  async function handleReveal(name: string) {
    const dir = settings?.outputDir ?? "";
    if (dir === "") {
      toast.error("还没读到输出目录，请稍后再试");
      return;
    }
    setRevealing(name);
    try {
      const r = await api.reveal(dir.replace(/\/+$/, "") + "/" + name);
      toast.success(
        r.via === "filemanager" ? "已在文件管理器中定位" : "已打开所在目录",
        { description: r.dir },
      );
    } catch (cause: unknown) {
      toast.error("无法打开文件管理器", { description: errorMessage(cause) });
    } finally {
      setRevealing(null);
    }
  }

  function fullPath(name: string): string {
    const dir = settings?.outputDir ?? "";
    const base = dir.endsWith("/") ? dir.slice(0, -1) : dir;
    return base + "/" + name;
  }

  function handlePublish(name: string) {
    if ((settings?.outputDir ?? "") === "") {
      toast.error("还没读到输出目录，请稍后再试");
      return;
    }
    onPublish?.(fullPath(name));
  }

  /** 文件行的一键发布：直接走服务端只读筛选，自动跳过不可用 / 已发布目的地。 */
  async function handleOneClick(name: string) {
    if ((settings?.outputDir ?? "") === "") {
      toast.error("还没读到输出目录，请稍后再试");
      return;
    }
    setOneClicking(name);
    try {
      const result = await api.publishLocal({
        file: fullPath(name),
        targets: settings?.publishTargets ?? [],
        // 设置还没读到时保守地只演练，避免误真发
        dryRun: settings?.publishDryRun ?? true,
        force: false,
        filter: true,
      });
      const count = result.filter?.selectable.length ?? 0;
      toast.success("一键发布任务已创建", {
        description: "任务 ID：" + result.jobId + "；" + count + " 个目的地" +
          ((settings?.publishDryRun ?? true) ? "（演练，不会外发）" : ""),
      });
      onTaskCreated?.();
    } catch (cause: unknown) {
      toast.error("一键发布失败", { description: errorMessage(cause) });
    } finally {
      setOneClicking(null);
    }
  }

  /** 顶部「全部发布」：默认演练，真发需关闭演练并再次勾选风险确认。 */
  async function handlePublishAll() {
    if (!publishAllDryRun && !window.confirm("将对输出目录内的全部成品向已选平台真实外发。此操作可能耗时数小时并产生大量重复内容；确认继续？")) {
      return;
    }
    const selected = settings?.publishTargets ?? [];
    if (selected.length === 0) {
      toast.error("还没有选择发布目的地");
      return;
    }
    if (!publishAllDryRun && !publishAllRiskAck) {
      toast.error("请先勾选真实外发风险确认");
      return;
    }
    setPublishAllBusy(true);
    try {
      const result = await api.publishAll({
        targets: selected,
        dryRun: publishAllDryRun,
        force: false,
        riskAck: !publishAllDryRun,
      });
      const mode = publishAllDryRun ? "演练，不会外发" : "真实外发已启用";
      toast.success("全部发布任务已创建", {
        description: "任务 ID：" + result.jobId + "；" + result.fileCount + " 个文件；" + mode,
      });
      setPublishAllOpen(false);
      setPublishAllRiskAck(false);
      setPublishAllDryRun(true);
      onTaskCreated?.();
    } catch (cause: unknown) {
      toast.error("创建全部发布任务失败", { description: errorMessage(cause) });
    } finally {
      setPublishAllBusy(false);
    }
  }

  async function handleRefresh() {
    setRefreshing(true);
    const ok = await refresh();
    setRefreshing(false);
    if (!ok) {
      toast.error("刷新失败");
    }
  }

  return (
    <Card>
      <CardHeader className="flex flex-row items-start justify-between gap-3">
        <div className="space-y-1">
          <CardTitle className="flex items-center gap-2 text-base">
            <FolderOpen className="size-4 text-muted-foreground" />
            输出文件
          </CardTitle>
          <CardDescription>下载完成的文件会保存在后端配置的输出目录中。</CardDescription>
        </div>
        <div className="flex flex-wrap items-center justify-end gap-2">
          <Button id="publish-all" onClick={() => setPublishAllOpen(true)} disabled={files.length === 0}>
            <Layers3 />
            全部发布
          </Button>
          <Button variant="outline" size="sm" onClick={() => void handleRefresh()} disabled={refreshing}>
            {refreshing ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
            刷新
          </Button>
        </div>
      </CardHeader>
      <CardContent className="space-y-3">
        <Dialog open={publishAllOpen} onOpenChange={(open) => {
          setPublishAllOpen(open);
          if (!open) {
            setPublishAllRiskAck(false);
            setPublishAllDryRun(true);
          }
        }}>
          <DialogContent className="sm:max-w-lg">
            <DialogHeader>
              <DialogTitle>全部发布（{files.length} 个文件）</DialogTitle>
              <DialogDescription>
                后端会重新扫描输出目录，并在每个文件发布前重新做只读能力预检与发布台账去重。
              </DialogDescription>
            </DialogHeader>
            <div className="space-y-3 text-sm">
              <div className="rounded-lg border border-border bg-muted/30 p-3">
                当前目的地：{(settings?.publishTargets ?? []).join(" / ") || "尚未读取"}
              </div>
              <div className="flex items-center justify-between gap-3 rounded-lg border border-border p-3">
                <div className="space-y-1">
                  <Label htmlFor="publish-all-dry-run">演练模式</Label>
                  <p className="text-xs text-muted-foreground">开启时只记录将要发布到哪些目的地，不上传、不发帖。</p>
                </div>
                <Switch
                  id="publish-all-dry-run"
                  checked={publishAllDryRun}
                  onCheckedChange={setPublishAllDryRun}
                  disabled={publishAllBusy}
                />
              </div>
              {!publishAllDryRun ? (
                <div className="flex items-center justify-between gap-3 rounded-lg border border-destructive/40 bg-destructive/5 p-3">
                  <div className="space-y-1">
                    <Label htmlFor="publish-all-risk-ack">确认真实外发</Label>
                    <p className="text-xs text-muted-foreground">
                      将对输出目录内全部成品向已选平台真实发布；台账已成功的会自动跳过。
                    </p>
                  </div>
                  <Switch
                    id="publish-all-risk-ack"
                    checked={publishAllRiskAck}
                    onCheckedChange={setPublishAllRiskAck}
                    disabled={publishAllBusy}
                  />
                </div>
              ) : null}
            </div>
            <DialogFooter>
              <Button
                variant="outline"
                onClick={() => { setPublishAllOpen(false); setPublishAllRiskAck(false); setPublishAllDryRun(true); }}
                disabled={publishAllBusy}
              >
                取消
              </Button>
              <Button
                id="publish-all-confirm"
                variant={publishAllDryRun ? "default" : "destructive"}
                onClick={() => void handlePublishAll()}
                disabled={publishAllBusy || (!publishAllDryRun && !publishAllRiskAck)}
              >
                {publishAllBusy ? <LoaderCircle className="animate-spin" /> : <Layers3 />}
                {publishAllBusy ? "创建中…" : publishAllDryRun ? "开始演练" : "确认真实发布"}
              </Button>
            </DialogFooter>
          </DialogContent>
        </Dialog>
        {error !== null ? <ErrorAlert title="获取文件列表失败" message={error} /> : null}

        {loading && files.length === 0 ? (
          <div className="space-y-2">
            {[0, 1, 2].map((key) => (
              <Skeleton key={key} className="h-10 w-full" />
            ))}
          </div>
        ) : null}

        {!loading && files.length === 0 && error === null ? (
          <EmptyState
            icon={FolderOpen}
            title="这里还空着"
            description="下载完成后这里会出现文件。"
          />
        ) : null}

        {files.length > 0 ? (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead className="min-w-0">名称</TableHead>
                <TableHead className="w-28 text-right">大小</TableHead>
                <TableHead className="w-44">修改时间</TableHead>
                <TableHead className="w-72 text-right">操作</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {files.map((file) => (
                <TableRow key={file.name}>
                  <TableCell className="max-w-0 truncate font-mono text-xs" title={file.name}>
                    {file.name}
                  </TableCell>
                  <TableCell className="text-right text-muted-foreground tabular-nums">
                    {formatBytes(file.size)}
                  </TableCell>
                  <TableCell className="text-muted-foreground">{formatDateTime(file.modified)}</TableCell>
                  <TableCell className="text-right">
                    <div className="flex items-center justify-end gap-1.5">
                      <Button
                        id={"one-click-publish-" + file.name}
                        size="xs"
                        onClick={() => void handleOneClick(file.name)}
                        disabled={oneClicking === file.name}
                        title="只读筛选后自动发布到可用且未发布的目的地"
                      >
                        {oneClicking === file.name ? <LoaderCircle className="animate-spin" /> : <WandSparkles />}
                        一键发布
                      </Button>
                      <Button
                        variant="outline"
                        size="xs"
                        onClick={() => handlePublish(file.name)}
                      >
                        <Send />
                        发布
                      </Button>
                      <Button
                        variant="ghost"
                        size="xs"
                        onClick={() => void handleReveal(file.name)}
                        disabled={revealing === file.name}
                      >
                        {revealing === file.name ? (
                          <LoaderCircle className="animate-spin" />
                        ) : (
                          <FolderOpen />
                        )}
                        定位
                      </Button>
                      <Button variant="outline" size="xs" asChild>
                        <a href={file.url} download={file.name}>
                          <Download />
                          下载
                        </a>
                      </Button>
                    </div>
                  </TableCell>
                </TableRow>
              ))}
            </TableBody>
          </Table>
        ) : null}
      </CardContent>
    </Card>
  );
}
