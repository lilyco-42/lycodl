import { useEffect, useRef, useState } from "react";
import { api, errorMessage, type Capabilities } from "@/lib/api";

/** 运行环境自检（ffmpeg / JS 运行时 / 指纹伪装 / Cookie / 内核 / 提取器 / 磁盘）。 */
export function useCaps() {
  const [caps, setCaps] = useState<Capabilities | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    api
      .caps()
      .then((data) => {
        if (mountedRef.current) {
          setCaps(data);
        }
      })
      .catch((cause: unknown) => {
        if (mountedRef.current) {
          setError(errorMessage(cause));
        }
      });
    return () => {
      mountedRef.current = false;
    };
  }, []);

  return { caps, error };
}
