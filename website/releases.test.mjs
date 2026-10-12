import test from 'node:test';
import assert from 'node:assert/strict';
import { downloads, filenames } from './releases.mjs';
const asset = { name: filenames.linux, state: 'uploaded', size: 1024 ** 2, browser_download_url: 'https://github.com/birkmann/zugriff/releases/download/v0.1.0/zugriff-linux-x86_64.tar.gz' };
test('only a published matching platform asset enables a download', () => {
  assert.deepEqual(Object.keys(downloads({ assets: [asset] })), ['linux']);
  assert.equal(downloads({ assets: [asset] }).linux.size, 1);
  for (const release of [null, {}, { assets: [] }, { draft: true, assets: [asset] }, { prerelease: true, assets: [asset] }, { assets: [{ ...asset, state: 'new' }] }]) assert.deepEqual(downloads(release), {});
});
test('foreign and malformed asset URLs cannot become download links', () => {
  for (const url of ['javascript:alert(1)', 'https://example.com/game.zip', 'https://github.com/other/repo/releases/download/v1/game.zip', '/relative']) {
    assert.deepEqual(downloads({ assets: [{ ...asset, browser_download_url: url }] }), {});
  }
});
