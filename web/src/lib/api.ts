/**
 * lycodl HTTP 契约的类型定义与 fetch 封装。
 * 所有请求同源，基址即当前页面 origin。
 */

export type WebMode = "embedded" | "disk";

export interface Health {
  name: string;
  version: string;
  ytdlpVersion: string | null;
  ytdlpPath: string | null;
  webMode: WebMode;
  webBuildId?: string | null;
  cdpReady?: boolean;
  cdpDetail?: string | null;
  cdpUrl?: string | null;
}

export interface Settings {
  outputDir: string;
  cookiesFromBrowser: string | null;
  proxy: string | null;
  concurrency: number;
  impersonate: string | null;
  /** 浏览器桥：起本机 chromium 代开页面，导 Cookie / 嗅探直链 */
  browserBridge: boolean;
  /** FlareSolverr 地址，null 表示不启用 */
  flaresolverrEndpoint: string | null;
  /** 下载完成后自动发布 */
  autoPublish: boolean;
  /** 发布 driver 命令；空串使用内置默认 driver */
  publishDriver: string;
  /** 目的地：quark / bilibili / douyin / xhs / kuaishou / youtube / x */
  publishTargets: string[];
  publishQuarkDir: string;
  publishShareUrl: string | null;
  publishSharePasscode: string | null;
  publishCdpUrl: string;
  publishPlatformCommand: string | null;
  /** 只演练、不真正外发 */
  publishDryRun: boolean;
}

export interface PublishLedgerEntry {
  file: string;
  size: number;
  dest: string;
  status: string;
  detail: string;
  url: string | null;
  at: number;
  dryRun: boolean;
  /** 手动发布任务 id；自动发布沿用下载任务 id，测试入口为 null */
  taskId?: string | null;
  /** auto / local / upload / one-click / batch / test */
  source?: string;
}

export interface PublishLedgerResponse {
  entries: PublishLedgerEntry[];
}

export interface PublishTestResponse {
  ok: true;
  file: string;
  results: PublishResult[];
}

export interface PublishResult {
  dest: string;
  status: string;
  detail: string;
  url: string | null;
  at: number;
  /** 批量发布：结果所属的成品文件名；普通任务可为空 */
  fileName?: string;
}

/** 发布前只读预检：可用接口 / 登录态 / 我的账号已发布状态 */
export interface PublishCapability {
  dest: string;
  available: boolean;
  published: boolean;
  reason: string;
  detail: string;
  source: string;
}

export interface PublishProbeResponse {
  ok: true;
  file: string;
  size: number;
  targets: string[];
  selectable: string[];
  skipped: PublishCapability[];
  caps: PublishCapability[];
  errors: string[];
  checkedAt: number;
  force: boolean;
}

/** 本机文件手动发布表单的选项；也用于扩展测试发布接口 */
export interface PublishSubmission {
  title?: string;
  targets: string[];
  dryRun: boolean;
  force: boolean;
  /** 一键发布：提交时由服务端重新筛选不可用 / 已发布目的地 */
  filter?: boolean;
}

export interface PublishTaskResponse {
  ok: true;
  jobId: string;
  file: string;
  size?: number;
  filter?: PublishProbeResponse | null;
}

export interface PublishBatchResponse {
  ok: true;
  jobId: string;
  dryRun: boolean;
  fileCount: number;
  targets: string[];
}

export interface SettingsResponse {
  ok: true;
  settings: Settings;
  /** 保存时正在跑的任务数（调小并发后能看到「当前仍有几个在跑」） */
  running?: number;
  /** 后端当前实际生效的并发上限 */
  effectiveConcurrency?: number;
}

export type YtdlpSource = "managed" | "system";

export interface YtdlpInfo {
  path: string;
  version: string;
  source: YtdlpSource;
  latest: string | null;
  updateAvailable: boolean;
  error: string | null;
}

export interface YtdlpUpdateResult {
  ok: true;
  from: string;
  to: string;
  path: string;
}

export interface FormatInfo {
  formatId: string;
  ext: string;
  resolution: string | null;
  height: number | null;
  vcodec: string | null;
  acodec: string | null;
  filesize: number | null;
  tbr: number | null;
  note: string | null;
}

export interface PlaylistEntry {
  title: string;
  url: string;
  duration: number | null;
}

