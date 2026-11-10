import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage, type FileItem } from "@/lib/api";

/** 输出目录里的文件列表；refresh 返回是否成功。 */
export function useFiles() {
  const [files, setFiles] = useState<FileItem[]>([]);
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
      const data = await api.listFiles();
      if (mountedRef.current) {
        setFiles(data.files);
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

  return { files, loading, error, refresh };
}
