import { useState } from "react";
import { Bot, Copy, LoaderCircle, RefreshCw, Terminal, ShieldCheck, Lock, Check } from "lucide-react";
import { toast } from "sonner";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "@/components/ui/card";
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
import { useCommands } from "@/hooks/useCommands";
import { copyText } from "@/lib/clipboard";
import type { CommandArg, CommandSafety } from "@/lib/api";

const SAFETY_META: Record<
  CommandSafety,
  { label: string; variant: "default" | "secondary" | "destructive" | "outline"; icon: typeof Check }
> = {
  ReadOnly: { label: "只读", variant: "outline", icon: Check },
  Confirm: { label: "需确认", variant: "secondary", icon: ShieldCheck },
  Token: { label: "令牌", variant: "default", icon: Lock },
  NeverAuto: { label: "禁止自动", variant: "destructive", icon: Lock },
};

function argTypeLabel(arg: CommandArg): string {
  const kind = arg.kind;
  if (kind.type === "enum" && kind.values && kind.values.length > 0) {
    return "enum: " + kind.values.join(" | ");
  }
  return kind.type;
}

/** 注册表命令展示：同一份 schema 驱动 CLI / TUI / Web / MCP。 */
export function CommandsPanel() {
  const { commands, loading, error, refresh } = useCommands();
  const [refreshing, setRefreshing] = useState(false);
  const [copied, setCopied] = useState<string | null>(null);

  async function handleCopy(name: string) {
    const ok = await copyText("lycodl " + name + " --help");
    if (ok) {
      setCopied(name);
      window.setTimeout(() => setCopied((current) => (current === name ? null : current)), 1600);
      toast.success("已复制 CLI 用法", { description: "lycodl " + name + " --help" });
    } else {
      toast.error("复制失败");
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
    <div className="space-y-4">
      <Alert>
        <Bot />
        <AlertTitle>同一份命令注册表，驱动所有入口</AlertTitle>
        <AlertDescription>
          CLI / TUI / Web / MCP 都由同一份注册表生成，可被 AI 直接调用。命令名与参数说明即 AI 可用的工具签名。
        </AlertDescription>
      </Alert>

      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="flex items-center gap-2 text-sm text-muted-foreground">
          <Terminal className="size-4" />
          <span>共 {commands.length} 个命令</span>
        </div>
        <Button variant="outline" size="sm" onClick={() => void handleRefresh()} disabled={refreshing}>
          {refreshing ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
          刷新
        </Button>
      </div>

      {error !== null ? <ErrorAlert title="获取命令列表失败" message={error} /> : null}

      {loading && commands.length === 0 ? (
        <div className="space-y-2">
          {[0, 1, 2].map((key) => (
            <Skeleton key={key} className="h-12 w-full" />
          ))}
        </div>
      ) : null}

      {!loading && commands.length === 0 && error === null ? (
        <EmptyState icon={Terminal} title="后端没有注册任何命令" description="检查后端是否以 web 模式注册了命令注册表。" />
      ) : null}

      <div className="space-y-3">
        {commands.map((command) => {
          const safety = SAFETY_META[command.safety] ?? SAFETY_META.ReadOnly;
          const SafetyIcon = safety.icon;
          const isCopied = copied === command.name;
          return (
            <Card key={command.name}>
              <CardHeader className="gap-2">
                <CardTitle className="flex flex-wrap items-center gap-2 text-sm">
                  <code className="rounded bg-muted px-1.5 py-0.5 font-mono text-xs text-foreground">
                    {command.name}
                  </code>
                  <Badge variant={safety.variant}>
                    <SafetyIcon />
                    {safety.label}
                  </Badge>
                  <span className="ml-auto">
                    <Button variant="outline" size="xs" onClick={() => void handleCopy(command.name)}>
                      {isCopied ? <Check /> : <Copy />}
                      {isCopied ? "已复制" : "复制 CLI 用法"}
                    </Button>
                  </span>
                </CardTitle>
                <CardDescription>{command.about}</CardDescription>
              </CardHeader>
              {command.args.length > 0 ? (
                <CardContent>
                  <Table>
                    <TableHeader>
                      <TableRow>
                        <TableHead className="w-48">参数</TableHead>
                        <TableHead className="w-32">类型</TableHead>
                        <TableHead>说明</TableHead>
                      </TableRow>
                    </TableHeader>
                    <TableBody>
                      {command.args.map((arg) => (
                        <TableRow key={arg.name}>
                          <TableCell className="font-mono text-xs break-all">
                            {arg.name}
                            {arg.required ? <span className="ml-1 text-destructive">*</span> : null}
                          </TableCell>
                          <TableCell className="font-mono text-xs text-muted-foreground">
                            {argTypeLabel(arg)}
                          </TableCell>
                          <TableCell className="text-muted-foreground">{arg.about}</TableCell>
                        </TableRow>
                      ))}
                    </TableBody>
                  </Table>
                </CardContent>
              ) : (
                <CardContent>
                  <p className="text-xs text-muted-foreground">该命令没有参数。</p>
                </CardContent>
              )}
            </Card>
          );
        })}
      </div>
    </div>
  );
}