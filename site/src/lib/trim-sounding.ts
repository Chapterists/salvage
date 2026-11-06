// The first sounding: salvage trim on Epic's Third Person template, on an
// Apple M4 Mac with 16 GB. Every number here is copied from
// docs/soundings/trim-third-person-m4-16gb.md, the write-up of the runs. If a
// number changes there, change it here; the site's pages all read it from this
// file.
//
// Memory is in MB of 1024 x 1024 bytes, as Unreal's memreport and salvage
// soundings report it, so 1 GB here is 1024 MB.

export const SOUNDING_HREF = "/soundings/trim-third-person-m4-16gb/";
export const SOUNDING_DOC =
  "https://github.com/Chapterists/salvage/blob/main/docs/soundings/trim-third-person-m4-16gb.md";

export type Config = {
  key: "stock" | "16" | "12" | "8";
  label: string;
  /** Process physical memory at 60 s (memreport): median of 4 runs, MB. */
  mem: number;
  memRange: [number, number];
  /** Peak process physical memory, 10 to 70 s (CSV profile). */
  peak: number;
  peakRange: [number, number];
  /** Peak LLM tracked total (LLM platform CSV). */
  llm: number;
  llmRange: [number, number];
  /** Render target pool in use (memreport). */
  rtPool: number;
  rtPoolRange: [number, number];
  /** Metal buffers (LLM platform CSV). */
  metal: number;
  /** Frame times, ms, medians of the per-run medians over frames 300 to 2100. */
  frame: number;
  frameP95: number;
  /** Frames over 50 ms, of 1800: median and range. */
  slow: number;
  slowRange: [number, number];
  gpu: number;
  render: number;
  game: number;
};

export const configs: Config[] = [
  {
    key: "stock",
    label: "Stock",
    mem: 7687, memRange: [7407, 7693],
    peak: 7692, peakRange: [7682, 7694],
    llm: 5509, llmRange: [5509, 5509],
    rtPool: 1828, rtPoolRange: [1828, 1830],
    metal: 576,
    frame: 33.33, frameP95: 33.62, slow: 4, slowRange: [3, 4],
    gpu: 25.06, render: 3.47, game: 1.25,
  },
  {
    key: "16",
    label: "16 GB tier",
    mem: 7582, memRange: [7072, 7590],
    peak: 7618, peakRange: [7111, 7631],
    llm: 5317, llmRange: [4805, 5317],
    rtPool: 1830, rtPoolRange: [1828, 1830],
    metal: 448,
    frame: 33.33, frameP95: 33.63, slow: 4, slowRange: [3, 6],
    gpu: 25.05, render: 3.5, game: 1.23,
  },
  {
    key: "12",
    label: "12 GB tier",
    mem: 7081, memRange: [7069, 7092],
    peak: 7100, peakRange: [7096, 7103],
    llm: 4742, llmRange: [4742, 4742],
    rtPool: 1428, rtPoolRange: [1426, 1428],
    metal: 384,
    frame: 33.33, frameP95: 33.75, slow: 4, slowRange: [4, 6],
    gpu: 23.48, render: 3.87, game: 1.35,
  },
  {
    key: "8",
    label: "8 GB tier",
    mem: 6583, memRange: [6349, 6586],
    peak: 6590, peakRange: [6586, 6598],
    llm: 4165, llmRange: [4165, 4165],
    rtPool: 904, rtPoolRange: [904, 904],
    metal: 320,
    frame: 33.33, frameP95: 33.74, slow: 6, slowRange: [4, 8],
    gpu: 23.43, render: 3.98, game: 1.36,
  },
];

export const stock = configs[0];
export const tier8 = configs[3];

/** 7687 -> "7,687". */
export const mb = (n: number) => n.toLocaleString("en-US");
/** MB to GB of 1024 MB, one decimal: 7687 -> "7.5". */
export const gb = (n: number) => (n / 1024).toFixed(1);
/** A signed difference with a real minus sign: -1104 -> "−1,104". */
export const signed = (n: number, digits = 0) => {
  const s = Math.abs(n).toLocaleString("en-US", { minimumFractionDigits: digits, maximumFractionDigits: digits });
  return n < 0 ? `−${s}` : n > 0 ? `+${s}` : "±0";
};
