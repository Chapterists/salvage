import { defineConfig } from "astro/config";
import { satteri } from "@astrojs/markdown-satteri";

const repo = "https://github.com/Chapterists/salvage";

// Markdown from the repository root (CONTRIBUTING.md) is written for GitHub, so
// its relative links point at files in the repo. On the site those links would
// 404; send them to the file on GitHub instead.
const repoLinks = {
  name: "salvage-repo-links",
  element: {
    filter: ["a"],
    visit(node, ctx) {
      const href = node.properties?.href;
      if (typeof href === "string" && !/^([a-z]+:|\/|#)/i.test(href)) {
        ctx.setProperty(node, "href", `${repo}/blob/main/${href.replace(/^\.\//, "")}`);
      }
    },
  },
};

// Pages supply their own h1, so drop any h1 from rendered markdown.
const dropH1 = {
  name: "salvage-drop-h1",
  element: {
    filter: ["h1"],
    visit(node, ctx) {
      ctx.removeNode(node);
    },
  },
};

export default defineConfig({
  site: "https://salvage.chapterists.com",
  trailingSlash: "always",
  // A Content-Security-Policy in a <meta> tag on every page, with hashes for
  // the scripts and styles Astro emits. Inline style attributes (the waterline
  // dividers and Shiki's code colors use them) can't be hashed, so they're
  // allowed through style-src-attr only. frame-ancestors can't go in a <meta>
  // tag; public/_headers sends it as a header.
  security: {
    csp: {
      directives: [
        "default-src 'self'",
        "img-src 'self' data:",
        "font-src 'self'",
        "object-src 'none'",
        "base-uri 'self'",
        "form-action 'self'",
      ],
      styleDirective: {
        resources: [
          { resource: "'self'", kind: "element" },
          { resource: "'unsafe-inline'", kind: "attribute" },
        ],
      },
    },
  },
  markdown: {
    processor: satteri({ hastPlugins: [repoLinks, dropH1] }),
    shikiConfig: { theme: "css-variables" },
  },
  vite: {
    // Allow importing CONTRIBUTING.md from the repository root.
    server: { fs: { allow: [".."] } },
  },
});
