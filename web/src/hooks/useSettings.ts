import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage, type Settings, type SettingsResponse } from "@/lib/api";

/** 设置读写。 */
export function useSettings() {
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const load = useCallback(async () => {
    setLoading(true);
    try {
      const data = await api.getSettings();
      if (mountedRef.current) {
        setSettings(data);
        setError(null);
      }
    } catch (cause: unknown) {
      if (mountedRef.current) {
        setError(errorMessage(cause));
      }
    } finally {
      if (mountedRef.current) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    void load();
  }, [load]);

  const save = useCallback(async (patch: Partial<Settings>): Promise<SettingsResponse> => {
    setSaving(true);
    try {
      const result = await api.saveSettings(patch);
      if (mountedRef.current) {
        setSettings(result.settings);
        setError(null);
      }
      return result;
    } finally {
      if (mountedRef.current) {
        setSaving(false);
      }
    }
  }, []);

  return { settings, loading, saving, error, save, reload: load };
}
