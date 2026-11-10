import { useEffect, useRef, useState } from "react";
import { api, errorMessage, type Health } from "@/lib/api";

/** 服务信息，用于页头徽章。 */
export function useHealth() {
  const [health, setHealth] = useState<Health | null>(null);
  const [error, setError] = useState<string | null>(null);
  const mountedRef = useRef(true);

  useEffect(() => {
    mountedRef.current = true;
    api
      .health()
      .then((data) => {
        if (mountedRef.current) {
          setHealth(data);
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

  return { health, error };
}