export interface VideoInfo {
  id: string;
  title: string;
  uploader: string | null;
  duration: number | null;
  thumbnail: string | null;
  webpageUrl: string;
  extractor: string;
  isPlaylist: boolean;
  entryCount: number;
  entries: PlaylistEntry[];
  formats: FormatInfo[];
}

export type CapSource = "managed" | "system";
export interface CapKernel { version: string; path: string; source: CapSource }
export interface CapTool { found: boolean; name: string | null; path: string | null; version: string | null }
export interface CapImpersonate { available: boolean; targets: string[]; error: string | null }
export interface CapBrowser { browser: string; label: string; path: string }
export interface CapCookies { detected: CapBrowser[] }
export interface CapExtractors { total: number; lines: number; broken: number; cachedAt: number | null; error: string | null }
export interface CapDisk { path: string; freeBytes: number | null; totalBytes: number | null }
export interface Capabilities {
  kernel: CapKernel | null;
  jsRuntime: CapTool;
  ffmpeg: CapTool;
  impersonate: CapImpersonate;
  cookies: CapCookies;
  extractors: CapExtractors;
  disk: CapDisk;
  warnings: string[];
}
export type SiteCategory = "video" | "music" | "live" | "social" | "anime" | "other";
export type SiteStatus = "ready" | "needs-cookies" | "needs-login" | "experimental";
export interface SiteInfo { key: string; name: string; category: SiteCategory; domains: string[]; status: SiteStatus; tips: string[]; sampleUrl: string }
export interface SitesResponse { curated: SiteInfo[]; matched: string | null; strategies: Record<string, string> }
export interface ExtractorItem { name: string; broken: boolean }
export interface ExtractorsResponse { total: number; items: ExtractorItem[]; query: string; truncated: boolean }
export interface ProbeAttempt { name: string; label: string; ok: boolean; error: string | null }
export interface ProbeStrategy { applied: string; label: string; attempts: ProbeAttempt[] }

/** 浏览器桥 / FlareSolverr 代取页面挖到的真实媒体直链。 */
export interface DirectMedia {
  url: string;
  referer: string | null;
  userAgent: string | null;
  source: "browser" | "flaresolverr";
}

export interface ProbeResponse {
  ok: true;
  info: VideoInfo;
  site: SiteInfo | null;
  strategy: ProbeStrategy | null;
  /** extractor 全挂时由浏览器桥 / FlareSolverr 挖到的直链；老后端可能没有该字段 */
  direct: DirectMedia | null;
}

export type DownloadMode = "video" | "audio" | "custom";
export type DownloadQuality = "best" | "2160" | "1440" | "1080" | "720" | "480";

export interface DownloadRequest {
  url: string;
  mode: DownloadMode;
  quality: DownloadQuality;
  formatId: string | null;
  playlist: boolean;
  /** 直链兜底：解析结果必须原样回传，否则后端会退回早已失败的 extractor */
  direct?: DirectMedia | null;
  /** 直链兜底时的标题，用 info.title */
  directTitle?: string | null;
}

export interface CreateDownloadResponse {
  ok: true;
  jobId: string;
}

export type JobStatus = "queued" | "running" | "done" | "failed" | "cancelled";

export interface Job {
  id: string;
  url: string;
  title: string;
  mode: string;
  status: JobStatus;
  percent: number;
  downloadedBytes: number;
  totalBytes: number | null;
  speed: number | null;
  eta: number | null;
  output: string | null;
  fileName: string | null;
  error: string | null;
  /** 播放列表里当前正在下第几项（单视频为 null） */
  playlistIndex: number | null;
  /** 播放列表总项数（单视频为 null） */
  playlistCount: number | null;
  /** 非致命告警：如播放列表里被跳过的条目（任务仍算成功） */
  warnings: string[];
  createdAt: number;
  endedAt: number | null;
  /** 以下三项由发布任务返回；旧后端可能没有 */
  jobKind?: string;
  publishTargets?: string[];
  publishResults?: PublishResult[];
  /** 批量发布计数；旧任务 / 普通任务可没有 */
  batchTotal?: number;
  batchProcessed?: number;
  batchSucceeded?: number;
  batchFailed?: number;
  batchSkipped?: number;
}

export interface JobsResponse {
  jobs: Job[];
}

export interface JobLogsResponse {
  id: string;
  /** 该任务最近若干行原始输出（yt-dlp stdout/stderr 提炼后的日志尾） */
  logs: string[];
}

export interface FileItem {
  name: string;
  size: number;
  modified: number;
  url: string;
}

