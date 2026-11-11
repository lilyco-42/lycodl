import { useState } from "react";
import { Bot, Download, FolderOpen, Globe, ListVideo, Settings2, Zap } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { Toaster } from "@/components/ui/sonner";
import { DownloadPanel } from "@/components/download-panel";
import { JobsPanel } from "@/components/jobs-panel";
import { FilesPanel } from "@/components/files-panel";
import { SettingsPanel } from "@/components/settings-panel";
import { CommandsPanel } from "@/components/commands-panel";
import { PlatformsPanel } from "@/components/platforms-panel";
import { useHealth } from "@/hooks/useHealth";

const TABS = [
  { value: "download", label: "下载", icon: Download },
  { value: "jobs", label: "任务", icon: ListVideo },
  { value: "files", label: "文件", icon: FolderOpen },
  { value: "settings", label: "设置", icon: Settings2 },
  { value: "platforms", label: "平台", icon: Globe },
  { value: "commands", label: "命令（AI）", icon: Bot },
] as const;

function App() {
  const [tab, setTab] = useState<string>("download");
  /** 平台页点示例链接后回填给下载页；置空再设值可让同一个链接重复触发 */
  const [exampleUrl, setExampleUrl] = useState<string>("");
  const { health } = useHealth();

  function handleUseExample(url: string) {
    setTab("download");
    if (exampleUrl === url) {
      setExampleUrl("");
      window.setTimeout(() => setExampleUrl(url), 0);
      return;
    }
    setExampleUrl(url);
  }

  return (
    <div className="min-h-dvh bg-background text-foreground">
      <div className="mx-auto w-full max-w-5xl px-4 pt-8 pb-16 sm:px-6">
        <header className="flex flex-wrap items-center justify-between gap-3 pb-6">
          <div className="flex items-center gap-2.5">
            <span className="flex size-9 items-center justify-center rounded-xl bg-primary text-primary-foreground">
              <Zap className="size-5" />
            </span>
            <div className="leading-tight">
              <h1 className="text-lg font-semibold tracking-tight">lycodl</h1>
              <p className="text-xs text-muted-foreground">视频下载器 · 由 yt-dlp 内核驱动</p>
            </div>
          </div>
          <Badge variant="outline" className="h-6 gap-1.5 px-2.5 font-mono text-xs">
            yt-dlp {health?.ytdlpVersion ?? "未就绪"}
          </Badge>
        </header>

        <Tabs value={tab} onValueChange={setTab} className="gap-5">
          <TabsList className="h-9 w-full justify-start gap-1 overflow-x-auto p-1 sm:w-fit">
            {TABS.map((item) => {
              const Icon = item.icon;
              return (
                <TabsTrigger key={item.value} value={item.value} className="px-3">
                  <Icon />
                  {item.label}
                </TabsTrigger>
              );
            })}
          </TabsList>

          <TabsContent value="download">
            <DownloadPanel onJobCreated={() => setTab("jobs")} exampleUrl={exampleUrl} />
          </TabsContent>
          <TabsContent value="jobs">
            <JobsPanel />
          </TabsContent>
          <TabsContent value="files">
            <FilesPanel />
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

        <footer className="mt-12 flex flex-wrap items-center justify-between gap-2 border-t border-border pt-4 text-xs text-muted-foreground">
          <span>
            {health !== null
              ? health.name + " v" + health.version + " · " + health.webMode
              : "lycodl"}
          </span>
          <span>内核为独立二进制，可热更新</span>
        </footer>
      </div>
      <Toaster position="top-center" theme="dark" richColors closeButton />
    </div>
  );
}

export default App;