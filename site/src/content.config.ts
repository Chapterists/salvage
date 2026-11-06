import { defineCollection } from "astro:content";
import { glob } from "astro/loaders";
import { z } from "astro/zod";

// The logbook: one markdown file per entry, newest first on the page.
// Name files YYYY-MM-DD-NN-slug.md; NN orders entries made on the same day.
const logbook = defineCollection({
  loader: glob({ pattern: "**/*.md", base: "./src/content/logbook" }),
  schema: z.object({
    title: z.string(),
    date: z.coerce.date(),
  }),
});

export const collections = { logbook };
