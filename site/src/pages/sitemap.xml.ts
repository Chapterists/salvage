// sitemap.xml: every page on the site, from the files in src/pages. Uses the
// `site` URL in astro.config.mjs, so every entry is https://salvage.chapterists.com.
import type { APIRoute } from "astro";

const files = Object.keys(import.meta.glob("./**/*.astro"));

export const GET: APIRoute = ({ site }) => {
  const paths = files
    .map((file) => file.replace(/^\.\//, "").replace(/\.astro$/, ""))
    .filter((page) => page !== "404" && !page.includes("["))
    .map((page) => (page === "index" ? "/" : `/${page.replace(/\/?index$/, "")}/`))
    .sort();
  const urls = paths.map((path) => `  <url><loc>${new URL(path, site)}</loc></url>`).join("\n");
  const body = `<?xml version="1.0" encoding="UTF-8"?>\n<urlset xmlns="http://www.sitemaps.org/schemas/sitemap/0.9">\n${urls}\n</urlset>\n`;
  return new Response(body, { headers: { "Content-Type": "application/xml; charset=utf-8" } });
};
