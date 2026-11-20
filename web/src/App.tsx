import { useState } from "react";
import { Bot, Download, FolderOpen, Globe, ListVideo, RefreshCw, Settings2, UploadCloud } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Separator } from "@/components/ui/separator";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/sonner";
import { AppLogo } from "@/components/app-logo";
import { DownloadPanel } from "@/components/download-panel";
import { JobsPanel } from "@/components/jobs-panel";
import { FilesPanel } from "@/components/files-panel";
import { PublishPanel } from "@/components/publish-panel";
import { SettingsPanel } from "@/components/settings-panel";
import { CommandsPanel } from "@/components/commands-panel";
import { PlatformsPanel } from "@/components/platforms-panel";
import { useHealth } from "@/hooks/useHealth";

const TABS = [
  { value: "download", label: "下载", icon: Download },
  { value: "jobs", label: "任务", icon: ListVideo },
  { value: "files", label: "文件", icon: FolderOpen },
  { value: "publish", label: "发布", icon: UploadCloud },
  { value: "settings", label: "设置", icon: Settings2 },
  { value: "platforms", label: "平台", icon: Globe },
  { value: "commands", label: "命令（AI）", icon: Bot },
] as const;

function App() {
  const [tab, setTab] = useState<string>("download");
  /** 平台页点示例链接后回填给下载页；置空再设值可让同一个链接重复触发 */
  const [exampleUrl, setExampleUrl] = useState<string>("");
  /** 文件页点「发布」时预填本机路径；token 递增保证同一路径可重复触发 */
  const [publishPath, setPublishPath] = useState<string>("");
  const [publishToken, setPublishToken] = useState(0);
  const { health, error, updateAvailable } = useHealth();

  function handleUseExample(url: string) {
    setTab("download");
    if (exampleUrl === url) {
      setExampleUrl("");
      window.setTimeout(() => setExampleUrl(url), 0);
      return;
    }
    setExampleUrl(url);
  }

  const online = health !== null && error === null;

  return (
    <div className="min-h-dvh text-foreground">
      <header className="sticky top-0 z-30 border-b border-border/70 bg-background/70 backdrop-blur-xl supports-[backdrop-filter]:bg-background/55">
        <div className="mx-auto flex w-full max-w-6xl flex-wrap items-center justify-between gap-3 px-4 py-3 sm:px-6">
          <div className="flex items-center gap-3">
            <AppLogo className="size-10 rounded-xl shadow-lg shadow-brand/20" />
            <div className="leading-tight">
              <h1 className="brand-text text-lg font-semibold tracking-tight">lycodl</h1>
              <p className="text-xs text-muted-foreground">视频下载 · 本机发布 · yt-dlp 内核</p>
            </div>
          </div>
          <div className="flex items-center gap-2">
            <Badge
              variant={online ? "default" : "outline"}
              className={
                online
                  ? "h-6 gap-1.5 border-emerald-500/40 bg-emerald-500/15 px-2.5 text-xs text-emerald-300"
                  : "h-6 gap-1.5 px-2.5 text-xs"
              }
            >
              <span
                className={
                  online
                    ? "size-1.5 rounded-full bg-emerald-400"
                    : "size-1.5 rounded-full bg-muted-foreground"
                }
              />
              {online ? "服务在线" : "连接中"}
            </Badge>
            <Badge variant="outline" className="h-6 gap-1.5 px-2.5 font-mono text-xs">
              yt-dlp {health?.ytdlpVersion ?? "未就绪"}
            </Badge>
            {health?.cdpReady === true && (
              <Badge
                variant="outline"
                className="h-6 gap-1.5 border-sky-500/40 bg-sky-500/10 px-2.5 text-xs text-sky-300"
                title={health.cdpDetail ?? undefined}
              >
                <span className="size-1.5 rounded-full bg-sky-400" />
                发布浏览器在线
              </Badge>
            )}
            {health?.cdpReady === false && (
              <Badge
                variant="destructive"
                className="h-6 gap-1.5 px-2.5 text-xs"
                title={health.cdpDetail ?? undefined}
              >
                <span className="size-1.5 rounded-full bg-destructive" />
                发布浏览器离线
              </Badge>
            )}
            {health?.webBuildId && (
              <Badge variant="outline" className="h-6 gap-1.5 px-2.5 font-mono text-[10px]">
                UI {health.webBuildId.slice(0, 8)}
              </Badge>
            )}
            {updateAvailable && (
              <Badge className="h-6 gap-1.5 px-2.5 text-xs">
                <RefreshCw className="h-3 w-3 animate-spin" /> 检测到新版，正在刷新…
              </Badge>
            )}
          </div>
        </div>
      </header>

      <main className="mx-auto w-full max-w-6xl px-4 pt-6 pb-16 sm:px-6">
        <Tabs value={tab} onValueChange={setTab} className="gap-6">
          <div className="-mx-4 overflow-x-auto px-4 sm:mx-0 sm:px-0">
            <TabsList
              variant="line"
              className="h-10 w-full justify-start gap-1 border-b border-border/60 pb-0 sm:w-fit"
            >
              {TABS.map((item) => {
                const Icon = item.icon;
                const active = tab === item.value;
                return (
                  <TabsTrigger
                    key={item.value}
                    value={item.value}
                    className={
                      active
                        ? "h-10 gap-2 px-3 text-brand after:bg-brand"
                        : "h-10 gap-2 px-3 text-muted-foreground after:bg-brand"
                    }
                  >
                    <Icon />
                    {item.label}
                  </TabsTrigger>
                );
              })}
            </TabsList>
          </div>

          <TabsContent value="download">
            <DownloadPanel onJobCreated={() => setTab("jobs")} exampleUrl={exampleUrl} />
          </TabsContent>
          <TabsContent value="jobs">
            <JobsPanel />
          </TabsContent>
          <TabsContent value="files">
            <FilesPanel
              onPublish={(path) => {
                setPublishPath(path);
                setPublishToken((token) => token + 1);
                setTab("publish");
              }}
              onTaskCreated={() => setTab("jobs")}
            />
          </TabsContent>
          <TabsContent value="publish">
            <PublishPanel
              prefillPath={publishPath !== "" ? publishPath : undefined}
              prefillToken={publishToken}
              cdpReady={health?.cdpReady ?? null}
              cdpDetail={health?.cdpDetail ?? null}
              onTaskCreated={() => setTab("jobs")}
            />
          </TabsContent>
          <TabsContent value="settings">
            <SettingsPanel />
          </TabsContent>
          <TabsContent value="platforms">
            <PlatformsPanel onUseExample={handleUseExample} />
          </TabsContent>
          <TabsContent value="commands">
            <CommandsPanel />
          </TabsContent>
        </Tabs>

        <Separator className="mt-12" />
        <footer className="flex flex-wrap items-center justify-between gap-2 pt-4 text-xs text-muted-foreground">
          <span className="font-mono">
            {health !== null
              ? health.name + " v" + health.version + " · " + health.webMode + (health.webBuildId ? " · UI " + health.webBuildId.slice(0, 8) : "")
              : "lycodl"}
          </span>
          <span>内核为独立二进制，可热更新</span>
        </footer>
      </main>
      <Toaster position="top-center" theme="dark" richColors closeButton />
    </div>
  );
}

export default App;
