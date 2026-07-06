/**
 * Small React hooks over the operations client. Keep the views thin: they declare WHAT to
 * fetch; these hooks own loading/error state and interval polling for liveness.
 */

import { useCallback, useEffect, useRef, useState } from "react";
import { getOperationsClient, OperationsError, type OperationsClient } from "./index";

/** The shared client singleton (invoke inside Tauri, HTTP standalone). */
export function useOperationsClient(): OperationsClient {
  const clientRef = useRef<OperationsClient | null>(null);
  if (!clientRef.current) {
    clientRef.current = getOperationsClient();
  }
  return clientRef.current;
}

function toErrorMessage(error: unknown): string {
  if (error instanceof OperationsError) {
    return error.message;
  }
  if (error instanceof Error) {
    return error.message;
  }
  return "알 수 없는 오류가 발생했습니다.";
}

export interface PolledResource<T> {
  data: T | null;
  isLoading: boolean;
  errorMessage: string | null;
  refresh: () => Promise<void>;
}

/**
 * Fetch a resource on mount and re-fetch on an interval for liveness. The initial load drives
 * the loading state; background refreshes keep the last good data on transient failure but
 * surface the error message.
 */
export function usePolledResource<T>(
  fetcher: () => Promise<T>,
  intervalMs: number,
): PolledResource<T> {
  const [data, setData] = useState<T | null>(null);
  const [isLoading, setIsLoading] = useState(true);
  const [errorMessage, setErrorMessage] = useState<string | null>(null);
  const fetcherRef = useRef(fetcher);
  fetcherRef.current = fetcher;
  const isMountedRef = useRef(true);

  const load = useCallback(async () => {
    try {
      const result = await fetcherRef.current();
      if (!isMountedRef.current) {
        return;
      }
      setData(result);
      setErrorMessage(null);
    } catch (error) {
      if (!isMountedRef.current) {
        return;
      }
      setErrorMessage(toErrorMessage(error));
    } finally {
      if (isMountedRef.current) {
        setIsLoading(false);
      }
    }
  }, []);

  useEffect(() => {
    isMountedRef.current = true;
    void load();
    const timerId = window.setInterval(() => {
      void load();
    }, intervalMs);
    return () => {
      isMountedRef.current = false;
      window.clearInterval(timerId);
    };
  }, [load, intervalMs]);

  return { data, isLoading, errorMessage, refresh: load };
}

export { toErrorMessage };
