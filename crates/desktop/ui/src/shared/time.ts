const pad = (value: number, width = 2) => String(value).padStart(width, "0");

export function formatDateTime(value: string | number | Date | undefined | null, timeZone: string, milliseconds = false): string {
  if (value == null || value === "") return "—";
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "—";
  return new Intl.DateTimeFormat("sv-SE", {timeZone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23", ...(milliseconds ? {fractionalSecondDigits: 3 as const} : {})}).format(date).replace(",", ".");
}

export function formatTime(value: string | number | Date, timeZone: string): string {
  const date = new Date(value);
  return Number.isFinite(date.getTime()) ? new Intl.DateTimeFormat("en-GB", {timeZone, hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23"}).format(date) : "—";
}

function partsAt(value: number, timeZone: string): number[] {
  const parts = new Intl.DateTimeFormat("en-CA", {timeZone, year: "numeric", month: "2-digit", day: "2-digit", hour: "2-digit", minute: "2-digit", second: "2-digit", hourCycle: "h23"}).formatToParts(value);
  return ["year", "month", "day", "hour", "minute", "second"].map(key => Number(parts.find(part => part.type === key)?.value));
}

export function toDateTimeInput(value: string, timeZone: string): string {
  const date = new Date(value);
  if (!Number.isFinite(date.getTime())) return "";
  const [year, month, day, hour, minute, second] = partsAt(date.getTime(), timeZone);
  return pad(year, 4) + "-" + pad(month) + "-" + pad(day) + "T" + pad(hour) + ":" + pad(minute) + ":" + pad(second);
}

export function dateInTimeZone(value: string | number | Date, timeZone: string): string {
  return toDateTimeInput(new Date(value).toISOString(), timeZone).slice(0, 10);
}

/** Resolve a wall-clock input without using the browser's own time zone.
 * Repeated DST times select the earlier instant; nonexistent times are rejected.
 */
export function fromDateTimeInput(value: string, timeZone: string): string {
  const match = /^(\d{4})-(\d{2})-(\d{2})T(\d{2}):(\d{2})(?::(\d{2}))?$/.exec(value);
  if (!match) throw new Error("날짜와 시간을 모두 입력하세요.");
  const wanted = match.slice(1).map(part => Number(part || 0));
  const [year, month, day, hour, minute, second] = wanted;
  const wall = Date.UTC(year, month - 1, day, hour, minute, second);
  const normalized = new Date(wall);
  if (normalized.getUTCFullYear() !== year || normalized.getUTCMonth() !== month - 1 || normalized.getUTCDate() !== day || hour > 23 || minute > 59 || second > 59) throw new Error("유효한 날짜와 시간을 입력하세요.");
  const offsets = new Set<number>();
  for (let hours = -36; hours <= 36; hours += 6) {
    const instant = wall + hours * 3600000;
    const parts = partsAt(instant, timeZone);
    offsets.add(Date.UTC(parts[0], parts[1] - 1, parts[2], parts[3], parts[4], parts[5]) - instant);
  }
  const candidates = [...offsets].map(offset => wall - offset).filter(instant => partsAt(instant, timeZone).every((part, index) => part === wanted[index]));
  if (!candidates.length) throw new Error("선택한 시간대에 존재하지 않는 시각입니다. 서머타임 전환 시간을 확인하세요.");
  return new Date(Math.min(...candidates)).toISOString();
}

export function dateRangeBoundary(day: string, timeZone: string, end = false): string {
  const start = fromDateTimeInput(day + "T00:00:00", timeZone);
  if (!end) return start;
  const nextDay = new Date(Date.parse(day + "T00:00:00Z") + 86400000).toISOString().slice(0, 10);
  return new Date(Date.parse(fromDateTimeInput(nextDay + "T00:00:00", timeZone)) - 1).toISOString();
}

export function supportedTimeZones(): string[] {
  const intl = Intl as typeof Intl & {supportedValuesOf?: (key: string) => string[]};
  const values = intl.supportedValuesOf?.("timeZone") ?? ["Asia/Seoul", "Asia/Tokyo", "America/New_York", "America/Los_Angeles", "Europe/London", "Europe/Paris", "Australia/Sydney"];
  return ["UTC", ...values.filter(zone => zone !== "UTC")];
}
