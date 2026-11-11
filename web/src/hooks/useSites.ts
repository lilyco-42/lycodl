import { useEffect, useRef, useState } from "react";
import { api, errorMessage, type SitesResponse } from "@/lib/api";

/** 主流平台清单 + 已记录的自定义解析策略。 */
export function useSites() {
  const [sites, setSites] = useState<SitesResponse | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    api
      .sites()
      .then((data) => {
        if (mountedRef.current) {
          setSites(data);
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

  return { sites, error };
}