export interface FilesResponse {
  files: FileItem[];
}

export type CommandSafety = "ReadOnly" | "Confirm" | "Token" | "NeverAuto";

export type CommandArgKindType = "flag" | "text" | "number" | "enum" | "path" | "list";

export interface CommandArgKind {
  type: CommandArgKindType;
  values?: string[];
}

export interface CommandArg {
  name: string;
  about: string;
  required: boolean;
  kind: CommandArgKind;
}

export interface CommandInfo {
  name: string;
  about: string;
  safety: CommandSafety;
  args: CommandArg[];
}

export interface CommandsResponse {
  commands: CommandInfo[];
}

/** 非 2xx 或网络失败时抛出，message 即后端返回的 error 原文。 */
export class ApiError extends Error {
  readonly status: number;

  constructor(message: string, status: number) {
    super(message);
    this.name = "ApiError";
    this.status = status;
  }
}

export function errorMessage(error: unknown): string {
  if (error instanceof Error) {
    return error.message;
  }
  if (typeof error === "string") {
    return error;
  }
  return "未知错误";
}

export function isRecord(value: unknown): value is Record<string, unknown> {
  return typeof value === "object" && value !== null && !Array.isArray(value);
}

export function isJob(value: unknown): value is Job {
  return (
    isRecord(value) &&
    typeof value.id === "string" &&
    typeof value.url === "string" &&
    typeof value.status === "string"
  );
}

async function readErrorPayload(response: Response): Promise<string> {
  const fallback =
    "HTTP " + response.status + (response.statusText ? " " + response.statusText : "");
  try {
    const body: unknown = await response.json();
    if (isRecord(body) && typeof body.error === "string" && body.error.trim() !== "") {
      return body.error;
    }
    return fallback;
  } catch {
    return fallback;
  }
}

async function request<T>(path: string, init?: RequestInit): Promise<T> {
  let response: Response;
  try {
    response = await fetch(path, {
      ...init,
      headers: {
        Accept: "application/json",
        ...(init?.body ? { "Content-Type": "application/json" } : {}),
        ...(init?.headers ?? {}),
      },
    });
  } catch (cause) {
    throw new ApiError(
      cause instanceof Error ? "网络请求失败：" + cause.message : "网络请求失败",
      0,
    );
  }

  if (!response.ok) {
    throw new ApiError(await readErrorPayload(response), response.status);
  }

  if (response.status === 204) {
    return undefined as T;
  }

  const body: unknown = await response.json();
  return body as T;
}

/** 浏览器扩展用的访问令牌（GET 取当前值；POST 轮换） */
export interface PluginToken {
  token: string;
  allowedOrigins: string[];
}

