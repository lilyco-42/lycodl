import { useEffect, useState } from "react";
import { Cpu, LoaderCircle, RotateCw, Save, Sparkles, Server, RefreshCw } from "lucide-react";
import { toast } from "sonner";
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
import { Separator } from "@/components/ui/separator";
import { Switch } from "@/components/ui/switch";
import { Skeleton } from "@/components/ui/skeleton";
import { ErrorAlert } from "@/components/error-alert";
import { errorMessage } from "@/lib/api";
import { useSettings } from "@/hooks/useSettings";
import { useYtDlp } from "@/hooks/useYtDlp";

const NO_COOKIE = "__none__";

/** 空串 / "none" 都视作关闭，统一发 null，别让后端存一个没意义的字符串。 */
function normalizeEndpoint(value: string): string | null {
  const trimmed = value.trim();
  if (trimmed === "" || trimmed.toLowerCase() === "none") {
    return null;
  }
  return trimmed;
}
const NO_IMP = "__none__";

const IMPERSONATE_OPTIONS: Array<{ value: string; label: string }> = [
  { value: "chrome", label: "chrome" },
  { value: "firefox", label: "firefox" },
  { value: "safari", label: "safari" },
  { value: "edge", label: "edge" },
  { value: "tor", label: "tor" },
];

const COOKIE_OPTIONS: Array<{ value: string; label: string }> = [
  { value: "chrome", label: "Chrome" },
  { value: "firefox", label: "Firefox" },
  { value: "edge", label: "Edge" },
  { value: "brave", label: "Brave" },
  { value: "chromium", label: "Chromium" },
];

