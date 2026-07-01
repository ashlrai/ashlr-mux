import { mkdir, readFile, rename, rm, rmdir, writeFile } from "node:fs/promises";
import path from "node:path";
import { fileURLToPath } from "node:url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const repoRoot = path.resolve(__dirname, "..", "..");
const webRoot = path.join(repoRoot, "apps", "desktop", "web");
const srcRoot = path.join(webRoot, "src");
const distRoot = path.join(webRoot, "dist");
const assetsRoot = path.join(distRoot, "assets");

await rm(distRoot, { recursive: true, force: true });
await mkdir(assetsRoot, { recursive: true });

const result = await Bun.build({
  entrypoints: [path.join(srcRoot, "main.tsx")],
  format: "esm",
  minify: false,
  outdir: assetsRoot,
  root: webRoot,
  sourcemap: "none",
  target: "browser",
});

if (!result.success) {
  for (const log of result.logs) {
    console.error(log);
  }
  throw new Error("Desktop web build failed.");
}

const emittedEntry = path.join(assetsRoot, "src", "main.js");
const normalizedEntry = path.join(assetsRoot, "main.js");
await rename(emittedEntry, normalizedEntry);
await rmdir(path.join(assetsRoot, "src"));

// xterm ships its own stylesheet; prepend it so the terminal renders correctly
// without a CSS-in-JS import in the entrypoint.
const xtermCssPath = path.join(
  webRoot,
  "node_modules",
  "@xterm",
  "xterm",
  "css",
  "xterm.css",
);
const xtermStyles = await readFile(xtermCssPath, "utf8");
const styles = await readFile(path.join(srcRoot, "styles.css"), "utf8");
await writeFile(
  path.join(assetsRoot, "styles.css"),
  `${xtermStyles}\n${styles}`,
  "utf8",
);

const html = `<!doctype html>
<html lang="en">
  <head>
    <meta charset="UTF-8" />
    <meta name="viewport" content="width=device-width, initial-scale=1.0" />
    <title>cmux for Windows</title>
    <link rel="stylesheet" href="./assets/styles.css" />
  </head>
  <body>
    <div id="root"></div>
    <script type="module" src="./assets/main.js"></script>
  </body>
</html>
`;

await writeFile(path.join(distRoot, "index.html"), html, "utf8");
console.log(`Built desktop web shell into ${distRoot}`);
