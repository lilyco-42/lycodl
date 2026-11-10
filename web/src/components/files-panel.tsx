import { useState } from "react";
import { Download, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import { toast } from "sonner";
import { api, errorMessage } from "@/lib/api";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardHeader, CardTitle, CardDescription } from "@/components/ui/card";
import { Skeleton } from "@/components/ui/skeleton";
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
export function FilesPanel() {
  const { files, loading, error, refresh } = useFiles();
  // 文件接口只给名字，定位到文件需要输出目录，所以顺带读一次设置
  const { settings } = useSettings();
  const [refreshing, setRefreshing] = useState(false);
  const [revealing, setRevealing] = useState<string | null>(null);

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
        <Button variant="outline" size="sm" onClick={() => void handleRefresh()} disabled={refreshing}>
          {refreshing ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
          刷新
        </Button>
      </CardHeader>
      <CardContent className="space-y-3">
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
                <TableHead className="w-40 text-right">操作</TableHead>
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