/** 设置页：常规设置 + yt-dlp 内核管理。 */
export function SettingsPanel() {
  const { settings, loading, saving, error, save } = useSettings();

  const [outputDir, setOutputDir] = useState("");
  const [cookiesFromBrowser, setCookiesFromBrowser] = useState<string>(NO_COOKIE);
  const [proxy, setProxy] = useState("");
  const [concurrency, setConcurrency] = useState("1");
  const [impersonate, setImpersonate] = useState<string>(NO_IMP);
  const [browserBridge, setBrowserBridge] = useState(false);
  const [flaresolverrEndpoint, setFlaresolverrEndpoint] = useState("");

  useEffect(() => {
    if (settings === null) {
      return;
    }
    setOutputDir(settings.outputDir);
    setCookiesFromBrowser(settings.cookiesFromBrowser ?? NO_COOKIE);
    setProxy(settings.proxy ?? "");
    setConcurrency(String(settings.concurrency));
    setImpersonate(settings.impersonate ?? NO_IMP);
    setBrowserBridge(settings.browserBridge ?? false);
    setFlaresolverrEndpoint(settings.flaresolverrEndpoint ?? "");
  }, [settings]);

  async function handleSave() {
    const parsedConcurrency = Number.parseInt(concurrency, 10);
    if (!Number.isFinite(parsedConcurrency) || parsedConcurrency < 1) {
      toast.error("并发数必须是不小于 1 的整数");
      return;
    }
    try {
      const prevConcurrency = settings?.concurrency ?? parsedConcurrency;
      const result = await save({
        outputDir: outputDir.trim(),
        cookiesFromBrowser: cookiesFromBrowser === NO_COOKIE ? null : cookiesFromBrowser,
        proxy: proxy.trim() === "" ? null : proxy.trim(),
        concurrency: parsedConcurrency,
        impersonate: impersonate === NO_IMP ? null : impersonate,
        browserBridge,
        flaresolverrEndpoint: normalizeEndpoint(flaresolverrEndpoint),
      });
      const running = result.running ?? 0;
      if (parsedConcurrency < prevConcurrency && running > parsedConcurrency) {
        // 调小时不打断已在跑的：名额是空出来一个少一个，说清楚免得以为没生效
        toast.success("设置已保存", {
          description: `并发已降到 ${parsedConcurrency}；当前仍在跑 ${running} 个任务，它们不会被打断，新任务按新上限排队`,
        });
      } else {
        toast.success("设置已保存");
      }
    } catch (cause) {
      toast.error("保存失败", { description: errorMessage(cause) });
    }
  }

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Server className="size-4 text-muted-foreground" />
            常规设置
          </CardTitle>
          <CardDescription>这些设置会写入后端配置，对之后的任务生效。</CardDescription>
        </CardHeader>
        <CardContent className="space-y-4">
          {error !== null ? <ErrorAlert title="读取设置失败" message={error} /> : null}

          {loading && settings === null ? (
            <div className="space-y-3">
              <Skeleton className="h-9 w-full" />
              <Skeleton className="h-9 w-full" />
              <Skeleton className="h-9 w-full" />
            </div>
          ) : (
            <>
              <div className="space-y-2">
                <Label htmlFor="output-dir">输出目录</Label>
                <Input
                  id="output-dir"
                  value={outputDir}
                  onChange={(event) => setOutputDir(event.target.value)}
                  placeholder="/home/user/Downloads"
                  className="font-mono text-xs"
                  spellCheck={false}
                />
                <p className="text-xs text-muted-foreground">下载文件保存的绝对路径。</p>
              </div>

              <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
                <div className="space-y-2">
                  <Label htmlFor="cookie-select">从浏览器取 Cookie</Label>
                  <Select value={cookiesFromBrowser} onValueChange={setCookiesFromBrowser}>
                    <SelectTrigger id="cookie-select" className="w-full">
                      <SelectValue placeholder="不读取 Cookie" />
                    </SelectTrigger>
                    <SelectContent>
                      <SelectItem value={NO_COOKIE}>无（不使用 Cookie）</SelectItem>
                      {COOKIE_OPTIONS.map((option) => (
                        <SelectItem key={option.value} value={option.value}>
                          {option.label}
                        </SelectItem>
                      ))}
                    </SelectContent>
                  </Select>
                  <p className="text-xs text-muted-foreground">用于访问需要登录的内容。</p>
                </div>

                <div className="space-y-2">
                  <Label htmlFor="concurrency">并发数</Label>
                  <Input
                    id="concurrency"
                    type="number"
                    min={1}
                    max={16}
                    step={1}
                    value={concurrency}
                    onChange={(event) => setConcurrency(event.target.value)}
                  />
                  <p className="text-xs text-muted-foreground">同时进行的下载任务数量。</p>
                </div>
              </div>

              <div className="space-y-2">
                <Label htmlFor="proxy">代理</Label>
                <Input
                  id="proxy"
                  value={proxy}
                  onChange={(event) => setProxy(event.target.value)}
                  placeholder="http://127.0.0.1:7890"
                  className="font-mono text-xs"
                  spellCheck={false}
                />
                <p className="text-xs text-muted-foreground">留空表示直连。</p>
              </div>

              <div className="space-y-2">
                <Label htmlFor="impersonate-select">浏览器指纹伪装</Label>
                <Select value={impersonate} onValueChange={setImpersonate}>
                  <SelectTrigger id="impersonate-select" className="w-full sm:max-w-xs">
                    <SelectValue placeholder="不启用" />
                  </SelectTrigger>
                  <SelectContent>
                    <SelectItem value={NO_IMP}>无（不启用）</SelectItem>
                    {IMPERSONATE_OPTIONS.map((option) => (
                      <SelectItem key={option.value} value={option.value}>
                        {option.label}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
                <p className="text-xs text-muted-foreground">
                  部分站点有 Cloudflare 防护，遇到 403 再开启。
                </p>
              </div>

              <Separator />

              <div className="space-y-3">
                <div>
                  <p className="text-sm font-medium text-foreground">直链兜底</p>
                  <p className="text-xs text-muted-foreground">
                    提取器全部失败时，前端会把「代取页面挖到的直链」带给后端下载。下面两种方式都是可选增强，默认都关闭。
                  </p>
                </div>

                <div className="flex items-start justify-between gap-4 rounded-lg border border-border bg-muted/30 px-3 py-2.5">
                  <div className="space-y-0.5">
                    <Label htmlFor="browser-bridge" className="cursor-pointer">
                      浏览器桥
                    </Label>
                    <p className="text-xs leading-relaxed text-muted-foreground">
                      开启后，解析失败时会起一个本机 chromium 代开页面，导出 Cookie 并嗅探真实在播的直链。
                      默认关闭；每次解析失败都会多起一个浏览器，约多花十几秒。本机 chromium 解不了
                      Cloudflare 的 JS challenge，CF 站点请用下面的 FlareSolverr。
                    </p>
                  </div>
                  <Switch id="browser-bridge" checked={browserBridge} onCheckedChange={setBrowserBridge} />
                </div>

                <div className="space-y-2">
                  <Label htmlFor="flaresolverr-endpoint">FlareSolverr 地址</Label>
                  <Input
                    id="flaresolverr-endpoint"
                    value={flaresolverrEndpoint}
                    onChange={(event) => setFlaresolverrEndpoint(event.target.value)}
                    placeholder="http://127.0.0.1:8191"
                    className="font-mono text-xs"
                    spellCheck={false}
                    autoComplete="off"
                  />
                  <p className="text-xs leading-relaxed text-muted-foreground">
                    留空或填 none 表示不启用。需要自己用 docker 跑一个 FlareSolverr 容器。
                    它只用来代取页面 HTML 挖直链；cf_clearance 绑定 IP + TLS 指纹，把 cookie 搬给
                    yt-dlp 无效。
                  </p>
                </div>
              </div>

              <div className="flex justify-end">
                <Button onClick={() => void handleSave()} disabled={saving}>
                  {saving ? <LoaderCircle className="animate-spin" /> : <Save />}
                  {saving ? "保存中…" : "保存"}
                </Button>
              </div>
            </>
          )}
        </CardContent>
      </Card>

      <YtDlpCard />
    </div>
  );
}

function YtDlpCard() {
  const { info, loading, checking, updating, error, checkForUpdate, update } = useYtDlp();

  async function handleCheck() {
    const latest = await checkForUpdate();
    if (latest === null) {
      toast.error("检查更新失败");
      return;
    }
    toast.success("已检查最新版本", {
      description: latest.updateAvailable
        ? "有新版本可用：" + (latest.latest ?? "未知")
        : "当前已是最新版本",
    });
  }

  async function handleUpdate() {
    try {
      const result = await update();
      toast.success("内核已更新", {
        description: result.from + " → " + result.to,
      });
    } catch (cause) {
      toast.error("更新失败", { description: errorMessage(cause) });
    }
  }

  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-base">
          <Cpu className="size-4 text-muted-foreground" />
          yt-dlp 内核
          {info !== null ? (
            <Badge variant={info.source === "managed" ? "default" : "secondary"}>
              {info.source === "managed" ? "托管" : "系统"}
            </Badge>
          ) : null}
        </CardTitle>
        <CardDescription>
          内核是独立的 yt-dlp 二进制，热更新后立刻生效，无需重新编译本程序。
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        {error !== null ? <ErrorAlert title="获取内核信息失败" message={error} /> : null}

        {loading && info === null ? (
          <div className="space-y-2">
            <Skeleton className="h-5 w-2/3" />
            <Skeleton className="h-5 w-1/3" />
          </div>
        ) : null}

        {info !== null ? (
          <>
            <dl className="grid grid-cols-1 gap-3 text-sm sm:grid-cols-2">
              <div className="space-y-1">
                <dt className="text-xs text-muted-foreground">当前版本</dt>
                <dd className="font-mono text-xs break-all">{info.version}</dd>
              </div>
              <div className="space-y-1">
                <dt className="text-xs text-muted-foreground">最新版本</dt>
                <dd className="flex items-center gap-2 font-mono text-xs break-all">
                  {info.latest ?? "未检查"}
                  {info.updateAvailable ? (
                    <Badge variant="destructive">
                      <Sparkles />
                      有更新
                    </Badge>
                  ) : null}
                </dd>
              </div>
              <div className="space-y-1 sm:col-span-2">
                <dt className="text-xs text-muted-foreground">二进制路径</dt>
                <dd className="font-mono text-xs break-all">{info.path}</dd>
              </div>
            </dl>

            {info.error !== null && info.error !== "" ? (
              <ErrorAlert title="内核状态有异常" message={info.error} />
            ) : null}

            <Separator />

            <div className="flex flex-wrap items-center gap-2">
              <Button variant="outline" onClick={() => void handleCheck()} disabled={checking || updating}>
                {checking ? <LoaderCircle className="animate-spin" /> : <RefreshCw />}
                {checking ? "检查中…" : "检查更新"}
              </Button>
              <Button onClick={() => void handleUpdate()} disabled={updating || checking}>
                {updating ? <LoaderCircle className="animate-spin" /> : <RotateCw />}
                {updating ? "更新中…" : "立即更新"}
              </Button>
            </div>
          </>
        ) : null}
      </CardContent>
    </Card>
  );
}