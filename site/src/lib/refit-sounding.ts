// The second sounding: salvage refit across Unreal Engine 5.8.4's own plugins,
// and Epic's PSDImporter rebuilt for the Mac editor. Every number here is
// copied from docs/soundings/refit-ue58-mac.md. If one changes there, change it
// here; the site reads them from this file.

export const REFIT_SOUNDING_HREF = "/soundings/refit-ue58-mac/";
export const REFIT_SOUNDING_DOC =
  "https://github.com/Chapterists/salvage/blob/main/docs/soundings/refit-ue58-mac.md";

export type Verdict = {
  key: string;
  /** The words salvage refit scan prints. */
  label: string;
  count: number;
  group: "works" | "refit" | "port";
  /** How the figure draws one plugin with this verdict. */
  shape: "dot" | "ring" | "half" | "square";
};

// In the order the figure lays them out: what works first, porting last.
export const verdicts: Verdict[] = [
  { key: "works", label: "Works on Mac", count: 721, group: "works", shape: "dot" },
  { key: "content", label: "Content only, works on Mac", count: 28, group: "works", shape: "ring" },
  { key: "partial", label: "Works on Mac without its Windows-only modules", count: 19, group: "works", shape: "half" },
  { key: "patch-only", label: "Allow list only, and the Mac binaries are already there: patch", count: 8, group: "refit", shape: "ring" },
  { key: "patch-rebuild", label: "Allow list only: patch, then rebuild", count: 73, group: "refit", shape: "dot" },
  { key: "rebuild", label: "Nothing in the way, just no Mac binaries: rebuild", count: 3, group: "refit", shape: "half" },
  { key: "port", label: "Needs porting", count: 42, group: "port", shape: "square" },
];

export const total = verdicts.reduce((n, v) => n + v.count, 0); // 894
export const inGroup = (g: Verdict["group"]) =>
  verdicts.filter((v) => v.group === g).reduce((n, v) => n + v.count, 0);
// 768 work, 84 need a patch, a rebuild or both, 42 need porting.
