import { getCollection } from "astro:content";

/** Logbook entries, newest first. Same-day entries sort by file name. */
export async function logbookEntries() {
  const entries = await getCollection("logbook");
  return entries.sort(
    (a, b) => b.data.date.getTime() - a.data.date.getTime() || b.id.localeCompare(a.id),
  );
}

export const formatDate = (d: Date) => d.toISOString().slice(0, 10);
