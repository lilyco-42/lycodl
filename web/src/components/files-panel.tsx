import { useState } from "react";
import { Download, FolderOpen, LoaderCircle, RefreshCw } from "lucide-react";
import { toast } from "sonner";
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
import { formatBytes, formatDateTime } from "@/lib/format";

/** 输出目录文件列表。 */
export function FilesPanel() {
  const { files, loading, error, refresh } = useFiles();
  const [refreshing, setRefreshing] = useState(false);

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
                <TableHead className="w-20 text-right">操作</TableHead>
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
                    <Button variant="outline" size="xs" asChild>
                      <a href={file.url} download={file.name}>
                        <Download />
                        下载
                      </a>
                    </Button>
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