import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage, type YtdlpInfo } from "@/lib/api";

/**
 * 内核版本信息：加载、检查更新、立即更新。
 * load 返回最新信息；失败时返回 null（错误同时写入 error 状态）。
 */
export function useYtDlp() {
  const [info, setInfo] = useState<YtdlpInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [checking, setChecking] = useState(false);
  const [updating, setUpdating] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const load = useCallback(async (check: boolean): Promise<YtdlpInfo | null> => {
    if (check) {
      setChecking(true);
    } else {
      setLoading(true);
    }
    setError(null);
    try {
      const data = await api.ytdlp(check);
      if (mountedRef.current) {
        setInfo(data);
      }
      return data;
    } catch (cause: unknown) {
      if (mountedRef.current) {
        setError(errorMessage(cause));
      }
      return null;
    } finally {
      if (mountedRef.current) {
        setLoading(false);
        setChecking(false);
      }
    }
  }, []);

  useEffect(() => {
    void load(false);
  }, [load]);

  /** 联网查询最新版（慢，5~10s）。 */
  const checkForUpdate = useCallback(() => load(true), [load]);

  /** 执行热更新，成功返回 from/to。 */
  const update = useCallback(async () => {
    setUpdating(true);
    setError(null);
    try {
      const result = await api.updateYtdlp();
      const fresh = await api.ytdlp(false);
      if (mountedRef.current) {
        setInfo(fresh);
      }
      return result;
    } finally {
      if (mountedRef.current) {
        setUpdating(false);
      }
    }
  }, []);

  const refresh = useCallback(() => load(false), [load]);

  return { info, loading, checking, updating, error, refresh, checkForUpdate, update };
}
