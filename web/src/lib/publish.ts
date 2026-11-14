export const PUBLISH_DESTS: string[] = ["quark", "bilibili", "douyin", "xhs", "kuaishou", "youtube", "x"];

export const PUBLISH_LABELS: Record<string, string> = {
  quark: "夸克网盘",
  bilibili: "B站",
  douyin: "抖音",
  xhs: "小红书",
  kuaishou: "快手",
  youtube: "YouTube",
  x: "X / 推特",
};

export type PublishBadgeVariant = "default" | "secondary" | "destructive" | "outline";

export function publishStatusMeta(status: string): { label: string; variant: PublishBadgeVariant } {
  if (status === "ok") return { label: "成功", variant: "default" };
  if (status === "dry-run") return { label: "演练未外发", variant: "secondary" };
  if (status === "unavailable") return { label: "未就绪", variant: "outline" };
  return { label: "失败", variant: "destructive" };
}
