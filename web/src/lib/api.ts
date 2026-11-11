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

export const api = {
  health: () => request<Health>("/api/health"),

  getSettings: () => request<Settings>("/api/settings"),

  saveSettings: (patch: Partial<Settings>) =>
    request<SettingsResponse>("/api/settings", {
      method: "PUT",
      body: JSON.stringify(patch),
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