import { useCallback, useEffect, useRef, useState } from "react";
import { api, errorMessage, isJob, jobEventsUrl, type Job } from "@/lib/api";

const RECONNECT_DELAY_MS = 3000;

function isActiveStatus(status: Job["status"]): boolean {
  return status === "queued" || status === "running";
}

/**
 * 任务列表状态：挂载时拉全量，随后为每个排队 / 运行中的任务订阅 SSE 实时增量。
 */
export function useJobs() {
  const [jobs, setJobs] = useState<Job[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [refreshToken, setRefreshToken] = useState(0);
  const [reconnectToken, setReconnectToken] = useState(0);

  const sourcesRef = useRef<Map<string, EventSource>>(new Map());
  const reconnectTimersRef = useRef<Map<string, number>>(new Map());

  const mergeJob = useCallback((incoming: Job) => {
    setJobs((prev) => {
      const index = prev.findIndex((job) => job.id === incoming.id);
      if (index === -1) {
        return [incoming, ...prev];
      }
      const next = prev.slice();
      next[index] = { ...prev[index], ...incoming };
      return next;
    });
  }, []);

  const refresh = useCallback(() => {
    setRefreshToken((token) => token + 1);
  }, []);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    api
      .listJobs()
      .then((data) => {
        if (cancelled) {
          return;
        }
        setJobs(data.jobs);
        setError(null);
      })
      .catch((cause: unknown) => {
        if (cancelled) {
          return;
        }
        setError(errorMessage(cause));
      })
      .finally(() => {
        if (!cancelled) {
          setLoading(false);
        }
      });
    return () => {
      cancelled = true;
    };
  }, [refreshToken]);

  const activeKey = jobs
    .filter((job) => isActiveStatus(job.status))
    .map((job) => job.id)
    .sort()
    .join(",");

  useEffect(() => {
    const wanted = new Set(activeKey === "" ? [] : activeKey.split(","));
    const sources = sourcesRef.current;
    const timers = reconnectTimersRef.current;

    for (const [id, source] of sources) {
      if (!wanted.has(id)) {
        source.close();
        sources.delete(id);
      }
    }

    for (const id of wanted) {
      if (sources.has(id)) {
        continue;
      }
      const source = new EventSource(jobEventsUrl(id));
      source.onmessage = (event: MessageEvent<string>) => {
        try {
          const parsed: unknown = JSON.parse(event.data);
          if (isJob(parsed)) {
            mergeJob(parsed);
          }
        } catch {
          // 忽略无法解析的帧
        }
      };
      source.onerror = () => {
        source.close();
        sources.delete(id);
        if (timers.has(id)) {
          return;
        }
        const handle = window.setTimeout(() => {
          timers.delete(id);
          setReconnectToken((token) => token + 1);
        }, RECONNECT_DELAY_MS);
        timers.set(id, handle);
      };
      sources.set(id, source);
    }
  }, [activeKey, mergeJob, reconnectToken]);

  useEffect(() => {
    const sources = sourcesRef.current;
    const timers = reconnectTimersRef.current;
    return () => {
      for (const source of sources.values()) {
        source.close();
      }
      sources.clear();
      for (const timer of timers.values()) {
        window.clearTimeout(timer);
      }
      timers.clear();
    };
  }, []);

  const cancelJob = useCallback(
    async (id: string): Promise<void> => {
      await api.cancelJob(id);
      refresh();
    },
    [refresh],
  );

  const sortedJobs = jobs.slice().sort((a, b) => {
    const rank = (job: Job) => (isActiveStatus(job.status) ? 0 : 1);
    const rankDiff = rank(a) - rank(b);
    return rankDiff !== 0 ? rankDiff : b.createdAt - a.createdAt;
  });

  return { jobs: sortedJobs, loading, error, refresh, cancelJob };
}
