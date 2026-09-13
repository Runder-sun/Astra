import { copyFile, mkdir, readFile, readdir, rename, writeFile } from 'node:fs/promises';

const root = new URL('..', import.meta.url);
const appRoot = new URL('.', import.meta.url);
const outDir = new URL('../mobile/', import.meta.url);

await mkdir(new URL('icons/', outDir), { recursive: true });
await copyFile(new URL('manifest.webmanifest', appRoot), new URL('manifest.webmanifest', outDir));
await copyFile(new URL('sw.js', appRoot), new URL('sw.js', outDir));
await copyFile(new URL('favicon.ico', appRoot), new URL('favicon.ico', outDir));
await copyFile(new URL('icons/icon.svg', appRoot), new URL('icons/icon.svg', outDir));

const assetsDir = new URL('assets/', outDir);
const assets = await readdir(assetsDir);
const cssAsset = assets.find((name) => name.endsWith('.css'));
if (cssAsset) {
  await rename(new URL(cssAsset, assetsDir), new URL('styles.css', outDir));
}

const indexPath = new URL('index.html', outDir);
let html = await readFile(indexPath, 'utf8');
html = html
  .replace(/<script type="module" crossorigin src="\/app\.js"><\/script>/, '<script src="/app.js" defer></script>')
  .replace(/<link rel="stylesheet" crossorigin href="\/assets\/[^"]+\.css">/, '<link rel="stylesheet" href="/styles.css">')
  .replace(/<link rel="icon" type="image\/svg\+xml" href="\/assets\/[^"]+\.ico">/, '<link rel="icon" type="image/svg+xml" href="/favicon.ico">')
  .replace(/<link rel="apple-touch-icon" href="\/assets\/[^"]+\.svg">/, '<link rel="apple-touch-icon" href="/icons/icon.svg">')
  .replace(/<link rel="manifest" href="\/assets\/[^"]+\.webmanifest">/, '<link rel="manifest" href="/manifest.webmanifest">');
await writeFile(indexPath, html);
