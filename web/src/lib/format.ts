/** 人类可读的格式化工具：体积、速度、时长、剩余时间、时间戳。 */

const BYTE_UNITS = ["B", "KB", "MB", "GB", "TB", "PB"];

export function formatBytes(bytes: number | null | undefined, fractionDigits = 1): string {
  if (bytes === null || bytes === undefined || !Number.isFinite(bytes) || bytes < 0) {
    return "—";
  }
  if (bytes === 0) {
    return "0 B";
  }
  let value = bytes;
  let unit = 0;
  while (value >= 1024 && unit < BYTE_UNITS.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 ? 0 : fractionDigits;
  return value.toFixed(digits) + " " + BYTE_UNITS[unit];
}

export function formatSpeed(bytesPerSecond: number | null | undefined): string {
  if (
    bytesPerSecond === null ||
    bytesPerSecond === undefined ||
    !Number.isFinite(bytesPerSecond) ||
    bytesPerSecond <= 0
  ) {
    return "—";
  }
  return formatBytes(bytesPerSecond) + "/s";
}

export function formatDuration(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) {
    return "—";
  }
  const total = Math.round(seconds);
  const hours = Math.floor(total / 3600);
  const minutes = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  return hours > 0
    ? hours + ":" + pad(minutes) + ":" + pad(secs)
    : pad(minutes) + ":" + pad(secs);
}

function pad(value: number): string {
  return value.toString().padStart(2, "0");
}

export function formatEta(seconds: number | null | undefined): string {
  if (seconds === null || seconds === undefined || !Number.isFinite(seconds) || seconds < 0) {
    return "—";
  }
  const total = Math.round(seconds);
  if (total < 60) {
    return total + " 秒";
  }
  const minutes = Math.floor(total / 60);
  const secs = total % 60;
  if (minutes < 60) {
    return secs > 0 ? minutes + " 分 " + secs + " 秒" : minutes + " 分";
  }
  const hours = Math.floor(minutes / 60);
  const restMinutes = minutes % 60;
  return restMinutes > 0 ? hours + " 小时 " + restMinutes + " 分" : hours + " 小时";
}

/** 后端时间戳可能是秒或毫秒，统一换算成毫秒。 */
export function toMillis(value: number): number {
  return value > 0 && value < 1_000_000_000_000 ? value * 1000 : value;
}

export function formatDateTime(value: number | null | undefined): string {
  if (value === null || value === undefined || !Number.isFinite(value) || value <= 0) {
    return "—";
  }
  const date = new Date(toMillis(value));
  if (Number.isNaN(date.getTime())) {
    return "—";
  }
  return new Intl.DateTimeFormat("zh-CN", {
    year: "numeric",
    month: "2-digit",
    day: "2-digit",
    hour: "2-digit",
    minute: "2-digit",
    hour12: false,
  }).format(date);
}

export function clampPercent(percent: number | null | undefined): number {
  if (percent === null || percent === undefined || !Number.isFinite(percent)) {
    return 0;
  }
  return Math.min(100, Math.max(0, percent));
}

export function formatPercent(percent: number | null | undefined): string {
  return clampPercent(percent).toFixed(1) + "%";
}
