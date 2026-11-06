// Geometry for the SALVAGE wordmark: hull lettering on a 100-unit cap height,
// drawn from straight lines and quarter circles. The same shapes are in the
// SVG files under assets/brand/ at the repository root.

export const capHeight = 100;
export const width = 684;

// The waterline: letters are painted above y = 64 and below y = 68.
export const waterline = { above: 64, below: 68 };

const S = "M100 0V22H30V39H78A22 22 0 0 1 100 61V78A22 22 0 0 1 78 100H0V78H70V61H22A22 22 0 0 1 0 39V22A22 22 0 0 1 22 0Z";
const A = "M44 0H66L110 100H80L75.16 89H34.84L30 100H0ZM55 43.18L65.92 68H44.08Z";
const L = "M0 0H30V78H84V100H0Z";
const V = "M0 0H30L55 56.82L80 0H110L66 100H44Z";
const G = "M100 0V22H30V78H70V64H52V42H100V78A22 22 0 0 1 78 100H22A22 22 0 0 1 0 78V22A22 22 0 0 1 22 0Z";
const E = "M0 0H90V22H30V39H80V61H30V78H90V100H0Z";

// Letters with their x offsets (kerned by eye).
export const letters = [
  [S, 0],
  [A, 106],
  [L, 226],
  [V, 288],
  [A, 368],
  [G, 482],
  [E, 594],
];
