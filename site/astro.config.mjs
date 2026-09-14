import { defineConfig } from "astro/config";
import sitemap from "@astrojs/sitemap";

export default defineConfig({
  site: "https://dallenlol.github.io",
  base: "/hark",
  trailingSlash: "always",
  integrations: [sitemap()],
  build: { format: "directory" },
});
