import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage, type CommandInfo } from "@/lib/api";

/** lilyco 框架注册表里的命令列表；refresh 返回是否成功。 */
export function useCommands() {
  const [commands, setCommands] = useState<CommandInfo[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    return () => {
      mountedRef.current = false;
    };
  }, []);

  const refresh = useCallback(async (): Promise<boolean> => {
    setLoading(true);
    try {
      const data = await api.listCommands();
      if (mountedRef.current) {
        setCommands(data.commands);
        setError(null);
      }
      return true;
    } catch (cause: unknown) {
      if (mountedRef.current) {
        setError(errorMessage(cause));
      }
      return false;
    } finally {
      if (mountedRef.current) {
        setLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  return { commands, loading, error, refresh };
}
