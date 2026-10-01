/** Formats elapsed recording time as MM:SS (or HH:MM:SS). Always whole seconds. */
export function formatElapsed(seconds: number): string {
  const total = Number.isFinite(seconds) ? Math.max(0, Math.floor(seconds)) : 0;
  const hrs = Math.floor(total / 3600);
  const mins = Math.floor((total % 3600) / 60);
  const secs = total % 60;
  const mm = mins.toString().padStart(2, "0");
  const ss = secs.toString().padStart(2, "0");
  return hrs > 0 ? `${hrs.toString().padStart(2, "0")}:${mm}:${ss}` : `${mm}:${ss}`;
}

export const DBFS_FLOOR = -60;
export const DBFS_SILENCE = -90;

/** Maps a dBFS reading (-60..0) to a 0..100 meter width. Non-finite => 0. */
export function dbfsToPercent(dbfs: number | null | undefined): number {
  if (dbfs == null || !Number.isFinite(dbfs)) return 0;
  const clamped = Math.min(0, Math.max(DBFS_FLOOR, dbfs));
  return ((clamped - DBFS_FLOOR) / -DBFS_FLOOR) * 100;
}

/** Human label for a dBFS reading. */
export function formatDbfs(dbfs: number | null | undefined): string {
  if (dbfs == null || !Number.isFinite(dbfs) || dbfs <= DBFS_SILENCE) return "-inf dBFS";
  return `${Math.round(dbfs)} dBFS`;
}
