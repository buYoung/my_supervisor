import {createContext, useCallback, useContext, useEffect, useMemo, useRef, useState, type ReactNode} from "react";
import {useOperationsClient, toErrorMessage} from "../services/use-operations";
import {formatDateTime, formatTime} from "./time";

interface DisplayTimeContextValue {
  timeZone: string;
  isLoading: boolean;
  error: string;
  setTimeZone: (value: string) => Promise<void>;
  dateTime: (value?: string | number | Date | null, milliseconds?: boolean) => string;
  time: (value: string | number | Date) => string;
}
const DisplayTimeContext = createContext<DisplayTimeContextValue | null>(null);

export function DisplayTimeProvider({children}: {children: ReactNode}) {
  const client = useOperationsClient();
  const [timeZone, setZone] = useState("UTC");
  const [isLoading, setIsLoading] = useState(true);
  const [error, setError] = useState("");
  const revision = useRef(0);
  const isSaving = useRef(false);
  useEffect(() => {
    let isCancelled = false;
    const load = async () => {
      if (isSaving.current) return;
      const current = ++revision.current;
      try {
        const settings = await client.getDisplaySettings();
        new Intl.DateTimeFormat("en-US", {timeZone: settings.timeZone});
        if (!isCancelled && current === revision.current) {setZone(settings.timeZone);setError("");}
      } catch (cause) {if (!isCancelled && current === revision.current) setError(toErrorMessage(cause));}
      finally {if (!isCancelled && current === revision.current) setIsLoading(false);}
    };
    void load(); const timer = window.setInterval(() => void load(), 5000);
    window.addEventListener("focus", load);
    return () => {isCancelled = true;window.clearInterval(timer);window.removeEventListener("focus", load);};
  }, [client]);
  const setTimeZone = useCallback(async (value: string) => {
    new Intl.DateTimeFormat("en-US", {timeZone: value});
    isSaving.current = true; revision.current++;
    try { const settings = await client.setDisplayTimeZone(value);setZone(settings.timeZone);setError("");setIsLoading(false); }
    finally {isSaving.current = false;}
  }, [client]);
  const value = useMemo(() => ({timeZone, isLoading, error, setTimeZone, dateTime: (date?: string | number | Date | null, milliseconds = false) => isLoading ? "—" : formatDateTime(date, timeZone, milliseconds), time: (date: string | number | Date) => isLoading ? "—" : formatTime(date, timeZone)}), [timeZone, isLoading, error, setTimeZone]);
  return <DisplayTimeContext.Provider value={value}>{children}</DisplayTimeContext.Provider>;
}

export function useDisplayTime() {
  const context = useContext(DisplayTimeContext);
  if (!context) throw new Error("DisplayTimeProvider is required");
  return context;
}
