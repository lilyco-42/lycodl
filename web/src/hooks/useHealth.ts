import { useEffect, useRef, useState } from "react";
import { api, errorMessage, type Health } from "@/lib/api";

const POLL_MS = 15_000;

/** 服务信息 + 前端版本检测；部署新二进制后旧标签自动刷新。 */
export function useHealth() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [updateAvailable, setUpdateAvailable] = useState(false);
  const buildRef = useRef<string | null>(null);

  useEffect(() => {
    let cancelled = false;
    let timer: ReturnType<typeof setInterval> | undefined;
    let reloadTimer: ReturnType<typeof setTimeout> | undefined;

    const check = async () => {
      try {
        const next = await api.health();
        if (cancelled) return;
        setHealth(next);
        setError(null);
        const id = next.webBuildId ?? null;
        if (id && buildRef.current === null) {
          buildRef.current = id;
        } else if (id && buildRef.current && id !== buildRef.current && !cancelled) {
          setUpdateAvailable(true);
          if (timer) clearInterval(timer);
          if (!reloadTimer) {
            reloadTimer = setTimeout(() => window.location.reload(), 1200);
          }
        }
      } catch (cause: unknown) {
        if (!cancelled) {
          setError(errorMessage(cause));
          setHealth(null);
        }
      }
    };

    void check();
    timer = setInterval(() => void check(), POLL_MS);
    return () => {
      cancelled = true;
      if (timer) clearInterval(timer);
      if (reloadTimer) clearTimeout(reloadTimer);
    };
  }, []);

  return { health, error, updateAvailable };
}