export const api = {
  health: () => request<Health>("/api/health"),

  getSettings: () => request<Settings>("/api/settings"),

  saveSettings: (patch: Partial<Settings>) =>
    request<SettingsResponse>("/api/settings", {
      method: "PUT",
      body: JSON.stringify(patch),
    }),

  // 浏览器扩展令牌：GET 取当前值，POST 轮换（会让已配置的扩展失效）
  pluginToken: () => request<PluginToken>("/api/plugin-token"),

  rotatePluginToken: () =>
    request<PluginToken>("/api/plugin-token", {
      method: "POST",
      body: JSON.stringify({}),
    }),

  ytdlp: (check = false) => request<YtdlpInfo>(check ? "/api/ytdlp?check=1" : "/api/ytdlp"),

  updateYtdlp: () =>
    request<YtdlpUpdateResult>("/api/ytdlp/update", {
      method: "POST",
      body: JSON.stringify({}),
    }),

  probe: (url: string) =>
    request<ProbeResponse>("/api/probe", {
      method: "POST",
      body: JSON.stringify({ url }),
    }),

  createDownload: (payload: DownloadRequest) =>
    request<CreateDownloadResponse>("/api/download", {
      method: "POST",
      body: JSON.stringify(payload),
    }),

  listJobs: () => request<JobsResponse>("/api/jobs"),

  // 重试：后端复用原任务的 url / 模式 / 画质 / 格式 ID / 列表开关，直接排一个新任务
  retryJob: (id: string) =>
    request<CreateDownloadResponse>("/api/jobs/" + encodeURIComponent(id) + "/retry", {
      method: "POST",
      body: JSON.stringify({}),
    }),

  jobLogs: (id: string) =>
    request<JobLogsResponse>("/api/jobs/" + encodeURIComponent(id) + "/logs"),

  /** 最近发布记录，新的在前 */
  publishLedger: (limit = 50) =>
    request<PublishLedgerResponse>("/api/publish/ledger?limit=" + limit),

  /** 只读筛选：不上传、不发帖、不创建平台作品 */
  publishCapabilities: (payload: {
    file: string;
    targets?: string[];
    force?: boolean;
  }) =>
    request<PublishProbeResponse>("/api/publish/capabilities", {
      method: "POST",
      body: JSON.stringify(payload),
    }),

  /** 用一个已有文件立刻试跑一次发布（会按 ledger 跳过成功过的目的地） */
  publishTest: (file: string, options: Partial<PublishSubmission> = {}) =>
    request<PublishTestResponse>("/api/publish/test", {
      method: "POST",
      body: JSON.stringify({ file, ...options }),
    }),

  /** 文件页「全部发布」：后端扫描输出目录，逐文件预检与去重后建一个批处理任务 */
  publishAll: (payload: {
    targets?: string[];
    dryRun: boolean;
    force?: boolean;
    riskAck?: boolean;
  }) =>
    request<PublishBatchResponse>("/api/publish/all", {
      method: "POST",
      body: JSON.stringify(payload),
    }),

  /** 对运行 lycodl 的本机已有路径建一个发布任务 */
  publishLocal: (payload: PublishSubmission & { file: string }) =>
    request<PublishTaskResponse>("/api/publish/local", {
      method: "POST",
      body: JSON.stringify(payload),
    }),

  /** 浏览器选中的文件以 raw body 流式上传到输出目录，再建发布任务 */
  publishUpload: (
    file: File,
    options: PublishSubmission,
    onProgress?: (percent: number) => void,
  ): Promise<PublishTaskResponse> =>
    new Promise<PublishTaskResponse>((resolve, reject) => {
      const params = new URLSearchParams({
        name: file.name,
        targets: options.targets.join(","),
        dryRun: String(options.dryRun),
        force: String(options.force),
        filter: String(options.filter === true),
      });
      const title = (options.title ?? "").trim();
      if (title !== "") params.set("title", title);
      const xhr = new XMLHttpRequest();
      xhr.open("POST", "/api/publish/upload?" + params.toString());
      xhr.setRequestHeader("Content-Type", "application/octet-stream");
      xhr.setRequestHeader("Accept", "application/json");
      xhr.onload = () => {
        let body: unknown = null;
        try { body = JSON.parse(xhr.responseText); } catch { /* 后端错误体可能不是 JSON */ }
        if (xhr.status >= 200 && xhr.status < 300) {
          resolve(body as PublishTaskResponse);
          return;
        }
        const msg = isRecord(body) && typeof body.error === "string" && body.error.trim() !== ""
          ? body.error
          : "HTTP " + xhr.status + (xhr.statusText !== "" ? " " + xhr.statusText : "");
        reject(new ApiError(msg, xhr.status));
      };
      xhr.onerror = () => reject(new ApiError("网络请求失败或上传中断", 0));
      if (onProgress !== undefined) {
        xhr.upload.onprogress = (event) => {
          if (event.lengthComputable && event.total > 0) {
            onProgress(Math.min(100, Math.round((event.loaded / event.total) * 100)));
          }
        };
      }
      xhr.send(file);
    }),

  cancelJob: (id: string) =>
    request<{ ok: true }>("/api/jobs/" + encodeURIComponent(id) + "/cancel", {
      method: "POST",
      body: JSON.stringify({}),
    }),

  listFiles: () => request<FilesResponse>("/api/files"),

  // 让系统文件管理器定位到该文件；失败时前端退回「复制路径」
  reveal: (path: string) =>
    request<{ ok: true; via: "filemanager" | "directory"; dir: string }>(
      "/api/reveal",
      { method: "POST", body: JSON.stringify({ path }) },
    ),

  listCommands: () => request<CommandsResponse>("/api/commands"),

  caps: () => request<Capabilities>("/api/caps"),

  sites: () => request<SitesResponse>("/api/sites"),

  extractors: (q: string) =>
    request<ExtractorsResponse>(
      "/api/extractors" + (q.trim() === "" ? "" : "?q=" + encodeURIComponent(q.trim())),
    ),
};

export function jobEventsUrl(id: string): string {
  return "/api/jobs/" + encodeURIComponent(id) + "/events";
}