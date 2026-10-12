#!/usr/bin/env node
// Export the Nexus Sachsen in a real browser: canvas markings are blank in the old headless GLB.
// Usage: node tools/models/export_sachsen.cjs ../eu-ui /tmp/nexus-vehicles
const fs = require('node:fs');
const path = require('node:path');
const http = require('node:http');

async function main() {
  const source = path.resolve(process.argv[2] || '../eu-ui');
  const output = path.resolve(process.argv[3] || '/tmp/nexus-vehicles');
  const packages = path.join(source, 'node_modules/.pnpm');
  const dependency = name => {
    const version = fs.readdirSync(packages).filter(p => p.startsWith(`${name}@`))
      .sort((a, b) => b.localeCompare(a, undefined, { numeric: true }))[0];
    if (!version) throw new Error(`Install ${name} in the eu-ui checkout first.`);
    return require(path.join(packages, version, 'node_modules', name));
  };
  const esbuild = dependency('esbuild');
  const { chromium } = dependency('playwright');
  const modelDir = path.join(source, 'packages/globe/src');
  const bundle = await esbuild.build({
    stdin: { resolveDir: source, contents: `
      import { modelToGlb } from ${JSON.stringify(path.join(modelDir, 'exportGlb.ts'))};
      import { F124_SACHSEN } from ${JSON.stringify(path.join(modelDir, 'f124Model.ts'))};
      window.exportShip = async () => {
        const data = await modelToGlb(F124_SACHSEN, { skinId: 'sachsen' });
        const a = document.createElement('a');
        a.href = URL.createObjectURL(new Blob([data]));
        a.download = 'f124.glb';
        a.click();
      };
    ` },
    bundle: true, write: false, format: 'iife', platform: 'browser',
    plugins: [{ name: 'retain-deck-detail', setup(build) {
      build.onLoad({ filter: /exportGlb\.ts$/ }, async args => {
        const original = fs.readFileSync(args.path, 'utf8');
        const filter = 'if ((pm.detailOnly || pm.marking !== undefined) && !brand) continue;';
        if (!original.includes(filter)) throw new Error('Nexus exporter changed: review detail filtering.');
        // Modify the bundle only; the source checkout stays untouched.
        const contents = original.replace(filter, 'if (pm.marking !== undefined && !brand) continue;');
        return { contents, loader: 'ts', resolveDir: path.dirname(args.path) };
      });
    } }],
  });
  const server = http.createServer((req, res) => {
    const script = req.url === '/model.js';
    res.setHeader('Content-Type', script ? 'text/javascript' : 'text/html');
    res.end(script ? bundle.outputFiles[0].text : '<script src="/model.js"></script>');
  });
  await new Promise(resolve => server.listen(0, '127.0.0.1', resolve));
  let browser;
  try {
    browser = await chromium.launch({ headless: true,
      executablePath: process.env.CHROMIUM_EXECUTABLE || chromium.executablePath(), args: ['--no-sandbox'] });
    const page = await browser.newPage({ acceptDownloads: true });
    await page.goto(`http://127.0.0.1:${server.address().port}`);
    const download = page.waitForEvent('download', { timeout: 180000 });
    await page.evaluate(() => window.exportShip());
    fs.mkdirSync(output, { recursive: true });
    await (await download).saveAs(path.join(output, 'f124.glb'));
    console.log(`Exported Sachsen with deck details and browser-rendered markings to ${output}`);
  } finally {
    if (browser) await browser.close();
    server.close();
  }
}
main().catch(error => { console.error(error); process.exitCode = 1; });
